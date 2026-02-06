# riglib — Project Plan

<!--
  SESSION GUIDE: Start here. Read sections 1-2 to understand current state.
  Jump to the current work item in Section 4 for your next task.
  Reference material (architecture, protocols, radio catalog) is in Sections 5-9.
-->

---

## 1. Progress Tracker

**Current Phase**: Phase 5 complete — all phases done
**Current Work Item**: None — project implementation complete
**Last Completed**: WI-5.2 Documentation and examples (2026-02-06)

### Crate Status

| Crate                | Status      | Tests | Notes                              |
|---------------------|-------------|-------|-------------------------------------|
| riglib              | Complete    | 2     | Facade crate, feature flags + audio, 5 examples |
| riglib-core         | Complete    | 38    | Types, errors, Transport, Rig, AudioCapable |
| riglib-transport    | Complete    | 41    | Serial + TCP + UDP + CpalAudioBackend (audio feature) |
| riglib-icom         | Complete    | 171   | CI-V codec, 14 models, IcomRig + AudioCapable |
| riglib-yaesu        | Complete    | 194   | CAT protocol, 6 models, YaesuRig + AudioCapable |
| riglib-elecraft     | Complete    | 244   | Extended Kenwood, 5 models, ElecraftRig + AudioCapable |
| riglib-kenwood      | Complete    | 202   | Kenwood CAT, 4 models, KenwoodRig + AudioCapable |
| riglib-flex         | Complete    | 204   | SmartSDR + VITA-49 + DAX audio, FlexRadio + AudioCapable |
| riglib-test-harness | Complete    | 9     | MockTransport + MockTcpServer       |

**Total tests**: 1139 passing, 0 failures (with audio features enabled)

### Quality Gates

| Check               | Status |
|---------------------|--------|
| `cargo fmt`         | Clean  |
| `cargo clippy -D warnings` | Clean |
| `cargo doc --no-deps` | Zero warnings |
| `cargo build --examples` | All 5 compile |
| GitHub Actions CI   | Configured (.github/workflows/ci.yml) |

### Test Hardware

| Rig             | Available | Used In   |
|-----------------|-----------|-----------|
| Icom IC-7610    | Yes       | Phase 1   |
| Yaesu FT-DX10   | Yes       | Phase 2   |
| Elecraft         | No        | Phase 2   |
| FlexRadio        | No        | Phase 3   |
| Kenwood          | No        | Phase 2   |

---

## 2. Decisions Log

All architectural and scope decisions, recorded so they are not re-asked.

| #  | Decision                                      | Choice                                                     |
|----|-----------------------------------------------|-------------------------------------------------------------|
| D1 | Manufacturers supported                       | Icom, Yaesu, FlexRadio, Elecraft, Kenwood                  |
| D2 | Amplifier/tuner/rotator control               | No — transceivers only                                      |
| D3 | Audio streaming                               | Yes, for radios that support it (Phase 4)                   |
| D4 | Transport phasing                             | Serial (USB) first; network added in Phase 3 for FlexRadio |
| D5 | rigctld compatibility                         | No                                                          |
| D6 | flrig XML-RPC compatibility                   | No                                                          |
| D7 | Async runtime                                 | Tokio (cross-platform: Linux, macOS, Windows)               |
| D8 | Frequency type                                | Simple `u64` (Hz); band info inferred, not embedded         |
| D9 | Builder pattern                               | Yes — consumers should have options for configuration       |
| D10| Error recovery philosophy                     | Smart defaults (auto-retry, collision recovery, reconnect); all overridable |
| D11| MSRV policy                                   | Latest stable Rust only                                     |
| D12| License                                       | MIT                                                         |
| D13| Crate naming                                  | Undecided. Working name: `riglib`                           |
| D14| Open source                                   | Yes, from day one                                           |
| D15| Manufacturer-specific API pattern             | Option B: generic `Rig` trait + concrete types expose manufacturer-specific methods |
| D16| VFO/Slice abstraction                         | `ReceiverId` — VFO A/B map to 0/1; FlexRadio slices map to 0-7 |
| D17| FlexRadio phasing                             | Phase 3 (network-only rig, acceptable to defer)             |
| D18| IC-R8600                                      | Excluded (receive-only)                                     |
| D19| Kenwood VHF/UHF handhelds/mobiles             | Excluded (HF focus)                                         |
| D20| Pre-2010 cutoff                               | Relaxed for K3 (2008) and IC-7600 (2009) — same protocol, no added complexity |

---

## 3. Open Items

Items that still need resolution. Check before starting work on the affected phase.

- [ ] **O1**: Crate naming — decide before crates.io publish (Phase 5)
- [ ] **O2**: FlexRadio test hardware — acquire or borrow before Phase 3 hardware validation (no longer blocking mock-based development; see PLAN Phase 3 prerequisite note)
- [x] **O3**: Serial crate selection — **resolved**: `tokio-serial` 5.4 selected (good API, active maintenance, cross-platform)
- [ ] **O4**: No SmartSDR simulator exists — all FlexRadio integration tests depend on riglib's own mock SmartSDR server; real hardware needed for final validation (see 003-flexradio-research.md Section 7.3)

---

## 4. Work Items

Each work item is designed to be completable in a single session (or a small
number of sessions). Work items within a phase are ordered — complete them
sequentially. Phases are sequential.

### Phase 1: Foundation + Icom IC-7610

**Goal**: Core traits, serial transport, first working rig validated against hardware.

---

#### WI-1.1: Create workspace and crate scaffolding

**Produces**: Cargo workspace with all 9 crates, empty `lib.rs` files, compiles with `cargo build`.

**Files created**:
- `Cargo.toml` (workspace virtual manifest)
- `crates/riglib/Cargo.toml` + `src/lib.rs`
- `crates/riglib-core/Cargo.toml` + `src/lib.rs`
- `crates/riglib-transport/Cargo.toml` + `src/lib.rs`
- `crates/riglib-icom/Cargo.toml` + `src/lib.rs`
- `crates/riglib-yaesu/Cargo.toml` + `src/lib.rs`
- `crates/riglib-elecraft/Cargo.toml` + `src/lib.rs`
- `crates/riglib-kenwood/Cargo.toml` + `src/lib.rs`
- `crates/riglib-flex/Cargo.toml` + `src/lib.rs`
- `crates/riglib-test-harness/Cargo.toml` + `src/lib.rs`
- `test-app/Cargo.toml` + `src/main.rs` (stub CLI, grows per phase)

**Done when**:
- `cargo build --workspace` succeeds
- `cargo test --workspace` succeeds (no tests yet, but compiles)
- Feature flags wired in `riglib` facade: `icom`, `yaesu`, `elecraft`, `kenwood`, `flex`
- Git repo initialized with `.gitignore`, initial commit

---

#### WI-1.2: Core types and error types (riglib-core)

**Produces**: All shared types that other crates depend on. No traits yet — just data types.

**Files created/modified**:
- `crates/riglib-core/src/types.rs` — `Mode`, `Passband`, `ReceiverId`, `RigInfo`, `RigCapabilities`, `BandRange`
- `crates/riglib-core/src/events.rs` — `RigEvent` enum
- `crates/riglib-core/src/error.rs` — `Error` type (thiserror)
- `crates/riglib-core/src/lib.rs` — module declarations + re-exports

