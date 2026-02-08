//! Core types used throughout riglib.
//!
//! These types provide a manufacturer-agnostic abstraction layer over the
//! various rig control protocols (CI-V, CAT, SmartSDR, etc.).

use std::fmt;
use std::str::FromStr;

/// Opaque receiver identifier.
///
/// Maps to VFO A/B on traditional rigs (Icom, Kenwood, Yaesu, Elecraft)
/// and to slice IDs on FlexRadio SmartSDR rigs. Using an opaque type
/// allows the same API to work across all architectures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReceiverId(u8);

impl ReceiverId {
    /// VFO A on traditional rigs, or slice 0 on FlexRadio.
    pub const VFO_A: ReceiverId = ReceiverId(0);

    /// VFO B on traditional rigs, or slice 1 on FlexRadio.
    pub const VFO_B: ReceiverId = ReceiverId(1);

    /// Create a `ReceiverId` from a raw index.
    ///
    /// Use this for FlexRadio slices or other rigs where receivers are
    /// identified by a numeric index beyond the traditional VFO A/B pair.
    pub fn from_index(index: u8) -> Self {
        ReceiverId(index)
    }

    /// Return the raw numeric index of this receiver.
    pub fn index(&self) -> u8 {
        self.0
    }
}

impl fmt::Display for ReceiverId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            0 => write!(f, "VFO-A"),
            1 => write!(f, "VFO-B"),
            n => write!(f, "Slice-{n}"),
        }
    }
}

/// Operating mode of the transceiver.
///
/// Covers standard analog modes plus data sub-modes used by digital
/// contesting software (WSJT-X, fldigi, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Upper sideband voice.
    USB,
    /// Lower sideband voice.
    LSB,
    /// CW (morse), typically with upper sideband offset.
    CW,
    /// CW reverse (lower sideband offset).
    CWR,
    /// Amplitude modulation.
    AM,
    /// Frequency modulation.
    FM,
    /// Radio teletype (FSK), upper sideband.
    RTTY,
    /// Radio teletype (FSK), reverse / lower sideband.
    RTTYR,
    /// Data mode using upper sideband (AFSK, sound-card digital).
    DataUSB,
    /// Data mode using lower sideband.
    DataLSB,
    /// Data mode using FM.
    DataFM,
    /// Data mode using AM.
    DataAM,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Mode::USB => "USB",
            Mode::LSB => "LSB",
            Mode::CW => "CW",
            Mode::CWR => "CWR",
            Mode::AM => "AM",
            Mode::FM => "FM",
            Mode::RTTY => "RTTY",
            Mode::RTTYR => "RTTYR",
            Mode::DataUSB => "DATA-USB",
            Mode::DataLSB => "DATA-LSB",
            Mode::DataFM => "DATA-FM",
            Mode::DataAM => "DATA-AM",
        };
        write!(f, "{s}")
    }
}

/// Error returned when a string cannot be parsed into a [`Mode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseModeError(String);

impl fmt::Display for ParseModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown mode: {}", self.0)
    }
}

impl std::error::Error for ParseModeError {}

impl FromStr for Mode {
    type Err = ParseModeError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "USB" => Ok(Mode::USB),
            "LSB" => Ok(Mode::LSB),
            "CW" => Ok(Mode::CW),
            "CWR" => Ok(Mode::CWR),
            "AM" => Ok(Mode::AM),
            "FM" => Ok(Mode::FM),
            "RTTY" => Ok(Mode::RTTY),
            "RTTYR" => Ok(Mode::RTTYR),
            "DATA-USB" | "DATAUSB" => Ok(Mode::DataUSB),
            "DATA-LSB" | "DATALSB" => Ok(Mode::DataLSB),
            "DATA-FM" | "DATAFM" => Ok(Mode::DataFM),
            "DATA-AM" | "DATAAM" => Ok(Mode::DataAM),
            _ => Err(ParseModeError(s.to_string())),
        }
    }
}

/// Receiver passband (filter width) in hertz.
///
/// Common values: 500 Hz for CW, 2700 Hz for SSB, 6000 Hz for AM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Passband(u32);

