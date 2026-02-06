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
}
