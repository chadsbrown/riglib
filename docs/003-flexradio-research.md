# FlexRadio SmartSDR API and VITA-49 Protocol Research

**Document**: 003-flexradio-research.md
**Date**: 2026-02-06
**Status**: Complete
**Previous**: 002-research-and-architecture.md (general architecture), PLAN.md Phase 3
**Purpose**: Detailed protocol documentation for FlexRadio SmartSDR implementation in riglib-flex

---

## Table of Contents

1. [Overview](#1-overview)
2. [SmartSDR TCP API](#2-smartsdr-tcp-api)
3. [VITA-49 UDP Protocol](#3-vita-49-udp-protocol)
4. [FlexRadio Model Specifications](#4-flexradio-model-specifications)
5. [Mapping to riglib Rig Trait](#5-mapping-to-riglib-rig-trait)
6. [Existing Implementations and Dependencies](#6-existing-implementations-and-dependencies)
7. [Risk Assessment](#7-risk-assessment)
8. [Implementation Recommendations](#8-implementation-recommendations)
9. [References](#9-references)

---

## 1. Overview

FlexRadio SDR transceivers are network-only radios with zero serial interface.
All communication happens over Ethernet using two channels:

- **TCP port 4992** -- Text-based command/response/status protocol (SmartSDR API)
- **UDP port 4991** -- Binary streaming data using VITA-49 framing (meters, audio, IQ, FFT)
- **UDP port 4992** -- Discovery broadcasts (radio announces itself on LAN)

Every FlexRadio model from the FLEX-6000 series onward uses the same SmartSDR
API. Differences between models are in hardware capabilities (slice count, SCU
count, panadapter count), not protocol.

```
  riglib Client
      |
      +--- TCP port 4992 -----> Commands, Responses, Status messages
      |
      +--- UDP port 4991 <----- VITA-49 frames (meters, DAX audio, IQ, FFT)
      |
      +--- UDP port 4992 <----- Discovery broadcasts
```

**Critical finding**: Meter data (S-meter, SWR, ALC, forward/reflected power)
is delivered EXCLUSIVELY via VITA-49 UDP. There is no TCP-only mechanism to
poll or subscribe to meter values. This makes VITA-49 parsing a hard
prerequisite for complete Rig trait support -- it cannot be deferred.

---

## 2. SmartSDR TCP API

### 2.1 Connection Handshake

The TCP connection sequence to port 4992 is:

```
Step  Direction     Data                              Notes
----  ---------     ----                              -----
1     Client->Radio (TCP connect to port 4992)
2     Radio->Client V1.4.0.0\n                        SmartSDR version string
3     Radio->Client H12345678\n                       32-bit hex client handle
4     Client->Radio client program riglib\n           Register client name
5     Client->Radio sub slice all\n                   Subscribe to slice status
6     Client->Radio sub meter all\n                   Subscribe to meter status
7     Client->Radio sub tx all\n                      Subscribe to TX status
```

The version line format is `V<major>.<minor>.<patch>.<build>\n`. The handle
line format is `H<8_hex_digits>\n`. Both are sent immediately upon TCP
connection, before the client sends anything.

After registering, the client should subscribe to status categories of
interest. Common subscriptions:

| Subscription Command | Status Objects Received |
|---------------------|------------------------|
| `sub slice all`     | Slice frequency, mode, filter, TX assignment |
| `sub meter all`     | Meter ID assignments (names, units) |
| `sub tx all`        | TX state, power, ALC, interlock |
| `sub radio all`     | Radio-level status (model, calibration) |
| `sub interlock all` | Interlock state (TX inhibit, ATU, amplifier) |
| `sub atu all`       | Automatic antenna tuner state |
| `sub display all`   | Panadapter and waterfall configuration |

### 2.2 Command Format

Commands are sent from client to radio as ASCII text lines:

```
C<sequence_number>|<command_text>\n
```

- Sequence numbers are unsigned integers, auto-incremented from 1
- Commands are space-separated tokens
- Line terminator is `\n` (LF), not `\r\n`

Examples:

```
C1|slice tune 0 14.250000\n
C2|slice set 0 mode=USB\n
C3|xmit 1\n
C4|transmit set power=75\n
C5|info\n
C6|meter list\n
C7|slice create freq=14.250000 mode=CW\n
C8|slice remove 2\n
```

### 2.3 Response Format

The radio responds to each command with a response line:

```
R<sequence_number>|<hex_error_code>|<response_data>\n
```

- The sequence number matches the command that triggered this response
- Error code is an 8-digit hex value: `00000000` = success, non-zero = error
- Response data is command-specific (may be empty)

Examples:

```
R1|00000000|\n                    Success, no data
R7|00000000|3\n                   Success, created slice index 3
R8|50000015|Slice not found\n     Error: invalid slice
```

**Command/response correlation**: Responses are NOT guaranteed to arrive in
command order. If commands C1, C2, C3 are sent rapidly, the responses may
arrive as R2, R1, R3. The client MUST correlate responses by sequence number,
not by arrival order. This is the primary reason for the sequence numbering
scheme.

### 2.4 Status Message Format

Status messages are unsolicited, pushed to all subscribed clients:

```
S<handle>|<object_type> <key1>=<value1> <key2>=<value2> ...\n
```

- Handle is the 32-bit hex client handle from the connection handshake
- Object type identifies what changed (slice, meter, tx, radio, etc.)
- Key-value pairs are space-separated, with `=` between key and value
- Values containing spaces are NOT quoted -- they use a different encoding

Examples:

```
S12345678|slice 0 RF_frequency=14.250000 mode=USB filter_lo=100 filter_hi=2900\n
S12345678|slice 0 tx=1\n
S12345678|tx freq=14.250000 power=100 rfpower=100\n
S12345678|radio model=FLEX-6600 serial=12345 name=MyStation\n
S12345678|interlock state=READY\n
```

**Slice status keys** (partial list, most relevant for Rig trait):

| Key            | Type   | Description                          |
|----------------|--------|--------------------------------------|
| `RF_frequency` | float  | Slice frequency in MHz               |
| `mode`         | string | Operating mode (USB, LSB, CW, etc.)  |
| `filter_lo`    | int    | Lower filter edge in Hz              |
| `filter_hi`    | int    | Upper filter edge in Hz              |
| `tx`           | 0/1    | Whether this slice is the TX slice   |
| `active`       | 0/1    | Whether this slice is the active slice |
| `nr_on`        | 0/1    | Noise reduction enabled              |
| `nb_on`        | 0/1    | Noise blanker enabled                |
| `agc_mode`     | string | AGC mode (off, slow, med, fast)      |
| `dax`          | int    | DAX channel assignment (0 = none)    |
| `dax_clients`  | int    | Number of clients receiving DAX audio |

### 2.5 Key Commands for Rig Trait Operations

#### Slice (Receiver) Operations

| Operation | Command | Notes |
|-----------|---------|-------|
| Create slice | `slice create freq=<mhz> mode=<mode>` | Returns slice index in response data |
| Remove slice | `slice remove <index>` | Destroys the slice |
| Tune slice | `slice tune <index> <freq_mhz>` | Frequency as MHz float |
| Set slice property | `slice set <index> <key>=<value>` | Any slice property |
| List slices | `slice list` | Returns active slice indices |

#### Mode Operations

Set mode via `slice set <index> mode=<mode_string>`. Mode strings:

| FlexRadio Mode | Description        | riglib Mode Mapping |
|----------------|--------------------|---------------------|
| `USB`          | Upper Sideband     | `Mode::USB`         |
| `LSB`          | Lower Sideband     | `Mode::LSB`         |
| `CW`           | CW (upper)         | `Mode::CW`          |
| `CWR`          | CW Reverse (lower) | `Mode::CWR`         |
| `AM`           | AM                 | `Mode::AM`          |
| `SAM`          | Synchronous AM     | `Mode::AM`          |
| `FM`           | FM (wide)          | `Mode::FM`          |
| `NFM`          | FM (narrow)        | `Mode::FM`          |
| `DFM`          | Data FM            | `Mode::DataFM`      |
| `DIGU`         | Digital Upper      | `Mode::DataUSB`     |
| `DIGL`         | Digital Lower      | `Mode::DataLSB`     |
| `RTTY`         | RTTY               | `Mode::RTTY`        |
| `FDV`          | FreeDV             | `Mode::DataUSB`     |

Note: `SAM` (synchronous AM) maps to `Mode::AM` since riglib has no separate
synchronous AM mode. `NFM` maps to `Mode::FM` since riglib does not
distinguish wide/narrow FM at the mode level (passband handles this). `FDV`
(FreeDV) maps to `Mode::DataUSB` as the closest approximation.

#### TX Operations

| Operation | Command | Notes |
|-----------|---------|-------|
| PTT on    | `xmit 1` | Engages PTT |
| PTT off   | `xmit 0` | Releases PTT |
| Set TX power | `transmit set power=<0-100>` | Power level 0-100 |
| Set TX slice | `slice set <index> tx=1` | Designate TX slice |

#### Radio Info

| Operation | Command | Notes |
|-----------|---------|-------|
| Get radio info | `info` | Returns model, serial, name, etc. |
| Get meter list | `meter list` | Returns meter ID-to-name mapping |

#### Split Operation

FlexRadio has no dedicated split command. Split is achieved by having
different slices for RX and TX:

1. The "RX slice" is the active slice the operator is listening to
2. The "TX slice" is the slice with `tx=1` set
3. When these are different slices at different frequencies, you have split

To enable split in riglib terms:
- Create two slices (if not already existing)
- Set RX slice to desired RX frequency
- Set TX slice to desired TX frequency via `slice set <tx_index> tx=1`

To disable split:
- Set `tx=1` on the same slice that is the active RX slice

### 2.6 Frequency Representation

**All SmartSDR frequencies are in MHz as floating-point values**, not Hz
integers. This is different from every other manufacturer riglib supports:

| Manufacturer | Frequency Format | Example for 14.250 MHz |
|-------------|-----------------|------------------------|
| Icom        | BCD Hz (binary)  | `00 00 25 41 00`       |
| Yaesu       | Hz integer (ASCII) | `014250000`          |
| Kenwood     | Hz integer (ASCII) | `00014250000`        |
| Elecraft    | Hz integer (ASCII) | `00014250000`        |
| FlexRadio   | **MHz float (ASCII)** | `14.250000`       |

The riglib Rig trait uses `u64` Hz internally. The FlexRadio implementation
must convert at the boundary:

```
riglib Hz (u64)     -->  SmartSDR MHz (f64)
14250000            -->  14.250000

SmartSDR MHz (f64)  -->  riglib Hz (u64)
14.250000           -->  14250000
```

Conversion:
- Hz to MHz: `freq_hz as f64 / 1_000_000.0`
- MHz to Hz: `(freq_mhz * 1_000_000.0).round() as u64`

The `.round()` is important to avoid floating-point truncation errors.
FlexRadio typically sends 6 decimal places (1 Hz resolution).

---

## 3. VITA-49 UDP Protocol

### 3.1 Overview

VITA-49 (formally VITA 49.0, also known as VRT -- VITA Radio Transport) is a
standard for digitized signal data transport. FlexRadio uses VITA-49 to stream
all real-time data between the radio and clients over UDP port 4991.

**Important**: FlexRadio uses VITA-49.0, NOT VITA-49.2. The `vita49` crate on
crates.io targets VITA-49.2 and is NOT compatible. We must write our own
parser.

### 3.2 Packet Header Format

All VITA-49 packets from FlexRadio use the Extension Data with Stream ID
packet type. The header is 28 bytes, big-endian:

```
Offset  Size   Field                  Description
------  ----   -----                  -----------
0       4      Packet Header Word     Type, flags, count, size
4       4      Stream ID              Identifies the specific stream instance
8       4      Class OUI + Info Code  FlexRadio OUI (0x001C2D) + info class
12      4      Packet Class Code      Identifies the stream type
16      4      Integer Timestamp      Seconds since epoch
20      8      Fractional Timestamp   Picoseconds (64-bit unsigned)
```

#### Packet Header Word (offset 0, 4 bytes)

```
Bits 31-28: Packet Type (0x3 = Extension Data with Stream ID)
Bit  27:    Class ID Present (1 = yes, for FlexRadio packets)
Bit  26:    Trailer Present (0 = no trailer for most FlexRadio packets)
Bits 25-24: Reserved
Bits 23-20: TSI (Timestamp Integer type, 0x01 = UTC)
Bits 19-18: TSF (Timestamp Fractional type)
Bits 17-16: Reserved
Bits 15-12: Packet Count (4-bit rolling counter, 0-15)
Bits 11-0:  Packet Size (in 32-bit words, including header)
```

#### Stream ID (offset 4, 4 bytes)

A 32-bit identifier for the specific stream instance. This is assigned by the
radio when a stream is created (e.g., a specific DAX channel or panadapter).

#### Class OUI + Information Class Code (offset 8, 4 bytes)

```
Bits 31-8:  OUI (Organizationally Unique Identifier) -- 0x001C2D for FlexRadio
Bits 7-0:   Information Class Code
```

#### Packet Class Code (offset 12, 4 bytes)

This is the field used to identify the type of data in the packet:

| Packet Class Code | Stream Type         | Description                     |
|-------------------|---------------------|---------------------------------|
| `0x8002`          | Meter data          | S-meter, power, SWR, ALC, etc. |
| `0x8003`          | FFT data            | Panadapter spectral data        |
| `0x8004`          | Waterfall data      | Waterfall display data          |
| `0x8005`          | Opus audio          | Compressed audio stream         |
| `0x02E3`          | DAX IQ 24 kHz       | I/Q data at 24 ksps            |
| `0x02E4`          | DAX IQ 48 kHz       | I/Q data at 48 ksps            |
| `0x02E5`          | DAX IQ 96 kHz       | I/Q data at 96 ksps            |
| `0x02E6`          | DAX IQ 192 kHz      | I/Q data at 192 ksps           |
| `0x03E3`          | DAX audio           | Demodulated audio, 24 ksps stereo float32 |
| `0xFFFF`          | Discovery           | Radio discovery broadcast       |

### 3.3 Meter Data Payload

Meter data packets (class code `0x8002`) carry an array of meter readings
after the 28-byte header. Each reading is a 4-byte record:

```
Offset  Size   Field         Description
------  ----   -----         -----------
0       2      Meter ID      16-bit unsigned, big-endian
2       2      Meter Value   16-bit signed, big-endian
```

The payload contains a variable number of these 4-byte records. The number of
records is determined from the packet size field in the header:

```
record_count = (packet_size_words * 4 - 28) / 4
```

**Meter ID assignment is dynamic.** The radio assigns numeric IDs to meters
at runtime. The mapping from meter ID to meter name must be obtained via the
TCP `meter list` command. Common meter names:

| Meter Name        | Description                | Rig Trait Method    |
|-------------------|----------------------------|---------------------|
| `s-meter`         | S-meter reading            | `get_s_meter()`     |
| `power_forward`   | Forward TX power           | `get_power()` (TX)  |
| `power_reflected` | Reflected TX power         | (extension)         |
| `swr`             | Standing wave ratio        | `get_swr()`         |
| `alc`             | Automatic level control    | `get_alc()`         |
| `mic_level`       | Microphone input level     | (extension)         |
| `comp_level`      | Compression level          | (extension)         |
| `voltage`         | Power supply voltage       | (extension)         |
| `temperature`     | PA temperature             | (extension)         |

Meter values are in internal units that vary by meter type. Scaling factors
must be determined empirically or from FlexRadio documentation. The `meter
list` TCP response includes unit information (dBm, dBFS, SWR ratio, volts,
degrees C, etc.).

### 3.4 DAX Audio Payload

DAX audio packets (class code `0x03E3`) carry demodulated audio samples after
the 28-byte header:

- **Sample rate**: 24,000 samples per second
- **Format**: Stereo (2 channels), IEEE 754 float32
- **Byte order**: Little-endian within the VITA-49 big-endian framing

Each audio frame is 8 bytes (4 bytes left + 4 bytes right). A typical packet
contains 128 or 256 frames.

```
Header (28 bytes, big-endian)
Payload:
  Frame 0: [left_f32_le] [right_f32_le]    (8 bytes)
  Frame 1: [left_f32_le] [right_f32_le]    (8 bytes)
  ...
  Frame N: [left_f32_le] [right_f32_le]    (8 bytes)
```

Note the mixed endianness: VITA-49 headers are big-endian, but the audio
sample payload uses little-endian float32. This is a FlexRadio-specific
convention, not a VITA-49 requirement.

### 3.5 Parsing Strategy

The recommended parsing approach for riglib-flex:

1. Read 28-byte header from each UDP datagram
2. Extract packet class code to determine stream type
3. Dispatch to type-specific payload parser:
   - `0x8002` -> meter parser (array of 4-byte records)
   - `0x03E3` -> DAX audio parser (stereo float32 frames)
   - Other codes -> ignore for now (FFT, waterfall, IQ are Phase 4+)
4. For meter data: look up meter IDs against the cached `meter list` mapping
5. For DAX audio: forward samples to the AudioStream consumer

The entire VITA-49 parser should be approximately 200 lines of Rust. There is
no need for a general-purpose VITA-49 library -- we only need to handle the
specific packet types FlexRadio produces.

---

## 4. FlexRadio Model Specifications

All models use the same SmartSDR API. Differences are in hardware capabilities.
All operate on HF + 6m (1.8-54 MHz) at 100W output.

| Model       | Year | Power | Slices | SCUs | Panadapters | Front Panel | Status       |
|-------------|------|-------|--------|------|-------------|-------------|--------------|
| FLEX-6400   | 2018 | 100W  | 2      | 1    | 2           | No          | Current      |
| FLEX-6400M  | 2018 | 100W  | 2      | 1    | 2           | Yes         | Current      |
| FLEX-6600   | 2018 | 100W  | 4      | 2    | 4           | No          | Current      |
| FLEX-6600M  | 2018 | 100W  | 4      | 2    | 4           | Yes         | Current      |
| FLEX-6700   | 2013 | 100W  | 8      | 2    | 8           | No          | Discontinued |
| FLEX-8400   | 2024 | 100W  | 4      | 2    | 4           | No          | Current      |
| FLEX-8600   | 2024 | 100W  | 8      | 2    | 8           | No          | Current      |

**Terminology**:
- **Slice**: An independent receiver with its own frequency, mode, and filter settings
- **SCU (Signal Capture Unit)**: An independent ADC/antenna input; each SCU can host multiple slices but all slices on one SCU share the same antenna and wideband digitizer
- **Panadapter**: A visual spectrum display; each can show one or more slices

The model can be queried at runtime via the `info` TCP command. The response
includes model name, serial number, radio name (user-configurable nickname),
and firmware version. The `FlexRadioModel` struct should store max_slices,
max_panadapters, and scu_count so the implementation can validate operations
(e.g., reject creating a 3rd slice on a FLEX-6400).

**Note on M variants**: The M (Maestro) variants have an integrated front
panel/display but are otherwise identical to their non-M counterparts from an
API perspective. The API does not distinguish between them. They can share the
same model definition with a `has_front_panel` flag.

---

## 5. Mapping to riglib Rig Trait

### 5.1 ReceiverId Mapping

FlexRadio slices map directly to `ReceiverId`:

```
ReceiverId(0) = Slice 0
ReceiverId(1) = Slice 1
...
ReceiverId(7) = Slice 7
```

The `primary_receiver()` method should return the slice with `tx=1` (the TX
slice), or slice 0 if no TX slice is designated. The `secondary_receiver()`
method should return the next active slice after the primary, or `None` if
only one slice exists.

### 5.2 Rig Trait Method Mapping

| Rig Trait Method          | SmartSDR Implementation                              |
|---------------------------|------------------------------------------------------|
| `info()`                  | Cached from `info` command at connection time         |
| `capabilities()`          | Static per model, looked up from model definitions   |
| `receivers()`             | List of active slice indices from status tracking    |
| `primary_receiver()`      | Slice with `tx=1`, or slice 0                        |
| `secondary_receiver()`    | Next active slice after primary                      |
| `get_frequency(rx)`       | Cached from slice status `RF_frequency` (MHz->Hz)   |
| `set_frequency(rx, hz)`   | `slice tune <rx> <hz_to_mhz>` TCP command            |
| `get_mode(rx)`            | Cached from slice status `mode` (string->Mode)       |
| `set_mode(rx, mode)`      | `slice set <rx> mode=<mode_to_string>` TCP command   |
| `get_passband(rx)`        | Cached from slice status `filter_lo`/`filter_hi`     |
| `set_passband(rx, pb)`    | `slice set <rx> filter_lo=<lo> filter_hi=<hi>`       |
| `get_ptt()`               | Cached from TX status or interlock status            |
| `set_ptt(on)`             | `xmit <0\|1>` TCP command                            |
| `get_power()`             | Cached from TX status `power` field                  |
| `set_power(watts)`        | `transmit set power=<0-100>` TCP command             |
| `get_s_meter(rx)`         | VITA-49 meter data, keyed by `s-meter` meter name    |
| `get_swr()`               | VITA-49 meter data, keyed by `swr` meter name        |
| `get_alc()`               | VITA-49 meter data, keyed by `alc` meter name        |
| `get_split()`             | True if TX slice != active RX slice                  |
| `set_split(on)`           | If on: set TX to different slice; if off: set TX=RX  |
| `set_tx_receiver(rx)`     | `slice set <rx> tx=1` TCP command                    |
| `subscribe()`             | Route TCP status messages -> RigEvent broadcast      |

### 5.3 State Caching Strategy

Many `get_*` methods can return cached state rather than issuing a TCP
command, because the radio pushes status updates continuously after
subscription. The implementation should:

1. At connection time, subscribe to all relevant status categories
2. Maintain an in-memory state cache updated from status messages
3. Return cached values for `get_*` methods (zero latency)
4. Issue TCP commands only for `set_*` methods
5. Validate `set_*` responses and update cache from subsequent status pushes

This is significantly different from the serial-based backends (Icom, Yaesu,
Kenwood, Elecraft) where `get_*` methods must issue a command and wait for a
response. The FlexRadio implementation will be more responsive but requires
careful concurrent access to the state cache.

---

## 6. Existing Implementations and Dependencies

### 6.1 Rust Ecosystem

**No Rust SmartSDR library exists.** The riglib-flex implementation will be
the first Rust client for the SmartSDR API.

The `vita49` crate on crates.io (version 0.1.x as of 2026) targets
**VITA-49.2**, which is a different (and incompatible) version of the standard
from the **VITA-49.0** that FlexRadio uses. Key differences include header
layout and class identifier encoding. This crate is NOT usable for our
purposes.

**Recommendation**: Write a purpose-built VITA-49.0 parser (~200 lines) within
riglib-flex rather than depending on an incompatible crate. The parser only
needs to handle the specific packet types FlexRadio produces (meter, DAX
audio, and potentially FFT/IQ in Phase 4).

### 6.2 Reference Implementations in Other Languages

**flexlib-go** (Go) -- https://github.com/hb9fxq/flexlib-go
- Good reference for the command/response correlation pattern
- Shows how to handle asynchronous response matching via sequence numbers
- Demonstrates status message parsing with key-value extraction
- Relatively small codebase, easy to read

**FlexLib_Core** (.NET) -- FlexRadio's official client library
- The most complete and authoritative reference implementation
- Written in C#, part of the SmartSDR CAT suite
- Covers all TCP commands, VITA-49 parsing, and DAX audio streaming
- Available as part of the SmartSDR software distribution
- Best source for understanding undocumented protocol behaviors

**smartsdr-api-docs** (GitHub Wiki) -- https://github.com/flexradio/smartsdr-api-docs
- FlexRadio's official API documentation
- Covers TCP command syntax, status message format
- Less complete on VITA-49 details (FlexLib_Core source is better for that)

### 6.3 Dependencies for riglib-flex

No new external crate dependencies are expected for the FlexRadio
implementation beyond what riglib already uses. The required functionality
maps to existing dependencies:

| Need                     | Dependency       | Already in workspace |
|--------------------------|------------------|---------------------|
| TCP client               | `tokio::net`     | Yes (tokio)         |
| UDP socket               | `tokio::net`     | Yes (tokio)         |
| Byte buffer manipulation | `bytes`          | Yes                 |
| Structured logging       | `tracing`        | Yes                 |
| Async channels           | `tokio::sync`    | Yes (tokio)         |
| Float parsing            | `std`            | Yes                 |

---

## 7. Risk Assessment

### 7.1 What Can Be Implemented Without Hardware

The following components can be developed and fully unit-tested using only
documented protocol specifications, synthetic test data, and mock servers:

- **TCP command/response/status codec** -- The text protocol is well-documented
  and deterministic. All encoding/decoding can be tested with string literals.
- **VITA-49 frame header parsing** -- The binary format is documented.
  Hardcoded byte arrays serve as test fixtures.
- **Meter data extraction from VITA-49** -- Known record format (16-bit ID +
  16-bit value). Synthetic frames can exercise all code paths.
- **SmartSDR command builders** -- Pure functions that produce command strings.
  No I/O needed for testing.
- **Mode mapping** -- Static lookup tables, fully testable without a radio.
- **Frequency conversion** -- Pure math (MHz float <-> Hz integer), including
  edge cases and rounding behavior.
- **Model definitions** -- Public specifications, all static data.
- **Connection handshake parsing** -- Parse version and handle lines from
  fixed strings.
- **Status message parsing** -- Parse key-value pairs from fixed strings.
- **Command/response correlation** -- Test with mock TCP server that sends
  responses out of order.
- **Network client integration** -- Test against a mock SmartSDR server (TCP)
  and mock VITA-49 sender (UDP) within the test harness.

### 7.2 What Requires Hardware to Validate

The following areas carry risk because they involve real-world behavior that
may differ from documentation:

- **Connection handshake timing** -- How quickly does the radio send the
  version/handle lines? Are there firmware-version-specific variations?
- **Meter ID assignment** -- Meter IDs are assigned dynamically at runtime.
  The mapping may vary by firmware version or radio model.
- **Status message format variations** -- Different firmware versions may
  include different keys or different value formats in status messages.
- **DAX audio stream setup/teardown** -- Stream creation involves multiple
  steps and the radio may reject requests under certain conditions.
- **Slice creation limits and error responses** -- What error codes are
  returned when slice limits are exceeded? How does the radio respond to
  requests for impossible configurations?
- **Real-world VITA-49 frame captures** -- Synthetic test frames may not
  cover all header flag combinations or payload sizes the radio actually
  produces.
- **Multi-client behavior** -- How does the radio behave when multiple
  clients subscribe to the same status categories? Are there ordering
  guarantees?
- **Reconnection after network interruption** -- How does the radio handle
  a client that disconnects and reconnects? Is state preserved?

### 7.3 Key Risk: No SmartSDR Simulator

**No SmartSDR simulator or emulator is known to exist.** Unlike serial-based
radios where protocol behavior can be tested with simple loopback scripts,
the SmartSDR protocol involves two simultaneous channels (TCP + UDP) with
complex state management. The mock SmartSDR server in riglib-test-harness will
need to be comprehensive enough to exercise the implementation, but it will
only reflect our understanding of the protocol -- not the radio's actual
behavior.

Mitigation strategies:
1. Build the mock server incrementally as each work item is completed
2. Design the mock to be strict about protocol compliance (reject malformed
   commands) so our implementation is validated as correct
3. When hardware becomes available, record real TCP/UDP exchanges and replay
   them as additional test fixtures
4. Engage with the FlexRadio developer community for protocol clarification

---

## 8. Implementation Recommendations

### 8.1 Work Item Sequencing

The implementation should proceed in this order to minimize risk and maximize
testability:

1. **VITA-49 frame parser** (pure parsing, no I/O) -- establishes the binary
   protocol foundation
2. **SmartSDR command/response/status codec** (pure parsing, no I/O) --
   establishes the text protocol foundation
3. **UDP transport** (I/O, but simple) -- enables VITA-49 data reception
4. **Network client** (I/O integration) -- combines TCP + UDP with the codecs
5. **Model definitions** (static data) -- can happen in parallel with anything
6. **Rig implementation + builder** (integration) -- ties everything together
7. **Test application** -- exercises the full stack

Items 1 and 2 are pure parsing with no I/O dependencies. They can be
developed and tested entirely with hardcoded byte arrays and string literals.
This front-loads the protocol understanding and produces high-confidence code
before any network integration.

### 8.2 Architecture Within riglib-flex

```
riglib-flex/src/
  lib.rs          -- FlexRadio struct, impl Rig, FlexRadio-specific methods
  builder.rs      -- FlexRadioBuilder
  codec.rs        -- SmartSDR TCP command/response/status encoding/decoding
  vita49.rs       -- VITA-49 frame parser (header + payload extraction)
  meters.rs       -- Meter ID mapping and value scaling
  client.rs       -- Network client (TCP command channel + UDP VITA-49 receiver)
  state.rs        -- Cached radio state (slices, meters, TX status)
  models.rs       -- FlexRadioModel definitions (6400, 6600, 6700, 8400, 8600)
  mode.rs         -- FlexRadio mode string <-> riglib Mode conversion
  discovery.rs    -- LAN discovery (UDP broadcast listener on port 4992)
```

### 8.3 Concurrency Model

The FlexRadio client requires at least three concurrent tasks:

1. **TCP read loop** -- reads lines from TCP socket, dispatches responses and
   status messages to appropriate channels
2. **UDP receive loop** -- reads VITA-49 datagrams from UDP socket, parses
   headers, dispatches meter data and audio to consumers
3. **Command sender** -- accepts commands from Rig trait methods, assigns
   sequence numbers, sends over TCP, waits for matching response

These should be Tokio tasks communicating via channels:
- `mpsc` channel for outgoing TCP commands
- `oneshot` channels for command-response correlation (one per pending command)
- `broadcast` channel for RigEvent distribution to subscribers
- `watch` channel for meter value updates (latest-value semantics)

---

## 9. References

- [FlexRadio SmartSDR API Documentation (GitHub)](https://github.com/flexradio/smartsdr-api-docs)
- [FlexRadio SmartSDR API Wiki](https://github.com/flexradio/smartsdr-api-docs/wiki/)
- [FlexRadio Developer Program](https://www.flexradio.com/api/developer-program/)
- [VITA-49.0 Standard (VITA)](https://www.vita.com/page-1855421) -- formal specification (paid)
- [flexlib-go (Go reference implementation)](https://github.com/hb9fxq/flexlib-go)
- [FlexLib_Core (.NET reference)](https://github.com/flexradio) -- official FlexRadio libraries
- [SmartSDR CAT Documentation](https://community.flexradio.com/categories/smartsdr-cat) -- FlexRadio community forums