impl Passband {
    /// Create a new passband width from a value in hertz.
    pub fn from_hz(hz: u32) -> Self {
        Passband(hz)
    }

    /// Return the passband width in hertz.
    pub fn hz(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for Passband {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Hz", self.0)
    }
}

/// Transceiver manufacturer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Manufacturer {
    /// Icom (CI-V protocol).
    Icom,
    /// Yaesu (CAT protocol).
    Yaesu,
    /// Elecraft (extended Kenwood command set).
    Elecraft,
    /// Kenwood (standard CAT commands).
    Kenwood,
    /// FlexRadio (SmartSDR TCP/IP API).
    FlexRadio,
}

impl Manufacturer {
    /// Returns the default connection type for this manufacturer.
    ///
    /// FlexRadio rigs are network-only (SmartSDR TCP/IP). All other
    /// manufacturers use serial (USB virtual COM port or RS-232).
    pub fn default_connection(&self) -> ConnectionType {
        match self {
            Manufacturer::FlexRadio => ConnectionType::Network,
            _ => ConnectionType::Serial,
        }
    }
}

impl fmt::Display for Manufacturer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Manufacturer::Icom => "Icom",
            Manufacturer::Yaesu => "Yaesu",
            Manufacturer::Elecraft => "Elecraft",
            Manufacturer::Kenwood => "Kenwood",
            Manufacturer::FlexRadio => "FlexRadio",
        };
        write!(f, "{s}")
    }
}

/// Error returned when a string cannot be parsed into a [`Manufacturer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseManufacturerError(String);

impl fmt::Display for ParseManufacturerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown manufacturer: '{}'. Expected: icom, yaesu, kenwood, elecraft, flex",
            self.0
        )
    }
}

impl std::error::Error for ParseManufacturerError {}

impl FromStr for Manufacturer {
    type Err = ParseManufacturerError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "icom" => Ok(Manufacturer::Icom),
            "yaesu" => Ok(Manufacturer::Yaesu),
            "kenwood" => Ok(Manufacturer::Kenwood),
            "elecraft" => Ok(Manufacturer::Elecraft),
            "flex" | "flexradio" => Ok(Manufacturer::FlexRadio),
            _ => Err(ParseManufacturerError(s.to_string())),
        }
    }
}

/// How the rig connects to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionType {
    /// Serial port (USB virtual COM or RS-232).
    Serial,
    /// Network (TCP/IP, UDP, or both).
    Network,
}

impl fmt::Display for ConnectionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionType::Serial => write!(f, "Serial"),
            ConnectionType::Network => write!(f, "Network"),
        }
    }
}

/// A supported rig model with enough information for a UI picker.
///
/// This struct provides a manufacturer-agnostic view of a rig model,
/// suitable for populating dropdown lists, table views, and other UI
/// elements where the application needs to enumerate all supported rigs
/// without pulling in manufacturer-specific types.
///
/// Obtained via [`riglib::supported_rigs()`] (facade crate) or by
/// converting a manufacturer-specific model type (e.g. `IcomModel`)
/// via its `From` implementation.
#[derive(Debug, Clone)]
pub struct RigDefinition {
    /// The manufacturer of the rig.
    pub manufacturer: Manufacturer,
    /// Human-readable model name (e.g. "IC-7610", "K4", "FLEX-6600").
    pub model_name: &'static str,
    /// How the rig connects to the host (serial or network).
    pub connection: ConnectionType,
    /// Default serial baud rate, if applicable (None for network-only rigs).
    pub default_baud_rate: Option<u32>,
    /// Full capability description for this model.
    pub capabilities: RigCapabilities,
}

/// Static information about a connected rig.
///
/// Returned by [`crate::rig::Rig::info()`] to identify the specific
/// radio model in use.
#[derive(Debug, Clone)]
pub struct RigInfo {
    /// The manufacturer of the rig.
    pub manufacturer: Manufacturer,
    /// Human-readable model name (e.g. "IC-7610", "K4", "FLEX-6600").
    pub model_name: String,
    /// Machine-readable model identifier.
    ///
    /// For Icom this is the CI-V default address in hex (e.g. "0x98" for IC-7610).
    /// For other manufacturers this is a protocol-specific identifier.
    pub model_id: String,
}

