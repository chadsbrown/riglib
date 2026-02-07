//! Formatting and conversion helpers for amateur radio applications.
//!
//! These are small utility functions that virtually every consuming
//! application (contest loggers, panadapter UIs, CLI tools) needs.

/// Format a frequency in hertz as a human-readable MHz string.
///
/// Returns a string like `"14.074000 MHz"` with six decimal places,
/// which is the standard display precision for amateur radio frequencies.
///
/// # Example
///
/// ```
/// use riglib_core::format_freq_mhz;
///
/// assert_eq!(format_freq_mhz(14_074_000), "14.074000 MHz");
/// assert_eq!(format_freq_mhz(432_100_000), "432.100000 MHz");
/// ```
pub fn format_freq_mhz(freq_hz: u64) -> String {
    let mhz = freq_hz as f64 / 1_000_000.0;
    format!("{mhz:.6} MHz")
}

/// Convert a signal strength in dBm to an S-unit string.
///
/// Uses the standard HF S-meter calibration (50 ohm, IARU):
/// - S9 = −73 dBm
/// - Each S-unit = 6 dB
/// - Below S1, returns "S0"
/// - Above S9, returns "S9+N dB" (rounded to the nearest integer dB)
///
/// # Example
///
/// ```
/// use riglib_core::s_units_from_dbm;
///
/// assert_eq!(s_units_from_dbm(-73.0), "S9");
/// assert_eq!(s_units_from_dbm(-79.0), "S8");
/// assert_eq!(s_units_from_dbm(-63.0), "S9+10 dB");
/// assert_eq!(s_units_from_dbm(-130.0), "S0");
/// ```
pub fn s_units_from_dbm(dbm: f32) -> String {
    if dbm > -73.0 {
        let over = (dbm + 73.0).round() as i32;
        format!("S9+{over} dB")
    } else {
        // S9 = -73 dBm, each S-unit is 6 dB below.
        // S1 = -73 - 8*6 = -121 dBm.
        let s = ((dbm + 127.0) / 6.0).round() as i32;
        let s = s.clamp(0, 9);
        format!("S{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_freq_mhz_hf() {
        assert_eq!(format_freq_mhz(14_074_000), "14.074000 MHz");
        assert_eq!(format_freq_mhz(7_000_000), "7.000000 MHz");
        assert_eq!(format_freq_mhz(1_840_000), "1.840000 MHz");
    }

    #[test]
    fn format_freq_mhz_vhf_uhf() {
        assert_eq!(format_freq_mhz(144_174_000), "144.174000 MHz");
        assert_eq!(format_freq_mhz(432_100_000), "432.100000 MHz");
    }

    #[test]
    fn format_freq_mhz_zero() {
        assert_eq!(format_freq_mhz(0), "0.000000 MHz");
    }

    #[test]
    fn s_units_s9() {
        assert_eq!(s_units_from_dbm(-73.0), "S9");
    }

    #[test]
    fn s_units_below_s9() {
        assert_eq!(s_units_from_dbm(-79.0), "S8");
        assert_eq!(s_units_from_dbm(-85.0), "S7");
        assert_eq!(s_units_from_dbm(-91.0), "S6");
        assert_eq!(s_units_from_dbm(-97.0), "S5");
        assert_eq!(s_units_from_dbm(-103.0), "S4");
        assert_eq!(s_units_from_dbm(-109.0), "S3");
        assert_eq!(s_units_from_dbm(-115.0), "S2");
        assert_eq!(s_units_from_dbm(-121.0), "S1");
    }

    #[test]
    fn s_units_above_s9() {
        assert_eq!(s_units_from_dbm(-63.0), "S9+10 dB");
        assert_eq!(s_units_from_dbm(-53.0), "S9+20 dB");
        assert_eq!(s_units_from_dbm(-33.0), "S9+40 dB");
    }

    #[test]
    fn s_units_very_weak() {
        assert_eq!(s_units_from_dbm(-130.0), "S0");
        assert_eq!(s_units_from_dbm(-150.0), "S0");
    }

    #[test]
    fn s_units_clamps_at_s9() {
        // -73.0 exactly is S9 (not above).
        assert_eq!(s_units_from_dbm(-73.0), "S9");
        // Slightly above -73 is S9+.
        assert_eq!(s_units_from_dbm(-72.0), "S9+1 dB");
    }
}
