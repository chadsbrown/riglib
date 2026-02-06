// riglib test application -- CLI tool for exercising all five manufacturer
// backends (Icom, Yaesu, Kenwood, Elecraft, FlexRadio) against real
// hardware or a mock transport.
//
// Usage:
//   riglib-test-app --manufacturer icom --model IC-7610 --port /dev/ttyUSB0 freq get
//   riglib-test-app --manufacturer yaesu --model FT-DX10 --mock info
//   riglib-test-app --manufacturer flex --host 192.168.1.100 info
//   riglib-test-app --manufacturer flex --discover info
//   riglib-test-app discover
//   riglib-test-app list
//   riglib-test-app list --manufacturer flex
//
// Audio commands:
//   riglib-test-app audio list-devices
//   riglib-test-app --manufacturer icom --model IC-7610 --port /dev/ttyUSB0 \
//       --device "USB Audio CODEC" audio rx --duration 10 --output recording.wav
//   riglib-test-app --manufacturer flex --host 192.168.1.100 --model FLEX-6600 \
//       audio rx --duration 5
//   riglib-test-app --manufacturer icom --model IC-7610 --port /dev/ttyUSB0 \
//       --device "USB Audio CODEC" audio monitor --duration 30

use std::io::{self, Write};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use rand::Rng;

use riglib::elecraft::builder::ElecraftBuilder;
use riglib::elecraft::models as elecraft_models;
use riglib::flex::FlexRadio;
use riglib::flex::builder::FlexRadioBuilder;
use riglib::flex::models as flex_models;
use riglib::icom::builder::IcomBuilder;
use riglib::icom::models as icom_models;
use riglib::kenwood::builder::KenwoodBuilder;
use riglib::kenwood::models as kenwood_models;
use riglib::yaesu::builder::YaesuBuilder;
use riglib::yaesu::models as yaesu_models;
use riglib::{AudioCapable, Mode, ReceiverId, Rig};
use riglib_test_harness::MockTransport;
use riglib_transport::SerialTransport;
use riglib_transport::audio::list_audio_devices;

// ---------------------------------------------------------------------------
// Combined trait for rig + audio capability
// ---------------------------------------------------------------------------

/// Combined trait so we can use a single `Box<dyn RigAudio>` for commands
/// that need both rig control and audio streaming.
trait RigAudio: Rig + AudioCapable {}
impl<T: Rig + AudioCapable> RigAudio for T {}

// ---------------------------------------------------------------------------
// CLI argument definitions
// ---------------------------------------------------------------------------

/// riglib test application -- exercises rig backends from the command line.
#[derive(Parser)]
#[command(name = "riglib-test-app", version, about)]
struct Cli {
    /// Manufacturer: icom, yaesu, kenwood, elecraft, flex.
    /// Required for all commands except `list` and `discover`.
    #[arg(long)]
    manufacturer: Option<String>,

    /// Rig model name (e.g. IC-7610, FT-DX10, TS-890S, K4, FLEX-6600).
    /// Required for all commands except `list` and `discover`.
    #[arg(long)]
    model: Option<String>,

    /// Serial port path (e.g. /dev/ttyUSB0, COM3).
    /// Required for serial manufacturers (Icom, Yaesu, Kenwood, Elecraft)
    /// unless --mock is used. Not valid for FlexRadio.
    #[arg(long)]
    port: Option<String>,

    /// FlexRadio IP address (e.g. 192.168.1.100).
    /// Used instead of --port for FlexRadio connections.
    #[arg(long)]
    host: Option<String>,

    /// Auto-discover a FlexRadio on the LAN and connect to it.
    /// Only valid with --manufacturer flex.
    #[arg(long)]
    discover: bool,

    /// Override the default baud rate for this model.
    #[arg(long)]
    baud: Option<u32>,

    /// Override the default CI-V address (hex, e.g. 0x98). Icom only.
    #[arg(long, value_parser = parse_hex_u8)]
    civ_addr: Option<u8>,

    /// USB audio device name for audio streaming (e.g. "USB Audio CODEC").
    /// Used by serial manufacturers (Icom, Yaesu, Kenwood, Elecraft) for
    /// the `audio rx` and `audio monitor` commands.
    #[arg(long)]
    device: Option<String>,

    /// Use a mock transport instead of a real serial port.
    /// Useful for verifying CLI parsing and builder wiring without hardware.
    /// Not supported for FlexRadio (network-only).
    #[arg(long)]
    mock: bool,

    #[command(subcommand)]
    command: Command,
}

/// Parse a hex string like "0x98" or "98" into a u8.
fn parse_hex_u8(s: &str) -> std::result::Result<u8, String> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u8::from_str_radix(s, 16).map_err(|e| format!("invalid hex byte: {e}"))
}

#[derive(Subcommand)]
enum Command {
    /// Print rig info and capabilities.
    Info,

    /// Frequency operations.
    Freq {
        #[command(subcommand)]
        action: FreqAction,
    },

    /// Mode operations.
    Mode {
        #[command(subcommand)]
        action: ModeAction,
    },

    /// PTT operations.
    Ptt {
        #[command(subcommand)]
        action: PttAction,
    },

    /// Read a meter value.
    Meter {
        /// Meter type to read.
        #[arg(long, default_value = "s", value_enum)]
        r#type: MeterType,

        /// Receiver index for S-meter (default: 0 = VFO-A).
        #[arg(long, default_value_t = 0)]
        rx: u8,
    },

    /// Split operations.
    Split {
        #[command(subcommand)]
        action: SplitAction,
    },

    /// Subscribe to rig events and print them in real time.
    Monitor {
        /// Duration in seconds (0 = run until Ctrl-C).
        #[arg(long, default_value_t = 0)]
        duration: u64,
    },

    /// Stress test: rapid-fire frequency read/write cycles.
    Stress {
        /// Number of read/write cycles.
        #[arg(long, default_value_t = 100)]
        count: u32,

        /// Receiver index (default: 0 = VFO-A).
        #[arg(long, default_value_t = 0)]
        rx: u8,
    },

    /// List all supported rig models (optionally filtered by manufacturer).
    List {
        /// Filter by manufacturer: icom, yaesu, kenwood, elecraft, flex.
        #[arg(long)]
        manufacturer: Option<String>,
    },

    /// Discover FlexRadio radios on the LAN (3-second timeout).
    /// Does not require --manufacturer, --model, or --port.
    Discover,

    /// List all active FlexRadio slices (FlexRadio only).
    Slices,

    /// Create a new FlexRadio slice at a given frequency and mode (FlexRadio only).
    CreateSlice {
        /// Frequency in hertz (e.g. 14250000).
        freq_hz: u64,
        /// Mode name (e.g. USB, LSB, CW).
        mode: String,
    },