/// A contiguous frequency range, typically corresponding to a band.
///
/// Used in [`RigCapabilities`] to describe the frequency coverage of a rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BandRange {
    /// Lower bound of the range in hertz (inclusive).
    pub low_hz: u64,
    /// Upper bound of the range in hertz (inclusive).
    pub high_hz: u64,
}

impl BandRange {
    /// Create a new band range.
    pub fn new(low_hz: u64, high_hz: u64) -> Self {
        BandRange { low_hz, high_hz }
    }

    /// Check whether a frequency (in hertz) falls within this range (inclusive).
    pub fn contains(&self, freq_hz: u64) -> bool {
        freq_hz >= self.low_hz && freq_hz <= self.high_hz
    }
}

impl fmt::Display for BandRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{} Hz", self.low_hz, self.high_hz)
    }
}

/// AGC (Automatic Gain Control) mode setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgcMode {
    /// AGC disabled.
    Off,
    /// Fast AGC — quick attack and release, typical for CW contesting.
    Fast,
    /// Medium AGC — balanced for SSB voice.
    Medium,
    /// Slow AGC — long time constant, useful for AM broadcast monitoring.
    Slow,
}

impl fmt::Display for AgcMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AgcMode::Off => "OFF",
            AgcMode::Fast => "FAST",
            AgcMode::Medium => "MED",
            AgcMode::Slow => "SLOW",
        };
        write!(f, "{s}")
    }
}

impl FromStr for AgcMode {
    type Err = ParseModeError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "OFF" => Ok(AgcMode::Off),
            "FAST" => Ok(AgcMode::Fast),
            "MED" | "MEDIUM" => Ok(AgcMode::Medium),
            "SLOW" => Ok(AgcMode::Slow),
            _ => Err(ParseModeError(s.to_string())),
        }
    }
}

/// Preamplifier level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreampLevel {
    /// Preamp off.
    Off,
    /// Preamp stage 1 (typically +10 dB).
    Preamp1,
    /// Preamp stage 2 (typically +20 dB).
    Preamp2,
}

impl fmt::Display for PreampLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PreampLevel::Off => "OFF",
            PreampLevel::Preamp1 => "P1",
            PreampLevel::Preamp2 => "P2",
        };
        write!(f, "{s}")
    }
}

impl FromStr for PreampLevel {
    type Err = ParseModeError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "OFF" | "0" => Ok(PreampLevel::Off),
            "1" | "P1" | "PREAMP1" => Ok(PreampLevel::Preamp1),
            "2" | "P2" | "PREAMP2" => Ok(PreampLevel::Preamp2),
            _ => Err(ParseModeError(s.to_string())),
        }
    }
}

/// Antenna port selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AntennaPort {
    /// Antenna port 1 (ANT1).
    Ant1,
    /// Antenna port 2 (ANT2).
    Ant2,
    /// Antenna port 3 (ANT3).
    Ant3,
    /// Antenna port 4 (ANT4).
    Ant4,
}

impl fmt::Display for AntennaPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AntennaPort::Ant1 => "ANT1",
            AntennaPort::Ant2 => "ANT2",
            AntennaPort::Ant3 => "ANT3",
            AntennaPort::Ant4 => "ANT4",
        };
        write!(f, "{s}")
    }
}

impl FromStr for AntennaPort {
    type Err = ParseModeError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "1" | "ANT1" => Ok(AntennaPort::Ant1),
            "2" | "ANT2" => Ok(AntennaPort::Ant2),
            "3" | "ANT3" => Ok(AntennaPort::Ant3),
            "4" | "ANT4" => Ok(AntennaPort::Ant4),
            _ => Err(ParseModeError(s.to_string())),
        }
    }
}

/// How PTT (push-to-talk) is activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PttMethod {
    /// PTT via CAT command (default, supported by all backends).
    #[default]
    Cat,
    /// PTT via the DTR serial line.
    Dtr,
    /// PTT via the RTS serial line.
    Rts,
}

