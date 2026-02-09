# Phase E — Flex Backend Alignment

**Depends on:** [Phase A](phase-a-io-task-unification.md) conventions established
**Deliverable:** Flex backend follows the same IO-task-based conventions, with builder and test patterns aligned.
**Estimated effort:** 3 sessions (sub-phases E.1 through E.3)

---

## Why Flex Is Different

Flex is not a serial-port request/response protocol. It uses:

- **TCP** for command/control with `\n`-terminated ASCII status lines
- **UDP** for meters (VITA-49 packets)
- A **shared state model** (`RadioState`) updated continuously from incoming messages

For Flex, **the IO task model is already the natural design.** There is no "direct mode" to remove. The goal is alignment with project conventions, not a fundamental architecture change.

---

## SmartSDR Connection Lifecycle

1. **Discovery** — UDP broadcast on port 4992. Radio responds with VITA-49 discovery packet (IP, model, serial, available client slots).
2. **TCP Connect** — Connect to port 4992 on the radio's IP.
3. **Handshake** — Exchange version info, register as client, receive client handle.
4. **Subscription** — Send `sub slice all`, `sub meter all`, `sub tx all` to receive status updates.
5. **Slice creation** — FlexRadio requires explicit `slice create` commands. Unlike traditional VFO A/B, slices are dynamic.

### SmartLink Remote Operation

SmartLink allows operation over the internet via a relay server. Latency can be 50-200ms. The IO task's timeout values should be configurable for remote vs local. `DEFAULT_COMMAND_TIMEOUT` of 2 seconds is reasonable for remote.

### Multi-Client Operation

FlexRadio supports 2-4 connected clients depending on model. Each client has its own slice assignments. The IO task must handle `client disconnected` status messages gracefully — another client may steal a slice.

---

## VITA-49 Meter Data

Meter packets (class code 0x8002) contain packed 16-bit signed values. Meter IDs are assigned dynamically at runtime and must be mapped via `meter list` responses.

Contest-relevant meters:
- S-meter (per slice)
- TX power forward
- SWR
- ALC level
- Compression meter
- Mic level

UDP packets can arrive out of order. The `packet_count` field in the VITA-49 header can detect this, but meter values should always be overwritten with the latest regardless of order.

---

## Target Architecture

### TCP Task
Owns the TCP transport:
- Reads status/control messages continuously
- Updates `RadioState` cache
- Accepts outbound commands via channel

### UDP Meter Task
Owns the UDP socket:
- Reads meter packets continuously
- Updates meter cache / event stream

### Rig Methods
Most getters are **cache reads** — fast, no wire round-trip:
- `get_frequency()` reads `RadioState`

Setters:
- Send TCP command via channel
- Optionally wait for acknowledgement or state to reflect (settle heuristics)

### "AI Off" for Flex

Flex is inherently event-driven. "AI off" means "suppress `RigEvent` emission to subscribers" but does NOT stop processing status messages — the cache must always be updated.

---

## Sub-Phase E.1 — Align FlexBuilder with Project Conventions

**Scope:** Builder pattern alignment. One session.

- Ensure `FlexBuilder::build()` and `build_with_transport()` follow the same pattern as Icom.
- `build_with_transport()` needs to accept mock TCP and UDP transports. May require:
  ```rust
  pub struct FlexTransports {
      pub tcp: Box<dyn Transport>,
      pub udp: Box<dyn Transport>,
  }
  ```
- Builder spawns both TCP and UDP tasks.

### Definition of Done

- Builder construction with mocks compiles and returns a rig.
- Follows same pattern as Icom/text-protocol backends.

---

## Sub-Phase E.2 — Align Flex Event Emission with RigEvent

**Scope:** Event consistency. One session. Depends on E.1.

- Verify all Flex state changes that map to `RigEvent` variants are emitted consistently.
- Slice frequency changes, mode changes, PTT changes should all map to correct `RigEvent` variants.

### Definition of Done

- Mock TCP status messages produce correct `RigEvent` emission.
- Event behavior matches other backends.

---

## Sub-Phase E.3 — Test Infrastructure for Flex

**Scope:** Test fixtures. One session. Depends on E.1.

- Create `MockSmartSdrServer` that simulates SmartSDR TCP protocol (handshake, status messages, command responses).
- Create `MockVita49Source` that feeds scripted VITA-49 packets for meter testing.
- These are Flex-specific test fixtures, not modifications to the generic MockTransport.

### Definition of Done

- Mock server supports full connection lifecycle.
- Meter data can be injected and verified.

---

## Protocol-Specific Gotchas

**TCP is line-oriented, not byte-framed.** SmartSDR messages are `\n`-terminated ASCII lines. The IO task should use a line-based reader (`AsyncBufReadExt` with `BufReader`).

**UDP meter packets can arrive out of order.** Always overwrite with latest value.

**Flex "AI off" does not stop the state receiver.** The cache must always be updated; only event emission is suppressed.

---

## Testing Scenarios

1. **Connection lifecycle.** Mock the full handshake: version exchange, client registration, subscription responses. Verify rig becomes usable.

2. **Slice creation and frequency change.** Create a slice, set its frequency, verify cache updates and `FrequencyChanged` event emitted.

3. **Meter data flow.** Feed VITA-49 meter packets via mock UDP. Verify S-meter values accessible via `get_s_meter()`.

4. **Disconnection handling.** Close mock TCP server mid-operation. Verify `ConnectionLost` event emitted and subsequent commands return `NotConnected`.

---

## Definition of Done (Phase E — all sub-phases)

- Flex builder follows same `build()` / `build_with_transport()` pattern
- Test infrastructure supports mocking TCP and UDP independently
- No direct transport access from rig methods
- Builder/test conventions match rest of project