**Key types to define**:
- `ReceiverId(u8)` with `VFO_A = 0`, `VFO_B = 1` constants
- `Mode` enum: `USB`, `LSB`, `CW`, `CWR`, `AM`, `FM`, `RTTY`, `RTTYR`, `Data` (and variants)
- `Passband` — filter width representation
- `RigInfo` — manufacturer, model name, model enum variant
- `RigCapabilities` — max receivers, supported modes, frequency ranges, feature flags
- `RigEvent` — frequency/mode/PTT/meter changes, connect/disconnect
- `Error` — transport errors, protocol errors, timeout, unsupported operation

**Done when**:
- `cargo build --workspace` succeeds
- `cargo test -p riglib-core` passes
- Types are documented with rustdoc comments
- Unit tests for `ReceiverId`, `Mode` Display/FromStr if applicable

---

#### WI-1.3: Transport trait and serial implementation (riglib-transport)

**Produces**: `Transport` trait in riglib-core, `SerialTransport` implementation in riglib-transport.

**Files created/modified**:
- `crates/riglib-core/src/transport.rs` — `Transport` trait definition
- `crates/riglib-transport/src/serial.rs` — `SerialTransport` using tokio serial crate
- `crates/riglib-transport/src/lib.rs` — module declarations
- `crates/riglib-test-harness/src/mock_serial.rs` — `MockTransport` for testing

**Transport trait**:
```rust
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&mut self, data: &[u8]) -> Result<()>;
    async fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;
    async fn close(&mut self) -> Result<()>;
    fn is_connected(&self) -> bool;
}
```

**Done when**:
- Serial crate selected (evaluate serial2-tokio vs tokio-serial — resolve O3)
- `SerialTransport` opens a serial port, sends bytes, receives bytes with timeout
- `MockTransport` can be pre-loaded with expected responses for testing
- Unit tests pass using `MockTransport`
- Compiles on Linux, macOS, Windows (CI or cross-check)

---

#### WI-1.4: Rig trait definition (riglib-core)

**Produces**: The `Rig` trait — the core abstraction that all manufacturer backends implement.

**Files created/modified**:
- `crates/riglib-core/src/rig.rs` — `Rig` trait with all methods
- `crates/riglib-core/src/lib.rs` — add module + re-export

**Rig trait methods** (see Section 5.2 for full signatures):
- `info()`, `capabilities()`
- `receivers()`, `primary_receiver()`, `secondary_receiver()`
- `get/set_frequency()`, `get/set_mode()`, `get/set_passband()`
- `get/set_ptt()`, `get/set_power()`
- `get_s_meter()`, `get_swr()`, `get_alc()`
- `get/set_split()`, `set_tx_receiver()`
- `subscribe()` — returns `broadcast::Receiver<RigEvent>`

**Done when**:
- Trait compiles with `async_trait`
- Trait is `Send + Sync` bounded
- `subscribe()` uses `tokio::sync::broadcast`
- Rustdoc comments on every method explaining semantics

---

#### WI-1.5: CI-V protocol engine (riglib-icom)

**Produces**: Icom CI-V frame encoder/decoder. No Rig trait impl yet — just the protocol layer.

**Files created/modified**:
- `crates/riglib-icom/src/civ.rs` — frame encode/decode, BCD frequency conversion
- `crates/riglib-icom/src/commands.rs` — command builders (set freq, get freq, set mode, etc.)
- `crates/riglib-icom/src/models.rs` — IC-7610 model definition (CI-V address, baud, capabilities)

**CI-V frame format** (see Section 6.1):
- Preamble: `0xFE 0xFE`, terminator: `0xFD`
- BCD frequency: 10 digits, LSB first
- ACK: `0xFB`, NAK: `0xFA`

**Done when**:
- Encode frequency to BCD and decode BCD to frequency — round-trip tested
- Build CI-V frames for: set freq, get freq, set mode, get mode, set PTT, get PTT
- Parse CI-V response frames (ACK, NAK, data responses)
- Handle transceive-mode unsolicited frames
- IC-7610 model entry: CI-V address `0x98`, baud `115200`, capabilities populated
- All protocol logic is pure (no I/O) — tested with byte slices, not serial ports
- Unit tests cover: valid frames, malformed frames, edge cases (collision bytes)

---

#### WI-1.6: Icom Rig implementation + builder (riglib-icom)

**Produces**: `IcomRig` struct implementing `Rig` trait, `IcomBuilder` for construction.

**Files created/modified**:
- `crates/riglib-icom/src/lib.rs` — `IcomRig` struct, `impl Rig for IcomRig`
- `crates/riglib-icom/src/builder.rs` — `IcomBuilder` with smart defaults

**Builder API**:
```rust
IcomBuilder::new(IcomModel::IC7610)
    .serial_port("/dev/ttyUSB0")
    .baud_rate(115200)            // default from model
    .civ_address(0x98)            // default from model
    .auto_retry(true)             // default: true
    .collision_recovery(true)     // default: true
    .reconnect_on_drop(true)      // default: true
    .command_timeout(Duration::from_millis(500))
    .build()
    .await?
```

**Done when**:
- `IcomRig` implements all `Rig` trait methods using CI-V protocol engine + Transport
- Builder validates configuration and returns clear errors
- Smart defaults: auto-retry (3 attempts), collision recovery, reconnect
- All smart defaults are individually overridable to `false`
- Event subscription works: transceive-mode frames → `RigEvent` broadcast
- Internal command serialization (one CI-V command at a time, no interleaving)
- Integration tests pass using `MockTransport` with recorded CI-V exchanges

---

#### WI-1.7: Facade crate wiring (riglib)

**Produces**: The `riglib` crate re-exports types from riglib-core and riglib-icom behind feature flags.

**Files created/modified**:
- `crates/riglib/src/lib.rs` — conditional re-exports
- `crates/riglib/Cargo.toml` — feature flag → optional dependency wiring

**Done when**:
- `use riglib::Rig` works (re-exported from riglib-core)
- `use riglib::icom::IcomBuilder` works when `icom` feature is enabled
- `use riglib::icom::IcomBuilder` fails to compile when `icom` feature is disabled
- All core types (`Mode`, `ReceiverId`, `RigEvent`, `Error`, etc.) accessible via `riglib::`
- Example program compiles: create an `IcomBuilder`, connect, read frequency

---

#### WI-1.8: Test application v1 — Icom CLI tool

**Produces**: A CLI test application that exercises all implemented Rig trait
functionality against a real or mock Icom radio. This application evolves
across phases as more manufacturers and features are added.

**Requires**: Physical IC-7610 connected via USB (for hardware validation).

**Files created**:
- `test-app/Cargo.toml` — binary crate, depends on `riglib`
- `test-app/src/main.rs` — CLI entry point with subcommands

**CLI subcommands (v1)**:
```
test-app connect --manufacturer icom --model IC7610 --port /dev/ttyUSB0
test-app info                     # print RigInfo + RigCapabilities
test-app freq get [--rx 0]        # read frequency
test-app freq set 14250000 [--rx 0]  # set frequency
test-app mode get [--rx 0]        # read mode
test-app mode set USB [--rx 0]    # set mode
test-app ptt get                  # read PTT state
test-app ptt on / ptt off         # set PTT (with safety confirmation)
test-app meter                    # read S-meter, SWR, ALC
test-app monitor                  # subscribe to events, print as they arrive
test-app stress                   # rapid-fire freq changes to test reliability
```

