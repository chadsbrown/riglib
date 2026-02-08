# otrsp — Rust OTRSP Library Plan

A standalone Rust library implementing the Open Two Radio Switching Protocol
(OTRSP) for SO2R contest station control. Separate repo from winkeyerlib
and riglib.

---

## Overview

OTRSP is a simple ASCII serial protocol for controlling SO2R (Single Operator
Two Radio) switching devices. It was designed by Paul Young, K1XM, to replace
legacy parallel-port SO2R control with a clean serial interface. The protocol
specification is version 0.9 and is stated to be stable ("not expected to
change in any incompatible ways in the future").

The entire protocol is ~10 commands. This crate is intentionally minimal.

---

## Protocol Reference

### Serial Parameters

| Parameter | Value |
|-----------|-------|
| Baud rate | 9600 |
| Data bits | 8 |
| Stop bits | 1 |
| Parity | None |
| Flow control | None |
| Terminator | CR (`\r`, 0x0D) |

RTS and DTR are set **low** on initialization.

### Commands (Host → Device)

All commands are ASCII text terminated with carriage return (`\r`).

#### TX Selection

| Command | Description |
|---------|-------------|
| `TX1\r` | Route transmit (key, mic, PTT) to Radio 1 |
| `TX2\r` | Route transmit to Radio 2 |

#### RX Audio Routing

| Command | Description |
|---------|-------------|
| `RX1\r` | Radio 1 audio to both ears (mono) |
| `RX2\r` | Radio 2 audio to both ears (mono) |
| `RX1S\r` | Stereo: Radio 1 left ear, Radio 2 right ear (focus R1) |
| `RX2S\r` | Stereo: Radio 1 left ear, Radio 2 right ear (focus R2) |
| `RX1R\r` | Reverse stereo: Radio 1 right ear, Radio 2 left ear (focus R1) |
| `RX2R\r` | Reverse stereo: Radio 1 right ear, Radio 2 left ear (focus R2) |

Note: Not all devices support all RX modes. `RX1R`/`RX2R` (reverse stereo)
is documented in Logger32 but may not be supported by all hardware.

#### Auxiliary Outputs

| Command | Description |
|---------|-------------|
| `AUX1nn\r` | Set auxiliary BCD output for Radio 1 (nn = decimal value) |
| `AUX2nn\r` | Set auxiliary BCD output for Radio 2 (nn = decimal value) |

AUX outputs are typically used for band decoder / antenna switching. The
BCD value encodes the current band, allowing external equipment to track
which band each radio is on.

#### Query Commands

| Command | Response | Description |
|---------|----------|-------------|
| `?NAME\r` | Device name string | Query the device identification |

The device responds with its name (e.g., `SO2RDUINO`, `RigSelect Pro`).
The response may be terminated with CR, LF, or CRLF depending on device.

### PTT via DSR

Some OTRSP implementations use the **DSR modem line** for PTT assertion.
This is device-dependent and not universally supported. When available,
asserting DSR keys the currently selected TX radio.

### Supported Devices

| Device | Manufacturer | Notes |
|--------|-------------|-------|
| RigSelect Pro | KD6X Designs | 4-radio switch, embedded WK3, FTDI dual-port |
| YCCC SO2R Box / SO2R+ | YCCC | Original OTRSP reference hardware |
| SO2RDuino | K1XM / community | Arduino-based, open-source |
| microHAM MK2R+ | microHAM | Also has embedded WinKeyer |
| microHAM Station Master | microHAM | Full station controller |

### Logging Software Support

N1MM Logger+ (v9.8.5+), WriteLog (v10.73+), Win-Test (v4.4+), DXLog.net,
so2sdr, Logger32.

---

## Architecture

OTRSP is simple enough that no threading model, event system, or async
wrapper is needed. It is a synchronous, write-mostly protocol:

```
┌──────────────┐        9600/8N1        ┌──────────────┐
│  Contest App  │ ───── TX1\r ────────► │  SO2R Device │
│               │ ───── RX1S\r ───────► │  (RigSelect, │
│               │ ───── AUX1nn\r ─────► │   SO2RDuino, │
│               │ ◄──── name\r ──────── │   etc.)      │
└──────────────┘                        └──────────────┘
```

- **Write path:** Fire-and-forget. No acknowledgment from device.
- **Read path:** Only for `?NAME` query response. Optional.
- **No unsolicited data.** Device never sends data without being asked.
- **No state tracking needed.** The device is the source of truth.

---

## Crate Structure

```
otrsp/
├── Cargo.toml
├── src/
│   ├── lib.rs          # OtrspController struct + public API
│   └── error.rs        # Error types
├── examples/
│   └── so2r_demo.rs    # Connect, switch radios, query name
├── tests/
│   └── commands.rs     # Command string encoding tests
├── LICENSE             # MIT
└── README.md
```

### Dependencies

```toml
[dependencies]
serialport = "4"
thiserror = "2"
log = "0.4"
```

No async dependencies. No feature flags. The crate is intentionally minimal.

---

## API Design

```rust
use otrsp::{OtrspController, Radio, RxMode};

/// Which radio (1 or 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radio {
    Radio1,
    Radio2,
}

/// Receive audio routing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxMode {
    /// Selected radio audio in both ears.
    Mono,
    /// Radio 1 left ear, Radio 2 right ear.
    Stereo,
    /// Radio 1 right ear, Radio 2 left ear.
    ReverseStereo,
}

pub struct OtrspController { /* serial port handle */ }

impl OtrspController {
    /// Open a connection to an OTRSP device.
    pub fn open(port: &str) -> Result<Self>;

    /// Close the connection.
    pub fn close(self) -> Result<()>;

    // --- TX Selection ---

    /// Select which radio receives transmit focus (key, mic, PTT).
    pub fn set_tx(&self, radio: Radio) -> Result<()>;

    // --- RX Audio Routing ---

    /// Set receive audio routing.
    ///
    /// - `Mono`: selected radio in both ears
    /// - `Stereo`: R1 left, R2 right (focus on selected radio)
    /// - `ReverseStereo`: R1 right, R2 left (focus on selected radio)
    pub fn set_rx(&self, radio: Radio, mode: RxMode) -> Result<()>;

    // --- Auxiliary Outputs ---

    /// Set the auxiliary BCD output for a radio (band decoder value).
    pub fn set_aux(&self, radio: Radio, value: u8) -> Result<()>;

    // --- Query ---

    /// Query the device name. Returns the device's identification string.
    /// Blocks up to 1 second waiting for the response.
    pub fn device_name(&self) -> Result<String>;

    // --- PTT via DSR (device-dependent) ---

    /// Assert or release PTT via the DSR modem line.
    /// Not supported by all devices.
    pub fn set_ptt_dsr(&self, on: bool) -> Result<()>;

    // --- Raw ---

    /// Send a raw OTRSP command string. CR terminator is appended
    /// automatically. Use for device-specific extensions.
    pub fn send_raw(&self, command: &str) -> Result<()>;
}

impl Drop for OtrspController {
    // Closes serial port
}
```

### Usage Example

```rust
use otrsp::{OtrspController, Radio, RxMode};

let ctl = OtrspController::open("/dev/ttyUSB1")?;

// Query device
println!("Connected to: {}", ctl.device_name()?);

// Focus on Radio 1
ctl.set_tx(Radio::Radio1)?;
ctl.set_rx(Radio::Radio1, RxMode::Mono)?;

// Switch to Radio 2 with stereo audio
ctl.set_tx(Radio::Radio2)?;
ctl.set_rx(Radio::Radio2, RxMode::Stereo)?;

// Set band decoder for Radio 1 to 20m
ctl.set_aux(Radio::Radio1, 4)?;
```

---

## Implementation Phases

### Phase 1: Core Implementation

**Goal:** Complete, working OTRSP controller.

**Deliverables:**
- `error.rs`: `Error` enum with variants:
  - `SerialPort(serialport::Error)` — port open/write failures
  - `Io(std::io::Error)` — general I/O
  - `Timeout` — `?NAME` query timeout
  - `InvalidValue(String)` — bad AUX value, etc.
- `lib.rs`: `OtrspController` struct with all methods:
  - `open()` — opens port at 9600/8N1, sets RTS/DTR low
  - `set_tx()` — sends `TX1\r` or `TX2\r`
  - `set_rx()` — sends `RX1\r`, `RX2\r`, `RX1S\r`, `RX2S\r`, `RX1R\r`, `RX2R\r`
  - `set_aux()` — sends `AUX1nn\r` or `AUX2nn\r`
  - `device_name()` — sends `?NAME\r`, reads response with 1s timeout
  - `set_ptt_dsr()` — toggles DSR line
  - `send_raw()` — sends arbitrary command + `\r`
  - `Drop` closes port
- `Radio` and `RxMode` enums
- Unit tests for command string construction (no serial port needed)
- Integration test with mock serial port

**Verification:** `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`

---

### Phase 2: Example + Polish

**Goal:** Usable example and crates.io-ready quality.

**Deliverables:**
- `so2r_demo` example: connects, queries name, cycles through TX/RX states
- README with:
  - Quick start code
  - Supported devices table
  - How it fits with riglib and winkeyerlib
  - Protocol reference summary
- `rustdoc` for all public items
- CI (test, clippy, fmt)
- `Cargo.toml` metadata for crates.io
- LICENSE (MIT)

**Verification:** `cargo doc --no-deps` clean, example builds, CI green.

---

## Reference Sources

- [OTRSP Protocol Specification v0.9](https://k1xm.org/OTRSP/OTRSP_Protocol.pdf) — canonical spec (K1XM)
- [OTRSP Home Page](https://www.k1xm.org/OTRSP/) — protocol overview and reference device
- [SO2RDuino NCJ Article](https://ncjweb.com/features/julaug10feat.pdf) — K1XM, July/August 2010 NCJ
- [so2sdr OTRSP implementation](https://github.com/n4ogw/so2sdr/blob/master/so2sdr/otrsp.cpp) — C++ reference implementation (N4OGW)
- [Logger32 OTRSP docs](https://www1.logger32.net/otrsp.html) — documents RX1R/RX2R reverse stereo
- [RigSelect Pro](https://rigselect.com/) — OTRSP on FTDI dual-port alongside WinKeyer
- [RigSelect N1MM App Note](https://rigselect.com/rigselect/RigSelect%20Assets/App%20Note%20-%20RigSelect%20used%20with%20N1MM%20Logger.pdf)
