# Phase A — IO Task Unification (Icom First)

**Deliverable:** Icom backend works with AI on/off, all ops route through a single IO task. No method touches the transport directly.

**Estimated effort:** 6 sessions (sub-phases A.1 through A.6, each independently committable)

---

## Sub-Phase A.1 — Timeout Unification ✅

**Status: COMPLETE** — Removed 11 double-timeout sites across 8 files (icom, yaesu, kenwood, elecraft rig.rs + transceive.rs). All 1,850+ workspace tests pass, clippy clean.

**Scope:** Standalone cleanup, no structural change. Single session.

### Problem

Many drivers wrap `Transport::receive(timeout)` inside another `tokio::time::timeout()`. For example, in `crates/riglib-icom/src/transceive.rs`:

```rust
match tokio::time::timeout(
    config.command_timeout,
    transport.receive(&mut buf, config.command_timeout),
)
```

This is actively harmful. If the transport's internal timeout fires first, you get `Err(Error::Timeout)`. If the tokio timeout fires first, you get `Err(Elapsed)`. The calling code handles both but with different branches, creating ambiguous behavior.

### What To Do

- Make `Transport::receive(buf, timeout)` the **only** timeout authority.
- Remove outer `tokio::timeout(...)` wrappers in the drivers' direct paths and IO-task paths.
- Enforce "no outer timeouts" as a project rule.

### Safety Net for USB Disconnect

Some serial transport implementations on Linux (particularly FTDI USB-serial adapters using `tokio-serial`) can hang on `read()` if the device is unplugged while a read is pending. Rather than keeping double timeouts, add a per-request deadline enforced in the IO task loop as a `tokio::select!` branch with `tokio::time::sleep` for the overall command deadline. This is cleaner than wrapping every transport call.

### Definition of Done

- Timeouts all originate from a single place (the transport layer).
- Logs/errors consistently report "timeout" in one form.
- Existing tests pass unchanged.

---

## Sub-Phase A.2 — IO Types (Request/Response/RigIo/IoConfig)

**Scope:** Type definitions only, no behavioral changes. Single session. Depends on A.1.

### What To Create

Define new types in `crates/riglib-icom/src/io.rs`:

```rust
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;
use riglib_core::transport::Transport;

use crate::civ::{CivFrame};

/// Configuration for the IO task.
pub(crate) struct IoConfig {
    pub civ_address: u8,
    pub ai_enabled: bool,
    pub command_timeout: Duration,
    pub auto_retry: bool,
    pub max_retries: u32,
    pub collision_recovery: bool,
}

/// A command request sent from rig methods to the IO task.
pub(crate) enum Request {
    /// A CI-V command with expected response.
    CivCommand {
        cmd_bytes: Vec<u8>,
        reply: oneshot::Sender<Result<CivFrame>>,
    },
    /// Fire-and-forget ACK-only command (SET operations).
    CivAckCommand {
        cmd_bytes: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Toggle a serial control line (DTR/RTS for hardware PTT/CW).
    SetLine {
        dtr: bool,
        on: bool,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Graceful shutdown; returns the transport for test recovery.
    Shutdown {
        reply: oneshot::Sender<Box<dyn Transport>>,
    },
}

/// Handle to the IO task. Stored inside IcomRig.
pub(crate) struct RigIo {
    /// Command channel (single for now; Phase B splits into rt_tx/bg_tx).
    pub cmd_tx: mpsc::Sender<Request>,
    /// Cancellation token for graceful shutdown.
    pub cancel: CancellationToken,
    /// Join handle for the IO task.
    pub task: JoinHandle<()>,
}

impl RigIo {
    /// Send a CI-V command and await the response.
    pub async fn command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<CivFrame> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(Request::CivCommand {
                cmd_bytes: cmd,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Error::NotConnected)?;

        // Safety-net timeout: command_timeout + 500ms for channel overhead.
        // The IO task enforces the real transport-level timeout internally.
        match tokio::time::timeout(timeout + Duration::from_millis(500), reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::NotConnected),
            Err(_) => Err(Error::Timeout),
        }
    }
}
```

### Design Notes

- `SetLine` for DTR/RTS is separate from `CivCommand` because hardware PTT/CW keying bypasses CI-V framing entirely — it's direct serial line toggling with sub-10ms latency requirements.
- Channel capacity of 32 recommended (vs current 16) to handle rapid sequential commands without backpressure.
- `Shutdown` variant enables test recovery of the transport.

### Definition of Done

- Types compile, unit tests for Request/Response construction pass.
- No behavioral changes yet — `spawn_io_task()` skeleton compiles but is not wired in.

---

