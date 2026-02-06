# Research, Radio Catalog, and Architecture Proposal

**Document**: 002-research-and-architecture.md
**Date**: 2026-02-06
**Status**: Draft for user review
**Previous**: 001-initial-planning.md (implicit; user Q&A session)

---

## Table of Contents

1. [Supported Radio Catalog](#1-supported-radio-catalog)
2. [Protocol Deep Dive](#2-protocol-deep-dive)
3. [FlexRadio Architectural Impact Analysis](#3-flexradio-architectural-impact-analysis)
4. [Revised Crate Architecture](#4-revised-crate-architecture)
5. [Phasing Strategy](#5-phasing-strategy)
6. [Follow-Up Questions for User](#6-follow-up-questions-for-user)

---

## 1. Supported Radio Catalog

All transceivers released 2010 or later from Icom, Elecraft, FlexRadio, and Kenwood.

### 1.1 Icom (CI-V Protocol)

All modern Icom amateur radios use the CI-V (Communication Interface V) protocol.
Newer models expose USB (virtual serial), and some add Ethernet/WiFi for both
control and audio streaming.

| Model | Year | Bands | Interface(s) | Protocol | Notable Features |
|-------|------|-------|--------------|----------|-----------------|
| IC-9100 | 2011 | HF/VHF/UHF/1.2G | USB (CI-V + audio) | CI-V | Satellite mode, optional D-STAR, optional 1.2 GHz |
| IC-7410 | 2012 | HF/6m | USB (CI-V + audio) | CI-V | Replaced IC-7400 |
| IC-7100 | 2012 | HF/VHF/UHF | USB (CI-V + audio) | CI-V | Detachable head, D-STAR built-in, touch screen |
| IC-7850 | 2015 | HF/6m | USB (CI-V + audio), LAN | CI-V | Limited edition 50th anniversary, 200W, dual RX |
| IC-7851 | 2015 | HF/6m | USB (CI-V + audio), LAN | CI-V | Production version of IC-7850, flagship, dual RX |
| IC-7300 | 2016 | HF/6m | USB (CI-V + audio) | CI-V | Direct-sampling SDR, first affordable SDR HF rig, 100K+ sold |
| IC-R8600 | 2017 | 10 kHz - 3 GHz | USB (CI-V + audio), LAN | CI-V | Wideband receiver (not transceiver), SDR architecture |
| IC-7610 | 2017 | HF/6m | USB (CI-V + audio), LAN | CI-V | Dual independent receivers, direct-sampling SDR, contest-grade |
| IC-9700 | 2019 | 2m/70cm/23cm | USB (CI-V + audio), LAN | CI-V | Direct-sampling SDR, satellite mode |
| IC-705 | 2020 | HF/VHF/UHF | USB-C (CI-V + audio), WiFi, Bluetooth | CI-V | Portable QRP (10W), GPS built-in, touch screen, D-STAR |
| IC-905 | 2022 | 144 MHz - 10 GHz | LAN (to RF unit), USB | CI-V | SHF/microwave, separate RF deck + control head |
| IC-7760 | 2024 | HF/6m | USB, LAN | CI-V | 200W, separate RF deck + control head, shack-style |
| IC-7300MK2 | 2025 | HF/6m(/4m) | USB-C, LAN, HDMI | CI-V | Successor to IC-7300, improved RMDR, built-in CW decoder, Ethernet |

**CI-V Interface Notes:**
- All models use CI-V protocol over USB virtual serial port (typically Silicon Labs or FTDI chip)
- CI-V uses BCD-encoded frequency data, command/sub-command structure
- Default baud: 19200 for most models; 115200 for IC-7300 and newer
- Collision detection needed on shared CI-V bus (less relevant with USB)
- LAN-equipped models (IC-7610, IC-9700, IC-705, IC-905, IC-7760, IC-7300MK2) support:
  - CI-V over UDP (port 50001/50002)
  - Audio streaming over UDP (Icom proprietary protocol)
  - Remote power on/off via network
- Audio streaming over USB is available on all models listed (PCM audio codec device)

**Audio Streaming Capability:**
- **USB audio**: All models above expose a USB audio codec (receive + transmit audio)
- **Network audio**: IC-7610, IC-9700, IC-705, IC-905, IC-7760, IC-7300MK2 support
  Icom's proprietary UDP audio streaming (used by RS-BA1 and wfview)

**Scope/Exclusion Consideration:**
- The IC-R8600 is a receiver only. Include or exclude?
- Models pre-2010 (IC-7600, IC-7200, IC-7000) are excluded per the 2010+ cutoff,
  though many are still in active use.

### 1.2 Elecraft (Extended Kenwood Protocol)

Elecraft radios use an extended version of the Kenwood serial protocol (text-based
commands). The K4 adds Ethernet connectivity.

| Model | Year | Bands | Interface(s) | Protocol | Notable Features |
|-------|------|-------|--------------|----------|-----------------|
| KX3 | 2012 | HF/6m | USB (serial + audio) | Extended Kenwood | QRP portable (15W), all-mode, panadapter via P3 |
| K3S | 2015 | HF/6m | USB (serial), RS-232 | Extended Kenwood | 100W, dual RX, upgraded K3, contest-grade |
| KX2 | 2016 | HF (80-10m) | USB (serial + audio) | Extended Kenwood | Ultra-portable QRP (12W), pocket-sized |
| K4 | 2020 | HF/6m | USB (serial + audio), RS-232, Ethernet | Extended Kenwood | Direct-sampling SDR, 100W, dual RX, K4D variant |

**Protocol Notes:**
- Text-based ASCII command/response protocol (e.g., `FA00014250000;` to set VFO A)
- K3/K3S/KX3/KX2 share a common command set with minor model-specific extensions
- K4 has an expanded command set with new commands for SDR-specific features
- K4 Ethernet: supports direct TCP control (same command syntax as serial)
- AI (Auto Information) mode: radio pushes state changes without polling
- Two virtual COM ports over USB: one for control, one for audio/data

**Audio Streaming:**
- USB audio codec available on KX3, KX2, and K4
- K4 may support network audio over Ethernet (to be confirmed)

### 1.3 FlexRadio (SmartSDR API)

FlexRadio radios are network-only SDR transceivers. There is NO serial interface.
All communication is over Ethernet using the SmartSDR API.

| Model | Year | Bands | Interface(s) | Protocol | Notable Features |
|-------|------|-------|--------------|----------|-----------------|
| FLEX-6300 | 2014 | HF/6m | Ethernet (1 GbE) | SmartSDR | Entry-level, 1 SCU, 2 slices, 100W |
| FLEX-6500 | 2014 | HF/6m | Ethernet (1 GbE) | SmartSDR | 1 SCU, 4 slices, 100W, discontinued |
| FLEX-6700 | 2013 | HF/6m | Ethernet (1 GbE) | SmartSDR | 2 SCUs, 8 slices, 100W, flagship (discontinued 2023) |
| FLEX-6400 | 2018 | HF/6m | Ethernet (1 GbE) | SmartSDR | Replaced 6300, 1 SCU, 2 slices, 100W |
| FLEX-6400M | 2018 | HF/6m | Ethernet (1 GbE) | SmartSDR | 6400 with built-in Maestro front panel |
| FLEX-6600 | 2018 | HF/6m | Ethernet (1 GbE) | SmartSDR | Replaced 6500, 1 SCU, 4 slices, 100W |
| FLEX-6600M | 2018 | HF/6m | Ethernet (1 GbE) | SmartSDR | 6600 with built-in Maestro front panel |
| FLEX-8400 | 2024 | HF/6m | Ethernet (1 GbE) | SmartSDR | Next-gen, 1 SCU, 4x CPU, 2x FPGA vs 6400 |
| FLEX-8400M | 2024 | HF/6m | Ethernet (1 GbE) | SmartSDR | 8400 with built-in front panel |
| FLEX-8600 | 2024 | HF/6m | Ethernet (1 GbE) | SmartSDR | 2 SCUs, 4 slices, 7th-order BPF, SO2R capable |
| FLEX-8600M | 2024 | HF/6m | Ethernet (1 GbE) | SmartSDR | 8600 with built-in front panel |

**SmartSDR API Architecture:**

```
  Client App
      |
      +--- TCP port 4992 -----> Command / Response / Status
      |
      +--- UDP port 4991 <----- VITA-49 Streams (IQ, audio, FFT, meters)
      |
      +--- UDP port 4992 <----- Discovery broadcasts
```

- **Command channel** (TCP 4992): ASCII text protocol
  - Command format: `C[D]<seq>|<command>\r\n`
  - Response format: `R<seq>|<hex_status>|<message>`
  - Status format: `S<handle>|<object> <key>=<value> ...`
  - Example: `C42|slice tune 0 14.250` (tune slice 0 to 14.250 MHz)
- **Data streams** (UDP 4991): VITA-49 framing for:
  - IQ data (per-slice or wideband)
  - Demodulated audio (per-slice, DAX channels)
  - FFT/panadapter data
  - Meter data (S-meter, power, SWR, etc.)
- **Discovery** (UDP 4992 broadcast): radio announces itself on LAN with model,
  serial number, nickname, IP address, firmware version
- **Authentication**: SmartLink (remote access) uses tokens; local LAN access
  typically requires no authentication
- **Multi-client**: Multiple clients can connect simultaneously; each gets a
  unique 32-bit handle
- **Slice model**: Instead of VFO A/VFO B, FlexRadio uses numbered "slices"
  (0-7 depending on model). Each slice has its own frequency, mode, filter
  width, audio gain, and can be independently routed.

**Audio Streaming:**
- All audio goes through VITA-49 UDP streams (DAX channels)
- Supports RX audio, TX audio (mic input), and IQ data per slice
- No USB audio -- all audio is network-based

### 1.4 Kenwood (Kenwood CAT Protocol)

Kenwood radios use a text-based serial command protocol (the original "Kenwood"
protocol that Elecraft extended). Recent models are HF-focused; Kenwood has
largely exited the amateur VHF/UHF base/mobile market.

| Model | Year | Bands | Interface(s) | Protocol | Notable Features |
|-------|------|-------|--------------|----------|-----------------|
| TS-590S | 2010 | HF/6m | USB (serial), RS-232 | Kenwood CAT | 100W, DSP, roofing filters |
| TS-990S | 2013 | HF/6m | USB (serial), RS-232, LAN | Kenwood CAT | 200W flagship, dual RX, dual displays |
| TS-590SG | 2014 | HF/6m | USB (serial), RS-232 | Kenwood CAT | Updated TS-590S, improved DSP |
| TS-890S | 2018 | HF/6m | USB (serial + audio), RS-232, LAN | Kenwood CAT | 100W, direct-conversion on all bands, 7" TFT |

**Protocol Notes:**
- Text-based ASCII commands (e.g., `FA00014250000;` to read/set VFO A frequency)
- Commands are 2-letter mnemonic + parameters + semicolon terminator
- AI (Auto Information) mode pushes state changes without polling
- Very similar to Elecraft protocol (Elecraft extended the Kenwood command set)
- TS-890S and TS-990S have LAN interfaces (for remote control)
- USB provides virtual serial port for CAT control

**Audio Streaming:**
- TS-890S: USB audio codec + network audio capability (wfview compatible)
- TS-990S: LAN interface supports some level of network control
- TS-590S/SG: USB virtual serial only (no USB audio codec; external audio interface required)

**Scope Note:**
- Kenwood's VHF/UHF amateur products (TM-V71, TH-D74, TH-D75) use different
  protocols and are primarily FM/D-STAR/APRS. These are arguably out of scope
  for an HF contest-oriented rig control library, but this is the user's call.

### 1.5 Summary Statistics

| Manufacturer | Models (2010+) | Protocol | Serial | Ethernet | WiFi | Audio over USB | Audio over Network |
|-------------|---------------|----------|--------|----------|------|---------------|-------------------|
| Icom | 13 | CI-V | All | 7 models | 1 (IC-705) | All | 7 models |
| Elecraft | 4 | Ext. Kenwood | All | 1 (K4) | None | 3 (KX3, KX2, K4) | 1 (K4, TBC) |
| FlexRadio | 11 | SmartSDR | **None** | **All** | None | **None** | **All** |
| Kenwood | 4 | Kenwood CAT | All | 2 (TS-890S, TS-990S) | None | 1 (TS-890S) | 2 |
| **Total** | **32** | | | | | | |

---

## 2. Protocol Deep Dive

### 2.1 CI-V (Icom)

```
Frame format:
  0xFE 0xFE <dst_addr> <src_addr> <cmd> [<sub_cmd>] [<data>...] 0xFD

  - Preamble: 0xFE 0xFE
  - dst_addr: Target device CI-V address (default varies by model, e.g., 0x98 for IC-7610)
  - src_addr: Controller address (default 0xE0)
  - cmd: Command number (e.g., 0x00 = set frequency, 0x03 = read frequency)
  - sub_cmd: Sub-command (some commands only)
  - data: BCD-encoded values (frequency in 10-digit BCD, Hz resolution)
  - Terminator: 0xFD

  Response: 0xFE 0xFE <src_addr> <dst_addr> 0xFB 0xFD   (OK)
            0xFE 0xFE <src_addr> <dst_addr> 0xFA 0xFD   (NG/error)
```

- Binary protocol, variable-length frames
- Frequency: 10-digit BCD, LSB first (e.g., 14.250.000 MHz = 00 00 25 41 00)
- Transceive mode: radio pushes state changes (like AI mode in Kenwood)
- Bus topology: multiple devices can share one CI-V bus (collision possible)
- Modern radios (IC-7300+): USB virtual serial port eliminates bus contention
- Typical baud: 115200 (IC-7300+), 19200 (older models), auto-detect available

### 2.2 Extended Kenwood / Kenwood CAT (Elecraft, Kenwood)

```
Command format:
  <CMD><PARAM>;

  - CMD: 2-character ASCII mnemonic (e.g., FA, FB, MD, IF)
  - PARAM: Variable-length ASCII parameter(s)
  - Terminator: semicolon (;)

  Example: FA00014250000;  (set VFO A to 14.250.000 MHz)
  Read:    FA;             (query VFO A frequency)
  Response: FA00014250000; (VFO A is 14.250.000 MHz)
```

- Text-based ASCII protocol, human-readable
- Frequency: 11-digit decimal in Hz (Kenwood) or model-specific (Elecraft)
- AI mode: `AI2;` enables auto-info push from radio
- Elecraft extensions: K3/K4-specific commands (e.g., `K31;` for K3 extended mode)
- IF command: returns comprehensive status in single response
- Error: `?;` response indicates unknown/invalid command

### 2.3 SmartSDR (FlexRadio)

```
Connection sequence:
  1. Discover radio via UDP broadcast on port 4992
  2. Connect TCP to radio IP on port 4992
  3. Receive: version string + client handle
  4. Send: C<seq>|sub slice all   (subscribe to all slice status)
  5. Receive: R<seq>|0|...        (acknowledge)
  6. Receive: S<handle>|slice 0 freq=14.250 mode=USB tx=1 ...

Command examples:
  C1|slice tune 0 14.250         (tune slice 0 to 14.250 MHz)
  C2|slice set 0 mode=CW         (set slice 0 to CW mode)
  C3|radio set power 100          (set TX power to 100W)
  C4|stream create dax=1 type=dax_rx  (create DAX audio stream)
  C5|sub meter all                (subscribe to all meter data)

Status push (async):
  S0|slice 0 freq=14.250000 mode=USB filter_lo=100 filter_hi=2800 tx=1
  S0|meter 1 val=6.2#2 val=3.1#  (S-meter, power meter readings)
```

- Text-based ASCII command channel over TCP
- Frequency: decimal MHz with variable precision
- Slice-oriented: not VFO A/B, but slice 0, 1, 2, ... (up to 8)
- Subscription model: client subscribes to status categories
- VITA-49 for all streaming data (separate UDP channel)
- No concept of "VFO A/B switch" -- each slice is independent
- TX assignment: any slice can be designated as the transmit slice

---

## 3. FlexRadio Architectural Impact Analysis

### 3.1 The Transport Layer Problem

The user said "serial (USB) only first is fine." FlexRadio has NO serial interface.
This creates a fundamental conflict:

**Option A: Defer FlexRadio to Phase 2 (after network transport is built)**
- Pro: Serial-only Phase 1 is simpler; covers Icom, Elecraft, Kenwood
- Pro: Clean separation of concerns; network transport is genuinely different
- Con: FlexRadio is a requested manufacturer from day one
- Con: If the trait abstraction is designed serial-only, it may not generalize well

**Option B: Build network transport in Phase 1 alongside serial**
- Pro: FlexRadio supported immediately; architecture validated against all protocols early
- Pro: Forces the transport abstraction to be protocol-agnostic from the start
- Con: Significantly more Phase 1 work (TCP client, UDP listener, VITA-49 framing)
- Con: Discovery protocol, multi-client semantics, and slice model add complexity

**Option C: Build the trait abstraction to accommodate both, but implement serial first and FlexRadio transport second (still Phase 1, but staggered)**
- Pro: Design is right from the start; implementation is phased within Phase 1
- Pro: Common traits are validated against serial rigs first, then FlexRadio
- Con: Risk of discovering the trait abstraction does not fit FlexRadio after building it for serial rigs

**Recommendation: Option C (staggered within Phase 1)**

Design the transport trait to be generic from day one (`async trait Transport`
with `send`/`receive` methods), implement `SerialTransport` first, validate
with IC-7610, then implement `TcpTransport` + `UdpTransport` for FlexRadio.
The core `Rig` trait can be designed with both VFO and slice models in mind
from the start.

### 3.2 The VFO vs. Slice Abstraction Problem

Traditional rigs (Icom, Elecraft, Kenwood) use VFO A and VFO B:
- VFO A is typically the "main" receiver
- VFO B is the "sub" receiver (if dual-RX capable)
- One VFO is designated for transmit
- Split operation: RX on VFO A, TX on VFO B

FlexRadio uses numbered slices (0-7):
- Each slice is an independent receiver with its own frequency, mode, filter
- Any slice can be designated as the TX slice
- Slices are created and destroyed dynamically
- The number of simultaneous slices depends on model (2, 4, or 8)
- There is no inherent "A/B" distinction

**Proposed Abstraction:**

```rust
/// A receiver channel -- maps to VFO A/B on traditional rigs,
/// or a slice on FlexRadio.
pub trait Receiver {
    /// Get the frequency of this receiver in Hz.
    async fn frequency(&self) -> Result<u64>;
    /// Set the frequency of this receiver in Hz.
    async fn set_frequency(&self, freq_hz: u64) -> Result<()>;
    /// Get the operating mode.
    async fn mode(&self) -> Result<Mode>;
    /// Set the operating mode.
    async fn set_mode(&self, mode: Mode) -> Result<()>;
    /// Get the filter/passband width.
    async fn passband(&self) -> Result<Passband>;
    /// Set the filter/passband width.
    async fn set_passband(&self, passband: Passband) -> Result<()>;
    // ... etc
}

/// Main rig control trait.
pub trait Rig {
    /// List available receivers (VFOs or slices).
    async fn receivers(&self) -> Result<Vec<Box<dyn Receiver>>>;

    /// Get the primary (main) receiver.
    /// On traditional rigs: VFO A.
    /// On FlexRadio: the active TX slice or slice 0.
    async fn primary_receiver(&self) -> Result<Box<dyn Receiver>>;

    /// Get the secondary receiver, if available.
    /// On traditional rigs: VFO B (if dual-RX).
    /// On FlexRadio: next slice after primary.
    async fn secondary_receiver(&self) -> Result<Option<Box<dyn Receiver>>>;

    /// Designate which receiver is used for transmit.
    async fn set_tx_receiver(&self, receiver: &dyn Receiver) -> Result<()>;

    /// Get transmit power level (0-100%).
    async fn power(&self) -> Result<u8>;
    /// Set transmit power level.
    async fn set_power(&self, power: u8) -> Result<()>;

    /// PTT control.
    async fn ptt(&self) -> Result<bool>;
    async fn set_ptt(&self, on: bool) -> Result<()>;

    // ... etc
}
```

This abstraction works because:
- Traditional rigs always have 1-2 receivers (VFO A, optionally VFO B)
- FlexRadio has 2-8 receivers (slices), but primary/secondary still makes sense
- For contest software, you almost always care about "main RX" and "sub RX"
- Advanced FlexRadio users who need slice-level control can downcast to the
  FlexRadio-specific implementation

### 3.3 Audio Streaming Architecture

Audio streaming varies significantly across manufacturers:

| Mechanism | Icom (USB) | Icom (LAN) | Elecraft | Kenwood | FlexRadio |
|-----------|-----------|------------|----------|---------|-----------|
| RX Audio | USB audio device | UDP proprietary | USB audio device | USB audio device | VITA-49 DAX |
| TX Audio | USB audio device | UDP proprietary | USB audio device | USB audio device | VITA-49 DAX |
| IQ Data | Not available* | UDP proprietary | Not available | Not available | VITA-49 |
| Format | PCM 16-bit | Proprietary | PCM 16-bit | PCM 16-bit | VITA-49 frames |
| Latency | Low (~10ms) | Medium (~30-50ms) | Low (~10ms) | Low (~10ms) | Medium (~20-40ms) |

*IC-7610 and IC-R8600 output IQ via USB, but as a standard audio device, not CI-V.

**Proposed Audio Trait:**

```rust
pub trait AudioStream {
    /// Start receiving audio from the radio.
    async fn start_rx_audio(&mut self, config: AudioConfig) -> Result<AudioReceiver>;
    /// Start sending audio to the radio (for TX).
    async fn start_tx_audio(&mut self, config: AudioConfig) -> Result<AudioSender>;
}

pub struct AudioConfig {
    pub sample_rate: u32,     // e.g., 48000
    pub channels: u16,        // 1 (mono) or 2 (stereo/IQ)
    pub format: AudioFormat,  // PCM16, PCM24, Float32
}

// AudioReceiver and AudioSender are async stream/sink wrappers
// over the actual transport (USB audio device, UDP stream, VITA-49)
```

Audio streaming should be an optional trait (`AudioStream`) that not all `Rig`
implementations are required to support. This is consistent with the reality
that some radios support it natively while others require external audio interfaces.

### 3.4 Discovery and Connection

FlexRadio has a formal discovery protocol. Other manufacturers do not.

| Manufacturer | Discovery | Connection |
|-------------|-----------|------------|
| Icom | Manual (COM port enumeration) | Open serial port to known COM port |
| Icom (LAN) | Manual (IP address) or mDNS (some models) | Connect UDP to known IP |
| Elecraft | Manual (COM port enumeration) | Open serial port to known COM port |
| Elecraft K4 | Manual (IP address) | Open TCP to known IP |
| Kenwood | Manual (COM port enumeration) | Open serial port to known COM port |
| FlexRadio | **Automatic** (UDP broadcast on port 4992) | Connect TCP to discovered IP:4992 |

**Proposed Connection API (Builder Pattern):**

```rust
// Serial connection (Icom, Elecraft, Kenwood)
let rig = IcomBuilder::new()
    .model(IcomModel::IC7610)
    .serial_port("/dev/ttyUSB0")
    .baud_rate(115200)
    .civ_address(0x98)
    .build()
    .await?;

// FlexRadio (automatic discovery)
let radios = FlexRadio::discover(Duration::from_secs(3)).await?;
let rig = FlexRadioBuilder::new()
    .radio(&radios[0])       // or .ip("192.168.1.100")
    .station_name("Contest")
    .build()
    .await?;

// FlexRadio (manual connection)
let rig = FlexRadioBuilder::new()
    .ip("192.168.1.100".parse()?)
    .build()
    .await?;

// Generic -- type-erased for contest logging software
let rig: Box<dyn Rig> = connect_from_config(&config).await?;
```

---

## 4. Revised Crate Architecture

Based on the research, the original "serial-only" assumption needs revision.
The crate structure should accommodate both transport types from the start.

### 4.1 Workspace Layout

```
riglib/                          (workspace root)
├── Cargo.toml                   (workspace definition)
├── crates/
│   ├── riglib/                  (public facade crate -- what users depend on)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs           (re-exports from internal crates)
│   │
│   ├── riglib-core/             (core traits and types)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── rig.rs           (Rig trait, Receiver trait)
│   │       ├── audio.rs         (AudioStream trait)
│   │       ├── types.rs         (Frequency, Mode, Passband, etc.)
│   │       ├── error.rs         (Error types)
│   │       ├── transport.rs     (Transport trait -- serial and network)
│   │       └── builder.rs       (Builder trait pattern)
│   │
│   ├── riglib-transport/        (transport implementations)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── serial.rs        (async serial port via tokio-serial)
│   │       ├── tcp.rs           (async TCP client)
│   │       ├── udp.rs           (async UDP client/listener)
│   │       └── discovery.rs     (FlexRadio discovery listener)
│   │
│   ├── riglib-icom/             (Icom CI-V implementation)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── civ.rs           (CI-V frame encode/decode)
│   │       ├── commands.rs      (command builders per model)
│   │       ├── models.rs        (per-model address/capability tables)
│   │       ├── builder.rs       (IcomBuilder)
│   │       ├── audio_usb.rs     (USB audio device integration)
│   │       └── audio_lan.rs     (network audio for LAN-equipped models)
│   │
│   ├── riglib-elecraft/         (Elecraft implementation)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── protocol.rs      (Kenwood-extended command encode/decode)
│   │       ├── commands.rs      (command builders, K3 vs K4 differences)
│   │       ├── models.rs        (per-model capability tables)
│   │       └── builder.rs       (ElecraftBuilder)
│   │
│   ├── riglib-kenwood/          (Kenwood implementation)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── protocol.rs      (Kenwood CAT command encode/decode)
│   │       ├── commands.rs      (command builders)
│   │       ├── models.rs        (per-model capability tables)
│   │       └── builder.rs       (KenwoodBuilder)
│   │
│   ├── riglib-flex/             (FlexRadio SmartSDR implementation)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── api.rs           (SmartSDR TCP command client)
│   │       ├── vita49.rs        (VITA-49 frame parser/builder)
│   │       ├── discovery.rs     (UDP discovery protocol)
│   │       ├── slice.rs         (slice management, maps to Receiver trait)
│   │       ├── stream.rs        (DAX audio + IQ streaming)
│   │       ├── models.rs        (per-model capability tables)
│   │       └── builder.rs       (FlexRadioBuilder)
│   │
│   └── riglib-test-harness/     (integration test utilities)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── mock_serial.rs   (mock serial port for unit testing)
│           ├── mock_tcp.rs      (mock TCP server for FlexRadio testing)
│           └── recorder.rs      (record/replay for protocol testing)
│
├── examples/
│   ├── scan_frequency.rs        (basic frequency read/write)
│   ├── contest_so2r.rs          (SO2R dual-receiver demo)
│   ├── flex_discovery.rs        (discover FlexRadio on LAN)
│   └── audio_stream.rs          (audio streaming demo)
│
└── docs/
    ├── 001-initial-planning.md
    └── 002-research-and-architecture.md  (this document)
```

### 4.2 Core Trait Hierarchy

```rust
// riglib-core/src/transport.rs

/// Low-level byte transport -- serial port, TCP socket, etc.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send raw bytes to the radio.
    async fn send(&mut self, data: &[u8]) -> Result<()>;

    /// Receive bytes from the radio.
    /// Returns when data is available or timeout expires.
    async fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;

    /// Close the transport.
    async fn close(&mut self) -> Result<()>;

    /// Check if the transport is still connected.
    fn is_connected(&self) -> bool;
}

// riglib-core/src/rig.rs

/// Core rig control trait -- implemented by all manufacturers.
#[async_trait]
pub trait Rig: Send + Sync {
    /// Get the manufacturer and model information.
    fn info(&self) -> &RigInfo;

    /// Get all available receivers (VFOs or slices).
    async fn receivers(&self) -> Result<Vec<ReceiverId>>;

    /// Get the primary receiver.
    async fn primary_receiver(&self) -> Result<ReceiverId>;

    /// Get/set frequency on a specific receiver.
    async fn get_frequency(&self, rx: ReceiverId) -> Result<u64>;
    async fn set_frequency(&self, rx: ReceiverId, freq_hz: u64) -> Result<()>;

    /// Get/set mode on a specific receiver.
    async fn get_mode(&self, rx: ReceiverId) -> Result<Mode>;
    async fn set_mode(&self, rx: ReceiverId, mode: Mode) -> Result<()>;

    /// PTT control.
    async fn get_ptt(&self) -> Result<bool>;
    async fn set_ptt(&self, on: bool) -> Result<()>;

    /// TX power (0.0 to 1.0 normalized, or watts -- TBD).
    async fn get_power(&self) -> Result<f32>;
    async fn set_power(&self, power: f32) -> Result<()>;

    /// S-meter reading in dBm (or S-units -- TBD).
    async fn get_s_meter(&self, rx: ReceiverId) -> Result<f32>;

    /// Subscribe to state changes (frequency, mode, PTT, etc.).
    /// Returns a stream of RigEvent values.
    fn subscribe(&self) -> Result<broadcast::Receiver<RigEvent>>;

    /// Query capabilities of this specific radio.
    fn capabilities(&self) -> &RigCapabilities;
}

/// Receiver identifier -- opaque handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiverId(u8);

impl ReceiverId {
    pub const VFO_A: ReceiverId = ReceiverId(0);
    pub const VFO_B: ReceiverId = ReceiverId(1);
}

/// What this radio can do.
pub struct RigCapabilities {
    pub max_receivers: u8,
    pub has_sub_receiver: bool,
    pub has_split: bool,
    pub has_audio_streaming: bool,
    pub has_iq_output: bool,
    pub supported_modes: Vec<Mode>,
    pub frequency_range: Vec<BandRange>,
    // ...
}
```

### 4.3 Feature Flags

The `riglib` facade crate uses Cargo features to control which manufacturers
are compiled in:

```toml
[features]
default = ["icom", "elecraft", "kenwood", "flex"]
icom = ["dep:riglib-icom"]
elecraft = ["dep:riglib-elecraft"]
kenwood = ["dep:riglib-kenwood"]
flex = ["dep:riglib-flex"]
audio = []  # Enable audio streaming support
full = ["icom", "elecraft", "kenwood", "flex", "audio"]
```

This allows consumers to depend only on what they need:
```toml
[dependencies]
riglib = { version = "0.1", features = ["icom"] }  # Icom only, no FlexRadio deps
```

---

## 5. Phasing Strategy

### Phase 1: Foundation + First Rig (Icom IC-7610)

**Duration estimate**: 6-8 weeks

1. **riglib-core**: All core traits (`Rig`, `Transport`, `ReceiverId`, types, errors)
2. **riglib-transport**: `SerialTransport` implementation (tokio-serial)
3. **riglib-icom**: CI-V protocol engine + IC-7610 model definition
4. **riglib-test-harness**: Mock serial transport, CI-V frame recorder
5. **Validation**: Test against physical IC-7610
   - Frequency read/write
   - Mode read/write
   - PTT control
   - S-meter reading
   - Dual receiver (main + sub) on IC-7610
   - State change subscription (CI-V transceive mode)

**Deliverable**: Working Icom IC-7610 control over USB serial.

### Phase 2: Protocol Generalization (Elecraft + Kenwood)

**Duration estimate**: 4-6 weeks

1. **riglib-elecraft**: Extended Kenwood protocol engine
2. **riglib-kenwood**: Kenwood CAT protocol engine
3. Validate against available hardware (if user has access to any Elecraft/Kenwood)
4. Refine the `Rig` trait based on lessons from three different protocols
5. **riglib-transport**: `TcpTransport` for Elecraft K4 Ethernet (shares transport
   with FlexRadio Phase 3)

**Deliverable**: Working Elecraft K3/K3S/KX3/KX2/K4 and Kenwood TS-590/890/990 control.

### Phase 3: FlexRadio + Network Transport

**Duration estimate**: 6-8 weeks

1. **riglib-transport**: `TcpTransport`, `UdpTransport`
2. **riglib-flex**: SmartSDR API client
   - Discovery protocol (UDP broadcast listener)
   - TCP command/response/status engine
   - Slice-to-Receiver mapping
   - VITA-49 frame parser (at minimum for metering)
3. Validate against FlexRadio hardware (user may need to acquire or borrow one)
4. Refine `Rig` trait if FlexRadio surfaces issues with the abstraction

**Deliverable**: Working FlexRadio FLEX-6000/8000 series control over Ethernet.

### Phase 4: Audio Streaming

**Duration estimate**: 4-6 weeks

1. USB audio device integration (cpal or similar crate) for Icom/Elecraft/Kenwood
2. VITA-49 audio stream parsing for FlexRadio (DAX channels)
3. Icom LAN audio streaming (reverse-engineer or reference wfview's implementation)
4. `AudioStream` trait implementation across all supported manufacturers

**Deliverable**: Cross-manufacturer audio streaming.

### Phase 5: Polish, CI, Documentation

1. Expand model support (all models in the catalog above)
2. CI pipeline (GitHub Actions: build, test, clippy, fmt)
3. Documentation (rustdoc, examples, protocol notes)
4. Publish to crates.io

---

## 6. Follow-Up Questions for User

### 6.1 CRITICAL: Yaesu Drop + Test Rig Paradox

You dropped Yaesu from the supported manufacturers list, but you have a **Yaesu
FT-DX10** available for physical testing. Meanwhile:

- You have an **Icom IC-7610** (supported, can test)
- You have a **Yaesu FT-DX10** (dropped, cannot use for testing)
- You likely do NOT have Elecraft, FlexRadio, or Kenwood hardware for testing

**Questions:**
1. Do you want to reconsider adding Yaesu back? The FT-DX10 uses Yaesu's CAT
   protocol (also text-based, similar complexity to Kenwood). Having two physical
   test rigs from two different manufacturers would significantly accelerate
   development and catch abstraction issues early.
2. Alternatively, do you have access to any Elecraft, FlexRadio, or Kenwood
   hardware for testing? Development without physical hardware is possible using
   protocol recordings and mock transports, but it is risky for production quality.

### 6.2 CRITICAL: FlexRadio Requires Network Transport

FlexRadio has **zero serial capability**. The "serial only first" phasing means
FlexRadio gets deferred to Phase 3 at the earliest. Is this acceptable, or do
you want FlexRadio support sooner?

If sooner: I recommend building the network transport in Phase 1 alongside serial,
and implementing FlexRadio in Phase 2 instead of Phase 3.

### 6.3 FlexRadio Slice Model vs. VFO Model

The proposed `ReceiverId` abstraction maps VFO A/B to receiver IDs 0/1, and
FlexRadio slices to receiver IDs 0-7. This works for basic operation, but
FlexRadio's dynamic slice creation/destruction has no equivalent in traditional
rigs.

**Question:** Should the library expose a `FlexRadio`-specific API for advanced
slice management (create/destroy slices, route audio, manage multiple panadapters),
in addition to the generic `Rig` trait? Or should we keep the public API strictly
generic and let advanced users access the underlying SmartSDR connection directly?

### 6.4 IC-R8600 Inclusion

The IC-R8600 is a wideband **receiver** (not a transceiver) -- it cannot transmit.
Should it be included in the supported model list? It uses CI-V and would work
with the Icom backend, but the `Rig` trait includes PTT and power control methods
that would be meaningless for a receiver.

### 6.5 Kenwood VHF/UHF Scope

Kenwood's amateur VHF/UHF products (TM-V71, TH-D74/D75) use different command
sets and are primarily FM/digital voice rigs. They are less relevant for HF
contesting. Should they be excluded from scope, or included for completeness?

### 6.6 Pre-2010 Radios That Are Still Popular

The 2010+ cutoff excludes some very popular radios that are still in widespread
use:
- **Icom IC-7600** (2009): Still widely used for contesting
- **Icom IC-7200** (2009): Popular field/portable rig
- **Kenwood TS-480SAT/HX** (2004): Still actively sold until recently
- **Elecraft K3** (2008): Premier contest radio, still in use everywhere

These all use the same protocols as their successors, so supporting them is
mostly a matter of adding model-specific capability tables rather than new code.
Should the cutoff be relaxed, or strictly enforced at 2010?

### 6.7 Naming

You said no naming decision yet. Some candidates for when you are ready:
- `riglib` (current working name, straightforward)
- `rigctl` (conflicts with hamlib's rigctld concept)
- `radioctl`
- `hamrig`
- Something domain-specific to contesting if that is the primary audience

No action needed now; just flagging for later.

---

## Appendix: Key References

- [Icom CI-V Reference Guides](https://www.icomamerica.com/support/manual/2574/) (per-model PDFs)
- [Elecraft K3 Programmer's Reference](https://ftp.elecraft.com/K3/Manuals%20Downloads/)
- [Elecraft K4 Operating Manual](https://ftp.elecraft.com/K4/Manuals%20Downloads/)
- [FlexRadio SmartSDR API Docs (GitHub)](https://github.com/flexradio/smartsdr-api-docs)
- [FlexRadio SmartSDR API Wiki](https://github.com/flexradio/smartsdr-api-docs/wiki/)
- [FlexRadio Developer Program](https://www.flexradio.com/api/developer-program/)
- [Kenwood TS-890S Command Reference](https://www.kenwood.com/usa/com/amateur/ts-890s/)
- [wfview - Open Source Icom/Kenwood Control](https://wfview.org/)
- [wfview Supported Radios](https://wfview.org/wfview-user-manual/supported-radios-and-features/)
