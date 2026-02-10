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

## Sub-Phase E.1 — Align FlexBuilder with Project Conventions ✅ COMPLETE

**Scope:** Builder pattern alignment + SmartSdrClient stream abstraction. One session.

**What was built:**
- Abstracted `SmartSdrClient` internals from `TcpStream` to generic `AsyncRead`/`AsyncWrite` trait objects (not the `Transport` trait — SmartSDR is line-oriented and needs `BufReader::read_line()`).
- Added `SmartSdrClient::from_streams()` constructor accepting `Box<dyn AsyncBufRead>` + `Box<dyn AsyncWrite>`.
- Refactored `connect_with_options()` to delegate to `from_streams()`.
- Added `FlexTransports` struct and `build_with_transport()` to builder:
  ```rust
  pub struct FlexTransports {
      pub tcp_read: Box<dyn AsyncRead + Unpin + Send + 'static>,
      pub tcp_write: Box<dyn AsyncWrite + Unpin + Send + 'static>,
  }
  ```
- 3 new tests using `tokio::io::duplex()`: basic construction, command/response, status event emission.
- UDP handling deferred to E.3 (already optional via `start_udp_receiver()`).
- 262 tests pass (259 original + 3 new).

---

## Sub-Phase E.2 — Align Flex Event Emission with RigEvent ✅ COMPLETE

**Status:** Already implemented prior to Phase E work.

All `RigEvent` variants are emitted by `process_status()` in `client.rs`:
- `FrequencyChanged` — slice RF_frequency status updates
- `ModeChanged` — slice mode status updates
- `PttChanged` — tx state updates
- `AgcChanged` — slice agc_mode status updates
- `RitChanged` / `XitChanged` — slice rit_on/rit_freq/xit_on/xit_freq updates
- `Connected` / `Disconnected` — connection lifecycle

Existing tests verify all of these (6+ tests in client.rs and rig.rs, plus the duplex-based `test_build_with_transport_status_event` from E.1).

---

## Sub-Phase E.3 — UDP Mock Injection for Flex Testing ✅ COMPLETE

**Scope:** UDP test infrastructure. One session. Depends on E.1.

E.1 established `tokio::io::duplex()` + helper functions as the TCP mock pattern, replacing the need for a formal `MockSmartSdrServer` struct. The remaining gap is **UDP mock injection** — the `test_start_rx_audio` test still binds a real `UdpSocket` and sends packets over localhost.

- Add `start_mock_udp_receiver(mpsc::Receiver<Vec<u8>>)` to `SmartSdrClient` — accepts pre-built VITA-49 packets via a channel instead of binding a real UDP socket.
- Migrate `test_start_rx_audio` to use the channel-based approach (no real socket).
- Add meter data injection test using the same channel pattern.
- Consider a `MockVita49Source` helper that builds valid VITA-49 meter/audio packets for test injection.

### Definition of Done

- Meter and DAX audio data can be injected via channel (no real UDP socket needed).
- At least one test demonstrates channel-based meter injection.
- Existing UDP tests still pass (real-socket path remains available).

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
