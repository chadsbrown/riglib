Got it. Below is the revised “detailed plan” with the missing items explicitly addressed, plus a pragmatic migration sequence that keeps risk low and avoids a flag day. I’ll keep it diff-oriented where possible, but I’m going to add the architectural context you’re asking for.

---

## Updated plan overview (with your checklist items)

### ✅ Transport trait implications — addressed already

### ⛔ Builder pattern changes — addressed below

### ⛔ Test strategy — addressed below

### ⛔ Flex backend — addressed below

### ⚠️ Coalescing complexity — examined below

### ⚠️ Phase 4 deferral — emphasized below

### ✅ Error model timing — explicitly treated as orthogonal below

---

# Phase 0 — Minimal safety cleanup (no behavior change)

## 0.1 Timeout unification

* **Rule:** `Transport::receive(timeout)` is the single timeout boundary.
* Remove outer `tokio::timeout(...)` wrappers in direct paths and task paths.
* This makes later scheduler behavior predictable.

**No builder/test changes needed yet.**

---

# Phase 1 — Builder pattern changes (IO task always spawned)

This is the first missing item.

## Current state (typical pattern)

Many backends have something like:

* `Builder::build()` or `connect()` creates a rig with `Arc<Mutex<Box<dyn Transport>>>`
* `enable_transceive()` optionally spawns a reader task and swaps the transport for a sentinel
* plus `build_with_transport(transport)` for testing.

## Target state (post-migration)

* The builder **always spawns the IO task** and returns a rig handle that sends requests to that task.
* “AI/transceive enabled” becomes **a flag passed into the IO task config** (controls event emission/caching), not a “spawn task or not” toggle.

### What changes in builder API (recommended)

Keep the public ergonomic surface almost the same. Internally, it changes a lot.

#### Keep these methods (or equivalents)

* `Builder::build()` (creates the transport itself from connection params)
* `Builder::build_with_transport(transport)` (test hook)

#### But change what they return/construct

Instead of returning a rig that owns/locks the transport, they:

1. create transport
2. create IO task channels + spawn the task
3. return rig { io_handle, cached_info, capabilities, event_tx, … }

### Concrete pattern (good, minimal churn)

**Before (conceptual):**

```rust
pub async fn build(self) -> Result<IcomRig> {
  let transport = SerialTransport::open(...).await?;
  Ok(IcomRig { transport: Arc<Mutex<Box<dyn Transport>>>, ... })
}
```

**After (conceptual):**

```rust
pub async fn build(self) -> Result<IcomRig> {
  let transport: Box<dyn Transport> = Box::new(SerialTransport::open(...).await?);
  self.build_with_transport(transport).await
}

pub async fn build_with_transport(self, transport: Box<dyn Transport>) -> Result<IcomRig> {
  let (io, event_rx) = spawn_io_task(transport, IoConfig { ai_enabled: self.ai_enabled, ... });
  Ok(IcomRig { io, event_rx, info: ..., caps: ... })
}
```

### Builder “AI mode” knobs (what happens to them?)

* `Builder::enable_ai()` / `enable_transceive()` no longer spawns a task.
* It only sets `ai_enabled: bool` in `IoConfig`.
* You may still expose:

  * `Builder::ai_enabled(bool)`
  * `Builder::event_buffer_size(n)`
  * `Builder::unsolicited_policy(...)` (ignore / emit / emit+cache)

### Important edge: returning a receiver from the builder

If you currently create `broadcast::Sender` in the rig and let `subscribe()` create receivers, you can keep that unchanged:

* IO task owns the `broadcast::Sender<RigEvent>`
* rig clones sender or holds it; `subscribe()` returns `sender.subscribe()`

So builder does not need to return an event receiver; it returns the rig with an event sender embedded.

---

# Phase 2 — Test strategy (IO-task-mediated, using MockTransport)

Second missing item.

## What changes in testing when IO task always owns transport?

You stop calling “execute command directly against MockTransport” and instead test via:

* rig method calls (through channels)
* deterministic IO-task loop with a mock transport (scripted reads/writes)

### Keep: `build_with_transport(MockTransport)`

This is the key. Post-migration, `build_with_transport()` still works—*it just spawns the IO task using that transport.*

## Two complementary test styles (recommended)

### A) Black-box driver tests (preferred)

Test the public rig API (`set_frequency`, `get_frequency`, etc.) against a scripted MockTransport.

**MockTransport needs:**

* an **expected write queue** (what the rig should send)
* a **read script** (what bytes arrive, and when)
* optionally, ability to inject unsolicited frames at arbitrary times

You likely already have most of this in `riglib-test-harness`.

**Test flow:**

1. Create `MockTransport` with:

   * expected writes: `[cmd1_bytes, cmd2_bytes, ...]`
   * scripted reads: `[resp1_bytes, resp2_bytes, ...]`
2. `let rig = Builder::new(...).build_with_transport(Box::new(mock)).await?;`
3. Call `rig.get_frequency(VFO_A).await?;`
4. Assert:

   * returned value correct
   * mock write expectations satisfied
   * no extra writes

**Important:** because IO task is async, ensure:

* tests run on Tokio
* you await the rig call (which awaits the oneshot response)
* the IO task progresses because it’s spawned on the runtime

This is already how you test many async actor patterns in Rust.

### B) IO-task unit tests (white-box, targeted)

Directly test the IO loop module with a `MockTransport` and explicit command requests.
This is useful for:

* correlation behavior
* priority scheduling
* coalescing correctness (if you implement it)

You’d export a `spawn_io_task()` that returns:

* `IoHandle { rt_tx, bg_tx, ... }`
* and maybe a `JoinHandle` so tests can shut it down