**Hardware validation checklist**:
- [ ] Connect to IC-7610 via USB serial
- [ ] `info` displays correct model, capabilities
- [ ] `freq get` / `freq set` — round-trip verified on rig
- [ ] `mode get` / `mode set` — USB, LSB, CW confirmed
- [ ] `ptt get` / `ptt on` / `ptt off` — confirmed (dummy load or low power)
- [ ] `meter` — S-meter, SWR, ALC read successfully
- [ ] `monitor` — change freq on rig, event appears in output
- [ ] Dual receiver: `freq get --rx 1` reads sub receiver
- [ ] `stress` — 100 rapid freq sets with no errors or hangs
- [ ] Disconnect USB, confirm reconnect behavior
- [ ] No panics, no hangs, no resource leaks

**Done when**:
- All subcommands work against mock transport (unit-testable)
- All hardware validation checklist items pass against IC-7610
- Any bugs found are fixed and tests updated

---

### Phase 2: Yaesu + Elecraft + Kenwood

**Goal**: Three more protocol backends, validated against FT-DX10 hardware.

---

#### WI-2.1: Yaesu CAT protocol engine (riglib-yaesu)

**Produces**: Yaesu CAT command encoder/decoder, pure protocol layer.

**Files created/modified**:
- `crates/riglib-yaesu/src/protocol.rs` — CAT command encode/decode
- `crates/riglib-yaesu/src/commands.rs` — command builders
- `crates/riglib-yaesu/src/models.rs` — FT-DX10 model definition + other models

**Done when**:
- Encode/decode frequency commands (FA, FB) — round-trip tested
- Encode/decode mode, PTT, power, meter commands
- Parse error responses (`?;`)
- FT-DX10 model entry with capabilities populated
- All other Yaesu models (FT-891, FT-991A, FT-DX101D/MP, FT-710) have model entries
- Unit tests cover valid commands, error responses, edge cases

---

#### WI-2.2: Yaesu Rig implementation + builder

**Produces**: `YaesuRig` struct implementing `Rig`, `YaesuBuilder`, hardware-validated against FT-DX10.

**Files created/modified**:
- `crates/riglib-yaesu/src/lib.rs` — `YaesuRig`, `impl Rig for YaesuRig`
- `crates/riglib-yaesu/src/builder.rs` — `YaesuBuilder`
- `crates/riglib/src/lib.rs` — add Yaesu re-exports under `yaesu` feature

**Done when**:
- `YaesuRig` implements all `Rig` trait methods
- Builder with smart defaults
- Integration tests pass with MockTransport
- Hardware validation against physical FT-DX10 (same checklist as WI-1.8, minus Icom-specific items)

---

#### WI-2.3: Kenwood CAT protocol engine + Rig implementation (riglib-kenwood)

**Produces**: Full Kenwood backend.

**Files created/modified**:
- `crates/riglib-kenwood/src/protocol.rs` — Kenwood CAT encode/decode
- `crates/riglib-kenwood/src/commands.rs` — command builders
- `crates/riglib-kenwood/src/models.rs` — TS-590S, TS-590SG, TS-990S, TS-890S
- `crates/riglib-kenwood/src/builder.rs` — `KenwoodBuilder`
- `crates/riglib-kenwood/src/lib.rs` — `KenwoodRig`, `impl Rig for KenwoodRig`
- `crates/riglib/src/lib.rs` — add Kenwood re-exports

**Done when**:
- Protocol engine with unit tests
- All 4 Kenwood model entries
- AI mode (unsolicited state push) → RigEvent broadcast
- Integration tests with MockTransport
- No hardware validation available — rely on protocol tests + community testing

---

#### WI-2.4: Elecraft Extended Kenwood protocol engine + Rig implementation (riglib-elecraft)

**Produces**: Full Elecraft backend.

**Files created/modified**:
- `crates/riglib-elecraft/src/protocol.rs` — Extended Kenwood encode/decode
- `crates/riglib-elecraft/src/commands.rs` — K3 vs K4 command differences
- `crates/riglib-elecraft/src/models.rs` — K3, K3S, KX3, KX2, K4
- `crates/riglib-elecraft/src/builder.rs` — `ElecraftBuilder`
- `crates/riglib-elecraft/src/lib.rs` — `ElecraftRig`, `impl Rig for ElecraftRig`
- `crates/riglib/src/lib.rs` — add Elecraft re-exports

**Done when**:
- Protocol engine with unit tests (including K3-specific and K4-specific extensions)
- All 5 Elecraft model entries
- AI mode → RigEvent broadcast
- Integration tests with MockTransport
- No hardware validation available — rely on protocol tests + community testing

---

#### WI-2.5: Test application v2 — multi-manufacturer support

**Produces**: Updated test-app that supports Yaesu, Elecraft, and Kenwood in
addition to Icom. Same CLI, new `--manufacturer` and `--model` options.

**Files modified**:
- `test-app/src/main.rs` — add manufacturer/model selection
- `test-app/Cargo.toml` — enable new riglib features

**New CLI capabilities**:
```
test-app connect --manufacturer yaesu --model FTDX10 --port COM3
test-app connect --manufacturer elecraft --model K4 --port /dev/ttyUSB1
test-app connect --manufacturer kenwood --model TS890S --port /dev/ttyUSB2
```

**Hardware validation** (FT-DX10):
- [ ] Same checklist as WI-1.8 adapted for Yaesu
- [ ] Confirm Yaesu-specific CAT behavior works correctly

**Done when**:
- All four serial manufacturers connectable via the same CLI
- FT-DX10 hardware validation passes
- Icom functionality still works (regression check)

---

#### WI-2.6: Expand Icom model support

**Produces**: Model entries and capability tables for all 13 Icom models.

**Files modified**:
- `crates/riglib-icom/src/models.rs` — add all models (IC-7600 through IC-7300MK2)

**Done when**:
- Every Icom model in Section 5.1 has a model entry with correct CI-V address, baud rate, and capabilities
- Unit tests verify each model's capability table

---

#### WI-2.7: TCP transport groundwork (riglib-transport)

**Produces**: `TcpTransport` implementing `Transport` trait, needed for K4 Ethernet and Phase 3.

**Files created/modified**:
- `crates/riglib-transport/src/tcp.rs` — `TcpTransport`
- `crates/riglib-test-harness/src/mock_tcp.rs` — `MockTcpServer` for testing

**Done when**:
- `TcpTransport` connects to a TCP endpoint, implements `Transport` trait
- Tested against `MockTcpServer`
- Elecraft K4 can use `TcpTransport` as an alternative to `SerialTransport`

---

### Phase 3: FlexRadio

**Goal**: Network-native SDR rig support via SmartSDR API.

**Prerequisite**: O2 (FlexRadio test hardware) is desirable but no longer
blocking. The revised plan proceeds with mock-only testing for everything
except final hardware validation. See 003-flexradio-research.md for the
complete risk assessment. Note that meter reading (S-meter, SWR, ALC, power)
requires VITA-49 UDP parsing — there is no TCP-only path to meter data.

---

#### WI-3.1: VITA-49 frame parser (riglib-flex) — PURE PARSING, NO I/O

