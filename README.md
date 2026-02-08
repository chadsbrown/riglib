# riglib

Modern rig control library for amateur radio transceivers, written in Rust.

riglib provides an async, manufacturer-agnostic API for controlling HF
transceivers from Icom, Yaesu, Kenwood, Elecraft, and FlexRadio. It is designed
for contest logging software, panadapter displays, and station automation where
low-latency, reliable rig control matters.

**Status: early development (0.1.0).** Protocol implementations, model
definitions, and serial/TCP transports are in place across all five
manufacturers. The API is functional but has not been extensively validated
against real hardware. USE AT YOUR OWN RISK.

## Supported Rigs (In theory, that is.  Most of these are completely untested)

36 models across 5 manufacturers:

| Manufacturer | Models |
|---|---|
| **Icom** | IC-705, IC-7100, IC-7300, IC-7300MKII, IC-7410, IC-7600, IC-7610, IC-7700, IC-7800, IC-7850, IC-7851, IC-905, IC-9100, IC-9700 |
| **Yaesu** | FT-891, FT-991A, FT-710, FT-DX10, FT-DX101D, FT-DX101MP |
| **Kenwood** | TS-590S, TS-590SG, TS-890S, TS-990S |
| **Elecraft** | K3, K3S, K4, KX2, KX3 |
| **FlexRadio** | FLEX-6400, FLEX-6400M, FLEX-6600, FLEX-6600M, FLEX-6700, FLEX-8400, FLEX-8600 |

## Quick Start

Add riglib to your `Cargo.toml`:

```toml
[dependencies]
riglib = { version = "0.1", features = ["icom"] }
tokio = { version = "1", features = ["full"] }
```

Connect to a rig and read its frequency:

```rust
use riglib::{Rig, ReceiverId};
use riglib::icom::{IcomBuilder, models::ic_7610};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let rig = IcomBuilder::new(ic_7610())
        .serial_port("/dev/ttyUSB0")
        .baud_rate(115_200)
        .build()
        .await?;

    let freq = rig.get_frequency(ReceiverId::VFO_A).await?;
    println!("VFO-A: {} Hz", freq);
    Ok(())
}
```

FlexRadio rigs connect over TCP instead of serial:

```rust
use riglib::flex::{FlexRadioBuilder, models::flex_6600};

let rig = FlexRadioBuilder::new()
    .model(flex_6600())
    .host("192.168.1.100")
    .build()
    .await?;
```

## Architecture

riglib is a workspace of focused crates. Most users only need the `riglib`
facade crate, which re-exports everything.

```
riglib (facade)          -- re-exports all public types
  riglib-core            -- Rig trait, types, errors, events
  riglib-transport       -- serial, TCP, UDP transports
  riglib-icom            -- Icom CI-V binary protocol
  riglib-yaesu           -- Yaesu CAT text protocol
  riglib-kenwood         -- Kenwood CAT text protocol
  riglib-elecraft        -- Elecraft extended Kenwood protocol
  riglib-flex            -- FlexRadio SmartSDR TCP + VITA-49
  riglib-test-harness    -- mock transports for testing
```

All manufacturer backends implement the `Rig` trait, so application code can
work with `dyn Rig` and stay manufacturer-agnostic:

```rust
async fn print_freq(rig: &dyn Rig) -> riglib::Result<()> {
    let freq = rig.get_frequency(ReceiverId::VFO_A).await?;
    println!("{:.3} MHz", freq as f64 / 1_000_000.0);
    Ok(())
}
```

### Feature Flags

Each manufacturer backend is behind a feature flag. All are enabled by default.

| Feature | Backend | Default |
|---|---|---|
| `icom` | Icom CI-V | yes |
| `yaesu` | Yaesu CAT | yes |
| `kenwood` | Kenwood CAT | yes |
| `elecraft` | Elecraft extended Kenwood | yes |
| `flex` | FlexRadio SmartSDR | yes |
| `audio` | USB audio streaming (cpal) | no |

