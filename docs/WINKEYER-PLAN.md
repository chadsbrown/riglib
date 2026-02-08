# winkeyerlib — Rust WinKeyer Library Plan

A standalone Rust library for controlling K1EL WinKeyer devices (WK2 and WK3),
designed for contest logging applications. Compatible with riglib by convention.

## Table of Contents

1. [Design Decisions](#design-decisions)
2. [Protocol Reference](#protocol-reference)
3. [Architecture](#architecture)
4. [Crate Structure](#crate-structure)
5. [API Design](#api-design)
6. [Implementation Phases](#implementation-phases)

---

## Design Decisions

### Scope

- **WK2 + WK3 support.** WK1 is excluded (ancient, rare).
- **Standalone K1EL devices** (WKUSB, WKmini, WKmini Nano, WKUSB-AF).
- **Embedded WK3 ICs** (microHAM DXP, Elecraft K3/K4, etc.) — same protocol
  over virtual COM port, supported via transport abstraction.
- **OTRSP is a separate library** (`otrsp`) — see [OTRSP Separation](#otrsp-separation).
- **Multiple simultaneous instances** — first-class support. Two separate
  WinKeyers managed independently by the application.

### Async vs. Dedicated Thread — Recommendation: Hybrid

**The I/O layer uses a dedicated reader thread. The public API is sync-first
with an async wrapper.**

Rationale:

| Concern | Dedicated thread | Tokio async serial |
|---------|------------------|--------------------|
| **Read latency** | Blocking read returns immediately when byte arrives. ~0μs overhead. | Wakes via epoll/kqueue, then schedules on executor. ~10-50μs overhead depending on executor load. |
| **Paddle break-in detection** | Status byte processed in reader thread, callback fires immediately. | Must await on executor, may be delayed by other tasks. |
| **CW timing impact** | None — WinKeyer handles all timing in hardware. | None — same reason. |
| **Integration with tokio apps** | `spawn_blocking` wrapper is trivial. | Native, no wrapper needed. |
| **Complexity** | One thread, one crossbeam channel. Simple. | tokio-serial dependency, codec, framing. More moving parts. |
| **1200 baud reality** | Max ~109 bytes/sec. Thread sleeps 99.9% of the time. Negligible resource cost. | Async machinery is overkill for this data rate. |

The WinKeyer protocol is fundamentally different from rig control protocols:
- Very low data rate (1200 baud)
- No request-response pattern for most operations (fire-and-forget writes,
  unsolicited reads)
- Time-sensitive status notifications (paddle break-in must be detected fast)
- CW timing is handled entirely in the WinKeyer hardware

A dedicated reader thread eliminates all executor scheduling jitter for status
processing. The sync API maps naturally to the protocol. Contest apps using
tokio (with riglib) wrap calls in `spawn_blocking` — this is idiomatic and
well-understood.

**Public API shape:**

```rust
// Sync (primary)
let wk = WinKeyer::open("/dev/ttyUSB0", Config::default())?;
wk.send_message("CQ TEST K3LR")?;
wk.set_speed(32)?;

// Async wrapper
let wk = AsyncWinKeyer::open("/dev/ttyUSB0", Config::default()).await?;
wk.send_message("CQ TEST K3LR").await?;
```

### Relationship to riglib

**Compatible by convention** — same ecosystem patterns, no shared crate
dependency:

- Rust, similar error handling patterns (`thiserror`)
- Similar builder/config patterns
- Similar event subscription model (crossbeam broadcast for sync,
  tokio::broadcast for async wrapper)
- No shared types or traits — the contest app orchestrates both libraries
  independently
- riglib already has CW keying support via CAT commands (`send_cw_message()`,
  `set_cw_key()`, `set_cw_speed()`) using the radio's built-in keyer.
  winkeyerlib provides an alternative CW path via dedicated WinKeyer hardware,
  which offers better timing, hardware paddle support, and PTT sequencing.
  The contest app chooses which CW path to use — they are not used
  simultaneously for the same radio

### Platform Support

- Linux, macOS, Windows (same as riglib)
- `serialport` crate for cross-platform serial I/O
- FTDI and CH340 USB-serial chips (covers all K1EL products)

### License

MIT (same as riglib)

---

## Protocol Reference

### Serial Parameters

| Parameter | Value |
|-----------|-------|
| Baud rate | 1200 (default), 9600 (optional via admin cmd) |
| Data bits | 8 |
| Stop bits | 2 |
| Parity | None |
| Flow control | DSR/DTR enabled, RTS/CTS disabled |

### Byte Classification (Host → WK)

| Range | Category | Description |
|-------|----------|-------------|
| `0x00 + sub` | Admin commands | 2+ byte sequences: `0x00` then sub-command |
| `0x01–0x1F` | Immediate commands | Execute immediately, not buffered |
| `0x20–0x7F` | Buffered text | ASCII characters queued for CW transmission |

### Byte Classification (WK → Host)

Unsolicited bytes from WinKeyer. The two MSBs determine the type:

| Bits 7-6 | Type | Description |
|----------|------|-------------|
| `11` | Status byte | WK status register changed |
| `10` | Speed pot | Speed pot position changed |
| `00` or `01` | Echo char | Character being sent (echoed back) |

### Admin Commands (`0x00` prefix)

| Sequence | Name | Parameters | Response |
|----------|------|------------|----------|
| `00 01` | Admin Calibrate | — | — |
| `00 02` | Open Host Mode | — | Version byte (23 = WK2, 30/31 = WK3) |
| `00 03` | Close Host Mode | — | — |
| `00 04` | Echo Test | 1 byte | Same byte echoed |
| `00 05` | Paddle A2D | — | ADC reading |
| `00 06` | Speed A2D | — | ADC reading |
| `00 07` | Get Values | — | Current register dump |
| `00 08` | Reserved | — | — |
| `00 09` | Get Cal | — | EEPROM cal value |
| `00 0A` | Set WK1 Mode | — | — |
| `00 0B` | Set WK2 Mode | — | — |
| `00 0C` | Dump EEPROM | — | 256 bytes |
| `00 0D` | Load EEPROM | 256 bytes | — |
| `00 0E` | Send Standalone Msg | 1 byte (msg#) | — |
| `00 0F` | Load X2MODE | 1 byte (WK3) | — |
| `00 10` | Reserved | — | — |
| `00 11` | Set High Baud (9600) | — | — |
| `00 12` | Set Low Baud (1200) | — | — |
| `00 13` | Set WK3 Mode | — | — |
| `00 14`–`00 19` | Extended admin | Varies (WK3) | Varies |

### Immediate Commands (`0x01`–`0x1F`)

| Code | Name | Parameters | Notes |
|------|------|------------|-------|
| `01` | Sidetone Control | 1 byte | WK3: freq = 62500/value (500-4000Hz) |
| `02` | Set WPM Speed | 1 byte | 5–99 WPM |
| `03` | Set Weighting | 1 byte | 10–90, 50 = normal |
| `04` | Set PTT Lead-In/Tail | 2 bytes | Lead-in (×10ms), Tail (×10ms) |
| `05` | Speed Pot Setup | 3 bytes | Min WPM, Range, 0 |
| `06` | Pause | 1 byte | 1=pause, 0=resume |
| `07` | Get Speed Pot | — | Returns pot value byte |
| `08` | Backspace | — | Erase last unbuffered char |
| `09` | Pin Configuration | 1 byte | See Pin Config Register |
| `0A` | Clear Buffer | — | Immediate abort, clears everything |
| `0B` | Key Immediate | 1 byte | 1=key down (tune), 0=key up |
| `0C` | HSCW Speed | 1 byte | High-speed CW rate |
| `0D` | Farnsworth Speed | 1 byte | 10–99 WPM, 0=disabled |
| `0E` | WinKeyer Mode | 1 byte | See Mode Register |
| `0F` | Load Defaults | 15 bytes | See Load Defaults Order |
| `10` | First Extension | 1 byte | 0–250 ms |
| `11` | Key Compensation | 1 byte | 0–250 ms |
| `12` | Dit/Dah Ratio | 1 byte | 33–66, 50 = 1:3 |
| `13` | PTT On/Off | 1 byte | 1=assert PTT, 0=release |
| `14` | Software Paddle | 1 byte | Simulate paddle: bit0=dit, bit1=dah |
| `15` | Request Status | — | Requests current status byte |
| `16` | Pointer Command | 1+ bytes | Buffer pointer manipulation (see below) |
| `17` | Dit/Dah Ratio | 1 byte | 33–66, 50 = standard 1:3 |

**Buffered commands** (execute in FIFO sequence with text data):

| Code | Name | Parameters | Notes |
|------|------|------------|-------|
| `18` | Buffered PTT | 1 byte | 1=assert, 0=release at buffer position |
| `19` | Buffered Key Down | 1 byte | Key for N seconds, stalls buffer |
| `1A` | Buffered Wait | 1 byte | Pause N seconds, stalls buffer |
| `1B` | Merge Letters | 2 bytes | Prosign: two chars merged |
| `1C` | Buffered Speed Change | 1 byte | New WPM, takes effect in buffer |
| `1D` | Port Select | 1 byte | Key output port (WK3, for SO2R) |
| `1E` | Cancel Speed Change | — | Revert to pre-buffer speed |
| `1F` | Buffered NOP | — | No operation (buffer sync marker) |

**Pointer Commands (0x16):** Used heavily by N1MM+ for live callsign
editing during CW transmission. Subcodes manipulate the buffer insert
position, allowing characters already queued (but not yet sent) to be
overwritten. Not all WinKeyer-compatible devices fully support this.
Contest loggers that need real-time callsign correction depend on it.

### Load Defaults Parameter Order (15 bytes after `0x0F`)

| # | Parameter | Typical Default |
|---|-----------|-----------------|
| 1 | Mode Register | `0xD2` (see Mode Register) |
| 2 | Speed (WPM) | 24 |
| 3 | Sidetone | 5 (1000 Hz) |
| 4 | Weight | 50 |
| 5 | PTT Lead-In (×10ms) | 0 |
| 6 | PTT Tail (×10ms) | 5 (50ms) |
| 7 | Min WPM (speed pot) | 10 |
| 8 | WPM Range (speed pot) | 25 |
| 9 | First Extension | 0 |
| 10 | Key Compensation | 0 |
| 11 | Farnsworth WPM | 0 (disabled) |
| 12 | Paddle Setpoint | 50 |
| 13 | Dit/Dah Ratio | 50 |
| 14 | Pin Configuration | `0x05` (see Pin Config) |
| 15 | Don't Care | 0 |

### Mode Register (byte, bit field)

| Bit | Name | Values |
|-----|------|--------|
| 7 | Paddle watchdog disable | 1=disabled |
| 6 | Paddle echo back | 1=enabled |
| 5-4 | Keyer mode | 00=B, 01=A, 10=Ultimatic, 11=Bug |
| 3 | Paddle swap | 1=swapped |
| 2 | Serial echo back | 1=enabled |
| 1 | Autospace | 1=enabled |
| 0 | CT spacing | 1=contest spacing |

### Pin Configuration Register

| Bit | Name | Values |
|-----|------|--------|
| 7-6 | Reserved | 00 |
| 5 | PTT enable on pin 5 | 1=PTT output enabled |
| 4 | SO2R mode (WK3) | 1=SO2R enabled |
| 3 | Sidetone paddle only | 1=sidetone only from paddle |
| 2 | PTT enable | 1=PTT enabled |
| 1 | Sidetone on/off | 1=sidetone enabled |
| 0 | Key output invert | 1=inverted |

### Status Byte (WK → Host)

```
Bit 7: 1  ┐
Bit 6: 1  ┘ Status byte identifier (always 11)
Bit 5: Reserved
Bit 4: Wait — processing a buffered Wait or Key Down command
Bit 3: Keydown — key output is currently asserted (tune mode)
Bit 2: Busy — actively sending Morse, buffer has content
Bit 1: Breakin — paddle interrupt occurred, buffer cleared
Bit 0: XOFF — input buffer >2/3 full, host must stop sending text
```

Key status transitions for contest use:
- **Breakin set**: Operator touched paddle, host message was interrupted.
  Contest logger should stop sending queued exchange parts. The send buffer
  has been automatically cleared by the WinKeyer.
- **Busy cleared**: Buffer empty, last character sent. Logger can queue next
  message or return focus to receive.
- **XOFF set**: Host must stop sending text data (0x20-0x7F) until XOFF
  clears. Commands (0x01-0x1F) may still be sent regardless of XOFF.
  The 160-byte buffer (WK2/WK3) drains at ~5 chars/sec at 25 WPM,
  providing ~32 seconds of look-ahead — ample margin for contest use.

**Note:** Bit positions must be verified against the WK3 Datasheet v1.3 PDF
(Figure 10 or equivalent). The K1EL datasheets are the authoritative source.
Multiple open-source implementations disagree on the exact bit layout.

### Speed Pot Change (WK → Host)

```
Bit 7: 1  ┐
Bit 6: 0  ┘ Speed pot identifier (always 10)
Bits 5-0: Pot value (add to Min WPM from Speed Pot Setup)
```

### Open/Close Sequence

**Open:**
```
1. Send: 00 03        (close, in case already open — defensive)
2. Wait 1000ms
3. Send: 00 02        (open host mode — defaults to WK1 mode)
4. Wait 500ms
5. Read version byte  (23=WK2, 30=WK3.0, 31=WK3.1)
6. Send: 00 0B        (set WK2 mode) or 00 13 (set WK3 mode)
7. Send: 0F + 15 bytes (load defaults to sync state)
8. (Optional) Send: 00 11 (set high baud 9600), then switch host port
```

**Close:**
```
1. Send: 0A           (clear buffer)
2. Send: 00 03        (close host mode — baud reverts to 1200)
```

**Important:** Mode always defaults to WK1 on open. The library must
re-negotiate WK2/WK3 mode on every connection, even if the same physical
device was previously in WK3 mode.

### OTRSP Protocol (separate `otrsp` crate)

OTRSP is implemented as a **separate library** (`otrsp`), not a module inside
winkeyerlib. See [OTRSP Separation](#otrsp-separation) for rationale. The
protocol is documented here for completeness since it is commonly used
alongside WinKeyer.

| Parameter | Value |
|-----------|-------|
| Baud rate | 9600 |
| Format | 8N1 |
| Terminator | CR (`\r`, 0x0D) |

| Command | Description |
|---------|-------------|
| `TX1\r` | Route TX (key, mic, PTT) to Radio 1 |
| `TX2\r` | Route TX to Radio 2 |
| `RX1\r` | Route Radio 1 audio to both ears (mono) |
| `RX2\r` | Route Radio 2 audio to both ears (mono) |
| `RX1S\r` | Stereo: R1 left ear, R2 right ear, focus R1 |
| `RX2S\r` | Stereo: R1 left ear, R2 right ear, focus R2 |
| `AUX1nn\r` | Auxiliary output for Radio 1 (BCD antenna/band) |
| `AUX2nn\r` | Auxiliary output for Radio 2 (BCD antenna/band) |

Some implementations also use the **DSR modem line** for PTT assertion.

**Devices supporting OTRSP:** RigSelect Pro, microHAM MK2R+, YCCC SO2R Box,
SO2RDuino, and many others.

### Known Protocol Gotchas

These traps are documented by K1EL and discovered by implementors. The
library must handle all of them correctly:

1. **Never block waiting for a response.** The protocol is fully
   asynchronous. Status, speed pot, and echo bytes arrive unsolicited.
   A synchronous send-then-wait pattern will deadlock or miss data.

2. **Mode resets to WK1 on every open.** Even on a WK3 IC. Must
   re-negotiate WK2/WK3 mode on every connection.

3. **Baud resets to 1200 on close.** If using 9600 baud, the host must
   switch back to 1200 for the next open sequence.

4. **Two stop bits (8N2) required.** Many serial libraries default to 1
   stop bit. Using 8N1 at 1200 baud may appear to work initially but
   will produce framing errors under load.

5. **DTR/RTS must be asserted.** Some USB-serial converters gate power or
   reset through these lines. Not asserting them can prevent WK response.

6. **Paddle break-in clears the buffer automatically.** When the operator
   touches paddles during serial sending, the FIFO is flushed. Queued
   text simply disappears. The BREAKIN status bit notifies the host.

7. **XOFF is a status bit, not hardware flow control.** The XOFF bit in
   the status byte is the only buffer-full indication. There is no
   XON/XOFF character-level flow control.

8. **Clear Buffer (0x0A) is destructive.** It stops mid-character. The
   partially-sent character will be truncated on air. Acceptable for
   contest ESC-abort but not for casual use.

9. **Echo characters arrive after transmission.** There is a variable
   delay between queuing a character and receiving its echo. Do not use
   echo for timing-critical synchronization.

10. **Prosign merge waits for both characters.** `<1B>` does not send
    anything until both following bytes arrive. If interrupted between
    the two characters, nothing is sent. Send both bytes atomically.

11. **USB latency causes byte bursting.** FTDI latency timer (~16ms) can
    cause multiple bytes to arrive in a single read at 1200 baud (~9.2ms
    per byte). The parser must handle multi-byte reads correctly.

12. **Command bytes overlap ASCII control chars.** Bytes 0x01-0x1F are
    commands. User-supplied text must be filtered to 0x20-0x7F only.

13. **Close before open on reconnect.** If the host crashed without
    closing, WK may still be in host mode. Defensive close-before-open
    is mandatory.

14. **Buffered speed changes stack but don't nest.** Multiple `<1C>`
    commands store the same "original" speed. Cancel (0x1E) always
    restores the speed before the first buffered change.

---

## Architecture

### Component Diagram

```
┌──────────────────────────────────────────────────────────┐
│                    Contest Application                    │
│                                                          │
│  ┌──────────┐   ┌──────────────┐   ┌──────────────────┐ │
│  │  riglib   │   │ winkeyerlib  │   │     otrsp        │ │
│  │ (rig ctrl)│   │  (CW keying) │   │  (SO2R switch)   │ │
│  └────┬─────┘   └──────┬───────┘   └────────┬─────────┘ │
│       │                │                     │           │
└───────┼────────────────┼─────────────────────┼───────────┘
        │                │                     │
   CAT serial      WK serial (1200)     OTRSP serial (9600)
        │                │                     │
   ┌────┴────┐    ┌──────┴──────┐      ┌──────┴──────┐
   │  Radio  │    │  WinKeyer   │      │ SO2R Switch │
   │ IC-7610 │    │  WKUSB/etc  │      │ RigSelect   │
   └─────────┘    └──────┬──────┘      └──────┬──────┘
                         │                    │
                    KEY + PTT ────────────►  Radio
```

Three independent libraries, three independent serial ports, three
independent protocols. The contest application orchestrates all three.

### Internal Threading Model

```
┌─────────────────────────────────────────────────────┐
│                    winkeyerlib                       │
│                                                      │
│  ┌──────────────────────┐  ┌──────────────────────┐ │
│  │     Writer (main)     │  │    Reader Thread      │ │
│  │                       │  │                       │ │
│  │  wk.send_message()   │  │  loop {               │ │
│  │  wk.set_speed()      │  │    byte = port.read() │ │
│  │  wk.clear_buffer()   │  │    match classify() { │ │
│  │       │               │  │      Status => tx()   │ │
│  │       ▼               │  │      SpeedPot => tx() │ │
│  │  port.write(bytes)    │  │      Echo => tx()     │ │
│  │  (Mutex<SerialPort>)  │  │    }                  │ │
│  │                       │  │  }                    │ │
│  └──────────────────────┘  └──────────┬───────────┘ │
│                                       │              │
│                              ┌────────▼────────┐    │
│                              │ crossbeam channel│    │
│                              │  (events)        │    │
│                              └────────┬────────┘    │
│                                       │              │
└───────────────────────────────────────┼──────────────┘
                                        │
                                        ▼
                              Application event loop
```

### Key Design Principles

1. **Zero-copy protocol parsing.** Parse bytes in-place in the reader thread.
   No heap allocation for status/speed pot events.

2. **Lock-free event delivery.** Reader thread sends events via bounded
   crossbeam channel. No mutex on the read path.

3. **Minimal writer contention.** Serial writes are short (1-3 bytes for
   commands, variable for text). Mutex held briefly. Text messages can be
   written in one `write()` call since the WinKeyer buffers internally.

4. **Deterministic shutdown.** Close sequence (clear buffer + close host
   mode) always runs, even on drop. Reader thread joins cleanly.

5. **No implicit state.** The library does not cache WinKeyer state. The
   WinKeyer is the source of truth. Status events tell you what changed.
   Exception: speed pot position and XOFF state are tracked internally.

6. **XOFF flow control.** The reader thread monitors the XOFF bit in
   status bytes. When XOFF is set, `send_message()` blocks (or returns
   an error in try-send mode) until XOFF clears. Commands (0x01-0x1F)
   bypass the buffer and are always allowed. This prevents buffer overrun
   without requiring the caller to understand WinKeyer flow control.

7. **Paddle break-in awareness.** When the BREAKIN status bit is set, the
   WinKeyer has already cleared its buffer. The library emits a distinct
   `PaddleBreakIn` event so contest apps can stop queuing additional
   message parts immediately.

---

## Crate Structure

Two separate crates in separate repositories:

### `winkeyerlib` — CW keying via WinKeyer hardware

```
winkeyerlib/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public re-exports
│   ├── error.rs            # Error types (thiserror)
│   ├── protocol.rs         # Byte-level codec: encode commands, decode responses
│   ├── types.rs            # WinKeyer types: Mode, PinConfig, Status, Version, etc.
│   ├── transport.rs        # Serial port abstraction trait + impl
│   ├── winkeyer.rs         # WinKeyer struct — the primary sync API
│   ├── reader.rs           # Reader thread: byte classification + event dispatch
│   ├── config.rs           # Builder/config for WinKeyer connection
│   ├── events.rs           # Event enum: Status, SpeedPot, Echo, PaddleBreakIn
│   └── async_wrapper.rs    # AsyncWinKeyer — tokio wrapper (optional feature)
├── examples/
│   ├── send_cw.rs          # Basic CW sending
│   ├── monitor.rs          # Status/speed pot/echo monitor
│   └── contest_keyer.rs    # Full contest-style keying with ESM
├── tests/
│   ├── protocol_tests.rs   # Codec unit tests
│   └── integration.rs      # Mock serial port integration tests
└── README.md
```

### `otrsp` — SO2R radio switching (separate repo)

```
otrsp/
├── Cargo.toml
├── src/
│   ├── lib.rs              # OtrspController, public API
│   └── error.rs            # Error types
├── examples/
│   └── so2r_switch.rs      # Basic radio switching demo
├── tests/
│   └── protocol_tests.rs   # Command encoding tests
└── README.md
```

### OTRSP Separation

OTRSP is a separate crate because:

- **Zero protocol relationship to WinKeyer.** OTRSP is ASCII text at 9600/8N1.
  WinKeyer is binary at 1200/8N2. They share nothing.
- **Independent use cases.** A contest app may use OTRSP without WinKeyer
  (e.g., using riglib's built-in CW via CAT + OTRSP for radio switching).
  Or use WinKeyer without OTRSP (single-radio station).
- **Different devices.** OTRSP is supported by devices that have no WinKeyer
  (YCCC SO2R Box, SO2RDuino). Bundling it with winkeyerlib would force a
  false dependency.
- **Trivially small.** The entire OTRSP implementation is ~200 lines. It
  deserves to be a focused, zero-dependency crate (besides `serialport`).
- **On the RigSelect Pro**, WinKeyer and OTRSP are on different serial ports
  managed by different USB chips. They are physically independent.

### winkeyerlib Feature Flags

```toml
[features]
default = []
async = ["tokio", "tokio-util"]  # AsyncWinKeyer wrapper
```

### winkeyerlib Dependencies

```toml
[dependencies]
serialport = "4"              # Cross-platform serial I/O
crossbeam-channel = "0.5"     # Lock-free MPMC channels
thiserror = "2"               # Error derive
log = "0.4"                   # Logging facade
bitflags = "2"                # Status/mode register bit fields

[dependencies.tokio]
version = "1"
features = ["rt", "sync", "macros"]
optional = true

[dev-dependencies]
env_logger = "0.11"
```

### otrsp Dependencies

```toml
[dependencies]
serialport = "4"
thiserror = "2"
log = "0.4"
```

---

## API Design

### Core Types

```rust
/// WinKeyer hardware version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinKeyerVersion {
    Wk2,          // version byte 23
    Wk3,          // version byte 30
    Wk31,         // version byte 31
}

/// Keyer mode (iambic A, B, ultimatic, bug).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyerMode {
    IambicB,
    IambicA,
    Ultimatic,
    Bug,
}

/// Events from the WinKeyer (unsolicited).
#[derive(Debug, Clone)]
pub enum WinKeyerEvent {
    /// Status byte changed. Contains decoded status.
    StatusChanged(Status),
    /// Speed pot position changed. Contains new WPM.
    SpeedPotChanged(u8),
    /// Character echo — the WinKeyer is sending this character now.
    CharEcho(char),
    /// Paddle break-in — operator interrupted with paddle.
    PaddleBreakIn,
    /// Connection lost (serial port error).
    Disconnected(String),
}

/// Decoded WinKeyer status byte.
#[derive(Debug, Clone, Copy)]
pub struct Status {
    pub xoff: bool,       // Buffer >2/3 full, stop sending text
    pub breakin: bool,    // Paddle interrupted host message, buffer cleared
    pub busy: bool,       // Actively sending or buffer has content
    pub keydown: bool,    // Key output is currently asserted (tune)
    pub waiting: bool,    // Processing a buffered Wait/KeyDown command
    pub raw: u8,          // Raw status byte for advanced use
}

/// WinKeyer configuration (Load Defaults parameters + connection settings).
pub struct Config {
    pub port: String,
    pub mode: KeyerMode,
    pub speed_wpm: u8,
    pub sidetone: u8,
    pub weight: u8,
    pub ptt_lead_in_ms: u16,    // Granularity: 10ms
    pub ptt_tail_ms: u16,       // Granularity: 10ms
    pub min_wpm: u8,
    pub wpm_range: u8,
    pub first_extension_ms: u8,
    pub key_compensation_ms: u8,
    pub farnsworth_wpm: u8,     // 0 = disabled
    pub paddle_setpoint: u8,
    pub dit_dah_ratio: u8,      // 50 = 1:3 standard
    pub pin_config: PinConfig,
    pub paddle_swap: bool,
    pub autospace: bool,
    pub contest_spacing: bool,
    pub serial_echo: bool,
    pub paddle_echo: bool,
}
```

### Primary API (Sync)

```rust
impl WinKeyer {
    /// Open a connection to a WinKeyer device.
    pub fn open(config: Config) -> Result<Self>;

    /// Close the connection gracefully.
    pub fn close(self) -> Result<()>;

    /// Get the detected hardware version.
    pub fn version(&self) -> WinKeyerVersion;

    // --- CW Transmission ---

    /// Send a CW message. Characters are buffered in the WinKeyer.
    /// Respects XOFF flow control: blocks if the WinKeyer's 160-byte
    /// buffer is >2/3 full, resumes when XOFF clears.
    /// Only accepts ASCII 0x20-0x7F; returns error for control chars.
    pub fn send_message(&self, text: &str) -> Result<()>;

    /// Try to send a CW message without blocking on XOFF.
    /// Returns `Error::BufferFull` if XOFF is currently set.
    pub fn try_send_message(&self, text: &str) -> Result<()>;

    /// Send a prosign (two merged characters, e.g., "AR", "SK").
    pub fn send_prosign(&self, c1: char, c2: char) -> Result<()>;

    /// Clear the send buffer and abort current character.
    pub fn clear_buffer(&self) -> Result<()>;

    // --- Speed Control ---

    /// Set CW speed in WPM (5–99).
    pub fn set_speed(&self, wpm: u8) -> Result<()>;

    /// Set CW speed change within the buffer (takes effect in sequence).
    pub fn set_buffered_speed(&self, wpm: u8) -> Result<()>;

    /// Cancel a buffered speed change, reverting to the base speed.
    pub fn cancel_buffered_speed(&self) -> Result<()>;

    /// Read the current speed pot value (returns WPM).
    pub fn get_speed_pot(&self) -> Result<u8>;

    /// Configure the speed pot range.
    pub fn set_speed_pot_range(&self, min_wpm: u8, range: u8) -> Result<()>;

    // --- PTT & Keying ---

    /// Assert or release PTT manually.
    pub fn set_ptt(&self, on: bool) -> Result<()>;

    /// Key down (tune mode). Pass true to key, false to un-key.
    pub fn set_key(&self, on: bool) -> Result<()>;

    /// Set PTT lead-in and tail times.
    pub fn set_ptt_timing(&self, lead_in_ms: u16, tail_ms: u16) -> Result<()>;

    // --- Configuration ---

    /// Set keyer mode (Iambic A/B, Ultimatic, Bug).
    pub fn set_keyer_mode(&self, mode: KeyerMode) -> Result<()>;

    /// Set dit/dah weight (10–90, 50 = standard).
    pub fn set_weight(&self, weight: u8) -> Result<()>;

    /// Set dit/dah ratio (33–66, 50 = standard 1:3).
    pub fn set_ratio(&self, ratio: u8) -> Result<()>;

    /// Set Farnsworth speed (0 = disabled, 10–99 WPM).
    pub fn set_farnsworth(&self, wpm: u8) -> Result<()>;

    /// Set sidetone frequency.
    pub fn set_sidetone(&self, value: u8) -> Result<()>;

    /// Load all defaults at once (re-syncs all 15 parameters).
    pub fn load_defaults(&self, config: &Config) -> Result<()>;

    /// Pause or resume sending.
    pub fn set_pause(&self, paused: bool) -> Result<()>;

    /// Insert a buffered delay.
    pub fn buffered_wait(&self, time_10ms: u8) -> Result<()>;

    // --- Events ---

    /// Subscribe to WinKeyer events. Returns a crossbeam Receiver.
    pub fn subscribe(&self) -> crossbeam_channel::Receiver<WinKeyerEvent>;

    // --- Buffer Pointer (for live callsign editing) ---

    /// Set the buffer insert position for overwriting queued characters.
    /// Used by contest loggers for real-time callsign correction.
    pub fn pointer_command(&self, subcmd: PointerCmd) -> Result<()>;

    // --- Software Paddle ---

    /// Simulate paddle input from the host.
    /// dit=true for dit paddle, dah=true for dah paddle.
    pub fn software_paddle(&self, dit: bool, dah: bool) -> Result<()>;

    /// Request the current status byte (explicit poll).
    pub fn request_status(&self) -> Result<()>;

    // --- Diagnostics ---

    /// Send an echo test byte and verify the response.
    pub fn echo_test(&self, byte: u8) -> Result<u8>;
}

impl Drop for WinKeyer {
    // Runs close sequence: clear_buffer + close host mode
}
```

### OTRSP API (separate `otrsp` crate)

```rust
pub struct OtrspController {
    // ...
}

impl OtrspController {
    /// Open an OTRSP connection.
    pub fn open(port: &str) -> Result<Self>;

    /// Select Radio 1 for transmit.
    pub fn tx_radio1(&self) -> Result<()>;

    /// Select Radio 2 for transmit.
    pub fn tx_radio2(&self) -> Result<()>;

    /// Route Radio 1 audio to headphones (mono).
    pub fn rx_radio1(&self) -> Result<()>;

    /// Route Radio 2 audio to headphones (mono).
    pub fn rx_radio2(&self) -> Result<()>;

    /// Stereo mode focused on Radio 1.
    pub fn rx_radio1_stereo(&self) -> Result<()>;

    /// Stereo mode focused on Radio 2.
    pub fn rx_radio2_stereo(&self) -> Result<()>;

    /// Set auxiliary output for a radio (BCD band decoder value).
    pub fn set_aux(&self, radio: u8, value: u8) -> Result<()>;
}
```

### Async Wrapper (behind `async` feature)

```rust
pub struct AsyncWinKeyer {
    inner: WinKeyer,
}

impl AsyncWinKeyer {
    pub async fn open(config: Config) -> Result<Self>;

    /// All methods delegate to sync via spawn_blocking.
    pub async fn send_message(&self, text: &str) -> Result<()>;
    pub async fn set_speed(&self, wpm: u8) -> Result<()>;
    pub async fn clear_buffer(&self) -> Result<()>;
    // ... etc

    /// Returns a tokio broadcast Receiver (bridged from crossbeam).
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<WinKeyerEvent>;
}
```

---

## Implementation Phases

### Phase 1: Protocol Codec + Types

**Goal:** Pure logic, no I/O. Complete byte-level encode/decode.

**Files:** `types.rs`, `protocol.rs`, `error.rs`

**Deliverables:**
- All type definitions: `WinKeyerVersion`, `KeyerMode`, `Status`, `PinConfig`,
  `ModeRegister`, `WinKeyerEvent`
- `bitflags!` macros for `ModeRegister`, `PinConfig`, `Status`
- `protocol::encode` module:
  - `admin_open() -> [u8; 2]`
  - `admin_close() -> [u8; 2]`
  - `admin_echo_test(byte: u8) -> [u8; 3]`
  - `admin_set_wk2_mode() -> [u8; 2]`
  - `admin_set_wk3_mode() -> [u8; 2]`
  - `set_speed(wpm: u8) -> [u8; 2]`
  - `set_weight(w: u8) -> [u8; 2]`
  - `set_ptt_timing(lead: u8, tail: u8) -> [u8; 3]`
  - `set_sidetone(val: u8) -> [u8; 2]`
  - `set_mode(reg: ModeRegister) -> [u8; 2]`
  - `set_pin_config(cfg: PinConfig) -> [u8; 2]`
  - `load_defaults(params: &LoadDefaults) -> [u8; 16]`
  - `clear_buffer() -> [u8; 1]`
  - `key_immediate(on: bool) -> [u8; 2]`
  - `set_ptt(on: bool) -> [u8; 2]`
  - `set_pause(on: bool) -> [u8; 2]`
  - `get_speed_pot() -> [u8; 1]`
  - `speed_pot_setup(min: u8, range: u8) -> [u8; 4]`
  - `merge_letters(c1: u8, c2: u8) -> [u8; 3]`
  - `set_farnsworth(wpm: u8) -> [u8; 2]`
  - `set_ratio(ratio: u8) -> [u8; 2]`
  - `set_first_extension(ms: u8) -> [u8; 2]`
  - `set_key_compensation(ms: u8) -> [u8; 2]`
  - `buffered_speed_change(wpm: u8) -> [u8; 2]`
  - `cancel_speed_change() -> [u8; 1]`
  - `buffered_wait(time: u8) -> [u8; 2]`
  - `buffered_ptt(on: bool) -> [u8; 2]`
  - `buffered_key_down(seconds: u8) -> [u8; 2]`
  - `buffered_port_select(port: u8) -> [u8; 2]`
  - `buffered_nop() -> [u8; 1]`
  - `software_paddle(dit: bool, dah: bool) -> [u8; 2]`
  - `request_status() -> [u8; 1]`
  - `pointer_command(sub: u8, ...) -> Vec<u8>`
  - `admin_set_wk2_mode() -> [u8; 2]`
  - `admin_set_wk3_mode() -> [u8; 2]`
  - `admin_set_high_baud() -> [u8; 2]`
  - `admin_get_values() -> [u8; 2]`
- `protocol::decode` module:
  - `classify_byte(byte: u8) -> ResponseByte` enum
  - `decode_status(byte: u8) -> Status`
  - `decode_speed_pot(byte: u8, min_wpm: u8) -> u8`
  - `decode_echo(byte: u8) -> char`
  - `decode_version(byte: u8) -> Option<WinKeyerVersion>`
- Error types: `Error`, `Result<T>`
- **200+ unit tests** for all encode/decode round-trips, edge cases, byte
  boundary classification

**Verification:** `cargo test`, `cargo clippy -- -D warnings`

---

### Phase 2: Transport + Reader Thread

**Goal:** Serial port I/O, reader thread with event dispatch.

**Files:** `transport.rs`, `reader.rs`

**Deliverables:**
- `Transport` trait:
  ```rust
  pub trait Transport: Send {
      fn write(&mut self, data: &[u8]) -> Result<()>;
      fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
      fn set_dtr(&mut self, on: bool) -> Result<()>;
  }
  ```
- `SerialTransport` impl (wraps `serialport::SerialPort`):
  - Opens port at 1200/8N2
  - DTR/DSR flow control
  - Configurable read timeout (short, e.g. 10ms, for responsive shutdown)
- `MockTransport` for testing (in-memory ring buffer)
- Reader thread (`reader.rs`):
  - Spawns `std::thread::spawn` with the read half
  - Continuously reads bytes, classifies via `protocol::classify_byte()`
  - Handles multi-byte reads (USB latency causes bursting)
  - Sends `WinKeyerEvent` via bounded `crossbeam_channel::Sender`
  - Updates shared `AtomicBool` for XOFF state (writer checks before send)
  - Monitors a shutdown flag (`AtomicBool`) for clean exit
  - Logs all decoded events at `trace!` level
- Channel sizing: bounded(64) — more than enough at 1200 baud
- Thread naming: `"winkeyer-reader-{port}"`

**Verification:** Unit tests with `MockTransport`, verify event dispatch for
all byte types, verify clean shutdown.

---

### Phase 3: WinKeyer Core API

**Goal:** The primary `WinKeyer` struct with full sync API.

**Files:** `winkeyer.rs`, `config.rs`

**Deliverables:**
- `Config` struct with builder-style defaults:
  ```rust
  let config = Config::new("/dev/ttyUSB0")
      .speed(28)
      .keyer_mode(KeyerMode::IambicB)
      .ptt_lead_in_ms(40)
      .ptt_tail_ms(30)
      .contest_spacing(true);
  ```
- `WinKeyer::open(config)`:
  1. Open serial port via `SerialTransport`
  2. Send close (defensive), wait 1s
  3. Send open, wait 500ms, read version byte
  4. Validate version (WK2 or WK3)
  5. Send Load Defaults
  6. Spawn reader thread
  7. Return `WinKeyer` handle
- `WinKeyer::close(self)`:
  1. Send clear buffer
  2. Send close host mode
  3. Signal reader thread shutdown
  4. Join reader thread
- All command methods from the API Design section
- `Drop` impl calls `close()` (best-effort, logs errors)
- `send_message()` validates input: ASCII 0x20-0x7F only, rejects others
  with descriptive error
- Thread safety: `Mutex<SerialTransport>` for writes, reader thread owns
  the read half (split the serial port, or clone the fd)

**Verification:** Integration tests with `MockTransport`. Test open/close
handshake, send message, speed changes, event subscription.

---

### Phase 4: Advanced Features

**Goal:** Complete protocol coverage for contest use.

**Files:** Updates to `winkeyer.rs`, `protocol.rs`, `types.rs`

**Deliverables:**
- Prosign support with common prosigns as constants:
  ```rust
  pub const PROSIGN_AR: (char, char) = ('A', 'R');
  pub const PROSIGN_SK: (char, char) = ('S', 'K');
  pub const PROSIGN_BT: (char, char) = ('B', 'T');
  pub const PROSIGN_KN: (char, char) = ('K', 'N');
  ```
- HSCW/QRSS mode support (WK3 only)
- Speed pot monitoring: reader thread tracks current WPM, API exposes
  `current_speed_pot_wpm() -> u8` (atomic read, no lock)
- Echo character tracking for contest apps that need to know when each
  character has been sent (useful for real-time display)
- Admin commands: echo test, get values, firmware version query
- WK3-specific: X2MODE register, port select, extended admin commands
- Paddle break-in detection: when status byte shows breakin, emit
  `WinKeyerEvent::PaddleBreakIn` as a distinct event (in addition to
  `StatusChanged`) for easy filtering
- Backspace support for correcting queued characters
- Key buffering on/off control

**Verification:** Unit tests for all new protocol commands. Integration tests
for prosign encoding, speed pot tracking.

---

### Phase 5: OTRSP Crate (separate repo)

**Goal:** SO2R radio switching as an independent library.

**Repo:** `otrsp` (separate from winkeyerlib)

**Deliverables:**
- `OtrspController::open(port)` — opens at 9600/8N1
- All 8 OTRSP commands: `tx_radio1()`, `tx_radio2()`, `rx_radio1()`,
  `rx_radio2()`, `rx_radio1_stereo()`, `rx_radio2_stereo()`,
  `set_aux(radio, value)`
- Optional DSR-based PTT assertion
- Convenience: `OtrspController::tx_radio(n: u8)` for variable selection
- `Drop` impl closes serial port
- **Works with:** RigSelect Pro, microHAM MK2R+, YCCC SO2R Box, SO2RDuino,
  any OTRSP-compatible device
- README with supported devices and integration examples
- Complete crate: Cargo.toml, CI, docs, ready for crates.io

**Verification:** Unit tests for command encoding. Integration test with mock
serial port. `cargo doc --no-deps` clean.

---

### Phase 6: Async Wrapper

**Goal:** Tokio-compatible API for async applications.

**Files:** `async_wrapper.rs`

**Deliverables:**
- `AsyncWinKeyer` wrapping `WinKeyer`:
  - All methods delegate via `tokio::task::spawn_blocking`
  - Event bridge: background task reads from crossbeam channel, rebroadcasts
    via `tokio::sync::broadcast`
- Feature-gated behind `async`

**Verification:** Async integration tests using `#[tokio::test]`.

---

### Phase 7: Test App + Examples

**Goal:** Usable CLI tool for testing and validation with real hardware.

**Files:** `examples/` directory

**Deliverables:**
- `send_cw` example: connect, send message, disconnect
- `monitor` example: connect, print all events (status, speed pot, echo)
- `multi_keyer` example: two WinKeyers managed independently
- `contest_keyer` example: full contest-style operation with:
  - F1-F12 message macros
  - ESM (Enter Sends Message) pattern
  - Paddle break-in detection and handling
  - Speed pot tracking with display
- Test app with clap CLI (similar to riglib's test-app):
  - `winkeyer send "CQ TEST K3LR"`
  - `winkeyer speed 32`
  - `winkeyer tune on`
  - `winkeyer monitor`

**Verification:** Manual testing with real hardware where available.

---

### Phase 8: Polish + CI

**Goal:** Production-ready quality.

**Deliverables:**
- Comprehensive `rustdoc` documentation with examples
- CI pipeline: test, clippy, fmt, MSRV check
- Cross-platform CI (Linux, macOS, Windows)
- README with:
  - Quick start
  - Supported devices
  - Architecture overview
  - Integration with riglib
  - SO2R setup guide
- `CHANGELOG.md`
- `crates.io` metadata in `Cargo.toml`
- License file (MIT)

**Verification:** CI green, `cargo doc --no-deps` clean, all examples build.

---

## Reference Sources

- [K1EL WinKeyer 3.1 Datasheet](https://www.k1elsystems.com/files/WK3_Datasheet_v1.3.pdf) — authoritative protocol spec
- [K1EL WinKeyer 2 Datasheet](https://www.k1elsystems.com/files/WK2_Datasheet_v23.pdf) — WK2 protocol spec
- [WKUSB Manual v1.4](https://www.hamcrafters2.com/files/WKUSB_Manual_v1.4.pdf) — user-level reference
- [OTRSP Protocol Specification](https://k1xm.org/OTRSP/OTRSP_Protocol.pdf) — SO2R switching protocol
- [PyWinKeyerSerial](https://github.com/mbridak/PyWinKeyerSerial) — Python implementation reference
- [K3NG CW Keyer](https://github.com/k3ng/k3ng_cw_keyer) — Arduino WinKeyer emulation (protocol source of truth)
- [TeensyWinkeyEmulator](https://github.com/dl1ycf/TeensyWinkeyEmulator) — another WK emulator reference
- [winkeydaemon](https://github.com/N0NB/winkeydaemon) — Perl WK host driver
- [pywinkeyerdaemon](https://github.com/drewarnett/pywinkeyerdaemon) — Python WK UDP bridge
- [RigSelect Pro](https://rigselect.com/) — embedded WK3 + OTRSP device
- [RigSelect N1MM App Note](https://rigselect.com/rigselect/RigSelect%20Assets/App%20Note%20-%20RigSelect%20used%20with%20N1MM%20Logger.pdf) — integration guide
- [N1MM + WinKeyer Engineering Notes](http://www.pj2t.org/ccc/winkeyer.and.n1mm.engineering.notes.pdf) — pointer commands, advanced integration
- [HamGadgets MK-1 WK Protocol](https://www.manualslib.com/manual/1341954/Hamgadgets-Masterkeyer-Mk-1.html?page=42) — third-party protocol reference
- [fldigi WinKeyer Interface](http://www.w1hkj.com/FldigiHelp/cw_winkeyer_page.html) — another implementation reference

**Important:** The exact byte-level command codes, status byte bit positions,
and Load Defaults parameter order in this plan were assembled from web search
excerpts, open-source implementations, and manual cross-referencing. Before
implementing Phase 1, the WK3 Datasheet v1.3 PDF must be obtained and used as
the authoritative source. Where this plan conflicts with the datasheet, the
datasheet wins. No existing Rust implementation of the WK protocol was found.
