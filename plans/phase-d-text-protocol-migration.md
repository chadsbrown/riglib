# Phase D — Text Protocol Backend Migration (Yaesu, Kenwood, Elecraft)

**Depends on:** [Phase A](phase-a-io-task-unification.md) proven on Icom
**Deliverable:** Yaesu, Kenwood, and Elecraft backends all use the universal IO-task pattern.
**Estimated effort:** 4-5 sessions (sub-phases D.1 through D.4)

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

## Sub-Phase D.1 — Extract Generic Text-Protocol IO Task Module

**Scope:** Shared infrastructure. One session.

Create `crates/riglib-core/src/text_io.rs` (or a shared utility crate) containing the IO task loop parameterized by:

- Frame terminator (`;` for all text protocols)
- AI prefix set (manufacturer-specific)
- Command prefix extraction logic (manufacturer-specific hooks)
- Event emission mapping

This avoids duplicating the reader loop three times. The three text-protocol families are similar enough to share structure but different enough in prefix parsing to need manufacturer-specific hooks.

### Definition of Done

- Generic text IO task compiles and passes unit tests with a mock.
- Parameterized for manufacturer-specific prefix parsing.

---

## Sub-Phase D.2 — Migrate Kenwood Backend

**Scope:** Simplest text-protocol migration. One session. Depends on D.1.

- Wire up the generic text IO task for Kenwood.
- Adapt builder to always spawn IO task.
- Remove direct path.
- Handle 11-digit frequency format.

### Definition of Done

- All existing Kenwood tests pass.
- AI on/off both work through the IO task.

---

## Sub-Phase D.3 — Migrate Elecraft Backend

**Scope:** Similar to Kenwood with extensions. One session. Depends on D.1.

- Wire up generic text IO task for Elecraft.
- Handle K3/K4 extended command prefixes in prefix extraction.
- K4 supports direct Ethernet (TCP) — verify IO task works identically since `Transport::receive()` abstracts the physical layer.

### Definition of Done

- All existing Elecraft tests pass.
- K3/K4 extended commands work through IO task.

---

## Sub-Phase D.4 — Migrate Yaesu Backend

**Scope:** Largest text-protocol backend. One session, possibly two if test migration is extensive. Depends on D.1.

- Handle 9-digit frequency format (vs Kenwood/Elecraft 11-digit).
- Handle `MD0`/`MD1` prefix absorption (the DIGIT_SUFFIX_PREFIXES logic in `extract_command_prefix`).
- Validate AI mode per model family (some models don't support it).
- Largest command set and most test coverage to validate.

### Definition of Done

- All existing Yaesu tests pass (336+ tests).
- AI on/off both work through IO task.

---

## Suggested Migration Order

1. **Kenwood** — simplest, shares most structure
2. **Elecraft** — similar to Kenwood with K extensions
3. **Yaesu** — largest command set, most models, most test coverage

---

## Protocol-Specific Gotchas

**`?;` error is not always 2 bytes.** Some Yaesu models send standalone `?;`. Others may echo the command prefix before `?;` (e.g., `FA?;`). The protocol decoder should handle both forms.

**Kenwood AI mode stale data.** When AI mode is first enabled, some Kenwood rigs send a burst of stale state from internal buffers. The IO task should not treat these as responses to pending commands. Response correlation by prefix handles this, but test specifically.

**Elecraft K4 Ethernet.** Same text protocol over TCP instead of serial. IO task works identically since Transport abstracts the physical layer. Worth adding a test with mock TCP.

**SetCommandMode divergence.** The `SetCommandMode::NoVerify` pattern with 50ms drain is Yaesu-specific. Kenwood and Elecraft have similar fire-and-forget SET behavior but may need different drain timing. The generic text IO task should accept `SetCommandMode` as a configuration parameter.

---

## Testing Scenarios

1. **Yaesu AI mode burst.** Enable AI, rapidly turn VFO knob. Mock injects 10+ `FA` frames in quick succession. Verify all emitted as events, no buffer overflow.

2. **Mixed AI and command responses.** Send `FA;` (get frequency), mock returns `MD02;` (unsolicited mode change) then `FA014074000;` (response). Verify mode change becomes event, frequency response returned to caller.

3. **SetCommandMode::NoVerify with `?;` error.** Send SET in NoVerify mode, mock returns `?;`. Verify error caught and returned, not swallowed.

4. **Frequency digit format.** Verify `FA00014074000;` (11 digits) parses for Kenwood and `FA014074000;` (9 digits) parses for Yaesu.

---

## Definition of Done (Phase D — all sub-phases)

Per backend:
- All `Rig` ops work with AI off
- All `Rig` ops work with AI on
- No method touches transport directly
- IO task is the single reader/writer authority
- All existing tests pass or are migrated
- Dropping the rig cleanly shuts down the IO task