**Shutdown test hook:** add a `Shutdown` request that causes the task to return.

## Testing unsolicited behavior with AI disabled vs enabled

This is crucial for your “AI off must still work” invariant.

You want tests like:

* **AI off:** unsolicited frame arrives before response

  * response should still match the command
  * no event should be emitted

* **AI on:** same setup

  * response matches command
  * unsolicited frame becomes a `RigEvent`
  * optionally cache updates (if you implement caching)

That’s entirely testable with scripted reads.

---

# Phase 3 — Flex backend (explicit plan)

Third missing item.

Flex is a special case because it’s not “send a command then read a discrete response” in the same way. It’s:

* TCP command/control with status lines
* UDP for meters (VITA-49-ish)
* a shared state model (`RadioState`) updated from incoming messages

## What you should do for Flex in this migration

**Do NOT force Flex into the same exact “single in-flight request/response” model.**
But do align it with the same high-level architecture: **transport(s) owned by tasks**; the rig becomes a handle.

### Flex target architecture

* **TCP task** owns the TCP transport:

  * reads status/control messages continuously
  * updates `RadioState` cache
  * accepts outbound commands via channel (and writes them)
* **UDP meter task** owns UDP socket:

  * reads meter packets continuously
  * updates meter cache / event stream

### How rig methods behave (Flex)

Most getters should be cache reads:

* `get_frequency` reads `RadioState` (fast, no wire)
  Setters:
* send TCP command via channel
* optionally wait for an acknowledgement or for state to reflect the change (you already have settle heuristics)

### Builder changes for Flex

* `FlexBuilder::build()` spawns both tasks
* “AI enabled” for Flex might mean:

  * emit more events (slice updates, spot changes)
  * or just enable event emission at all
    But Flex already *is* event-driven by nature, so “AI off” likely means “don’t emit events outward,” not “don’t run state receiver.”

**Key point:** for Flex, **the IO task model is already the natural design**. You don’t need a direct mode at all.

---

# Coalescing complexity (worth questioning)

You’re right: coalescing can get messy. It’s optional. Here’s how to decide.

## When coalescing is worth it

* You have a UI/panadapter polling at high frequency (10–50 Hz)
* You have multiple consumers (UI + logger + automation)
* You see command queue growth or latency spikes
* You’re targeting SO2R and want predictable RT response

## When to skip coalescing

* You are still validating protocol correctness with real rigs
* You’re not seeing high polling load yet
* You want to keep the IO task simple until you have stable drivers

## If you do implement it, keep it narrow

Start with only *idempotent, high-frequency reads*:

* `get_frequency(rx)`
* `get_mode(rx)`
* maybe `get_ptt_state()`

Avoid coalescing:

* setters
* anything with side effects
* reads whose results can legitimately differ between identical requests in-flight (rare, but be conservative)

### Minimal coalescing implementation (low risk)

* Coalesce **only while an identical command is in-flight**
* Do not add time-window caching yet
* Do not add “drop older reads” yet

That keeps it from becoming a “cache invalidation project.”

---

# Phase 4 deferral (emphasize this)

This needs to be explicit: **Phase 4 (atomic sequences / SO2R “transactions”) should be deferred** until:

1. IO-task-only is stable on at least one backend (Icom)
2. priority scheduling is in place (RT vs BG)
3. you have some hardware validation

Why? Because “atomic sequences” are:

* backend-specific
* easy to implement incorrectly (interactions with split, TX selection, A/B VFO semantics)
* less valuable if base command latency is still unstable

So: prioritize:

* IO task unification
* priority (PTT/CW preemption)
* validation
  before introducing sequence semantics.

---

# Error model timing (orthogonal)

Totally agree: structured errors are valuable but **should not block** the IO-task migration.

## Recommended timing

* Do IO-task-only + builder changes + testing first
* Add structured errors once you have:

  * stable response matching
  * stable shutdown behavior
  * a few Tier-1 rigs validated

Otherwise you’ll spend time re-plumbing errors while still changing the execution model.

---

# Revised phased sequence (practical, low-risk)

## Phase A — “IO task without behavior change” (Icom first)

1. Timeout cleanup
2. Builder always spawns IO task
3. AI flag gates event emission only
4. Delete direct path (`execute_command_direct`) **only after** parity tests pass
5. Add lifecycle cancellation/abort

**Deliverable:** Icom backend works with AI on/off, all ops route through IO task.

## Phase B — Add priority (RT/BG), no coalescing yet

* Route PTT/CW/TX selection via RT channel
* Keep polling via BG channel

**Deliverable:** predictable PTT/CW response under heavy polling.

## Phase C — Decide on coalescing (based on profiling)

* If needed, implement “in-flight coalescing only” for 2–3 getters

## Phase D — Repeat for Yaesu/Elecraft/Kenwood

* Same pattern

## Phase E — Flex alignment

* Ensure Flex is already IO-task-based; adjust builder/test patterns to match the new conventions (especially `build_with_transport` equivalents if you want mockable TCP/UDP).

## Phase F — (Deferred) Atomic sequences / SO2R transactions

* Only after above is stable and you have validated rigs.

---

## What I need from you to tailor the diff plan precisely

No big clarifying interrogation—just pick one:

* If you want to start with **Icom**, I’ll give you the exact builder and module refactor layout (structs/enums, where to store senders, how to gate AI, and the tests to add).
* If you want to start with **Flex** alignment, I’ll outline the task split and how to mock TCP/UDP cleanly.

If you don’t want to choose, the default best path is **Icom first** because your transceive/IO loop is already the most mature.

