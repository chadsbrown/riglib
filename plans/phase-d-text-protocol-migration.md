# Phase D — Text Protocol Backend Migration (Yaesu, Kenwood, Elecraft)

**Depends on:** [Phase A](phase-a-io-task-unification.md) proven on Icom, [Phase B](phase-b-priority-scheduling.md) for RT/BG pattern
**Deliverable:** Yaesu, Kenwood, and Elecraft backends all use the universal IO-task pattern with RT/BG priority scheduling.
**Estimated effort:** 5-6 sessions (sub-phases D.1 through D.4)

---

## Current State (Pre-Migration)

All three crates share an identical architecture that predates the Icom IO task work:

- **Transport access:** `Arc<Mutex<Box<dyn Transport>>>` — shared mutable access via async mutex
- **Dual path:** `execute_command_direct()` (AI off) and `execute_command_via_channel()` (AI on)
- **Transceive:** Per-crate `transceive.rs` with `TransceiveHandle`, `CommandRequest` enum, background reader task — nearly identical across all three
- **`DisconnectedTransport` sentinel:** Real transport is swapped out when entering AI mode, replaced with a dummy that returns `NotConnected`

After migration, all of this is replaced by the IO task pattern proven in Phases A and B.

---

## Protocol Comparison

These three families share semicolon-terminated ASCII but differ in important ways:

| Feature | Yaesu NewCAT | Kenwood Standard | Elecraft Extended |
|---------|-------------|-------------------|-------------------|
| AI mode command | `AI2;` (full updates) | `AI2;` (auto-info) | `AI2;` (same as Kenwood) |
| AI disable | `AI0;` | `AI0;` | `AI0;` |
| Frequency digits | 9 digits (`FA014074000;`) | 11 digits (`FA00014074000;`) | 11 digits (`FA00014074000;`) |
| Mode per VFO | `MD0x;` / `MD1x;` | `MD x;` (active VFO) | `MD x;` (active VFO) |
| SET response | Silent (no echo) | Silent (no echo) | Silent (no echo) |
| Error response | `?;` | `?;` | `?;` |
| RIT format | `RT0x;` / `RT1;` | `RT x;` | `RT x;` |
| Prefix length | 2 chars typically | 2 chars typically | 2 chars + K extensions |

### Models Without AI Mode

The FT-891, FT-818, and FT-857D do NOT support AI mode. They use older "CAT" protocol without auto-information. The IO task must work correctly in AI-off mode for these models. The `has_transceive` flag in `RigCapabilities` should gate whether AI mode is even attempted.

### Elecraft K3/K4 Extended Commands

Commands not in Kenwood standard: `K3;`/`K4;` (identify), `BN;`/`BN$;` (band number for contest band switching), `SWR;` (real-time SWR), `FW;` (filter width in Hz), `TQ;` (TX query). All follow `;`-terminated format and work transparently with the IO task.

### Kenwood TS-890/TS-990

TS-990 has dual-receiver (`DC 00;`/`DC 01;` for main/sub). TS-890 has enhanced AI mode with more parameter updates. Both support 115200 baud over USB.

### Yaesu FTDX101D (SO2R)

Dual-receiver addressable via `FA`/`FB` and separate `MD0`/`MD1`. Already handled by the AI_PREFIXES constant in `transceive.rs`.

---

## Module Location Decision

The shared text IO task module should live in a new `riglib-text-io` crate rather than in `riglib-core`. Rationale: `riglib-core` is currently protocol-agnostic (traits, types, error types). Adding text-protocol-specific IO code there would blur that boundary. A separate crate keeps concerns clean and parallels `riglib-icom` having its own `io.rs`.

Alternative: `riglib-core/src/text_io.rs` — simpler dependency graph but muddies the core crate's purpose. Either works; decide before starting D.1.

---

## Sub-Phase D.1a — Text-Protocol IO Task Core ✅ COMPLETE

**Scope:** IO task loop, types, and command execution. One session.

Create the text-protocol IO task module with the core command/response engine:

- **`TextProtocolConfig` struct** — parameterizes the IO task for manufacturer differences:

```rust
pub struct TextProtocolConfig {
    /// AI prefix set for unsolicited frame classification.
    pub ai_prefixes: &'static [&'static str],
    /// Prefixes that include a trailing digit (Yaesu: MD0, MD1, RT0, XT0).
    pub digit_suffix_prefixes: &'static [&'static str],
    /// Drain timeout for NoVerify SET commands (50ms for all current protocols).
    pub set_drain_timeout: Duration,
}
```