**Produces**: VITA-49.0 binary frame parser for FlexRadio data streams.

**Files created/modified**:
- `crates/riglib-flex/src/vita49.rs` — VITA-49 frame header parsing, stream type identification, payload extraction

**Protocol details** (see 003-flexradio-research.md Section 3):
- Parse 28-byte VITA-49.0 Extension Data packet headers (big-endian)
- Extract packet type, stream ID, class OUI, packet class code, timestamps
- Identify stream types via packet class codes:
  - `0x8002` — Meter data
  - `0x8003` — FFT data
  - `0x8004` — Waterfall data
  - `0x8005` — Opus audio
  - `0x02E3`-`0x02E6` — DAX IQ (24/48/96/192 kHz)
  - `0x03E3` — DAX audio (24 ksps stereo float32)
  - `0xFFFF` — Discovery
- Extract meter data records: array of 4-byte records (16-bit meter ID + 16-bit signed value, both big-endian)
- Extract DAX audio samples: stereo float32, little-endian within big-endian framing
- Packet size validation (header packet_size field vs actual datagram length)

**All unit tests use hardcoded byte arrays — no network needed.**

**Done when**:
- All stream class codes identified from packet header
- Meter payload parsed into Vec of (meter_id, meter_value) tuples
- DAX audio payload parsed into Vec of (f32, f32) stereo frames
- Header validation rejects malformed packets (truncated, wrong packet type)
- Tested with synthetic VITA-49 frames covering all stream types
- ~200 lines of focused parsing code, no external VITA-49 crate dependency

---

#### WI-3.2: SmartSDR command/response/status codec (riglib-flex) — PURE PARSING, NO I/O

**Produces**: Complete encoder/decoder for the SmartSDR TCP text protocol.

**Files created/modified**:
- `crates/riglib-flex/src/codec.rs` — command encoding, response decoding, status message decoding
- `crates/riglib-flex/src/mode.rs` — FlexRadio mode string ↔ riglib Mode conversion
- `crates/riglib-flex/src/meters.rs` — meter name/ID mapping types, value scaling

**Encoding — commands**:
- Format: `C<seq>|<command>\n`
- Sequence numbers auto-increment from 1
- Command builders for all Rig trait operations:
  - `slice tune <index> <freq_mhz>` — tune slice (Hz u64 → MHz f64 conversion)
  - `slice set <index> mode=<mode>` — set mode (riglib Mode → FlexRadio string)
  - `slice set <index> filter_lo=<lo> filter_hi=<hi>` — set passband
  - `slice set <index> tx=1` — designate TX slice
  - `slice create freq=<mhz> mode=<mode>` — create new slice
  - `slice remove <index>` — destroy slice
  - `slice list` — list active slices
  - `xmit <0|1>` — PTT control
  - `transmit set power=<0-100>` — set TX power
  - `info` — query radio info
  - `meter list` — query meter ID assignments
  - `sub slice all`, `sub meter all`, `sub tx all`, etc. — subscriptions
  - `client program <name>` — register client

**Decoding — responses**:
- Format: `R<seq>|<hex_error_code>|<message>\n`
- Parse sequence number, error code (00000000 = success), response data
- Error code as u32 from hex string

**Decoding — status messages**:
- Format: `S<handle>|<object> <key>=<value> <key>=<value>...\n`
- Parse handle, object type, key-value pairs
- Status parsers for: slice, meter, tx, radio, interlock objects

**Decoding — handshake lines**:
- Version line: `V<major>.<minor>.<patch>.<build>\n`
- Handle line: `H<hex_handle>\n`

**Mode string mapping** (FlexRadio ↔ riglib Mode):
- `USB`↔USB, `LSB`↔LSB, `CW`↔CW, `CWR`↔CWR, `AM`↔AM, `FM`↔FM
- `DIGU`↔DataUSB, `DIGL`↔DataLSB, `RTTY`↔RTTY
- `SAM`→AM, `NFM`→FM, `DFM`→DataFM, `FDV`→DataUSB (lossy mappings documented)

**Frequency conversion** (MHz float ↔ Hz u64):
- Hz→MHz: `hz as f64 / 1_000_000.0` (for commands)
- MHz→Hz: `(mhz * 1_000_000.0).round() as u64` (for status parsing)
- Round-trip tested to verify no precision loss at 1 Hz resolution

**Done when**:
- All command builders produce correct wire format
- Response parser handles success, error, and empty-data cases
- Status parser extracts all key-value pairs for slice/meter/tx/radio objects
- Handshake line parser extracts version and handle
- Mode mapping round-trips for all non-lossy modes
- Frequency conversion round-trips for full HF range (1.8–54 MHz)
- All tests are pure string/byte operations — no network I/O

---

#### WI-3.3: UDP transport (riglib-transport)

**Produces**: `UdpTransport` for sending and receiving UDP datagrams.

**Files created/modified**:
- `crates/riglib-transport/src/udp.rs` — `UdpTransport` wrapping `tokio::net::UdpSocket`

**Done when**:
- `UdpTransport` can bind to a local port
- Send datagrams to a specified remote address
- Receive datagrams with timeout
- Tested with loopback UDP (send to localhost, receive on same)

---

#### WI-3.4: FlexRadio network client (riglib-flex) — I/O INTEGRATION

**Produces**: Full network client combining TCP command channel and UDP VITA-49 receiver.

**Files created/modified**:
- `crates/riglib-flex/src/client.rs` — TCP connection management, command/response correlation, status routing
- `crates/riglib-flex/src/state.rs` — cached radio state (slices, meters, TX status)
- `crates/riglib-flex/src/discovery.rs` — UDP broadcast listener, `DiscoveredRadio` type

**TCP client (port 4992)**:
- Connect and handle handshake (read version + handle lines)
- Send commands with auto-incrementing sequence numbers
- Command/response correlation: send command, await matching response via oneshot channel
- Status message subscription and routing to state cache + event broadcast
- Background TCP read loop (Tokio task) that dispatches responses and status messages
- Reconnection handling on TCP disconnect

**VITA-49 UDP receiver (port 4991)**:
- Bind UDP socket, receive datagrams
- Parse VITA-49 headers (using WI-3.1 parser), demux by stream class code
- Meter stream processing: maintain current meter values keyed by meter name
- Background UDP receive loop (Tokio task)
- Route meter updates to state cache

**Discovery (UDP port 4992)**:
- Listen for FlexRadio discovery broadcasts
- Parse discovery packets into `DiscoveredRadio` (model, serial, nickname, IP, firmware version)
- Timeout-based: listen for N seconds, return all discovered radios

**Concurrency model** (three Tokio tasks):
1. TCP read loop — reads lines, dispatches responses and status
2. UDP receive loop — reads datagrams, parses VITA-49, updates meters
3. Command sender — accepts commands from Rig trait, sends over TCP, awaits response

**Done when**:
- Can connect to mock SmartSDR TCP server, complete handshake
- Send commands and receive correlated responses (including out-of-order)
- Receive and route status messages to state cache
- Receive VITA-49 meter data from mock UDP sender, update meter values
- Discovery finds mock radio on loopback
- Reconnection works after simulated TCP disconnect
- Integration tests using MockTcpServer + mock UDP sender

---

#### WI-3.5: FlexRadio model definitions (riglib-flex)

