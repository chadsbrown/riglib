You’re right to call that out. The “single IO task owns the transport” decision has **direct implications** for the `Transport` trait and for how drivers hold/consume it. The good news: you *don’t* have to redesign the trait immediately to execute the plan, but you **should** decide what the “post-migration contract” is so you don’t paint yourself into a corner.

Below is the missing piece: **what the `Transport` trait should look like post-migration, what changes are optional vs required, and a migration path that doesn’t explode the codebase.**

---

## 1) What “IO task owns transport” actually implies

### The core invariant

After migration, **there is exactly one task that calls `send()` and `receive()` on a given transport instance.** Everything else communicates with that task via channels.

That means:

* `Arc<Mutex<Box<dyn Transport>>>` becomes an implementation detail you *should be able to delete*, because you no longer need cross-task shared mutable access.
* The `Transport` trait can remain **`&mut self`-based**, which is actually *ideal* for exclusive ownership.
* Most of the “rethinking” is about *whether the trait should become more message/framing oriented*, not about thread safety.

So: **exclusive ownership makes your current trait shape more correct, not less.**

---

## 2) What the Transport trait should look like post-migration (recommended target)

### Keep: `&mut self` I/O methods

Your current approach (conceptually):

* `async fn send(&mut self, data: &[u8]) -> Result<()>`
* `async fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>`

…is consistent with a single owner. I would keep that (with small refinements below).

### Add: connection identity + lifecycle hooks (optional but valuable)

Once transport is in an IO task, you’ll want better logging and better failure semantics:

* `fn kind(&self) -> TransportKind` (Serial/Tcp/Udp)
* `fn peer(&self) -> Option<String>` (COM port or IP:port)
* optional: `async fn close(&mut self)` (best-effort; serial close is implicit but TCP shutdown may matter)

Not required for correctness, but helps ops/debugging.

### Consider: split read/write (only if needed later)

In high-load or UDP-ish worlds, you *might* eventually want a split:

* `Transport::split(self) -> (TransportTx, TransportRx)`

But for CAT/CI-V style request/response, **don’t do this now**. It increases complexity, and you’ll still want serialization at a higher layer for response correlation.

---

## 3) What must change immediately vs later

### Must change to support IO-task-only

**Nothing in the `Transport` trait must change** to adopt IO-task ownership.

What must change is **how it is held**:

* Before: `Arc<Mutex<Box<dyn Transport>>>` stored in rig struct, used directly by methods.
* After: `Box<dyn Transport>` moved into IO task at spawn time.

So the minimal required change is: **the IO task takes ownership of the transport object**.

### Changes that are “nice to have” but not required

* Add `AsyncRead/AsyncWrite` trait bounds: not needed; you’re already abstracting.
* Replace `receive(buf, timeout)` with `read_frame()` in transport: not recommended; framing belongs above transport.
* Make `Transport` `Send + Sync`: not needed if IO task is the only owner (it still must be `Send` to move into a spawned task, but not `Sync`).

---

## 4) How to handle the “disconnect sentinel” pattern post-migration

Right now you sometimes replace transport with `DisconnectedTransport` and keep an `Arc<Mutex<_>>` in the rig.

Post-migration, you can delete that entire category by making the rig hold only an IO handle:

```rust
struct RigIo {
  rt_tx: mpsc::Sender<Request>,
  bg_tx: mpsc::Sender<Request>,
  cancel: CancellationToken,
  task: JoinHandle<()>,
}
```

There is no “transport field” to replace, so no sentinel required.

If you still want a “graceful shutdown that returns the transport” (handy in tests), you can keep a `Shutdown { reply: oneshot::Sender<Box<dyn Transport>> }` request variant. But in practice, many users won’t care about getting the transport back.

---

## 5) The biggest trait-level question: should Transport expose timeouts?

You called out the send/receive interface. The critical design choice is:

### Option A (recommended): `receive` includes timeout

Keep:

```rust
async fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;
```

Pros:

* The transport is the single source of truth for OS-level reads + timeouts
* Drivers/IO task stay consistent (and you can remove all outer `tokio::timeout` wrappers)
* Works well for serial/TCP

Cons:

* Your IO task might want different timeout policies for “idle unsolicited read” vs “in-flight response wait”

  * But you can just pass different durations.

### Option B: no timeout in trait; IO task wraps with `tokio::timeout`

Pros:

* One timeout mechanism (Tokio) everywhere
* Transport stays “dumb” and closer to `AsyncRead`

Cons:

* You must ensure every transport impl is truly non-blocking and cancellation-safe
* Serial libs sometimes behave better with their own timing behavior
* You reintroduce the double-timeout footgun unless you are very disciplined

Given what you already have, I’d stick to **Option A** and enforce “no outer timeouts” as a project rule.

---

## 6) Post-migration “contract” between IO task and Transport

Once IO task owns the transport, the *real* contract is:

1. Transport provides bytes, not frames
2. IO task is responsible for:

   * buffering
   * framing
   * response correlation
   * backpressure / scheduling
3. Transport errors are **terminal** (ConnectionLost) or **recoverable** (Timeout). Define these clearly.

### Practical refinements to the trait to support that contract

I would explicitly codify these in `riglib-transport`:

* `Timeout` should mean “no bytes arrived within duration” and should be recoverable.
* `ConnectionLost` should mean “read/write failed in a way that likely requires reconnect.”
* Avoid ambiguous “Io(String)” errors; keep the original error or categorize it.

That’s more important than changing the shape of `send/receive`.

---

## 7) Migration steps with Transport implications explicitly included

Here’s the updated plan section you asked for (focused on Transport):

### Step 2 (IO task universal) — Transport changes

* **No trait changes required**
* Move `Box<dyn Transport>` into the IO task at spawn time
* Delete `Arc<Mutex<Box<dyn Transport>>>` from rig structs for migrated backends
* Remove `DisconnectedTransport` usage for migrated backends

### Step 3 (priority + coalescing) — Transport changes

* Still none to trait
* IO task becomes the only place that decides read timeouts:

  * short timeout while waiting for in-flight command response
  * longer/shorter idle timeouts for unsolicited polling/drain loops

### Step 4 (structured errors) — Transport changes

* Improve `TransportError` classification
* Keep trait stable, but improve error typing returned by transport methods

### Optional Step (future): `Transport::close()` and `peer()`

* If you want more graceful teardown and better diagnostics, add them later without affecting drivers much.

---

## 8) What I would *not* do (common trap)

Don’t push “frame parsing” down into `Transport`. You’ll be tempted to add:

* `async fn read_frame(&mut self) -> Result<Vec<u8>>`

That seems nice until you realize:

* different protocols frame differently (CI-V, CAT text, Flex TCP status, UDP meters)
* you *want* the IO task to manage buffer reuse and correlation
* you’ll end up reimplementing framing per backend inside transport anyway, defeating the abstraction

Transport should stay bytes-in/bytes-out.

---

## 9) Concrete “target” definitions (so the team knows when Transport work is “done”)

**Transport trait is “done” for the IO-task architecture when:**

* it is `Send` (so you can move it into a spawned task)
* its read method has clear timeout + error semantics
* it never requires shared/mutexed access in normal operation
* it provides enough metadata for logs/debug (optional but helpful)

---

If you want, I can go one level deeper and propose a specific trait signature set (exact Rust code) and a “Transport v2” module that’s backward compatible (keep old trait, implement new over old, migrate gradually). That lets you modernize without forcing a flag-day across all backends.