## Sub-Phase A.3 — IO Task Loop Implementation

**Scope:** The core IO loop. This is the most complex sub-phase. One full session. Depends on A.2.

### Implementation

```rust
/// Spawn the IO task. Returns the handle and an event broadcast sender.
pub(crate) fn spawn_io_task(
    transport: Box<dyn Transport>,
    config: IoConfig,
    event_tx: broadcast::Sender<RigEvent>,
) -> RigIo {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Request>(32);
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    let task = tokio::spawn(io_loop(transport, config, event_tx, cmd_rx, cancel_clone));

    RigIo { cmd_tx, cancel, task }
}

async fn io_loop(
    mut transport: Box<dyn Transport>,
    config: IoConfig,
    event_tx: broadcast::Sender<RigEvent>,
    mut cmd_rx: mpsc::Receiver<Request>,
    cancel: CancellationToken,
) {
    let mut idle_buf = Vec::new();
    const MAX_IDLE_BUF: usize = 4096;

    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                tracing::debug!("IO task cancelled");
                break;
            }

            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(Request::CivCommand { cmd_bytes, reply }) => {
                        let result = execute_civ_command(
                            &mut *transport, &cmd_bytes, &config,
                            if config.ai_enabled { Some(&event_tx) } else { None },
                        ).await;
                        let _ = reply.send(result);
                    }
                    Some(Request::CivAckCommand { cmd_bytes, reply }) => {
                        let result = execute_civ_ack_command(
                            &mut *transport, &cmd_bytes, &config,
                        ).await;
                        let _ = reply.send(result);
                    }
                    Some(Request::SetLine { dtr, on, reply }) => {
                        let result = if dtr {
                            transport.set_dtr(on).await
                        } else {
                            transport.set_rts(on).await
                        };
                        let _ = reply.send(result);
                    }
                    Some(Request::Shutdown { reply }) => {
                        tracing::debug!("IO task shutdown requested");
                        let _ = reply.send(transport);
                        return;
                    }
                    None => {
                        tracing::debug!("all command senders dropped, exiting IO task");
                        break;
                    }
                }
            }

            // Idle: read unsolicited data from the bus.
            _ = async {
                let mut buf = [0u8; 256];
                match transport.receive(&mut buf, Duration::from_millis(100)).await {
                    Ok(n) if n > 0 => {
                        idle_buf.extend_from_slice(&buf[..n]);
                        if idle_buf.len() > MAX_IDLE_BUF {
                            tracing::warn!(len = idle_buf.len(), "idle buffer overflow, resetting");
                            idle_buf.clear();
                            return;
                        }
                        if config.ai_enabled {
                            process_idle_frames(&mut idle_buf, config.civ_address, &event_tx);
                        } else {
                            // Drain complete frames to prevent buffer growth,
                            // but don't emit events.
                            drain_idle_frames(&mut idle_buf);
                        }
                    }
                    _ => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            } => {}
        }
    }
}
```

### CI-V Protocol Details To Handle

**Echo behavior varies by configuration:**
- On USB-connected rigs (IC-7610, IC-7851), USB echo is typically OFF by default — no echo frame.
- On rigs with CI-V USB Echo Back enabled, or rigs on a true CI-V bus, every sent byte appears in the receive buffer.
- The IO task must handle both: zero echoes and full echoes. If an echo arrives, skip it and continue reading for the actual response.

**Dual-watch on IC-7610:**
- Two independent CI-V ports (Main and Sub), both default to address 0x98.
- Sub-receiver commands use 0xD0 (select main) / 0xD1 (select sub).
- Transceive broadcasts from the sub receiver use the same source address.
- Response correlation must track which receiver context is active.

**CI-V address 0x00 (wildcard):**
- Some users configure address 0x00, meaning the rig accepts commands to any address.
- The IO task must not use 0x00 as a source match filter — it's the broadcast address used in transceive frames.

**Collision recovery:**
- Current backoff: `20ms * attempt` (adequate for single-rig setups).
- On a real CI-V bus with multiple controllers, add jitter: `20ms * attempt + rand(0..10ms)`.
- Not critical for Phase A but worth noting for Phase F (SO2R with two rigs on one bus).

### Definition of Done

- IO task loop handles: command/response, echo skip, unsolicited frames (AI on), frame drain (AI off), collision retry, NAK detection, timeout.
- White-box tests using `spawn_io_task()` directly with MockTransport pass.

---

## Sub-Phase A.4 — Builder Wiring

**Scope:** Wire builder to always spawn IO task. Single session. Depends on A.3.

### Current State

