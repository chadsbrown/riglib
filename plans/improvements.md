Absolutely. Here’s the **detailed plan again**, with **extra context**—especially on the “remove `execute_command_direct()`” part—plus a practical migration path that won’t break “AI/transceive disabled” usage.

---

## Goal and guiding invariants

### End-state goals

1. **One place reads from the wire** (per rig connection)
2. **All commands serialize through a scheduler** (so you can do priority, coalescing, atomic sequences)
3. **AI/transceive becomes a behavior toggle** (“emit unsolicited events + update caches”), not an architectural fork (“different I/O model”).

### Invariants you should preserve throughout the migration

* **AI/transceive disabled must still fully work** (commands + responses).
* No backend should regress in supported operations (freq/mode/split/PTT/etc.).
* You should be able to flip AI on/off without changing correctness—only “extra features” (events/caching).

---

## Phase 0 — Baseline audit + quick safety fixes

### 0.1 Remove redundant timeout layering (improves determinism)

**Why:** many drivers wrap `Transport::receive(timeout)` inside another `tokio::time::timeout()`. This creates ambiguous behavior and makes tuning harder.

**What to do:**

* Make `Transport::receive(buf, timeout)` the *only* timeout authority.
* Remove outer timeouts in the drivers’ direct paths and IO-task paths.

**Definition of done:**

* Timeouts all originate from a single place (transport layer).
* Logs / errors consistently report “timeout” in one form.

---

## Phase 1 — Lifecycle hygiene for spawned tasks

### 1.1 Make sure background tasks do not outlive the rig object

**Why:** even if logic is correct, logger apps will care about clean shutdown.

**Implementation options:**

* Minimal: store `JoinHandle` and `abort()` it in `Drop`.
* Better: add `CancellationToken`; `Drop` triggers cancellation; abort only as fallback.

**Definition of done:**

* Dropping the rig does not hang the process.
* Serial ports are not left “busy” due to lingering tasks.

---

## Phase 2 — The key change: remove `execute_command_direct()` by making the IO task universal

This is the part you asked to expand, so I’ll be very explicit.

### What `execute_command_direct()` is doing today

In “direct mode” it typically:

1. locks `Arc<Mutex<Transport>>`
2. writes a command
3. reads bytes until it decodes a matching response or times out

This is fine early on, but it becomes a problem when you want:

* concurrent UI polling + fast user knob changes
* SO2R “RT” operations (PTT/CW) that must not get stuck behind polling
* response correlation under unsolicited traffic

### What replacing it means (and does NOT mean)

**Replacing `execute_command_direct()` does *not* mean AI/transceive must be enabled.**

It means:

* you **always** use “single owner task” for the transport
* and AI/transceive becomes simply: “do we decode unsolicited frames into events/caches?”

So: **AI disabled radios still work**. They still send commands and get responses—just via the IO task.

---

## Phase 2A — Create a universal IO task (even when AI is off)

### 2A.1 Always spawn IO task during connect/build

Instead of:

* AI off → direct mode
* AI on → IO task

You move to:

* Always → IO task
* AI flag controls behavior *inside the IO task*

### 2A.2 Behavior when AI/transceive is disabled

When AI is off, the IO task:

* still **reads** from the transport (must do this anyway to receive responses)
* still **decodes** enough to match command responses (otherwise you can’t complete requests)
* does **not** publish unsolicited frames as `RigEvent`s
* optionally ignores unsolicited frames or logs them at trace/debug

That’s the key decoupling: **transport ownership and read loop are always on.**
AI decides whether unsolicited frames become user-visible events/cached state.

### 2A.3 What changes in the rig struct

Instead of storing an `Arc<Mutex<Transport>>` used by methods, store an IO handle:

* `mpsc::Sender<CommandRequest>` (or two senders later for RT/BG)
* `JoinHandle` for the IO task
* optional cancellation token
* optionally, a way to “return transport” on shutdown if you still want that path

### 2A.4 Migration strategy that avoids breaking anything

Do it per backend:

1. Pick one backend (Icom is best because your “transceive” path is already mature)
2. Make IO task spawn unconditional
3. Ensure AI-off path still satisfies all `Rig` trait methods
4. Only then delete/retire `execute_command_direct()` for that backend

**Definition of done for a backend:**

* All `Rig` ops work with AI off
* All `Rig` ops work with AI on
* No method touches transport directly (no direct locking path remains)
* IO task is the single reader/writer authority

---

## Phase 2B — Remove `execute_command_direct()` safely

Once Phase 2A is true for a backend, removing `execute_command_direct()` is mostly cleanup:

### What you delete

* `execute_command_direct()` and any helper logic used only by it
* any “direct mode” branching in builder/connect code

### What you keep / refactor

* the command encoding/decoding logic
* the response correlation rules (now centralized in IO task)
* the `RigEvent` emission logic (gated by AI flag)

### Why removing it is important (practically)

As long as you keep direct mode, you have *two different correctness surfaces*:

* direct path’s buffering, matching, edge cases
* IO task’s buffering, matching, edge cases

That doubles debugging and makes SO2R-grade stability hard.

---

## Phase 3 — Add scheduling (priority + coalescing) once IO task is universal

This is why Phase 2 matters: scheduling is only clean if everything goes through the IO task.

### 3.1 Priority queues (RT vs BG)

Introduce two command channels:

* **RT:** PTT, CW keying, TX/RX switching, “must happen now”
* **BG:** polling reads, meter reads, UI refresh

IO loop uses `select! { biased; }`:

* RT always serviced first

**Definition of done:**

* hammer polling at 20–50Hz and still have PTT/CW respond immediately

### 3.2 Coalescing idempotent reads

For hot reads (`get_frequency`, `get_mode`, etc.):

* If an identical read is already in-flight, **don’t send another wire command**
* Attach multiple waiters to the same pending request
* Fan out the response

**Why:** UI frameworks and panadapters tend to cause accidental stampedes.

**Definition of done:**

* 1,000 concurrent `get_frequency()` calls produce ~1 wire transaction (per time window/in-flight)

### 3.3 Bounded buffers + resync strategy

In IO task decode loop:

* enforce max buffer size
* on overflow or unrecoverable parse, reset buffer and continue
* never allow unbounded growth on malformed input

---

## Phase 4 — SO2R-specific atomic operations

Once scheduling exists, add “sequence/transaction” commands:

### 4.1 Atomic-ish sequences

Example operations:

* set frequency + mode + set TX slice/vfo selection
* set split + set TX offset + confirm state

Represent as:

* a single request that expands into a short list of wire commands
* executed without BG interleaving (optionally still allow RT preemption)

**Definition of done:**

* “change run freq while maintaining correct TX selection” is consistent and low-latency

---

## Phase 5 — Error model upgrades (optional but high ROI)

Move from stringly errors to structured enums:

* `TransportError` (connection lost, timeout, io)
* `ProtocolError` (NAK, parse, unexpected response)
* add context fields (command, model, retry count)

This makes logger UX (and bug triage) much better.

---

## How to think about AI/transceive after these changes

After Phase 2, AI/transceive is no longer “the mode you must enable for the library to work.”
It becomes:

* **Off:** command/response only (no unsolicited event publishing)
* **On:** command/response + unsolicited decode → `RigEvent`s + optional caching

So **removing `execute_command_direct()` does not remove AI-off support**. It removes a second I/O pathway.

---

## Practical “don’t shoot yourself” checklist when removing direct mode

When you migrate a backend to universal IO task, verify these in tests:

1. **No deadlocks:** IO task owns transport; no `Mutex` around transport is required
2. **Response matching:** correct response goes to correct request under unsolicited traffic
3. **Shutdown:** dropping rig aborts/cancels IO task; no hanging read on serial
4. **AI-off correctness:** all core ops work even if unsolicited frames are ignored
5. **Load:** BG polling cannot starve RT ops (once priority is implemented)

---

If you want the next step to be extremely concrete: tell me which backend you’re starting with (Icom is the best first move), and I’ll outline the exact module-level refactor (new structs/enums, where the senders live, how the builder returns a rig, and how each `Rig` method routes to RT vs BG).