**Produces**: `FlexRadioModel` struct and factory functions for all supported models.

**Files created/modified**:
- `crates/riglib-flex/src/models.rs` — model definitions with capabilities

**Model struct fields**:
- Model name (string)
- Max slices
- Max panadapters
- SCU count
- Has front panel (M variants)
- Frequency range (1.8–54 MHz for all current models)

**Models** (7 entries):

| Model       | Max Slices | SCUs | Panadapters | Front Panel |
|-------------|-----------|------|-------------|-------------|
| FLEX-6400   | 2         | 1    | 2           | No          |
| FLEX-6400M  | 2         | 1    | 2           | Yes         |
| FLEX-6600   | 4         | 2    | 4           | No          |
| FLEX-6600M  | 4         | 2    | 4           | Yes         |
| FLEX-6700   | 8         | 2    | 8           | No          |
| FLEX-8400   | 4         | 2    | 4           | No          |
| FLEX-8600   | 8         | 2    | 8           | No          |

**Done when**:
- All 7 models defined with correct capabilities
- `RigCapabilities` populated from model definitions
- Unit tests verify each model's capability values

---

#### WI-3.6: FlexRadio Rig implementation + builder (riglib-flex)

**Produces**: `FlexRadio` struct implementing `Rig` trait, `FlexRadioBuilder`, and FlexRadio-specific extensions.

**Files created/modified**:
- `crates/riglib-flex/src/lib.rs` — `FlexRadio` struct, `impl Rig for FlexRadio`
- `crates/riglib-flex/src/builder.rs` — `FlexRadioBuilder` (host/port configuration, not serial)
- `crates/riglib-flex/src/slice.rs` — slice-to-ReceiverId mapping, slice lifecycle
- `crates/riglib/src/lib.rs` — add FlexRadio re-exports under `flex` feature

**Generic Rig trait mapping**:
- `receivers()` → list active slices as ReceiverId(0..N) from cached state
- `primary_receiver()` → slice with tx=1 (or slice 0)
- `secondary_receiver()` → next active slice after primary
- `get_frequency(rx)` → cached slice frequency (MHz→Hz conversion at boundary)
- `set_frequency(rx, hz)` → `slice tune <rx> <hz_to_mhz>` TCP command
- `get_mode(rx)` → cached slice mode (FlexRadio string→riglib Mode)
- `set_mode(rx, mode)` → `slice set <rx> mode=<mode_string>` TCP command
- `get_s_meter(rx)` / `get_swr()` / `get_alc()` → VITA-49 meter data from cache
- `get/set_ptt()` → `xmit` command + TX status cache
- `get/set_power()` → `transmit set power` command + TX status cache
- `get/set_split()` → TX slice == RX slice logic
- If slice N does not exist when accessed, auto-create it (smart default, overridable)

**FlexRadio-specific extensions** (Option B — concrete type methods):
```rust
impl FlexRadio {
    pub async fn create_slice(freq_hz: u64, mode: Mode) -> Result<ReceiverId>;
    pub async fn destroy_slice(rx: ReceiverId) -> Result<()>;
    pub async fn set_dax_channel(rx: ReceiverId, channel: u8) -> Result<()>;
    pub async fn set_tx_slice(rx: ReceiverId) -> Result<()>;
    pub async fn discover(timeout: Duration) -> Result<Vec<DiscoveredRadio>>;
}
```

**Builder API**:
```rust
FlexRadioBuilder::new()
    .host("192.168.1.100".parse()?)
    .tcp_port(4992)              // default: 4992
    .udp_port(4991)              // default: 4991
    .client_name("riglib")       // default: "riglib"
    .auto_create_slices(true)    // default: true
    .command_timeout(Duration::from_millis(1000))
    .reconnect_on_drop(true)
    .build()
    .await?

// Or from discovery:
let radios = FlexRadio::discover(Duration::from_secs(3)).await?;
FlexRadioBuilder::new()
    .radio(&radios[0])
    .build()
    .await?
```

**Done when**:
- All 21 Rig trait methods implemented and working
- FlexRadio-specific extension methods working
- Builder validates configuration, returns clear errors
- MHz↔Hz conversion tested at all Rig trait boundaries
- State cache updated from status messages, meter values from VITA-49
- RigEvent broadcast from status message changes
- Integration tests with mock SmartSDR TCP server + mock VITA-49 UDP sender
- All previously supported manufacturers still compile and pass tests (regression)

---

#### WI-3.7: Test application v3 — FlexRadio + discovery

**Produces**: Updated test-app with FlexRadio support including network
connection, discovery, and slice management commands.

**Files modified**:
- `test-app/src/main.rs` — add FlexRadio connection modes + slice commands
- `test-app/Cargo.toml` — enable flex feature

**New CLI capabilities**:
```
test-app discover                  # discover FlexRadio radios on LAN
test-app connect --manufacturer flex --ip 192.168.1.100
test-app connect --manufacturer flex --discover  # auto-discover, pick first

# FlexRadio-specific subcommands:
test-app flex slices               # list active slices
test-app flex create-slice 14250000 USB  # create a new slice
test-app flex destroy-slice 2      # destroy slice 2
test-app flex dax 0 1              # assign slice 0 to DAX channel 1
```

Network connection (host:port) instead of serial port for FlexRadio.

**Done when**:
- `--manufacturer flex` accepted, connects via TCP/UDP (not serial)
- `discover` subcommand finds FlexRadio radios on LAN (mock or real)
- Generic commands (freq, mode, ptt, meter, monitor) work against FlexRadio
- FlexRadio-specific slice commands work
- All previously supported manufacturers still work (regression)

---

### Phase 4: Audio Streaming

**Goal**: RX audio capture from all manufacturers via `AudioCapable` trait.
TX audio is deferred to Phase 4b (see WI-4.5 notes).

**Research**: See `docs/004-audio-streaming-research.md` for full findings.

**Key architectural decisions**:
- Audio is a separate `AudioCapable` trait, not part of `Rig` (not all rigs support it)
- USB audio uses `cpal` 0.17 (callback-based, bridged to tokio via mpsc channels)
- FlexRadio uses DAX audio over VITA-49 (no OS audio device, purely network-based)
- All audio normalized to f32 at the API boundary
- cpal is an optional dependency behind the `audio` feature flag
- FlexRadio DAX audio does NOT require the `audio` feature (no OS audio device)
- Sample rate is native: 48 kHz for USB, 24 kHz for DAX (no built-in resampling)
- Audio device selection is manual (by name); auto-detection is a future enhancement

---

#### WI-4.1: Core audio types and AudioCapable trait (riglib-core)

**Produces**: Audio type definitions and the `AudioCapable` trait. Pure types, no I/O.

**Files created/modified**:
- `crates/riglib-core/src/audio.rs` — `AudioCapable` trait, `AudioStreamConfig`, `AudioSampleFormat`, `AudioBuffer`, `AudioReceiver`, `AudioSender`
- `crates/riglib-core/src/lib.rs` — add module + re-export

**Types to define**:
- `AudioStreamConfig { sample_rate: u32, channels: u16, sample_format: AudioSampleFormat }`
- `AudioSampleFormat` enum: `F32`, `I16`
- `AudioBuffer { samples: Vec<f32>, channels: u16, sample_rate: u32 }`
- `AudioReceiver` — wraps `tokio::sync::mpsc::Receiver<AudioBuffer>`, exposes `async recv()` + `config()`
- `AudioSender` — wraps `tokio::sync::mpsc::Sender<AudioBuffer>`, exposes `async send()` + `config()`