/// Which serial line is used for CW keying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyLine {
    /// No hardware CW keying (default).
    #[default]
    None,
    /// CW key via the DTR serial line.
    Dtr,
    /// CW key via the RTS serial line.
    Rts,
}

/// Capabilities and limits of a specific rig model.
///
/// Obtained via [`crate::rig::Rig::capabilities()`]. Drivers populate this
/// struct at connection time so callers can adapt their behavior to the
/// specific radio (e.g. showing SO2R controls only when `max_receivers >= 2`).
#[derive(Debug, Clone)]
pub struct RigCapabilities {
    /// Maximum number of independent receivers (1 for most rigs, 2 for
    /// IC-7610, IC-R8600, etc., up to 8 for FlexRadio).
    pub max_receivers: u8,
    /// Whether the rig has a dedicated sub-receiver (e.g. IC-7610 dual watch).
    pub has_sub_receiver: bool,
    /// Whether the rig supports split-frequency TX/RX operation.
    pub has_split: bool,
    /// Whether the rig can stream decoded audio over USB or network.
    pub has_audio_streaming: bool,
    /// Whether the rig can output raw I/Q data (e.g. IC-7610 USB I/Q,
    /// FlexRadio VITA-49 streams).
    pub has_iq_output: bool,
    /// The set of operating modes the rig supports.
    pub supported_modes: Vec<Mode>,
    /// Frequency ranges the rig can tune to.
    pub frequency_ranges: Vec<BandRange>,
    /// Maximum transmit power in watts.
    pub max_power_watts: f32,
    /// AGC modes supported by this rig.
    pub agc_modes: Vec<AgcMode>,
    /// Preamp levels supported by this rig.
    pub preamp_levels: Vec<PreampLevel>,
    /// Attenuator levels supported by this rig (each value is dB of attenuation).
    ///
    /// `0` means attenuator off. Other values are the dB settings the rig supports.
    /// For example `vec![0, 6, 12, 18]` means the rig offers off, 6 dB, 12 dB,
    /// and 18 dB attenuation steps.
    pub attenuator_levels: Vec<u8>,
    /// Antenna ports available on this rig.
    pub antenna_ports: Vec<AntennaPort>,
    /// Whether the rig supports RIT (Receiver Incremental Tuning).
    pub has_rit: bool,
    /// Whether the rig supports XIT (Transmitter Incremental Tuning).
    pub has_xit: bool,
    /// Whether the rig has a built-in CW keyer with adjustable speed.
    pub has_cw_keyer: bool,
    /// Whether the rig can send CW messages from memory or text input.
    pub has_cw_messages: bool,
    /// Whether the rig supports VFO A=B (copy active VFO to inactive).
    pub has_vfo_ab_swap: bool,
    /// Whether the rig supports VFO swap (exchange A and B).
    pub has_vfo_ab_equal: bool,
    /// Whether the rig supports transceive (AI) mode for unsolicited state updates.
    pub has_transceive: bool,
}