    /// Destroy a FlexRadio slice by index (FlexRadio only).
    DestroySlice {
        /// Slice index to destroy (e.g. 0, 1, 2).
        slice_index: u8,
    },

    /// Audio streaming operations.
    Audio {
        #[command(subcommand)]
        action: AudioAction,
    },
}

#[derive(Subcommand)]
enum AudioAction {
    /// List available audio input/output devices.
    ListDevices,

    /// Record RX audio from the rig to a WAV file.
    Rx {
        /// Recording duration in seconds.
        #[arg(long, default_value_t = 5)]
        duration: u64,

        /// Output WAV file path.
        #[arg(long, default_value = "output.wav")]
        output: String,

        /// Receiver index (default: 0 = VFO-A).
        #[arg(long, default_value_t = 0)]
        receiver: u8,
    },

    /// Monitor RX audio levels (RMS/peak) in real time.
    Monitor {
        /// Duration in seconds (0 = run until Ctrl-C).
        #[arg(long, default_value_t = 0)]
        duration: u64,

        /// Receiver index (default: 0 = VFO-A).
        #[arg(long, default_value_t = 0)]
        receiver: u8,
    },
}

#[derive(Subcommand)]
enum FreqAction {
    /// Read the current frequency.
    Get {
        /// Receiver index (default: 0 = VFO-A).
        #[arg(long, default_value_t = 0)]
        rx: u8,
    },
    /// Set the frequency (in Hz).
    Set {
        /// Frequency in hertz (e.g. 14250000).
        freq_hz: u64,

        /// Receiver index (default: 0 = VFO-A).
        #[arg(long, default_value_t = 0)]
        rx: u8,
    },
}

#[derive(Subcommand)]
enum ModeAction {
    /// Read the current mode.
    Get {
        /// Receiver index (default: 0 = VFO-A).
        #[arg(long, default_value_t = 0)]
        rx: u8,
    },
    /// Set the operating mode.
    Set {
        /// Mode name (e.g. USB, LSB, CW, CWR, AM, FM, RTTY, DATA-USB).
        mode: String,

        /// Receiver index (default: 0 = VFO-A).
        #[arg(long, default_value_t = 0)]
        rx: u8,
    },
}

#[derive(Subcommand)]
enum PttAction {
    /// Read the current PTT state.
    Get,
    /// Key the transmitter (with safety confirmation).
    On,
    /// Unkey the transmitter.
    Off,
}

#[derive(Subcommand)]
enum SplitAction {
    /// Read the current split state.
    Get,
    /// Enable split operation.
    On,
    /// Disable split operation.
    Off,
}

#[derive(Clone, Debug, ValueEnum)]
enum MeterType {
    S,
    Swr,
    Alc,
    Power,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a frequency in Hz as a human-readable MHz string.
fn format_freq(hz: u64) -> String {
    let mhz = hz as f64 / 1_000_000.0;
    format!("{mhz:.6} MHz")
}

/// Convert a receiver index argument to a ReceiverId.
fn receiver_id(rx: u8) -> ReceiverId {
    ReceiverId::from_index(rx)
}

/// Prompt the user for y/N confirmation. Returns true only if "y" or "Y" entered.
fn confirm(prompt: &str) -> bool {
    print!("{prompt}");
    io::stdout().flush().ok();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim(), "y" | "Y")
}

/// Check if the given manufacturer string refers to FlexRadio.
fn is_flex_manufacturer(mfr: &str) -> bool {
    mfr.to_lowercase() == "flex"
}

/// Check if the given manufacturer string refers to a serial-port manufacturer.
fn is_serial_manufacturer(mfr: &str) -> bool {
    matches!(
        mfr.to_lowercase().as_str(),
        "icom" | "yaesu" | "kenwood" | "elecraft"
    )
}

// ---------------------------------------------------------------------------
// Model lookup
// ---------------------------------------------------------------------------

/// Normalize a model name for case-insensitive comparison.
/// Strips hyphens and converts to lowercase: "IC-7610" -> "ic7610".
fn normalize_model(name: &str) -> String {
    name.to_lowercase().replace('-', "")
}

/// Look up an Icom model by name (case-insensitive, hyphen-insensitive).
fn lookup_icom_model(name: &str) -> Result<icom_models::IcomModel> {
    let norm = normalize_model(name);
    let result = match norm.as_str() {
        "ic7600" => icom_models::ic_7600(),
        "ic7700" => icom_models::ic_7700(),
        "ic7800" => icom_models::ic_7800(),
        "ic7850" => icom_models::ic_7850(),
        "ic7851" => icom_models::ic_7851(),
        "ic7300" => icom_models::ic_7300(),
        "ic7610" => icom_models::ic_7610(),
        "ic9700" => icom_models::ic_9700(),
        "ic705" => icom_models::ic_705(),
        "ic7300mk2" => icom_models::ic_7300mk2(),
        "ic7100" => icom_models::ic_7100(),
        "ic9100" => icom_models::ic_9100(),
        "ic7410" => icom_models::ic_7410(),
        "ic905" => icom_models::ic_905(),
        _ => {
            let known: Vec<&str> = icom_models::all_icom_models()
                .iter()
                .map(|m| m.name)
                .collect();
            bail!(
                "unknown Icom model '{}'. Supported models: {}",
                name,
                known.join(", ")
            );
        }
    };
    Ok(result)
}

/// Look up a Yaesu model by name (case-insensitive, hyphen-insensitive).
fn lookup_yaesu_model(name: &str) -> Result<yaesu_models::YaesuModel> {
    let norm = normalize_model(name);
    let result = match norm.as_str() {
        "ftdx10" => yaesu_models::ft_dx10(),
        "ft891" => yaesu_models::ft_891(),
        "ft991a" => yaesu_models::ft_991a(),
        "ftdx101d" => yaesu_models::ft_dx101d(),
        "ftdx101mp" => yaesu_models::ft_dx101mp(),
        "ft710" => yaesu_models::ft_710(),
        _ => {
            bail!(
                "unknown Yaesu model '{}'. Supported models: FT-DX10, FT-891, FT-991A, \
                 FT-DX101D, FT-DX101MP, FT-710",
                name
            );
        }
    };
    Ok(result)
}

/// Look up a Kenwood model by name (case-insensitive, hyphen-insensitive).
fn lookup_kenwood_model(name: &str) -> Result<kenwood_models::KenwoodModel> {
    let norm = normalize_model(name);
    let result = match norm.as_str() {
        "ts590s" => kenwood_models::ts_590s(),
        "ts590sg" => kenwood_models::ts_590sg(),
        "ts990s" => kenwood_models::ts_990s(),
        "ts890s" => kenwood_models::ts_890s(),
        _ => {
            bail!(
                "unknown Kenwood model '{}'. Supported models: TS-590S, TS-590SG, \
                 TS-990S, TS-890S",
                name
            );
        }
    };
    Ok(result)
}

