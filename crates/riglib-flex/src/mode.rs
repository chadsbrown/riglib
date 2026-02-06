//! FlexRadio mode string ↔ riglib `Mode` conversion.
//!
//! FlexRadio uses uppercase ASCII mode strings (`"USB"`, `"CW"`, `"DIGU"`, etc.)
//! in SmartSDR commands and status messages. This module maps between those
//! strings and the manufacturer-agnostic [`Mode`] enum.
//!
//! Some mappings are lossy:
//! - `SAM` (synchronous AM) -> `Mode::AM` (riglib has no synchronous AM)
//! - `NFM` (narrow FM) -> `Mode::FM` (riglib does not distinguish narrow/wide)
//! - `FDV` (FreeDV) -> `Mode::DataUSB` (closest approximation)
//! - `Mode::RTTYR` -> `"RTTY"` (FlexRadio has no RTTY reverse mode)
//! - `Mode::DataAM` -> `"AM"` (FlexRadio has no data-AM mode)

use riglib_core::{Error, Mode, Result};

/// Convert a riglib `Mode` to the FlexRadio mode string.
pub fn mode_to_flex(mode: &Mode) -> &'static str {
    match mode {
        Mode::USB => "USB",
        Mode::LSB => "LSB",
        Mode::CW => "CW",
        Mode::CWR => "CWR",
        Mode::AM => "AM",
        Mode::FM => "FM",
        Mode::RTTY => "RTTY",
        Mode::RTTYR => "RTTY", // lossy: FlexRadio has no RTTY reverse
        Mode::DataUSB => "DIGU",
        Mode::DataLSB => "DIGL",
        Mode::DataFM => "DFM",
        Mode::DataAM => "AM", // lossy: FlexRadio has no data-AM mode
    }
}

/// Convert a FlexRadio mode string to a riglib `Mode`.
///
/// The input is compared case-sensitively (FlexRadio always uses uppercase).
/// If you need case-insensitive matching, uppercase the input first.
pub fn flex_to_mode(flex_mode: &str) -> Result<Mode> {
    match flex_mode {
        "USB" => Ok(Mode::USB),
        "LSB" => Ok(Mode::LSB),
        "CW" => Ok(Mode::CW),
        "CWR" => Ok(Mode::CWR),
        "AM" => Ok(Mode::AM),
        "SAM" => Ok(Mode::AM), // lossy: synchronous AM -> AM
        "FM" => Ok(Mode::FM),
        "NFM" => Ok(Mode::FM), // lossy: narrow FM -> FM
        "DIGU" => Ok(Mode::DataUSB),
        "DIGL" => Ok(Mode::DataLSB),
        "RTTY" => Ok(Mode::RTTY),
        "DFM" => Ok(Mode::DataFM),
        "FDV" => Ok(Mode::DataUSB), // lossy: FreeDV -> DataUSB
        _ => Err(Error::Protocol(format!(
            "unknown FlexRadio mode: {flex_mode}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Round-trip for non-lossy modes ------------------------------------

    #[test]
    fn round_trip_usb() {
        let flex = mode_to_flex(&Mode::USB);
        assert_eq!(flex, "USB");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::USB);
    }

    #[test]
    fn round_trip_lsb() {
        let flex = mode_to_flex(&Mode::LSB);
        assert_eq!(flex, "LSB");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::LSB);
    }

    #[test]
    fn round_trip_cw() {
        let flex = mode_to_flex(&Mode::CW);
        assert_eq!(flex, "CW");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::CW);
    }

    #[test]
    fn round_trip_cwr() {
        let flex = mode_to_flex(&Mode::CWR);
        assert_eq!(flex, "CWR");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::CWR);
    }

    #[test]
    fn round_trip_am() {
        let flex = mode_to_flex(&Mode::AM);
        assert_eq!(flex, "AM");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::AM);
    }

    #[test]
    fn round_trip_fm() {
        let flex = mode_to_flex(&Mode::FM);
        assert_eq!(flex, "FM");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::FM);
    }

    #[test]
    fn round_trip_data_usb() {
        let flex = mode_to_flex(&Mode::DataUSB);
        assert_eq!(flex, "DIGU");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::DataUSB);
    }

    #[test]
    fn round_trip_data_lsb() {
        let flex = mode_to_flex(&Mode::DataLSB);
        assert_eq!(flex, "DIGL");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::DataLSB);
    }

    #[test]
    fn round_trip_rtty() {
        let flex = mode_to_flex(&Mode::RTTY);
        assert_eq!(flex, "RTTY");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::RTTY);
    }

    #[test]
    fn round_trip_data_fm() {
        let flex = mode_to_flex(&Mode::DataFM);
        assert_eq!(flex, "DFM");
        assert_eq!(flex_to_mode(flex).unwrap(), Mode::DataFM);
    }

    // -- Lossy mappings: FlexRadio -> riglib --------------------------------

    #[test]
    fn sam_maps_to_am() {
        assert_eq!(flex_to_mode("SAM").unwrap(), Mode::AM);
    }

    #[test]
    fn nfm_maps_to_fm() {
        assert_eq!(flex_to_mode("NFM").unwrap(), Mode::FM);
    }

    #[test]
    fn fdv_maps_to_data_usb() {
        assert_eq!(flex_to_mode("FDV").unwrap(), Mode::DataUSB);
    }

    // -- Lossy mappings: riglib -> FlexRadio --------------------------------

    #[test]
    fn rttyr_maps_to_rtty() {
        assert_eq!(mode_to_flex(&Mode::RTTYR), "RTTY");
    }

    #[test]
    fn data_am_maps_to_am() {
        assert_eq!(mode_to_flex(&Mode::DataAM), "AM");
    }

    // -- Error cases --------------------------------------------------------

    #[test]
    fn unknown_mode_returns_error() {
        assert!(flex_to_mode("GARBAGE").is_err());
    }

    #[test]
    fn empty_mode_returns_error() {
        assert!(flex_to_mode("").is_err());
    }

    #[test]
    fn lowercase_mode_returns_error() {
        // FlexRadio always uses uppercase; lowercase should not match.
        assert!(flex_to_mode("usb").is_err());
        assert!(flex_to_mode("cw").is_err());
    }
}