- `Builder::build()` creates a rig with `Arc<Mutex<Box<dyn Transport>>>`
- `enable_transceive()` optionally spawns a reader task and swaps the transport for a sentinel
- `build_with_transport(transport)` exists as a test hook

### Target State

```rust
pub async fn build(self) -> Result<IcomRig> {
    let transport: Box<dyn Transport> = Box::new(SerialTransport::open(...).await?);
    self.build_with_transport(transport).await
}

pub async fn build_with_transport(self, transport: Box<dyn Transport>) -> Result<IcomRig> {
    let (io, event_rx) = spawn_io_task(transport, IoConfig {
        ai_enabled: self.ai_enabled,
        ...
    });
    Ok(IcomRig { io, event_rx, info: ..., caps: ... })
}
```

### AI Mode Knobs

- `Builder::enable_ai()` / `enable_transceive()` no longer spawns a task. It only sets `ai_enabled: bool` in `IoConfig`.
- Optional additional knobs: `event_buffer_size(n)`, `unsolicited_policy(...)` (ignore / emit / emit+cache)

### Event Receiver Pattern

IO task owns the `broadcast::Sender<RigEvent>`. Rig clones the sender or holds it; `subscribe()` returns `sender.subscribe()`. Builder does not need to return an event receiver.

### Notes

- Keep `execute_command_direct()` temporarily as dead code (do not delete yet).
- `start_transceive()` becomes a no-op or sends a config-change message to the IO task.
- Remove `transceive_handle: Mutex<Option<TransceiveHandle>>` — it becomes `IoConfig.ai_enabled`.

### Definition of Done

- All existing black-box tests adapted to work through the IO task.
- Both `build()` and `build_with_transport()` spawn the IO task unconditionally.

---

## Sub-Phase A.5 — Delete Direct Path

**Scope:** Remove `execute_command_direct()` and all direct-mode branching. Single session. Depends on A.4.

### What To Delete

- `execute_command_direct()` and any helper logic used only by it
- The `Arc<Mutex<Box<dyn Transport>>>` field from the rig struct
- The `DisconnectedTransport` sentinel pattern
- Any "direct mode" branching in builder/connect code
- `enable_transceive()` / `start_transceive()` as separate lifecycle methods

### What To Keep/Refactor

- Command encoding/decoding logic (unchanged)
- Response correlation rules (now centralized in IO task)
- `RigEvent` emission logic (gated by AI flag in IoConfig)

### Why This Matters

As long as direct mode exists, you have two different correctness surfaces (direct path buffering/matching vs IO task buffering/matching). That doubles debugging and makes SO2R-grade stability hard.

### Definition of Done

- Full parity test — every existing test must pass without the direct path.
- No method touches transport directly; no direct locking path remains.

---

## Sub-Phase A.6 — Lifecycle Hygiene (CancellationToken + Drop)

**Scope:** Clean shutdown. Single session. Depends on A.5 (or can parallel with A.5).

### Implementation

- Add `CancellationToken` to `RigIo` (already in the struct sketch).
- Implement `Drop` for `IcomRig` (or a wrapper) that triggers cancellation.
- Add `Shutdown` request variant that returns the transport for test recoverability.
- Abort as fallback if cancellation doesn't complete within a short deadline.

### Definition of Done

- Dropping the rig does not hang the process.
- Serial ports are not left "busy" due to lingering tasks.
- Test for drop-during-command: start a long-timeout command, drop the rig, verify clean exit.

---

## Transport Contract

### Core Invariant

After migration, exactly one task calls `send()` and `receive()` on a given transport instance. Everything else communicates with that task via channels.

### What Changes In The Trait

**Nothing.** The `&mut self` shape is ideal for exclusive ownership — it becomes *more* correct, not less. The Transport trait already requires `Send + Sync` (though `Sync` is not strictly needed post-migration since only one task owns the transport).

The minimal required change is how the transport is held:

- **Before:** `Arc<Mutex<Box<dyn Transport>>>` stored in rig struct, used directly by methods
- **After:** `Box<dyn Transport>` moved into IO task at spawn time

### Timeout Design (Option A — recommended)

Keep timeout in `receive()`:

```rust
async fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;
```

The transport is the single source of truth for OS-level reads + timeouts. IO task passes different durations for different situations (short timeout for in-flight response wait, longer for idle unsolicited polling).

### Post-Migration Contract

1. Transport provides **bytes, not frames**
2. IO task is responsible for buffering, framing, response correlation, backpressure/scheduling
3. Transport errors are **terminal** (`ConnectionLost`) or **recoverable** (`Timeout`) — define these clearly

### What NOT To Do