**AudioCapable trait**:
```rust
#[async_trait]
pub trait AudioCapable: Rig {
    async fn start_rx_audio(&self, rx: ReceiverId, config: Option<AudioStreamConfig>) -> Result<AudioReceiver>;
    async fn start_tx_audio(&self, config: Option<AudioStreamConfig>) -> Result<AudioSender>;
    async fn stop_audio(&self) -> Result<()>;
    fn audio_supported(&self) -> bool;
}
```

**Done when**:
- All types compile and are documented with rustdoc
- `AudioReceiver`/`AudioSender` have unit tests (mock producer/consumer)
- `AudioCapable` trait compiles with async_trait
- Feature flag `audio` wired in riglib facade Cargo.toml
- No cpal dependency yet — this WI is pure Rust types

---

#### WI-4.2: cpal USB audio backend (riglib-transport)

**Produces**: A `CpalAudioBackend` that opens USB audio devices via cpal and bridges
the callback model to tokio mpsc channels. Shared by all serial-port manufacturers.

**Files created/modified**:
- `crates/riglib-transport/src/audio.rs` — `CpalAudioBackend`, device enumeration, cpal-to-async bridge
- `crates/riglib-transport/Cargo.toml` — add optional `cpal` dependency behind `audio` feature
- `crates/riglib/Cargo.toml` — wire `audio` feature through to riglib-transport

**CpalAudioBackend responsibilities**:
- Enumerate available audio input/output devices (expose `list_audio_devices()`)
- Open a named audio device (by device name string) for input or output
- Create cpal input stream with callback that converts i16 -> f32 and sends `AudioBuffer` via `mpsc::Sender`
- Create cpal output stream with callback that pulls `AudioBuffer` via `mpsc::Receiver`
- Use `try_send()` in callbacks to avoid blocking the audio thread; drop samples on backpressure

**Audio format**: 48 kHz, 16-bit stereo (i16 from cpal, converted to f32 in callback).
All USB-connected transceivers use UAC 1.0 at this configuration.

**Build dependencies**:
- Linux: `libasound2-dev` (ALSA headers)
- macOS: Xcode (CoreAudio)
- Windows: none (WASAPI via `windows` crate)

**Done when**:
- `list_audio_devices()` returns available devices with names (tested on dev machine)
- Input stream opens, receives audio via callback, delivers `AudioBuffer` to mpsc channel
- Output stream opens, receives `AudioBuffer` from mpsc channel, plays via callback
- Unit tests verify the i16-to-f32 conversion
- Integration test opens default audio device, captures a short burst, verifies non-silent data
- Compiles on Linux, macOS, Windows with `audio` feature enabled
- Compiles WITHOUT `audio` feature (cpal not pulled in)
- cpal dependency is `optional = true` in Cargo.toml

---

#### WI-4.3: USB audio implementation for serial manufacturers

**Produces**: `impl AudioCapable` for `IcomRig`, `YaesuRig`, `ElecraftRig`, `KenwoodRig`
using the `CpalAudioBackend` from WI-4.2.

**Files created/modified**:
- `crates/riglib-icom/src/audio.rs` — `impl AudioCapable for IcomRig`
- `crates/riglib-yaesu/src/audio.rs` — `impl AudioCapable for YaesuRig`
- `crates/riglib-elecraft/src/audio.rs` — `impl AudioCapable for ElecraftRig`
- `crates/riglib-kenwood/src/audio.rs` — `impl AudioCapable for KenwoodRig`
- Each manufacturer's `builder.rs` — add `.audio_device("name")` option

**Builder integration**:
```rust
IcomBuilder::new(IcomModel::IC7610)
    .serial_port("/dev/ttyUSB0")
    .audio_device("USB Audio CODEC")  // optional — audio only if specified
    .build().await?
```

If `.audio_device()` is not called, `audio_supported()` returns false and
`start_rx_audio()` returns `Error::Unsupported`. This keeps audio fully optional.

**Implementation is identical for all four manufacturers** — they all delegate to
`CpalAudioBackend`. The only difference is the builder accepting the device name.
Consider a shared helper or default trait implementation to avoid duplication.

**Done when**:
- `IcomRig` implements `AudioCapable`, opens the named cpal device, streams RX audio
- Other three manufacturers implement `AudioCapable` identically
- Builder `.audio_device()` wired for all four
- `audio_supported()` returns false when no audio device configured
- Hardware validation: IC-7610 RX audio captured and is audible (WAV file)
- Hardware validation: FT-DX10 RX audio captured (if hardware available)
- Integration tests with mock audio (cpal on default device)

---

#### WI-4.4: FlexRadio DAX audio streaming (riglib-flex)

**Produces**: DAX audio RX stream support in the FlexRadio client via VITA-49.

**Files created/modified**:
- `crates/riglib-flex/src/dax.rs` — DAX stream management (create/remove/configure)
- `crates/riglib-flex/src/client.rs` — extend UDP read loop to route DaxAudio packets
- `crates/riglib-flex/src/lib.rs` — `impl AudioCapable for FlexRadio`
- `crates/riglib-flex/src/codec.rs` — add DAX stream command builders

**DAX stream lifecycle**:
1. `start_rx_audio(rx, config)` sends: `stream create type=dax_rx dax_channel=<ch>`
2. Response contains stream handle (VITA-49 stream_id)
3. `slice set <rx.index()> dax=<ch>` associates DAX channel with slice
4. UDP read loop matches incoming DaxAudio packets by stream_id
5. Parsed `AudioSample` frames bundled into `AudioBuffer`, sent via mpsc channel
6. `stop_audio()` sends: `stream remove <handle>`

**Audio format**: 24 kHz, stereo, f32 (native DAX format — no conversion needed).

**Does NOT depend on cpal or the `audio` feature flag** — DAX is purely network-based.

**Done when**:
- DAX stream create/remove commands wired in codec.rs
- UDP read loop routes DaxAudio packets to per-stream mpsc channels
- `FlexRadio` implements `AudioCapable`, creates DAX RX stream, returns `AudioReceiver`
- `AudioReceiver.recv()` yields f32 stereo audio at 24 ksps
- Integration tests with mock SmartSDR server: create stream, receive synthetic
  VITA-49 DaxAudio packets, verify audio samples arrive in correct order
- DAX channel ↔ slice association tested
- No cpal dependency in riglib-flex

---

#### WI-4.5: TX audio (deferred — both backends)

**Produces**: TX audio support for both USB audio (cpal) and FlexRadio (DAX TX).

**Note**: This work item is deliberately separated from RX audio. TX audio involves
additional complexity (PTT coordination, audio timing, safety considerations) and
should be implemented after RX audio is validated. It may be deferred to Phase 4b
or Phase 5 based on project priorities.

**USB audio TX**:
- `start_tx_audio()` opens the cpal output device
- Consumer sends `AudioBuffer` to `AudioSender`, cpal callback plays it
- PTT must be asserted separately via `set_ptt(true)` before TX audio
- Safety: TX audio without PTT should be a no-op (radio stays in RX)

