# Phase B — Priority Scheduling (RT/BG)

**Depends on:** [Phase A](phase-a-io-task-unification.md)
**Deliverable:** Predictable PTT/CW response under heavy polling load.
**Estimated effort:** 3 sessions (sub-phases B.1 through B.3)

---

## Why This Matters for Contesting

At 35 WPM CW (typical for competitive CQWW or WPX CW), inter-character timing is ~34ms per dit. PTT assertion must happen within a few milliseconds of the operator pressing the foot switch. If a polling loop is reading S-meter values at 20 Hz (50ms per poll), and each poll takes 30-50ms on CI-V, a PTT command queued behind 3 pending S-meter reads could be delayed by 90-150ms. That is audible clipping at the start of a CW transmission.

---

## Sub-Phase B.1 — Split Command Channel into RT/BG  **[COMPLETE]**

**Scope:** Channel plumbing only. Single session.
**Status:** Complete — `RigIo` has `rt_tx`/`bg_tx`, IO loop has biased select (cancel > RT > BG > idle), `handle_request()` helper extracted, 1,878 workspace tests pass, zero warnings.

### Changes

- Change `RigIo` to have two `mpsc::Sender<Request>` fields: `rt_tx` and `bg_tx`.
- Update `spawn_io_task()` to create two channels.
- Update the IO task loop to use `biased` select with RT checked first.
- All rig methods still use `bg_tx` by default (no routing logic yet).

### IO Loop Selection

```rust
loop {
    tokio::select! {
        biased;  // RT always checked first

        _ = cancel.cancelled() => { break; }
        Some(req) = rt_rx.recv() => { /* handle RT command */ }
        Some(req) = bg_rx.recv() => { /* handle BG command */ }
        // ... idle transport read for unsolicited frames
    }
}
```

The `biased` keyword ensures RT commands are always serviced before BG, even if both are pending.

### Definition of Done

- IO task runs and commands complete through both channels.
- No behavioral change yet (everything still routes through BG).

---

## Sub-Phase B.2 — Route Rig Methods to Correct Channel

**Scope:** Method-level routing. Single session. Depends on B.1.

### Routing Rules

**RT (Real-Time) — "must happen now":**
- `set_ptt()` (both CAT and hardware)
- `set_cw_key()` / `send_cw_message()` / `stop_cw_message()`
- `SetLine` (DTR/RTS for hardware PTT/CW)

**BG (Background) — polling and standard operations:**
- All getters: `get_frequency()`, `get_mode()`, `get_signal_strength()`, etc.
- Standard setters: `set_frequency()`, `set_mode()`
- VFO swap, VFO A=B (not time-critical)

### DTR/RTS vs CAT PTT Subtlety

`SetLine` requests for DTR/RTS do NOT involve CI-V framing — they are direct serial line toggling. These should ALWAYS be RT priority because CW break-in (QSK) timing requires sub-10ms latency.

However, if PTT is via CAT (`PttMethod::Cat`), the PTT command IS a CI-V frame (cmd 0x1C, sub 0x00, data 0x01/0x00) and must wait for the transport to finish any in-progress CI-V transaction. The IO task cannot preempt a CI-V frame mid-transmission — it can only prioritize what goes NEXT on the wire.

### SO2R Audio Routing Timing

In SO2R, switching TX focus from Radio 1 to Radio 2 typically requires:
1. Key Radio 2 PTT (RT)
2. Switch audio routing (external to riglib)
3. Change Radio 1 from TX to RX (RT)

If steps 1 and 3 go to different rig instances, this is fine. But if they share a CI-V bus (unusual but possible with IC-7610 main+sub), RT priority ensures PTT operations are not delayed.

### Definition of Done

- Hammer BG polling at 50 Hz, verify RT PTT completes in under 50ms (one CI-V round-trip) even under polling load.
- For DTR/RTS keying, latency from `SetLine` request to mock transport's `set_dtr()` call should be near-zero (microseconds).

---

## Lifecycle

Priority scheduling does not change shutdown requirements. The IO task lifecycle remains explicit:

- Dropping the rig cancels the IO task via `CancellationToken`.
- If cancellation does not promptly unwind (e.g., a blocked read in an OS driver), `JoinHandle::abort()` is an acceptable fallback.
- Tests should verify dropping a rig does not hang the process.

---

## Sub-Phase B.3 — Bounded Buffer + Resync

**Scope:** Safety limits. Single session. Depends on B.1.

### Implementation

- Add `MAX_BUFFER` constant: 4096 bytes for CI-V, 8192 for text protocols.
- On overflow, log a warning, clear the buffer, and continue.
- Never allow unbounded growth on malformed input.

### Definition of Done

- Feed the IO task megabytes of garbage through MockTransport — verify no OOM and clean resync.

---

## Protocol-Specific Gotchas

**CI-V cannot preempt mid-frame.** Once the IO task starts writing a CI-V frame, it must finish. An RT request arriving mid-write must wait. The preemption point is BETWEEN complete command/response cycles, not within them. The `biased` select naturally handles this because the IO task only checks for new requests after completing the current command execution.

**Starvation with aggressive RT use.** If a software bug continuously sends RT requests, BG polling will starve. Consider a fairness heuristic: after serving N consecutive RT requests (e.g., 10), service one BG request if pending. Not needed initially — flag as a potential B.1 enhancement.

---

## Testing Scenarios

1. **PTT latency under load.** Spawn a task sending `get_s_meter()` at 50 Hz. Simultaneously, send `set_ptt(true)`. Measure wall-clock time from PTT request to response. Should be under 100ms even under polling load.

2. **CW key timing.** For DTR/RTS keying, measure time from `SetLine` request to mock transport's `set_dtr()` call. Should be near-zero (microseconds) because it does not involve CI-V framing.

3. **Mixed RT and BG burst.** Send 10 BG commands, then 5 RT commands, then 10 more BG. Verify RT commands complete before the second batch of BG, even if the first BG batch is still in-flight.

---

## Definition of Done (Phase B — all sub-phases)

- Hammer polling at 20-50 Hz and PTT/CW still responds immediately
- No RT command starved by BG traffic
- Buffer overflow does not crash — resync and continue
- Dropping the rig cleanly cancels/aborts the IO task (no hanging reads, no lingering port/socket locks)