/// Look up an Elecraft model by name (case-insensitive, hyphen-insensitive).
fn lookup_elecraft_model(name: &str) -> Result<elecraft_models::ElecraftModel> {
    let norm = normalize_model(name);
    let result = match norm.as_str() {
        "k3" => elecraft_models::k3(),
        "k3s" => elecraft_models::k3s(),
        "kx3" => elecraft_models::kx3(),
        "kx2" => elecraft_models::kx2(),
        "k4" => elecraft_models::k4(),
        _ => {
            bail!(
                "unknown Elecraft model '{}'. Supported models: K3, K3S, KX3, KX2, K4",
                name
            );
        }
    };
    Ok(result)
}

/// Look up a FlexRadio model by name (case-insensitive, hyphen-insensitive).
fn lookup_flex_model(name: &str) -> Result<flex_models::FlexRadioModel> {
    let norm = normalize_model(name);
    let result = match norm.as_str() {
        "flex6400" => flex_models::flex_6400(),
        "flex6400m" => flex_models::flex_6400m(),
        "flex6600" => flex_models::flex_6600(),
        "flex6600m" => flex_models::flex_6600m(),
        "flex6700" => flex_models::flex_6700(),
        "flex8400" => flex_models::flex_8400(),
        "flex8600" => flex_models::flex_8600(),
        _ => {
            let known: Vec<&str> = flex_models::all_flex_models()
                .iter()
                .map(|m| m.name)
                .collect();
            bail!(
                "unknown FlexRadio model '{}'. Supported models: {}",
                name,
                known.join(", ")
            );
        }
    };
    Ok(result)
}

// ---------------------------------------------------------------------------
// List command
// ---------------------------------------------------------------------------

/// A display row for the model listing table.
struct ModelEntry {
    manufacturer: &'static str,
    name: &'static str,
    max_power: f32,
    freq_summary: String,
}