- **Request enum** — `CatCommand`, `SetCommand`, `SetLine`, `Shutdown` (no `CivAckCommand` equivalent needed; text SET commands are silent, so ACK semantics are handled by drain/timeout)
- **`RigIo` handle struct** — same shape as Icom: `rt_tx`, `bg_tx`, `cancel`, `task`
- **RT/BG two-channel dispatch** — carry forward the Phase B pattern from Icom. The `biased select!` priority ordering is identical: cancel > RT > BG > idle.
- **`io_loop`** — command execution (send command, read until `;` terminator, correlate response by prefix), `?;` error detection, NoVerify drain logic, idle frame processing for unsolicited AI responses.
- **Bounded buffer** — `MAX_BUF` (8192 for text protocols, per Phase B.3) guards both idle and response buffers. Overflow → warn → clear → retry.
- **`CancellationToken` lifecycle** — drop rig → cancel token → IO task exits.

### What Does NOT Translate from Icom

- CI-V framing, echo skipping, collision detection — not applicable to point-to-point text serial
- BCD encoding — text protocols use ASCII digits
- ACK frame concept — text SET commands are silent (success = no `?;` within drain window)

### What DOES Translate

- Two-channel dispatch (RT > BG) with `biased select!`
- `CancellationToken` lifecycle
- Bounded buffers with overflow → warn → clear → retry
- Idle frame processing for unsolicited AI responses
- Response correlation by command prefix

### Definition of Done

- IO task compiles and handles basic command/response exchanges with MockTransport.
- `RigIo` methods (`command`, `set_command`, `rt_*`, `set_line`) work through both channels.
- `TextProtocolConfig` accepted and wired through.

---

## Sub-Phase D.1b — Text-Protocol IO Task Tests and Edge Cases ✅ COMPLETE

**Scope:** Test suite and edge-case validation. One session. Depends on D.1a.

Build comprehensive tests for the generic text IO task:

- **RT priority ordering** — pre-fill BG with polling commands, inject RT PTT, verify RT completes first (same pattern as Icom `io_task_rt_priority_over_bg`).
- **Buffer overflow resync** — feed oversized garbage, verify warning + clear + successful retry.
- **Mixed AI and command responses** — send GET command, mock injects unsolicited AI frame before the response, verify AI frame becomes event and response returns to caller.
- **`?;` error in Verify and NoVerify modes** — verify error detected and returned in both paths.
- **NoVerify drain timeout** — verify timeout-as-success semantics (no `?;` within drain window = success).
- **Shutdown lifecycle** — verify cancel token stops the IO task cleanly.
- **SetLine (DTR/RTS)** — verify RT path for hardware PTT/CW keying.

### Definition of Done

- All edge-case tests pass with MockTransport.
- RT/BG priority channels verified under simulated polling load.
- Buffer overflow produces warning and clean resync, not OOM.
- Ready for per-manufacturer wiring in D.2-D.4.

---

## Sub-Phase D.2 — Migrate Kenwood Backend

**Scope:** Simplest text-protocol migration. One session. Depends on D.1.

- Provide `TextProtocolConfig` for Kenwood (11-digit freq, bare alphabetic prefixes, `["FA", "FB", "MD", "TX", "RT", "XT"]` AI prefixes).
- Wire up the generic text IO task in Kenwood builder (`build()` and `build_with_transport()`).
- Replace `execute_command_direct()` / `execute_command_via_channel()` with `RigIo::command()` / `RigIo::rt_*()`.
- Route PTT/CW call sites to RT channel (same 7 call sites as Icom Phase B.2).
- **Delete:** `transceive.rs`, `execute_command_direct()`, `execute_set_command_direct()`, `DisconnectedTransport` usage, `TransceiveHandle`.
- Migrate existing tests (~98 test functions) to work against the IO task path.

### Definition of Done

- All existing Kenwood tests pass.
- AI on/off both work through the IO task.
- No direct transport access remains in rig methods.
- `transceive.rs` deleted.

---

## Sub-Phase D.3 — Migrate Elecraft Backend

**Scope:** Similar to Kenwood with extensions. One session. Depends on D.1.

- Provide `TextProtocolConfig` for Elecraft (same as Kenwood — 11-digit freq, bare alphabetic prefixes).
- K3/K4 extended command prefixes (`K3`, `K4`, `BN`, `SWR`, `FW`, `TQ`) are standard `;`-terminated and need no special IO handling — they work transparently with prefix-based response correlation.
- K4 supports direct Ethernet (TCP) — verify IO task works identically since `Transport::receive()` abstracts the physical layer.
- Route PTT/CW call sites to RT channel.
- **Delete:** `transceive.rs`, direct path, `DisconnectedTransport` usage.
- Migrate existing tests (~116 test functions).

### Definition of Done