Do not push frame parsing into `Transport`. Different protocols frame differently (CI-V, CAT text, Flex TCP status, UDP meters). Transport stays bytes-in/bytes-out.

### USB-Serial Adapter Quirks (Linux)

- **FTDI (FT232R, FT231X):** Most common for CI-V cables. Buffers up to 64 bytes before flushing; internal latency timer can delay delivery by up to 16ms. At 19200 baud, a 10-byte CI-V frame takes ~5.2ms to transmit. Relevant for timeout tuning.
- **Prolific PL2303:** Known for driver issues on recent kernels. Can occasionally drop bytes or deliver partial reads.
- **CP2102 (Silicon Labs):** Generally reliable, limited to 921600 baud max.
- **Device path stability:** `/dev/ttyUSB0` can reassign after unplug/replug. IO task should produce `ConnectionLost` rather than hanging.

---

## Test Strategy

### Key Principle

`build_with_transport(MockTransport)` survives as the test entry point. Post-migration, it spawns the IO task using that mock transport.

### Style A — Black-Box Driver Tests (preferred)

Test the public rig API (`set_frequency`, `get_frequency`, etc.) against a scripted MockTransport.

**Test flow:**
1. Create MockTransport with expected writes and scripted reads
2. `let rig = Builder::new(...).build_with_transport(Box::new(mock)).await?;`
3. Call `rig.get_frequency(VFO_A).await?;`
4. Assert: returned value correct, mock write expectations satisfied, no extra writes

**Important:** Tests run on Tokio; await the rig call (which awaits the oneshot response); the IO task progresses because it's spawned on the runtime.

### Style B — IO-Task Unit Tests (white-box, targeted)

Directly test the IO loop with a MockTransport and explicit command requests. Useful for correlation behavior, priority scheduling, and coalescing correctness.

Export `spawn_io_task()` that returns `IoHandle { rt_tx, bg_tx, ... }` and a `JoinHandle`. Add a `Shutdown` request variant for clean test teardown.

### MockTransport Enhancements Needed

The current MockTransport is synchronous expect/respond. For IO-task-mediated testing, add:

- **Unsolicited frame injection:** `inject_unsolicited(bytes)` that prepends data to the next `receive()` call.
- **Non-blocking idle read support:** `receive()` returns `Err(Timeout)` when no pending response exists (already works, but verify the IO task's idle branch handles this correctly).
- **Multi-frame responses:** Ability to return echo frame + unsolicited frame + response frame as separate `receive()` calls, simulating real serial timing.

Consider a `ScriptedMockTransport` variant using `mpsc::Receiver<Vec<u8>>` internally, allowing tests to feed bytes asynchronously.

### Critical Test Scenarios

1. **AI-off, no echo (IC-7300 USB, echo disabled):** MockTransport sends only the response frame. IO task returns correct CivFrame.

2. **AI-off, with echo (IC-7300 USB, echo enabled):** MockTransport sends echo + response. IO task skips echo, returns response.

3. **AI-on, interleaved unsolicited frame:** MockTransport sends: echo, unsolicited freq change, response. IO task emits `FrequencyChanged` event AND returns correct response.

4. **Collision detection and retry:** MockTransport sends echo with 0xFC collision byte. IO task retries. On retry, MockTransport sends clean response.

5. **NAK response:** Rig returns NAK (0xFA). IO task returns error.

6. **Drop during command:** Start a long-timeout command, drop `IcomRig`. IO task exits cleanly via CancellationToken.

7. **Rapid sequential commands:** 100 `get_frequency` commands in tight loop. All complete without deadlock or channel backup.

### Testing Unsolicited Behavior (AI on vs off)

- **AI off:** unsolicited frame arrives before response. Response should still match the command. No event emitted.
- **AI on:** same setup. Response matches command. Unsolicited frame becomes a `RigEvent`.

---

## Verification Checklist

When migrating a backend to the universal IO task, verify in tests:

1. **No deadlocks:** IO task owns transport; no `Mutex` around transport is required
2. **Response matching:** correct response goes to correct request under unsolicited traffic
3. **Shutdown:** dropping rig aborts/cancels IO task; no hanging read on serial
4. **AI-off correctness:** all core ops work even if unsolicited frames are ignored
5. **Load:** BG polling cannot starve RT ops (once priority is implemented in Phase B)

---

## Definition of Done (Phase A — all sub-phases)

- All `Rig` ops work with AI off (Icom backend)
- All `Rig` ops work with AI on (Icom backend)
- No method touches transport directly (no direct locking path remains)
- IO task is the single reader/writer authority
- Dropping the rig cleanly shuts down the IO task
- All existing Icom tests pass or are migrated