/// Format frequency ranges into a compact summary string.
fn summarize_freq_ranges(ranges: &[riglib::BandRange]) -> String {
    ranges
        .iter()
        .map(|r| {
            let low = r.low_hz as f64 / 1_000_000.0;
            let high = r.high_hz as f64 / 1_000_000.0;
            if high >= 1000.0 {
                // Show in GHz for microwave bands
                format!("{:.0}-{:.0} MHz", low, high)
            } else {
                format!("{:.1}-{:.1} MHz", low, high)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Gather all model entries, optionally filtered by manufacturer.
fn gather_models(filter_mfr: Option<&str>) -> Vec<ModelEntry> {
    let mut entries = Vec::new();

    let show_icom = filter_mfr.is_none() || filter_mfr == Some("icom");
    let show_yaesu = filter_mfr.is_none() || filter_mfr == Some("yaesu");
    let show_kenwood = filter_mfr.is_none() || filter_mfr == Some("kenwood");
    let show_elecraft = filter_mfr.is_none() || filter_mfr == Some("elecraft");
    let show_flex = filter_mfr.is_none() || filter_mfr == Some("flex");

    if show_icom {
        for m in icom_models::all_icom_models() {
            entries.push(ModelEntry {
                manufacturer: "Icom",
                name: m.name,
                max_power: m.capabilities.max_power_watts,
                freq_summary: summarize_freq_ranges(&m.capabilities.frequency_ranges),
            });
        }
    }

    if show_yaesu {
        let yaesu_models_list = [
            yaesu_models::ft_dx10(),
            yaesu_models::ft_891(),
            yaesu_models::ft_991a(),
            yaesu_models::ft_dx101d(),
            yaesu_models::ft_dx101mp(),
            yaesu_models::ft_710(),
        ];
        for m in yaesu_models_list {
            entries.push(ModelEntry {
                manufacturer: "Yaesu",
                name: m.name,
                max_power: m.capabilities.max_power_watts,
                freq_summary: summarize_freq_ranges(&m.capabilities.frequency_ranges),
            });
        }
    }

    if show_kenwood {
        for m in kenwood_models::all_kenwood_models() {
            entries.push(ModelEntry {
                manufacturer: "Kenwood",
                name: m.name,
                max_power: m.capabilities.max_power_watts,
                freq_summary: summarize_freq_ranges(&m.capabilities.frequency_ranges),
            });
        }
    }

    if show_elecraft {
        for m in elecraft_models::all_elecraft_models() {
            entries.push(ModelEntry {
                manufacturer: "Elecraft",
                name: m.name,
                max_power: m.capabilities.max_power_watts,
                freq_summary: summarize_freq_ranges(&m.capabilities.frequency_ranges),
            });
        }
    }

    if show_flex {
        for m in flex_models::all_flex_models() {
            entries.push(ModelEntry {
                manufacturer: "FlexRadio",
                name: m.name,
                max_power: m.capabilities.max_power_watts,
                freq_summary: summarize_freq_ranges(&m.capabilities.frequency_ranges),
            });
        }
    }

    entries
}

fn cmd_list(filter_mfr: Option<&str>) -> Result<()> {
    // Validate the filter if provided.
    if let Some(mfr) = filter_mfr {
        let mfr_lower = mfr.to_lowercase();
        if !["icom", "yaesu", "kenwood", "elecraft", "flex"].contains(&mfr_lower.as_str()) {
            bail!(
                "unknown manufacturer '{}'. Supported: icom, yaesu, kenwood, elecraft, flex",
                mfr
            );
        }
    }

    let filter = filter_mfr.map(|s| s.to_lowercase());
    let entries = gather_models(filter.as_deref());

    if entries.is_empty() {
        println!("No models found.");
        return Ok(());
    }

    // Calculate column widths for alignment.
    let mfr_width = entries
        .iter()
        .map(|e| e.manufacturer.len())
        .max()
        .unwrap_or(12)
        .max(12);
    let name_width = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(12)
        .max(12);

    // Print header.
    println!(
        "{:<mfr_width$}  {:<name_width$}  {:>7}  Frequency Coverage",
        "Manufacturer",
        "Model",
        "Power",
        mfr_width = mfr_width,
        name_width = name_width,
    );
    println!(
        "{:<mfr_width$}  {:<name_width$}  {:>7}  -------------------",
        "-".repeat(mfr_width),
        "-".repeat(name_width),
        "-------",
        mfr_width = mfr_width,
        name_width = name_width,
    );

    for entry in &entries {
        println!(
            "{:<mfr_width$}  {:<name_width$}  {:>5.0} W  {}",
            entry.manufacturer,
            entry.name,
            entry.max_power,
            entry.freq_summary,
            mfr_width = mfr_width,
            name_width = name_width,
        );
    }

    println!();
    println!("{} models total.", entries.len());

    Ok(())
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate CLI option combinations before attempting to create a rig.
/// Returns an error for invalid combinations like --port with FlexRadio
/// or --host with serial manufacturers.
fn validate_options(cli: &Cli) -> Result<()> {
    if let Some(mfr) = cli.manufacturer.as_deref() {
        if is_flex_manufacturer(mfr) {
            if cli.port.is_some() {
                bail!("--port is not valid for FlexRadio (use --host instead)");
            }
            if cli.civ_addr.is_some() {
                bail!("--civ-addr is only supported for Icom rigs");
            }
            if cli.baud.is_some() {
                bail!("--baud is not valid for FlexRadio (network-only)");
            }
        }

        if is_serial_manufacturer(mfr) {
            if cli.host.is_some() {
                bail!("--host is only valid for FlexRadio (use --port for serial manufacturers)");
            }
            if cli.discover {
                bail!("--discover is only valid for FlexRadio");
            }
        }
    }

    // --discover without flex manufacturer
    if cli.discover {
        if let Some(mfr) = cli.manufacturer.as_deref() {
            if !is_flex_manufacturer(mfr) {
                bail!("--discover is only valid for FlexRadio");
            }
        }
    }

    Ok(())
}

/// Validate that a command is appropriate for the current manufacturer.
/// FlexRadio-specific commands (slices, create-slice, destroy-slice) require
/// --manufacturer flex.
fn validate_command_for_manufacturer(cli: &Cli) -> Result<()> {
    let flex_only = matches!(
        &cli.command,
        Command::Slices | Command::CreateSlice { .. } | Command::DestroySlice { .. }
    );

    if flex_only {
        let mfr = cli
            .manufacturer
            .as_deref()
            .context("--manufacturer flex is required for this command")?;
        if !is_flex_manufacturer(mfr) {
            bail!(
                "the '{}' command is only available for FlexRadio (--manufacturer flex)",
                match &cli.command {
                    Command::Slices => "slices",
                    Command::CreateSlice { .. } => "create-slice",
                    Command::DestroySlice { .. } => "destroy-slice",
                    _ => unreachable!(),
                }
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Rig construction
// ---------------------------------------------------------------------------

/// Construct a rig from CLI arguments, dispatching to the correct manufacturer
/// builder. Returns a trait object that supports both rig control and audio.
///
/// When `audio_device` is provided (via `--device`), it is passed to the
/// builder for serial manufacturers so the rig supports [`AudioCapable`].
async fn create_rig(cli: &Cli) -> Result<Box<dyn RigAudio>> {
    let manufacturer = cli
        .manufacturer
        .as_deref()
        .context("--manufacturer is required for this command")?;
    let model_name = cli
        .model
        .as_deref()
        .context("--model is required for this command")?;

    let mfr = manufacturer.to_lowercase();

    match mfr.as_str() {
        "icom" => {
            let model = lookup_icom_model(model_name)?;
            let mut builder = IcomBuilder::new(model.clone());

            if let Some(baud) = cli.baud {
                builder = builder.baud_rate(baud);
            }
            if let Some(addr) = cli.civ_addr {
                builder = builder.civ_address(addr);
            }
            if let Some(ref dev) = cli.device {
                builder = builder.audio_device(dev);
            }

            if cli.mock {
                let mock = MockTransport::new();
                let rig = builder
                    .build_with_transport(Box::new(mock))
                    .await
                    .context("failed to build IcomRig with mock transport")?;
                println!("Connected (mock transport) -- Icom {}", model.name);
                Ok(Box::new(rig))
            } else {
                let port = cli
                    .port
                    .as_deref()
                    .context("--port is required when not using --mock")?;
                let baud = cli.baud.unwrap_or(model.default_baud_rate);

                let transport = SerialTransport::open(port, baud)
                    .await
                    .with_context(|| format!("failed to open serial port {port} at {baud} baud"))?;

                if let Some(p) = &cli.port {
                    builder = builder.serial_port(p);
                }

                let rig = builder
                    .build_with_transport(Box::new(transport))
                    .await
                    .context("failed to build IcomRig")?;

                println!("Connected to {port} at {baud} baud -- Icom {}", model.name);
                Ok(Box::new(rig))
            }
        }
        "yaesu" => {
            if cli.civ_addr.is_some() {
                bail!("--civ-addr is only supported for Icom rigs");
            }

            let model = lookup_yaesu_model(model_name)?;
            let mut builder = YaesuBuilder::new(model.clone());

            if let Some(baud) = cli.baud {
                builder = builder.baud_rate(baud);
            }
            if let Some(ref dev) = cli.device {
                builder = builder.audio_device(dev);
            }

            if cli.mock {
                let mock = MockTransport::new();
                let rig = builder.build_with_transport(Box::new(mock));
                println!("Connected (mock transport) -- Yaesu {}", model.name);
                Ok(Box::new(rig))
            } else {
                let port = cli
                    .port
                    .as_deref()
                    .context("--port is required when not using --mock")?;
                let baud = cli.baud.unwrap_or(model.default_baud_rate);

                let transport = SerialTransport::open(port, baud)
                    .await
                    .with_context(|| format!("failed to open serial port {port} at {baud} baud"))?;

                builder = builder.serial_port(port);

                let rig = builder.build_with_transport(Box::new(transport));

                println!("Connected to {port} at {baud} baud -- Yaesu {}", model.name);
                Ok(Box::new(rig))
            }
        }
        "kenwood" => {
            if cli.civ_addr.is_some() {
                bail!("--civ-addr is only supported for Icom rigs");
            }

            let model = lookup_kenwood_model(model_name)?;
            let mut builder = KenwoodBuilder::new(model.clone());

            if let Some(baud) = cli.baud {
                builder = builder.baud_rate(baud);
            }
            if let Some(ref dev) = cli.device {
                builder = builder.audio_device(dev);
            }

            if cli.mock {
                let mock = MockTransport::new();
                let rig = builder
                    .build_with_transport(Box::new(mock))
                    .await
                    .context("failed to build KenwoodRig with mock transport")?;
                println!("Connected (mock transport) -- Kenwood {}", model.name);
                Ok(Box::new(rig))
            } else {
                let port = cli
                    .port
                    .as_deref()
                    .context("--port is required when not using --mock")?;
                let baud = cli.baud.unwrap_or(model.default_baud_rate);

                let transport = SerialTransport::open(port, baud)
                    .await
                    .with_context(|| format!("failed to open serial port {port} at {baud} baud"))?;

                builder = builder.serial_port(port);

                let rig = builder
                    .build_with_transport(Box::new(transport))
                    .await
                    .context("failed to build KenwoodRig")?;

                println!(
                    "Connected to {port} at {baud} baud -- Kenwood {}",
                    model.name
                );
                Ok(Box::new(rig))
            }
        }
        "elecraft" => {
            if cli.civ_addr.is_some() {
                bail!("--civ-addr is only supported for Icom rigs");
            }

            let model = lookup_elecraft_model(model_name)?;
            let mut builder = ElecraftBuilder::new(model.clone());

            if let Some(baud) = cli.baud {
                builder = builder.baud_rate(baud);
            }
            if let Some(ref dev) = cli.device {
                builder = builder.audio_device(dev);
            }

            if cli.mock {
                let mock = MockTransport::new();
                let rig = builder
                    .build_with_transport(Box::new(mock))
                    .await
                    .context("failed to build ElecraftRig with mock transport")?;
                println!("Connected (mock transport) -- Elecraft {}", model.name);
                Ok(Box::new(rig))
            } else {
                let port = cli
                    .port
                    .as_deref()
                    .context("--port is required when not using --mock")?;
                let baud = cli.baud.unwrap_or(model.default_baud_rate);

                let transport = SerialTransport::open(port, baud)
                    .await
                    .with_context(|| format!("failed to open serial port {port} at {baud} baud"))?;

                builder = builder.serial_port(port);

                let rig = builder
                    .build_with_transport(Box::new(transport))
                    .await
                    .context("failed to build ElecraftRig")?;

                println!(
                    "Connected to {port} at {baud} baud -- Elecraft {}",
                    model.name
                );
                Ok(Box::new(rig))
            }
        }
        "flex" => {
            if cli.mock {
                bail!(
                    "Mock mode is not supported for FlexRadio (network-only). \
                     FlexRadio uses TCP/UDP and has no serial mock transport."
                );
            }

            let model = lookup_flex_model(model_name)?;
            let mut builder = FlexRadioBuilder::new().model(model.clone());

            if let Some(host) = &cli.host {
                builder = builder.host(host);
            } else if cli.discover {
                let radios = FlexRadio::discover(Duration::from_secs(3))
                    .await
                    .context("FlexRadio discovery failed")?;
                if radios.is_empty() {
                    bail!("No FlexRadio found on the network");
                }
                println!(
                    "Found: {} ({}) at {}",
                    radios[0].model, radios[0].nickname, radios[0].ip
                );
                builder = builder.radio(&radios[0]);
            } else {
                bail!("FlexRadio requires --host <IP> or --discover");
            }

            let rig = builder
                .build()
                .await
                .context("failed to connect to FlexRadio")?;

            println!("Connected to FlexRadio {}", model.name);
            Ok(Box::new(rig))
        }
        _ => {
            bail!(
                "unknown manufacturer '{}'. Supported: icom, yaesu, kenwood, elecraft, flex",
                manufacturer
            );
        }
    }
}

/// Construct a FlexRadio specifically (not as `Box<dyn Rig>`) so we can
/// access FlexRadio-specific extension methods (slices, create_slice, etc.).
async fn create_flex_radio(cli: &Cli) -> Result<FlexRadio> {
    if cli.mock {
        bail!(
            "Mock mode is not supported for FlexRadio (network-only). \
             FlexRadio uses TCP/UDP and has no serial mock transport."
        );
    }

    let model_name = cli
        .model
        .as_deref()
        .context("--model is required for this command")?;

    let model = lookup_flex_model(model_name)?;
    let mut builder = FlexRadioBuilder::new().model(model.clone());

    if let Some(host) = &cli.host {
        builder = builder.host(host);
    } else if cli.discover {
        let radios = FlexRadio::discover(Duration::from_secs(3))
            .await
            .context("FlexRadio discovery failed")?;
        if radios.is_empty() {
            bail!("No FlexRadio found on the network");
        }
        println!(
            "Found: {} ({}) at {}",
            radios[0].model, radios[0].nickname, radios[0].ip
        );
        builder = builder.radio(&radios[0]);
    } else {
        bail!("FlexRadio requires --host <IP> or --discover");
    }

    let rig = builder
        .build()
        .await
        .context("failed to connect to FlexRadio")?;

    println!("Connected to FlexRadio {}", model.name);
    Ok(rig)
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

async fn cmd_info(rig: &dyn Rig) -> Result<()> {
    let info = rig.info();
    let caps = rig.capabilities();

    println!("Rig Information");
    println!("  Manufacturer:   {}", info.manufacturer);
    println!("  Model:          {}", info.model_name);
    println!("  Model ID:       {}", info.model_id);
    println!();
    println!("Capabilities");
    println!("  Max receivers:  {}", caps.max_receivers);
    println!("  Sub receiver:   {}", caps.has_sub_receiver);
    println!("  Split:          {}", caps.has_split);
    println!("  Audio stream:   {}", caps.has_audio_streaming);
    println!("  I/Q output:     {}", caps.has_iq_output);
    println!("  Max power:      {} W", caps.max_power_watts);
    println!(
        "  Modes:          {}",
        caps.supported_modes
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  Freq ranges:    {}",
        caps.frequency_ranges
            .iter()
            .map(|r| format!("{} - {}", format_freq(r.low_hz), format_freq(r.high_hz)))
            .collect::<Vec<_>>()
            .join("; ")
    );
    Ok(())
}

async fn cmd_freq_get(rig: &dyn Rig, rx: u8) -> Result<()> {
    let rid = receiver_id(rx);
    let freq = rig.get_frequency(rid).await?;
    println!("{rid}: {}", format_freq(freq));
    Ok(())
}

async fn cmd_freq_set(rig: &dyn Rig, rx: u8, freq_hz: u64) -> Result<()> {
    let rid = receiver_id(rx);
    rig.set_frequency(rid, freq_hz).await?;
    println!("{rid}: set to {}", format_freq(freq_hz));
    Ok(())
}

async fn cmd_mode_get(rig: &dyn Rig, rx: u8) -> Result<()> {
    let rid = receiver_id(rx);
    let mode = rig.get_mode(rid).await?;
    println!("{rid}: {mode}");
    Ok(())
}

async fn cmd_mode_set(rig: &dyn Rig, rx: u8, mode_str: &str) -> Result<()> {
    let mode: Mode = mode_str
        .parse()
        .map_err(|e: riglib::ParseModeError| anyhow::anyhow!("{e}"))?;
    let rid = receiver_id(rx);
    rig.set_mode(rid, mode).await?;
    println!("{rid}: mode set to {mode}");
    Ok(())
}

async fn cmd_ptt_get(rig: &dyn Rig) -> Result<()> {
    let on = rig.get_ptt().await?;
    if on {
        println!("PTT: ON (transmitting)");
    } else {
        println!("PTT: OFF (receiving)");
    }
    Ok(())
}

async fn cmd_ptt_on(rig: &dyn Rig) -> Result<()> {
    println!("WARNING: This will key the transmitter.");
    println!("Ensure an antenna or dummy load is connected.");
    if !confirm("Continue? [y/N] ") {
        println!("Aborted.");
        return Ok(());
    }
    rig.set_ptt(true).await?;
    println!("PTT: ON");
    Ok(())
}

async fn cmd_ptt_off(rig: &dyn Rig) -> Result<()> {
    rig.set_ptt(false).await?;
    println!("PTT: OFF");
    Ok(())
}

async fn cmd_meter(rig: &dyn Rig, meter_type: &MeterType, rx: u8) -> Result<()> {
    match meter_type {
        MeterType::S => {
            let rid = receiver_id(rx);
            let dbm = rig.get_s_meter(rid).await?;
            // Convert dBm to approximate S-units for display.
            let s_unit = if dbm <= -127.0 {
                "S0".to_string()
            } else if dbm <= -73.0 {
                let s = ((dbm + 127.0) / 6.0).round() as i32;
                format!("S{s}")
            } else {
                let over = (dbm + 73.0).round() as i32;
                format!("S9+{over} dB")
            };
            println!("{rid} S-meter: {dbm:.1} dBm ({s_unit})");
        }
        MeterType::Swr => {
            let swr = rig.get_swr().await?;
            println!("SWR: {swr:.2}:1");
        }
        MeterType::Alc => {
            let alc = rig.get_alc().await?;
            println!("ALC: {:.0}%", alc * 100.0);
        }
        MeterType::Power => {
            let watts = rig.get_power().await?;
            println!("Power: {watts:.1} W");
        }
    }
    Ok(())
}

async fn cmd_split_get(rig: &dyn Rig) -> Result<()> {
    let on = rig.get_split().await?;
    if on {
        println!("Split: ON");
    } else {
        println!("Split: OFF");
    }
    Ok(())
}

async fn cmd_split_set(rig: &dyn Rig, on: bool) -> Result<()> {
    rig.set_split(on).await?;
    if on {
        println!("Split: ON");
    } else {
        println!("Split: OFF");
    }
    Ok(())
}

async fn cmd_monitor(rig: &dyn Rig, duration_secs: u64) -> Result<()> {
    let mut event_rx = rig.subscribe()?;

    println!("Monitoring rig events (Ctrl-C to stop)...");

    let deadline = if duration_secs > 0 {
        Some(Instant::now() + Duration::from_secs(duration_secs))
    } else {
        None
    };

    loop {
        let timeout = match deadline {
            Some(dl) => {
                let remaining = dl.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    println!("Monitor duration elapsed.");
                    break;
                }
                remaining
            }
            None => Duration::from_secs(3600),
        };

        match tokio::time::timeout(timeout, event_rx.recv()).await {
            Ok(Ok(event)) => {
                println!("[event] {event:?}");
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                println!("[warning] missed {n} events (consumer too slow)");
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                println!("Event channel closed.");
                break;
            }
            Err(_) => {
                // Timeout expired (either deadline or the 1-hour fallback).
                if deadline.is_some() {
                    println!("Monitor duration elapsed.");
                }
                break;
            }
        }
    }

    Ok(())
}

async fn cmd_stress(rig: &dyn Rig, count: u32, rx: u8) -> Result<()> {
    let rid = receiver_id(rx);

    // Read the current frequency as our baseline.
    let base_freq = rig.get_frequency(rid).await?;
    println!("Stress test: {count} cycles on {rid}");
    println!("Base frequency: {}", format_freq(base_freq));

    let mut rng = rand::thread_rng();
    let mut success = 0u32;
    let mut failures = 0u32;
    let start = Instant::now();

    for i in 1..=count {
        // Generate a random offset within +/- 500 kHz of the base.
        let offset: i64 = rng.gen_range(-500_000..=500_000);
        let target = (base_freq as i64 + offset).max(0) as u64;

        // Set the frequency.
        if let Err(e) = rig.set_frequency(rid, target).await {
            eprintln!("[{i}/{count}] set_frequency failed: {e}");
            failures += 1;
            continue;
        }

        // Read it back and verify.
        match rig.get_frequency(rid).await {
            Ok(readback) => {
                if readback == target {
                    success += 1;
                } else {
                    eprintln!(
                        "[{i}/{count}] mismatch: set {} but read back {}",
                        format_freq(target),
                        format_freq(readback)
                    );
                    failures += 1;
                }
            }
            Err(e) => {
                eprintln!("[{i}/{count}] get_frequency failed: {e}");
                failures += 1;
            }
        }
    }

    let elapsed = start.elapsed();
    let rate = if elapsed.as_secs_f64() > 0.0 {
        count as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    println!();
    println!("Results:");
    println!("  Total cycles:   {count}");
    println!("  Successes:      {success}");
    println!("  Failures:       {failures}");
    println!("  Elapsed:        {:.3} s", elapsed.as_secs_f64());
    println!("  Rate:           {rate:.1} cycles/sec");

    // Restore original frequency.
    if let Err(e) = rig.set_frequency(rid, base_freq).await {
        eprintln!("Warning: failed to restore base frequency: {e}");
    } else {
        println!("  Restored:       {}", format_freq(base_freq));
    }

    if failures > 0 {
        bail!("{failures} out of {count} stress test cycles failed");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// FlexRadio-specific command handlers
// ---------------------------------------------------------------------------

/// Discover FlexRadio radios on the LAN (3-second timeout).
async fn cmd_discover() -> Result<()> {
    println!("Discovering FlexRadio radios on the LAN (3 seconds)...");
    println!();

    let radios = FlexRadio::discover(Duration::from_secs(3))
        .await
        .context("FlexRadio discovery failed")?;

    if radios.is_empty() {
        println!("No FlexRadio radios found.");
        return Ok(());
    }

    // Print a table of discovered radios.
    println!(
        "{:<14}  {:<10}  {:<16}  {:<16}  Firmware",
        "Model", "Serial", "Nickname", "IP Address"
    );
    println!(
        "{:<14}  {:<10}  {:<16}  {:<16}  {}",
        "-".repeat(14),
        "-".repeat(10),
        "-".repeat(16),
        "-".repeat(16),
        "-".repeat(12),
    );

    for radio in &radios {
        println!(
            "{:<14}  {:<10}  {:<16}  {:<16}  {}",
            radio.model, radio.serial, radio.nickname, radio.ip, radio.firmware_version
        );
    }

    println!();
    println!("{} radio(s) found.", radios.len());

    Ok(())
}

/// List all active FlexRadio slices.
async fn cmd_slices(rig: &FlexRadio) -> Result<()> {
    let receivers = rig.receivers().await?;

    if receivers.is_empty() {
        println!("No active slices.");
        return Ok(());
    }

    // Determine which slice is the TX slice.
    let primary = rig.primary_receiver().await?;

    println!("{:<7}  {:>16}  {:<8}  TX", "Slice", "Frequency", "Mode");
    println!(
        "{:<7}  {:>16}  {:<8}  {}",
        "-".repeat(7),
        "-".repeat(16),
        "-".repeat(8),
        "-".repeat(3),
    );

    for rx in &receivers {
        let freq = rig.get_frequency(*rx).await.unwrap_or(0);
        let mode_str = match rig.get_mode(*rx).await {
            Ok(m) => m.to_string(),
            Err(_) => "---".to_string(),
        };
        let tx_marker = if *rx == primary { "TX" } else { "" };
        println!(
            "  {:>3}    {:>16}  {:<8}  {}",
            rx.index(),
            format_freq(freq),
            mode_str,
            tx_marker,
        );
    }

    Ok(())
}

/// Create a new FlexRadio slice at the given frequency and mode.
async fn cmd_create_slice(rig: &FlexRadio, freq_hz: u64, mode_str: &str) -> Result<()> {
    let mode: Mode = mode_str
        .parse()
        .map_err(|e: riglib::ParseModeError| anyhow::anyhow!("{e}"))?;

    let rx = rig
        .create_slice(freq_hz, mode)
        .await
        .context("failed to create slice")?;

    println!(
        "Created slice {} at {} ({})",
        rx.index(),
        format_freq(freq_hz),
        mode
    );
    Ok(())
}

/// Destroy a FlexRadio slice by index.
async fn cmd_destroy_slice(rig: &FlexRadio, slice_index: u8) -> Result<()> {
    let rx = ReceiverId::from_index(slice_index);
    rig.destroy_slice(rx)
        .await
        .context("failed to destroy slice")?;

    println!("Destroyed slice {}", slice_index);
    Ok(())
}

// ---------------------------------------------------------------------------
// Audio command handlers
// ---------------------------------------------------------------------------

/// List available audio input/output devices.
fn cmd_audio_list_devices() -> Result<()> {
    let devices = list_audio_devices()
        .map_err(|e| anyhow::anyhow!("failed to enumerate audio devices: {e}"))?;

    if devices.is_empty() {
        println!("No audio devices found.");
        return Ok(());
    }

    println!("{:<40}  {:>5}  {:>6}", "Device Name", "Input", "Output");
    println!("{:<40}  {:>5}  {:>6}", "-".repeat(40), "-----", "------",);

    for dev in &devices {
        println!(
            "{:<40}  {:>5}  {:>6}",
            dev.name,
            if dev.is_input { "yes" } else { "no" },
            if dev.is_output { "yes" } else { "no" },
        );
    }

    println!();
    println!("{} device(s) found.", devices.len());

    Ok(())
}

/// Record RX audio from the rig to a WAV file.
async fn cmd_audio_rx(
    rig: &dyn AudioCapable,
    rx: u8,
    duration_secs: u64,
    output_path: &str,
) -> Result<()> {
    if !rig.audio_supported() {
        bail!(
            "Audio not supported for this rig. For serial rigs, pass --device <name> \
             to specify the USB audio device."
        );
    }

    let rid = receiver_id(rx);
    let config = rig.native_audio_config();

    println!(
        "Starting RX audio: receiver={}, sample_rate={} Hz, channels={}, format={:?}",
        rid, config.sample_rate, config.channels, config.sample_format
    );

    let mut audio_rx = rig
        .start_rx_audio(rid, None)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start RX audio: {e}"))?;

    let stream_config = audio_rx.config().clone();

    // Set up WAV writer with 16-bit integer output.
    let wav_spec = hound::WavSpec {
        channels: stream_config.channels,
        sample_rate: stream_config.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output_path, wav_spec)
        .with_context(|| format!("failed to create WAV file: {output_path}"))?;

    println!("Recording {} seconds to {} ...", duration_secs, output_path);

    let deadline = Instant::now() + Duration::from_secs(duration_secs);
    let mut total_frames: u64 = 0;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, audio_rx.recv()).await {
            Ok(Some(buffer)) => {
                let frames = buffer.frame_count() as u64;
                for &sample in &buffer.samples {
                    // Convert f32 [-1.0, 1.0] to i16 with clamping.
                    let clamped = sample.clamp(-1.0, 1.0);
                    let i16_sample = (clamped * 32767.0) as i16;
                    writer
                        .write_sample(i16_sample)
                        .context("failed to write WAV sample")?;
                }
                total_frames += frames;
            }
            Ok(None) => {
                println!("Audio stream closed.");
                break;
            }
            Err(_) => {
                // Timeout -- recording duration elapsed.
                break;
            }
        }
    }

    writer.finalize().context("failed to finalize WAV file")?;

    let duration_actual = total_frames as f64 / stream_config.sample_rate as f64;
    println!(
        "Wrote {} frames ({:.2} s) to {}",
        total_frames, duration_actual, output_path
    );

    rig.stop_audio()
        .await
        .map_err(|e| anyhow::anyhow!("failed to stop audio: {e}"))?;

    Ok(())
}

/// Monitor RX audio levels with a text-based meter.
async fn cmd_audio_monitor(rig: &dyn AudioCapable, rx: u8, duration_secs: u64) -> Result<()> {
    if !rig.audio_supported() {
        bail!(
            "Audio not supported for this rig. For serial rigs, pass --device <name> \
             to specify the USB audio device."
        );
    }

    let rid = receiver_id(rx);
    let config = rig.native_audio_config();

    println!(
        "Starting audio monitor: receiver={}, sample_rate={} Hz, channels={}",
        rid, config.sample_rate, config.channels
    );
    println!("Press Ctrl-C to stop.");
    println!();

    let mut audio_rx = rig
        .start_rx_audio(rid, None)
        .await
        .map_err(|e| anyhow::anyhow!("failed to start RX audio: {e}"))?;

    let stream_config = audio_rx.config().clone();
    let channels = stream_config.channels as usize;

    let deadline = if duration_secs > 0 {
        Some(Instant::now() + Duration::from_secs(duration_secs))
    } else {
        None
    };

    // Accumulate samples between display updates (~100ms intervals).
    let mut last_display = Instant::now();
    let mut peak_per_channel = vec![0.0_f32; channels];
    let mut rms_accum_per_channel = vec![0.0_f64; channels];
    let mut sample_count_per_channel = vec![0_u64; channels];

    loop {
        let timeout = match deadline {
            Some(dl) => {
                let remaining = dl.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    println!("\nMonitor duration elapsed.");
                    break;
                }
                remaining.min(Duration::from_millis(100))
            }
            None => Duration::from_millis(100),
        };

        match tokio::time::timeout(timeout, audio_rx.recv()).await {
            Ok(Some(buffer)) => {
                // Accumulate per-channel statistics.
                for (i, &sample) in buffer.samples.iter().enumerate() {
                    let ch = i % channels;
                    let abs = sample.abs();
                    if abs > peak_per_channel[ch] {
                        peak_per_channel[ch] = abs;
                    }
                    rms_accum_per_channel[ch] += (sample as f64) * (sample as f64);
                    sample_count_per_channel[ch] += 1;
                }

                // Display at ~100ms intervals.
                if last_display.elapsed() >= Duration::from_millis(100) {
                    display_level_meter(
                        &peak_per_channel,
                        &rms_accum_per_channel,
                        &sample_count_per_channel,
                        channels,
                    );

                    // Reset accumulators.
                    peak_per_channel.fill(0.0);
                    rms_accum_per_channel.fill(0.0);
                    sample_count_per_channel.fill(0);
                    last_display = Instant::now();
                }
            }
            Ok(None) => {
                println!("\nAudio stream closed.");
                break;
            }
            Err(_) => {
                // Timeout -- check if we have accumulated data to display.
                if sample_count_per_channel.iter().any(|&c| c > 0) {
                    display_level_meter(
                        &peak_per_channel,
                        &rms_accum_per_channel,
                        &sample_count_per_channel,
                        channels,
                    );
                    peak_per_channel.fill(0.0);
                    rms_accum_per_channel.fill(0.0);
                    sample_count_per_channel.fill(0);
                    last_display = Instant::now();
                }
            }
        }
    }

    rig.stop_audio()
        .await
        .map_err(|e| anyhow::anyhow!("failed to stop audio: {e}"))?;

    Ok(())
}

/// Display a text-based level meter for each channel.
fn display_level_meter(peak: &[f32], rms_accum: &[f64], sample_count: &[u64], channels: usize) {
    let channel_labels = if channels == 2 {
        vec!["L", "R"]
    } else {
        (0..channels)
            .map(|i| {
                // Leak a string for the label -- this is fine for a CLI tool
                // with a bounded number of channels.
                Box::leak(format!("{i}").into_boxed_str()) as &str
            })
            .collect()
    };

    let meter_width = 40;
    let mut line = String::new();

    for ch in 0..channels {
        let peak_val = peak[ch];
        let rms_val = if sample_count[ch] > 0 {
            (rms_accum[ch] / sample_count[ch] as f64).sqrt() as f32
        } else {
            0.0
        };

        // Convert to dBFS for display.
        let peak_dbfs = if peak_val > 0.0 {
            20.0 * peak_val.log10()
        } else {
            -96.0
        };
        let rms_dbfs = if rms_val > 0.0 {
            20.0 * rms_val.log10()
        } else {
            -96.0
        };

        // Map peak to meter bar (0 dBFS = full, -60 dBFS = empty).
        let bar_len = ((peak_dbfs + 60.0) / 60.0 * meter_width as f32)
            .clamp(0.0, meter_width as f32) as usize;
        let bar: String = "#".repeat(bar_len) + &" ".repeat(meter_width - bar_len);

        if !line.is_empty() {
            line.push_str("  ");
        }
        line.push_str(&format!(
            "{}: [{}] pk:{:>6.1} rms:{:>6.1} dBFS",
            channel_labels[ch], bar, peak_dbfs, rms_dbfs
        ));
    }

    // Use carriage return to overwrite the line in-place.
    print!("\r{line}");
    io::stdout().flush().ok();
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Validate option combinations early.
    validate_options(&cli)?;
    validate_command_for_manufacturer(&cli)?;

    // The `list` command does not require a rig connection.
    if let Command::List { manufacturer } = &cli.command {
        return cmd_list(manufacturer.as_deref());
    }

    // The `discover` command does not require a rig connection.
    if matches!(&cli.command, Command::Discover) {
        return cmd_discover().await;
    }

    // The `audio list-devices` command does not require a rig connection.
    if matches!(
        &cli.command,
        Command::Audio {
            action: AudioAction::ListDevices
        }
    ) {
        return cmd_audio_list_devices();
    }

    // FlexRadio-specific commands need the concrete FlexRadio type for
    // extension methods (slices, create_slice, destroy_slice).
    match &cli.command {
        Command::Slices => {
            let rig = create_flex_radio(&cli).await?;
            let result = cmd_slices(&rig).await;
            rig.disconnect().await.ok();
            return result;
        }
        Command::CreateSlice { freq_hz, mode } => {
            let rig = create_flex_radio(&cli).await?;
            let result = cmd_create_slice(&rig, *freq_hz, mode).await;
            rig.disconnect().await.ok();
            return result;
        }
        Command::DestroySlice { slice_index } => {
            let rig = create_flex_radio(&cli).await?;
            let result = cmd_destroy_slice(&rig, *slice_index).await;
            rig.disconnect().await.ok();
            return result;
        }
        _ => {}
    }

    let rig = create_rig(&cli).await?;

    match &cli.command {
        Command::Info => cmd_info(rig.as_ref()).await,
        Command::Freq { action } => match action {
            FreqAction::Get { rx } => cmd_freq_get(rig.as_ref(), *rx).await,
            FreqAction::Set { freq_hz, rx } => cmd_freq_set(rig.as_ref(), *rx, *freq_hz).await,
        },
        Command::Mode { action } => match action {
            ModeAction::Get { rx } => cmd_mode_get(rig.as_ref(), *rx).await,
            ModeAction::Set { mode, rx } => cmd_mode_set(rig.as_ref(), *rx, mode).await,
        },
        Command::Ptt { action } => match action {
            PttAction::Get => cmd_ptt_get(rig.as_ref()).await,
            PttAction::On => cmd_ptt_on(rig.as_ref()).await,
            PttAction::Off => cmd_ptt_off(rig.as_ref()).await,
        },
        Command::Meter { r#type, rx } => cmd_meter(rig.as_ref(), r#type, *rx).await,
        Command::Split { action } => match action {
            SplitAction::Get => cmd_split_get(rig.as_ref()).await,
            SplitAction::On => cmd_split_set(rig.as_ref(), true).await,
            SplitAction::Off => cmd_split_set(rig.as_ref(), false).await,
        },
        Command::Monitor { duration } => cmd_monitor(rig.as_ref(), *duration).await,
        Command::Stress { count, rx } => cmd_stress(rig.as_ref(), *count, *rx).await,
        Command::Audio { action } => match action {
            AudioAction::ListDevices => unreachable!("audio list-devices handled above"),
            AudioAction::Rx {
                duration,
                output,
                receiver,
            } => cmd_audio_rx(rig.as_ref(), *receiver, *duration, output).await,
            AudioAction::Monitor { duration, receiver } => {
                cmd_audio_monitor(rig.as_ref(), *receiver, *duration).await
            }
        },
        Command::List { .. } => unreachable!("list handled above"),
        Command::Discover => unreachable!("discover handled above"),
        Command::Slices => unreachable!("slices handled above"),
        Command::CreateSlice { .. } => unreachable!("create-slice handled above"),
        Command::DestroySlice { .. } => unreachable!("destroy-slice handled above"),
    }
}