**FlexRadio DAX TX**:
- `start_tx_audio()` sends: `stream create type=dax_tx`
- Client constructs VITA-49 packets with float32 audio, sends to UDP port 4991
- `transmit set dax=1` enables DAX as TX source
- VITA-49 TX packet construction (reverse of parse_dax_audio_payload)

**Done when**:
- `AudioSender` works for USB audio (cpal output stream)
- `AudioSender` works for FlexRadio (DAX TX VITA-49 packets)
- Hardware validation: TX audio transmitted on IC-7610 (dummy load, low power)
- Integration tests with mock infrastructure

---

#### WI-4.6: Test application v4 — audio commands

**Produces**: Updated test-app with audio streaming subcommands.

**Files modified**:
- `test-app/src/main.rs` — add audio subcommands
- `test-app/Cargo.toml` — add `audio` feature, `hound` dependency for WAV I/O

**New CLI capabilities**:
```
test-app audio list-devices                               # list audio devices
test-app audio rx --device "USB Audio CODEC" --duration 10 --output rx.wav
test-app audio rx --duration 10 --output rx.wav           # use default device
test-app audio monitor --device "USB Audio CODEC"         # stream to speakers
test-app audio tx --device "USB Audio CODEC" --input tx.wav  # (Phase 4b)
```

For FlexRadio, audio device is implicit (DAX):
```
test-app connect --manufacturer flex --ip 192.168.1.100
test-app audio rx --duration 10 --output rx.wav   # uses DAX automatically
```

**Hardware validation checklist**:
- [ ] `audio list-devices` shows USB Audio CODEC for connected rig
- [ ] `audio rx` captures RX audio from IC-7610, saves to WAV
- [ ] WAV file is audible and correct (play back, compare to rig speaker output)
- [ ] `audio monitor` plays RX audio through computer speakers in real-time
- [ ] FlexRadio `audio rx` captures DAX audio (mock or real hardware)
- [ ] Audio works alongside rig control (freq/mode changes during audio capture)
- [ ] No panics, no resource leaks, graceful shutdown on Ctrl+C
- [ ] All previously supported features still work (regression)

**Done when**:
- All checklist items pass
- `audio` feature compiles and works on Linux (primary), macOS, Windows
- Without `audio` feature, audio subcommands print a helpful error

---

### Phase 5: Polish + Publish

---

#### WI-5.1: CI pipeline

GitHub Actions: build + test + clippy + fmt on Linux, macOS, Windows.
ARM cross-compilation for Raspberry Pi.

---

#### WI-5.2: Documentation and examples

Rustdoc for all public APIs. Example programs for common use cases.

---

#### WI-5.3: Publish to crates.io

Resolve O1 (naming). Publish all crates.

---

## 5. Architecture Reference

### 5.1 Workspace Layout

```
riglib/
├── Cargo.toml                       # workspace definition (virtual manifest)
├── crates/
│   ├── riglib/                      # public facade crate
│   │   └── src/lib.rs               # re-exports behind feature flags
│   │
│   ├── riglib-core/                 # core traits and types (zero I/O deps)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── rig.rs               # Rig trait, ReceiverId
│   │       ├── audio.rs             # AudioCapable trait, AudioReceiver, AudioSender
│   │       ├── transport.rs         # Transport trait
│   │       ├── types.rs             # Frequency, Mode, Passband, Power, etc.
│   │       ├── capabilities.rs      # RigCapabilities, RigInfo
│   │       ├── events.rs            # RigEvent, event stream types
│   │       └── error.rs             # Error types (thiserror)
│   │
│   ├── riglib-transport/            # transport implementations
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── serial.rs            # async serial (tokio-serial / serial2-tokio)
│   │       ├── tcp.rs               # async TCP client
│   │       ├── udp.rs               # async UDP client/listener
│   │       └── discovery.rs         # FlexRadio LAN discovery
│   │
│   ├── riglib-icom/                 # Icom CI-V
│   ├── riglib-yaesu/                # Yaesu CAT
│   ├── riglib-elecraft/             # Elecraft Extended Kenwood
│   ├── riglib-kenwood/              # Kenwood CAT
│   ├── riglib-flex/                 # FlexRadio SmartSDR
│   └── riglib-test-harness/         # test utilities
│
├── test-app/                    # CLI test application (evolves per phase)
│   ├── Cargo.toml
│   └── src/main.rs
│
├── examples/                    # standalone examples for documentation
└── docs/
```

### 5.2 Core Trait: Rig

```rust
#[async_trait]
pub trait Rig: Send + Sync {
    fn info(&self) -> &RigInfo;
    fn capabilities(&self) -> &RigCapabilities;

    async fn receivers(&self) -> Result<Vec<ReceiverId>>;
    async fn primary_receiver(&self) -> Result<ReceiverId>;
    async fn secondary_receiver(&self) -> Result<Option<ReceiverId>>;

    async fn get_frequency(&self, rx: ReceiverId) -> Result<u64>;
    async fn set_frequency(&self, rx: ReceiverId, freq_hz: u64) -> Result<()>;

    async fn get_mode(&self, rx: ReceiverId) -> Result<Mode>;
    async fn set_mode(&self, rx: ReceiverId, mode: Mode) -> Result<()>;

    async fn get_passband(&self, rx: ReceiverId) -> Result<Passband>;
    async fn set_passband(&self, rx: ReceiverId, pb: Passband) -> Result<()>;

    async fn get_ptt(&self) -> Result<bool>;
    async fn set_ptt(&self, on: bool) -> Result<()>;

    async fn get_power(&self) -> Result<f32>;
    async fn set_power(&self, watts: f32) -> Result<()>;

    async fn get_s_meter(&self, rx: ReceiverId) -> Result<f32>;
    async fn get_swr(&self) -> Result<f32>;
    async fn get_alc(&self) -> Result<f32>;

    async fn get_split(&self) -> Result<bool>;
    async fn set_split(&self, on: bool) -> Result<()>;
    async fn set_tx_receiver(&self, rx: ReceiverId) -> Result<()>;

    fn subscribe(&self) -> Result<broadcast::Receiver<RigEvent>>;
}
```

### 5.3 Manufacturer-Specific Extensions (Option B)

Concrete types implement `Rig` AND expose additional methods:

- `FlexRadio`: `create_slice()`, `destroy_slice()`, `set_dax_channel()`, `discover()`
- `IcomRig`: `send_raw_civ()`, `set_civ_address()`
- `ElecraftRig`: K4 panadapter commands (TBD)
- `KenwoodRig`: TBD
- `YaesuRig`: TBD

### 5.4 Builder Pattern

```rust
// Icom
IcomBuilder::new(IcomModel::IC7610)
    .serial_port("/dev/ttyUSB0")
    .baud_rate(115200)
    .civ_address(0x98)
    .auto_retry(true)
    .collision_recovery(true)
    .reconnect_on_drop(true)
    .command_timeout(Duration::from_millis(500))
    .build().await?

// FlexRadio (discovery)
let radios = FlexRadio::discover(Duration::from_secs(3)).await?;
FlexRadioBuilder::new().radio(&radios[0]).build().await?

// FlexRadio (manual)
FlexRadioBuilder::new().ip("192.168.1.100".parse()?).build().await?

// Type-erased
let rig: Box<dyn Rig> = connect_from_config(&config).await?;
```