impl Default for RigCapabilities {
    fn default() -> Self {
        RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: false,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: Vec::new(),
            frequency_ranges: Vec::new(),
            max_power_watts: 0.0,
            agc_modes: Vec::new(),
            preamp_levels: Vec::new(),
            attenuator_levels: Vec::new(),
            antenna_ports: Vec::new(),
            has_rit: false,
            has_xit: false,
            has_cw_keyer: false,
            has_cw_messages: false,
            has_vfo_ab_swap: false,
            has_vfo_ab_equal: false,
            has_transceive: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_id_constants() {
        assert_eq!(ReceiverId::VFO_A.index(), 0);
        assert_eq!(ReceiverId::VFO_B.index(), 1);
        assert_ne!(ReceiverId::VFO_A, ReceiverId::VFO_B);
    }

    #[test]
    fn receiver_id_from_index() {
        let slice3 = ReceiverId::from_index(3);
        assert_eq!(slice3.index(), 3);
        assert_ne!(slice3, ReceiverId::VFO_A);
    }

    #[test]
    fn receiver_id_display() {
        assert_eq!(ReceiverId::VFO_A.to_string(), "VFO-A");
        assert_eq!(ReceiverId::VFO_B.to_string(), "VFO-B");
        assert_eq!(ReceiverId::from_index(4).to_string(), "Slice-4");
    }

    #[test]
    fn mode_display_round_trip() {
        let modes = [
            Mode::USB,
            Mode::LSB,
            Mode::CW,
            Mode::CWR,
            Mode::AM,
            Mode::FM,
            Mode::RTTY,
            Mode::RTTYR,
            Mode::DataUSB,
            Mode::DataLSB,
            Mode::DataFM,
            Mode::DataAM,
        ];
        for mode in &modes {
            let s = mode.to_string();
            let parsed: Mode = s.parse().expect("should parse back");
            assert_eq!(*mode, parsed, "round-trip failed for {mode}");
        }
    }

    #[test]
    fn mode_from_str_case_insensitive() {
        assert_eq!("usb".parse::<Mode>().unwrap(), Mode::USB);
        assert_eq!("Cw".parse::<Mode>().unwrap(), Mode::CW);
        assert_eq!("data-usb".parse::<Mode>().unwrap(), Mode::DataUSB);
        assert_eq!("DATAUSB".parse::<Mode>().unwrap(), Mode::DataUSB);
    }

    #[test]
    fn mode_from_str_invalid() {
        let result = "UNKNOWN".parse::<Mode>();
        assert!(result.is_err());
    }

    #[test]
    fn passband_construction() {
        let pb = Passband::from_hz(2700);
        assert_eq!(pb.hz(), 2700);
        assert_eq!(pb.to_string(), "2700 Hz");
    }

    #[test]
    fn passband_ordering() {
        let narrow = Passband::from_hz(500);
        let wide = Passband::from_hz(2700);
        assert!(narrow < wide);
    }

    #[test]
    fn band_range_contains() {
        let twenty_meters = BandRange::new(14_000_000, 14_350_000);
        assert!(twenty_meters.contains(14_000_000));
        assert!(twenty_meters.contains(14_074_000));
        assert!(twenty_meters.contains(14_350_000));
        assert!(!twenty_meters.contains(13_999_999));
        assert!(!twenty_meters.contains(14_350_001));
    }

    #[test]
    fn band_range_display() {
        let range = BandRange::new(7_000_000, 7_300_000);
        assert_eq!(range.to_string(), "7000000-7300000 Hz");
    }

    #[test]
    fn manufacturer_display() {
        assert_eq!(Manufacturer::Icom.to_string(), "Icom");
        assert_eq!(Manufacturer::FlexRadio.to_string(), "FlexRadio");
    }

    #[test]
    fn manufacturer_from_str() {
        assert_eq!("icom".parse::<Manufacturer>().unwrap(), Manufacturer::Icom);
        assert_eq!(
            "YAESU".parse::<Manufacturer>().unwrap(),
            Manufacturer::Yaesu
        );
        assert_eq!(
            "Kenwood".parse::<Manufacturer>().unwrap(),
            Manufacturer::Kenwood
        );
        assert_eq!(
            "elecraft".parse::<Manufacturer>().unwrap(),
            Manufacturer::Elecraft
        );
        assert_eq!(
            "flex".parse::<Manufacturer>().unwrap(),
            Manufacturer::FlexRadio
        );
        assert_eq!(
            "FlexRadio".parse::<Manufacturer>().unwrap(),
            Manufacturer::FlexRadio
        );
    }

    #[test]
    fn manufacturer_from_str_invalid() {
        assert!("unknown".parse::<Manufacturer>().is_err());
    }

    #[test]
    fn manufacturer_default_connection() {
        assert_eq!(
            Manufacturer::Icom.default_connection(),
            ConnectionType::Serial
        );
        assert_eq!(
            Manufacturer::Yaesu.default_connection(),
            ConnectionType::Serial
        );
        assert_eq!(
            Manufacturer::Kenwood.default_connection(),
            ConnectionType::Serial
        );
        assert_eq!(
            Manufacturer::Elecraft.default_connection(),
            ConnectionType::Serial
        );
        assert_eq!(
            Manufacturer::FlexRadio.default_connection(),
            ConnectionType::Network
        );
    }
}