- All existing Elecraft tests pass.
- K3/K4 extended commands work through IO task.
- No direct transport access remains.
- `transceive.rs` deleted.

---

## Sub-Phase D.4 — Migrate Yaesu Backend

**Scope:** Largest text-protocol backend. One session, possibly two if test migration is extensive. Depends on D.1.

- Provide `TextProtocolConfig` for Yaesu (9-digit freq, digit-suffix prefixes `["MD", "RM", "SM", "SH", "NA", "AN", "PA", "RA"]`, AI prefixes `["FA", "FB", "MD0", "MD1", "TX", "RT0", "XT0"]`).
- Handle `MD0`/`MD1` prefix absorption (the DIGIT_SUFFIX_PREFIXES logic in `extract_command_prefix`).
- Validate AI mode per model family (some models don't support it — `has_transceive` flag gates this).
- Route PTT/CW call sites to RT channel.
- **Delete:** `transceive.rs`, direct path, `DisconnectedTransport` usage.
- Migrate existing tests (~82 test functions).

### Definition of Done

- All existing Yaesu tests pass.
- AI on/off both work through IO task.
- No direct transport access remains.
- `transceive.rs` deleted.

---

## Suggested Migration Order

1. **Kenwood** — simplest, shares most structure with the generic module
2. **Elecraft** — nearly identical to Kenwood, adds K extensions
3. **Yaesu** — largest command set, most models, digit-suffix prefix complexity

Kenwood and Elecraft are similar enough that D.2 and D.3 could potentially be combined into a single session, but keeping them separate is safer and keeps each session focused.

---

## Protocol-Specific Gotchas

**`?;` error is not always 2 bytes.** Some Yaesu models send standalone `?;`. Others may echo the command prefix before `?;` (e.g., `FA?;`). The protocol decoder should handle both forms.

**Kenwood AI mode stale data.** When AI mode is first enabled, some Kenwood rigs send a burst of stale state from internal buffers. The IO task should not treat these as responses to pending commands. Response correlation by prefix handles this, but test specifically.

**Elecraft K4 Ethernet.** Same text protocol over TCP instead of serial. IO task works identically since Transport abstracts the physical layer. Worth adding a test with mock TCP.

**SetCommandMode drain timing.** All three protocols currently use 50ms drain for NoVerify SET commands. This is a parameter on `TextProtocolConfig` in case it needs per-manufacturer tuning, but the initial value is 50ms for all.

**DisconnectedTransport goes away.** The current pattern of swapping the real transport with a `DisconnectedTransport` sentinel when entering AI mode is eliminated. The IO task owns the transport permanently — AI mode becomes a behavior toggle (emit unsolicited events), not a transport ownership change.

---

## Test Strategy

Each crate has significant test coverage (Yaesu ~82, Kenwood ~98, Elecraft ~116 — ~296 total). Tests should remain per-crate to validate each manufacturer's `TextProtocolConfig` produces correct behavior. The generic text IO module gets its own unit tests with MockTransport for:

- RT priority ordering under BG load
- Buffer overflow resync
- Mixed AI and command response interleaving
- `?;` error detection in both Verify and NoVerify modes

Per-crate tests migrate from `execute_command_direct()` expectations to IO-task-based expectations. The mock setup changes (expectations go through spawn_io_task instead of direct transport calls) but the assertions stay the same.

---

## Testing Scenarios

1. **Yaesu AI mode burst.** Enable AI, rapidly turn VFO knob. Mock injects 10+ `FA` frames in quick succession. Verify all emitted as events, no buffer overflow.

2. **Mixed AI and command responses.** Send `FA;` (get frequency), mock returns `MD02;` (unsolicited mode change) then `FA014074000;` (response). Verify mode change becomes event, frequency response returned to caller.

3. **SetCommandMode::NoVerify with `?;` error.** Send SET in NoVerify mode, mock returns `?;`. Verify error caught and returned, not swallowed.

4. **Frequency digit format.** Verify `FA00014074000;` (11 digits) parses for Kenwood and `FA014074000;` (9 digits) parses for Yaesu.

5. **RT PTT under polling load.** Same as Phase B.2 test scenario — hammer BG polling at 50 Hz, verify RT PTT completes promptly.

---

## Definition of Done (Phase D — all sub-phases)

Per backend:
- All `Rig` ops work with AI off
- All `Rig` ops work with AI on
- No method touches transport directly
- IO task is the single reader/writer authority
- RT/BG priority scheduling active (PTT/CW through RT channel)
- Bounded buffers guard against overflow
- `transceive.rs` and direct-path code deleted
- `DisconnectedTransport` pattern eliminated
- All existing tests pass or are migrated
- Dropping the rig cleanly shuts down the IO task