To include only the backends you need:

```toml
riglib = { version = "0.1", default-features = false, features = ["icom", "yaesu"] }
```

## The Rig Trait

The `Rig` trait is the central API. All methods that talk to the radio are
`async`. Methods that return cached data (`info()`, `capabilities()`) are sync.

**Frequency and mode:**
- `get_frequency(rx)` / `set_frequency(rx, hz)`
- `get_mode(rx)` / `set_mode(rx, mode)`
- `get_passband(rx)` / `set_passband(rx, pb)`

**Transmit:**
- `get_ptt()` / `set_ptt(on)`
- `get_power()` / `set_power(watts)`
- `get_split()` / `set_split(on)` / `set_tx_receiver(rx)`

**CW:**
- `set_cw_key(on)` -- hardware key line (DTR/RTS)
- `get_cw_speed()` / `set_cw_speed(wpm)`
- `send_cw_message(text)` / `stop_cw_message()`

**Metering:**
- `get_s_meter(rx)` -- signal strength in dBm
- `get_swr()` -- standing wave ratio while transmitting
- `get_alc()` -- automatic level control reading

**Receiver controls:**
- `get_agc(rx)` / `set_agc(rx, mode)`
- `get_preamp(rx)` / `set_preamp(rx, level)`
- `get_attenuator(rx)` / `set_attenuator(rx, db)`
- `get_rit()` / `set_rit(enabled, offset_hz)`
- `get_xit()` / `set_xit(enabled, offset_hz)`
- `get_antenna(rx)` / `set_antenna(rx, port)`

**VFO operations:**
- `set_vfo_a_eq_b(rx)` -- copy active VFO to inactive
- `swap_vfo(rx)` -- swap VFO A and B

**Transceive (auto-information):**
- `enable_transceive()` / `disable_transceive()`

**Info and capabilities:**
- `info()` -- manufacturer, model name, model ID
- `capabilities()` -- frequency ranges, modes, power, feature flags
- `receivers()` -- list available receiver IDs
- `primary_receiver()` / `secondary_receiver()`

**Events:**
- `subscribe()` -- broadcast channel of real-time state changes

Methods that a particular rig doesn't support return `Error::Unsupported`.
Optional trait methods (CW, AGC, RIT/XIT, etc.) have default implementations
that return `Unsupported`, so backends only need to implement what they
actually support.

## ReceiverId

`ReceiverId` is an opaque type that abstracts over VFOs and slices:

- Traditional rigs: `ReceiverId::VFO_A` (0) and `ReceiverId::VFO_B` (1)
- FlexRadio: `ReceiverId::from_index(n)` for slices 0-7

This lets the same API work whether the rig has VFO A/B or FlexRadio's dynamic
slice architecture.

## Events

All rig drivers emit events through a `tokio::sync::broadcast` channel.
Subscribe once and receive frequency changes, mode changes, PTT transitions,
and meter readings without polling:

```rust
let mut events = rig.subscribe()?;
loop {
    match events.recv().await {
        Ok(RigEvent::FrequencyChanged { receiver, freq_hz }) => {
            println!("{}: {:.3} MHz", receiver, freq_hz as f64 / 1e6);
        }
        Ok(RigEvent::PttChanged { on }) => {
            println!("{}", if on { "TX" } else { "RX" });
        }
        Ok(event) => println!("{:?}", event),
        Err(_) => break,
    }
}
```

Event types: `FrequencyChanged`, `ModeChanged`, `PttChanged`, `SplitChanged`,
`SmeterReading`, `PowerReading`, `SwrReading`, `AgcChanged`, `PreampChanged`,
`AttenuatorChanged`, `CwSpeedChanged`, `RitChanged`, `XitChanged`,
`Connected`, `Disconnected`, `Reconnecting`.

## Builder Pattern

Each manufacturer has a fluent builder. Serial-based rigs follow the same
pattern:

```rust
// Icom
let rig = IcomBuilder::new(ic_7610())
    .serial_port("/dev/ttyUSB0")
    .baud_rate(115_200)
    .civ_address(0x98)        // override CI-V address
    .command_timeout(Duration::from_millis(300))
    .auto_retry(true)
    .max_retries(3)
    .collision_recovery(true) // Icom-specific: CI-V bus collision handling
    .ptt_method(PttMethod::Cat)
    .key_line(KeyLine::Dtr)
    .build()
    .await?;

// Yaesu
let rig = YaesuBuilder::new(ft_dx10())
    .serial_port("/dev/ttyUSB0")
    .build()
    .await?;

// Kenwood
let rig = KenwoodBuilder::new(ts_890s())
    .serial_port("/dev/ttyUSB0")
    .build()
    .await?;

// Elecraft
let rig = ElecraftBuilder::new(k4())
    .serial_port("/dev/ttyUSB0")
    .build()
    .await?;
```

FlexRadio connects over TCP:

```rust
// By IP address
let rig = FlexRadioBuilder::new()
    .model(flex_6600())
    .host("192.168.1.100")
    .client_name("my-logger")
    .auto_create_slices(true)
    .build()
    .await?;

// From LAN discovery
let rig = FlexRadioBuilder::new()
    .radio(&discovered_radio)
    .build()
    .await?;
```

All builders also expose `build_with_transport()` (serial backends) or
`build_with_client()` (FlexRadio) for injecting mock transports in tests.

## PTT and CW Keying

PTT can be triggered three ways, configured on the builder:

| Method | How | When to use |
|---|---|---|
| `PttMethod::Cat` | Protocol command (default) | Standard CAT/CI-V PTT |
| `PttMethod::Dtr` | DTR serial line | Hardware PTT via DTR |
| `PttMethod::Rts` | RTS serial line | Hardware PTT via RTS |

CW keying uses a dedicated serial control line:

| Line | How |
|---|---|
| `KeyLine::None` | No hardware keying (default) |
| `KeyLine::Dtr` | CW key via DTR |
| `KeyLine::Rts` | CW key via RTS |

PTT and key line cannot share the same serial line (the builder validates this).

## Model Enumeration

Applications can enumerate supported rigs for UI pickers:

```rust
// List all supported rigs
for rig in riglib::supported_rigs() {
    println!("{} {} ({:?})", rig.manufacturer, rig.model_name, rig.connection);
}

// Find a specific rig (case-insensitive, hyphen-insensitive)
if let Some(rig) = riglib::find_rig("IC-7610") {
    println!("{} W max", rig.capabilities.max_power_watts);
}
```

## RigCapabilities

Each model reports its capabilities:

- `max_receivers` -- 1 for single-VFO, 2 for dual-receiver rigs
- `has_sub_receiver`, `has_split`
- `max_power_watts`
- `frequency_ranges` -- list of `BandRange { low_hz, high_hz }`
- `modes` -- supported operating modes
- `agc_modes` -- available AGC settings
- `preamp_levels`, `attenuator_levels`
- `antenna_ports`
- `has_rit`, `has_xit`
- `has_cw_keyer`, `has_cw_memory`
- `has_transceive` -- supports auto-information mode
- `has_iq_output` -- IQ data output capable

## Examples

See [`crates/riglib/examples/`](crates/riglib/examples/) for runnable examples:

- **basic_icom.rs** -- connect to an IC-7610, read/set frequency and mode
- **monitor_events.rs** -- subscribe to real-time rig events
- **scan_frequencies.rs** -- frequency scanning
- **flexradio_discover.rs** -- FlexRadio LAN discovery
- **audio_record.rs** -- audio recording (requires `audio` feature)

```sh
cargo run -p riglib --features icom --example basic_icom
cargo run -p riglib --features icom --example monitor_events
```

## Requirements

- Rust 1.85+ (edition 2024)
- Tokio runtime

## License

MIT