### 5.5 Smart Defaults

| Behavior                  | Default | Overridable |
|--------------------------|---------|-------------|
| `auto_retry`             | true (3 attempts) | Yes |
| `collision_recovery`     | true    | Yes (Icom only) |
| `reconnect_on_drop`      | true    | Yes |
| `command_timeout`        | 500ms   | Yes |
| `reconnect_delay`        | 1s      | Yes |
| `max_reconnect_attempts` | 5       | Yes |

### 5.6 Feature Flags (riglib facade)

```toml
[features]
default = ["icom", "yaesu", "elecraft", "kenwood", "flex"]
icom     = ["dep:riglib-icom"]
yaesu    = ["dep:riglib-yaesu"]
elecraft = ["dep:riglib-elecraft"]
kenwood  = ["dep:riglib-kenwood"]
flex     = ["dep:riglib-flex"]
audio    = ["dep:cpal"]  # optional: enables USB audio via cpal
full     = ["icom", "yaesu", "elecraft", "kenwood", "flex", "audio"]
```

---

## 6. Protocol Reference

### 6.1 Icom CI-V

```
Frame: 0xFE 0xFE <dst> <src> <cmd> [<sub>] [<data>...] 0xFD
  - Frequency: 10-digit BCD, LSB first
  - ACK: 0xFB, NAK: 0xFA
  - Default baud: 115200 (IC-7300+), 19200 (older)
  - Transceive mode: unsolicited state push
  - USB: virtual serial (Silicon Labs / FTDI)
  - LAN: CI-V over UDP ports 50001/50002 (future)
```

### 6.2 Yaesu CAT

```
Frame: <CMD><PARAM>;
  - CMD: 2-character ASCII mnemonic (FA, FB, MD, IF, etc.)
  - Frequency: 9-digit Hz (e.g., FA014250000;)
  - Error: ?; response
  - USB: virtual serial
```

### 6.3 Kenwood CAT

```
Frame: <CMD><PARAM>;
  - CMD: 2-character ASCII mnemonic
  - Frequency: 11-digit Hz (e.g., FA00014250000;)
  - AI mode: unsolicited state push (AI2;)
  - Error: ?; response
```

### 6.4 Elecraft Extended Kenwood

```
Frame: <CMD><PARAM>;
  - Kenwood-compatible base + Elecraft extensions
  - K31; enables K3 extended mode
  - K4 adds unique commands for SDR features
  - AI mode: unsolicited state push
  - K4 Ethernet: same command syntax over TCP
```

### 6.5 FlexRadio SmartSDR

```
Discovery:  UDP broadcast on port 4992
Command:    TCP port 4992, text-based
  - Send:    C<seq>|<command>\r\n
  - Receive: R<seq>|<status>|<message>
  - Status:  S<handle>|<object> <key>=<value> ...
Data:       UDP port 4991, VITA-49 framing
  - IQ data, demodulated audio (DAX), FFT, meters
Multi-client: each client gets unique 32-bit handle
Slice-based: 0-8 slices depending on model
```

---

## 7. Supported Radio Catalog

### 7.1 Icom

| Model       | Year | Interface(s)         | CI-V Addr | Default Baud |
|-------------|------|----------------------|-----------|--------------|
| IC-7600     | 2009 | USB                  | 0x7A      | 19200        |
| IC-9100     | 2011 | USB                  | 0x7C      | 19200        |
| IC-7410     | 2012 | USB                  | 0x80      | 19200        |
| IC-7100     | 2012 | USB                  | 0x88      | 19200        |
| IC-7850     | 2015 | USB, LAN             | 0x8E      | 115200       |
| IC-7851     | 2015 | USB, LAN             | 0x8E      | 115200       |
| IC-7300     | 2016 | USB                  | 0x94      | 115200       |
| IC-7610     | 2017 | USB, LAN             | 0x98      | 115200       |
| IC-9700     | 2019 | USB, LAN             | 0xA2      | 115200       |
| IC-705      | 2020 | USB-C, WiFi, BT      | 0xA4      | 115200       |
| IC-905      | 2022 | LAN, USB             | 0xAC      | 115200       |
| IC-7760     | 2024 | USB, LAN             | TBD       | 115200       |
| IC-7300MK2  | 2025 | USB-C, LAN, HDMI     | TBD       | 115200       |

### 7.2 Yaesu

| Model          | Year      | Interface |
|----------------|-----------|-----------|
| FT-891         | 2016      | USB       |
| FT-991/991A    | 2015/2016 | USB       |
| FT-DX101D/MP   | 2018      | USB       |
| FT-DX10        | 2021      | USB       |
| FT-710         | 2022      | USB       |

### 7.3 Elecraft

| Model | Year | Interface(s)          |
|-------|------|-----------------------|
| K3    | 2008 | USB, RS-232           |
| KX3   | 2012 | USB                   |
| K3S   | 2015 | USB, RS-232           |
| KX2   | 2016 | USB                   |
| K4    | 2020 | USB, RS-232, Ethernet |

### 7.4 FlexRadio

| Model          | Year | Slices | SCUs |
|----------------|------|--------|------|
| FLEX-6700      | 2013 | 8      | 2    |
| FLEX-6300      | 2014 | 2      | 1    |
| FLEX-6500      | 2014 | 4      | 1    |
| FLEX-6400/6400M| 2018 | 2      | 1    |
| FLEX-6600/6600M| 2018 | 4      | 1    |
| FLEX-8400/8400M| 2024 | 4      | 1    |
| FLEX-8600/8600M| 2024 | 4      | 2    |

### 7.5 Kenwood

| Model    | Year | Interface(s)          |
|----------|------|-----------------------|
| TS-590S  | 2010 | USB, RS-232           |
| TS-990S  | 2013 | USB, RS-232, LAN      |
| TS-590SG | 2014 | USB, RS-232           |
| TS-890S  | 2018 | USB, RS-232, LAN      |

---

## 8. Dependencies (Expected)

| Crate                      | Purpose                    | Phase |
|---------------------------|----------------------------|-------|
| tokio                     | Async runtime              | 1     |
| tokio-serial / serial2-tokio | Async serial port       | 1     |
| async-trait               | Async trait methods        | 1     |
| thiserror                 | Error type derivation      | 1     |
| bytes                     | Byte buffer manipulation   | 1     |
| tracing                   | Structured logging         | 1     |
| cpal                      | Cross-platform audio (optional, behind `audio` feature) | 4 |
| hound                     | WAV file reading/writing   | 4     |

---

## 9. References

- [Icom CI-V Reference Guides](https://www.icomamerica.com/support/manual/2574/)
- [Elecraft Programmer's Reference Manuals](https://elecraft.com/pages/programmers-reference-manuals)
- [Elecraft K4 Programmer's Reference](https://ftp.elecraft.com/K4/Manuals%20Downloads/)
- [FlexRadio SmartSDR API (GitHub)](https://github.com/flexradio/smartsdr-api-docs)
- [FlexRadio Developer Program](https://www.flexradio.com/api/developer-program/)
- [Kenwood TS-890S Command Reference](https://www.kenwood.com/usa/com/amateur/ts-890s/)
- [wfview — Open Source Icom/Kenwood Control](https://wfview.org/)
- [Yaesu CAT Operation Reference Manuals](https://www.yaesu.com/indexVS.cfm)
