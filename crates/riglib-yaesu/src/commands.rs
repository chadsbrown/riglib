//! Yaesu CAT command builders and response parsers.
//!
//! This module provides functions to construct CAT command byte sequences for
//! common transceiver operations (frequency, mode, PTT, metering, split, power)
//! and to parse the corresponding responses from the rig.
//!
//! All functions are pure -- they produce or consume byte vectors / string slices
//! without performing any I/O. The caller is responsible for sending the bytes
//! over a transport and feeding received data back into the response parsers.
//!
//! # Yaesu CAT command reference
//!
//! Based on the FT-DX10, FT-991A, and FT-DX101D CAT protocol manuals.
//! Frequencies are always 9 ASCII digits in hertz, zero-padded on the left.
//! Mode codes are single hex-digit characters (`1`-`E`).

use riglib_core::{Error, Mode, Result};

use crate::protocol::encode_command;

// ---------------------------------------------------------------
// Yaesu mode code mapping
// ---------------------------------------------------------------

/// Yaesu CAT mode code for LSB.
const YAESU_MODE_LSB: &str = "1";
/// Yaesu CAT mode code for USB.
const YAESU_MODE_USB: &str = "2";
/// Yaesu CAT mode code for CW.
const YAESU_MODE_CW: &str = "3";
/// Yaesu CAT mode code for FM.
const YAESU_MODE_FM: &str = "4";
/// Yaesu CAT mode code for AM.
const YAESU_MODE_AM: &str = "5";
/// Yaesu CAT mode code for RTTY-LSB (FSK).
const YAESU_MODE_RTTY_LSB: &str = "6";
/// Yaesu CAT mode code for CW-R (reverse beat).
const YAESU_MODE_CWR: &str = "7";
/// Yaesu CAT mode code for DATA-LSB (AFSK lower sideband).
const YAESU_MODE_DATA_LSB: &str = "8";
/// Yaesu CAT mode code for RTTY-USB (FSK reverse).
const YAESU_MODE_RTTY_USB: &str = "9";
/// Yaesu CAT mode code for DATA-FM.
const YAESU_MODE_DATA_FM: &str = "A";
// Note: FM-N (code "B") is handled in yaesu_to_mode() but has no
// dedicated constant because no Mode variant maps *to* FM-N; it only
// maps *from* FM-N back to Mode::FM.

/// Yaesu CAT mode code for DATA-USB (AFSK upper sideband).
const YAESU_MODE_DATA_USB: &str = "C";
// Note: AM-N (code "D") is used for DataAM mapping. We define it as a
// constant since mode_to_yaesu maps DataAM -> "D".
/// Yaesu CAT mode code for AM-N / DATA-AM (narrow AM).
const YAESU_MODE_AM_N: &str = "D";

// Note: mode code E = C4FM is not mapped — riglib_core::Mode has no C4FM variant.

// ---------------------------------------------------------------
// Command builders
// ---------------------------------------------------------------

/// Build a "read VFO-A frequency" command (`FA;`).
pub fn cmd_read_frequency_a() -> Vec<u8> {
    encode_command("FA", "")
}

/// Build a "read VFO-B frequency" command (`FB;`).
pub fn cmd_read_frequency_b() -> Vec<u8> {
    encode_command("FB", "")
}

/// Build a "set VFO-A frequency" command (`FA{freq:09};`).
///
/// The frequency is encoded as exactly 9 zero-padded ASCII digits in hertz.
///
/// # Arguments
///
/// * `freq_hz` - Frequency in hertz (e.g. `14_250_000` for 14.250 MHz).
pub fn cmd_set_frequency_a(freq_hz: u64) -> Vec<u8> {
    encode_command("FA", &format!("{freq_hz:09}"))
}

/// Build a "set VFO-B frequency" command (`FB{freq:09};`).
///
/// Same format as VFO-A.
pub fn cmd_set_frequency_b(freq_hz: u64) -> Vec<u8> {
    encode_command("FB", &format!("{freq_hz:09}"))
}

/// Build a "read VFO-A operating mode" command (`MD0;`).
pub fn cmd_read_mode_a() -> Vec<u8> {
    encode_command("MD0", "")
}

/// Build a "read VFO-B operating mode" command (`MD1;`).
pub fn cmd_read_mode_b() -> Vec<u8> {
    encode_command("MD1", "")
}

/// Build a "set VFO-A operating mode" command (`MD0{code};`).
///
/// Maps the generic [`Mode`] enum to the corresponding Yaesu CAT mode code.
///
/// # Arguments
///
/// * `mode` - The operating mode to set.
pub fn cmd_set_mode_a(mode: &Mode) -> Vec<u8> {
    let code = mode_to_yaesu(mode);
    encode_command("MD0", code)
}

/// Build a "set VFO-B operating mode" command (`MD1{code};`).
pub fn cmd_set_mode_b(mode: &Mode) -> Vec<u8> {
    let code = mode_to_yaesu(mode);
    encode_command("MD1", code)
}

/// Build a "read PTT state" command (`TX;`).
pub fn cmd_read_ptt() -> Vec<u8> {
    encode_command("TX", "")
}

/// Build a "set PTT" command.
///
/// - `TX1;` keys the transmitter (via microphone/front panel).
/// - `TX0;` returns to receive.
///
/// # Arguments
///
/// * `on` - `true` to transmit, `false` to receive.
pub fn cmd_set_ptt(on: bool) -> Vec<u8> {
    if on {
        encode_command("TX", "1")
    } else {
        encode_command("TX", "0")
    }
}

/// Build a "read RF power" command (`PC;`).
pub fn cmd_read_power() -> Vec<u8> {
    encode_command("PC", "")
}

/// Build a "set RF power" command (`PC{watts:03};`).
///
/// Power is encoded as exactly 3 zero-padded ASCII digits in watts.
///
/// # Arguments
///
/// * `watts` - Output power in watts (0-999, though most rigs cap at 100 or 200).
pub fn cmd_set_power(watts: u16) -> Vec<u8> {
    encode_command("PC", &format!("{watts:03}"))
}

/// Build a "read S-meter" command (`SM0;`).
///
/// The response returns a 3-digit value from 000 to 255.
pub fn cmd_read_s_meter() -> Vec<u8> {
    encode_command("SM0", "")
}

/// Build a "read SWR meter" command (`RM1;`).
///
/// Only meaningful while transmitting. Response is 3-digit value (000-255).
pub fn cmd_read_swr() -> Vec<u8> {
    encode_command("RM1", "")
}

/// Build a "read ALC meter" command (`RM5;`).
///
/// Only meaningful while transmitting. Response is 3-digit value (000-255).
pub fn cmd_read_alc() -> Vec<u8> {
    encode_command("RM5", "")
}

/// Build a "read split state" command (`FT;`).
pub fn cmd_read_split() -> Vec<u8> {
    encode_command("FT", "")
}

/// Build a "set split" command.
///
/// - `FT1;` enables split operation.
/// - `FT0;` disables split operation.
///
/// # Arguments
///
/// * `on` - `true` to enable split, `false` to disable.
pub fn cmd_set_split(on: bool) -> Vec<u8> {
    if on {
        encode_command("FT", "1")
    } else {
        encode_command("FT", "0")
    }
}

// ---------------------------------------------------------------
// Response parsers
// ---------------------------------------------------------------

/// Parse a frequency response from the data portion of an `FA` or `FB` response.
///
/// Expects exactly 9 ASCII digits representing the frequency in hertz.
///
/// # Arguments
///
/// * `data` - The data field from a decoded `FA` or `FB` response
///   (e.g. `"014250000"`).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is not exactly 9 digits or
/// cannot be parsed as a valid integer.
pub fn parse_frequency_response(data: &str) -> Result<u64> {
    if data.len() != 9 {
        return Err(Error::Protocol(format!(
            "expected 9 digits for frequency, got {} characters: {:?}",
            data.len(),
            data
        )));
    }
    data.parse::<u64>()
        .map_err(|e| Error::Protocol(format!("invalid frequency digits: {data:?} ({e})")))
}

/// Parse a mode response from the data portion of an `MD0` or `MD1` response.
///
/// Expects a single character (hex digit `1`-`E`) representing the Yaesu
/// mode code.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the mode code is unrecognised.
pub fn parse_mode_response(data: &str) -> Result<Mode> {
    yaesu_to_mode(data)
}

/// Parse a PTT response from the data portion of a `TX` response.
///
/// - `"0"` = receive
/// - `"1"` = transmit (mic)
/// - `"2"` = transmit (data)
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is empty or not a valid TX state.
pub fn parse_ptt_response(data: &str) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected TX state digit, got empty data".into(),
        ));
    }
    match data {
        "0" => Ok(false),
        "1" | "2" => Ok(true),
        _ => Err(Error::Protocol(format!("unexpected TX state: {data:?}"))),
    }
}

/// Parse a meter response from the data portion of an `RM` or `SM` response.
///
/// Expects a 3-character numeric string (000-255) representing the raw
/// meter value.
///
/// # Returns
///
/// The raw meter value as a `u16` (0-255 scale).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed as a valid
/// integer or is out of the expected range.
pub fn parse_meter_response(data: &str) -> Result<u16> {
    if data.len() != 3 {
        return Err(Error::Protocol(format!(
            "expected 3 digits for meter, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u16 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid meter digits: {data:?} ({e})")))?;
    Ok(val)
}

/// Parse a power response from the data portion of a `PC` response.
///
/// Expects a 3-character numeric string (000-999) representing power in watts.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_power_response(data: &str) -> Result<u16> {
    if data.len() != 3 {
        return Err(Error::Protocol(format!(
            "expected 3 digits for power, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u16 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid power digits: {data:?} ({e})")))?;
    Ok(val)
}

/// Parse a split response from the data portion of an `FT` response.
///
/// - `"0"` = split off
/// - `"1"` = split on
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is empty or not a valid split state.
pub fn parse_split_response(data: &str) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected split state digit, got empty data".into(),
        ));
    }
    match data {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(Error::Protocol(format!("unexpected split state: {data:?}"))),
    }
}

// ---------------------------------------------------------------
// Mode conversion helpers
// ---------------------------------------------------------------

/// Convert a generic [`Mode`] to the Yaesu CAT mode code string.
fn mode_to_yaesu(mode: &Mode) -> &'static str {
    match mode {
        Mode::LSB => YAESU_MODE_LSB,
        Mode::USB => YAESU_MODE_USB,
        Mode::CW => YAESU_MODE_CW,
        Mode::CWR => YAESU_MODE_CWR,
        Mode::AM => YAESU_MODE_AM,
        Mode::FM => YAESU_MODE_FM,
        Mode::RTTY => YAESU_MODE_RTTY_LSB,
        Mode::RTTYR => YAESU_MODE_RTTY_USB,
        Mode::DataUSB => YAESU_MODE_DATA_USB,
        Mode::DataLSB => YAESU_MODE_DATA_LSB,
        Mode::DataFM => YAESU_MODE_DATA_FM,
        Mode::DataAM => YAESU_MODE_AM_N,
    }
}

/// Convert a Yaesu CAT mode code string to a generic [`Mode`].
///
/// Yaesu mode codes are single hex-digit characters:
/// - `1` = LSB, `2` = USB, `3` = CW, `4` = FM, `5` = AM
/// - `6` = RTTY-LSB, `7` = CW-R, `8` = DATA-LSB, `9` = RTTY-USB
/// - `A` = DATA-FM, `B` = FM-N (mapped to FM), `C` = DATA-USB
/// - `D` = AM-N (mapped to DataAM), `E` = C4FM (unsupported)
fn yaesu_to_mode(code: &str) -> Result<Mode> {
    match code {
        "1" => Ok(Mode::LSB),
        "2" => Ok(Mode::USB),
        "3" => Ok(Mode::CW),
        "4" => Ok(Mode::FM),
        "5" => Ok(Mode::AM),
        "6" => Ok(Mode::RTTY),
        "7" => Ok(Mode::CWR),
        "8" => Ok(Mode::DataLSB),
        "9" => Ok(Mode::RTTYR),
        "A" => Ok(Mode::DataFM),
        "B" => Ok(Mode::FM), // FM-N maps to generic FM
        "C" => Ok(Mode::DataUSB),
        "D" => Ok(Mode::DataAM), // AM-N maps to DataAM
        _ => Err(Error::Protocol(format!(
            "unknown Yaesu mode code: {code:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Command building verification
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_frequency_a_bytes() {
        assert_eq!(cmd_read_frequency_a(), b"FA;");
    }

    #[test]
    fn cmd_read_frequency_b_bytes() {
        assert_eq!(cmd_read_frequency_b(), b"FB;");
    }

    #[test]
    fn cmd_set_frequency_a_14250() {
        let cmd = cmd_set_frequency_a(14_250_000);
        assert_eq!(cmd, b"FA014250000;");
    }

    #[test]
    fn cmd_set_frequency_a_7000() {
        let cmd = cmd_set_frequency_a(7_000_000);
        assert_eq!(cmd, b"FA007000000;");
    }

    #[test]
    fn cmd_set_frequency_a_zero_padded() {
        let cmd = cmd_set_frequency_a(1_800_000);
        assert_eq!(cmd, b"FA001800000;");
    }

    #[test]
    fn cmd_set_frequency_b_28500() {
        let cmd = cmd_set_frequency_b(28_500_000);
        assert_eq!(cmd, b"FB028500000;");
    }

    #[test]
    fn cmd_set_frequency_a_50mhz() {
        let cmd = cmd_set_frequency_a(50_100_000);
        assert_eq!(cmd, b"FA050100000;");
    }

    #[test]
    fn cmd_read_mode_a_bytes() {
        assert_eq!(cmd_read_mode_a(), b"MD0;");
    }

    #[test]
    fn cmd_read_mode_b_bytes() {
        assert_eq!(cmd_read_mode_b(), b"MD1;");
    }

    #[test]
    fn cmd_set_mode_a_usb() {
        let cmd = cmd_set_mode_a(&Mode::USB);
        assert_eq!(cmd, b"MD02;");
    }

    #[test]
    fn cmd_set_mode_a_lsb() {
        let cmd = cmd_set_mode_a(&Mode::LSB);
        assert_eq!(cmd, b"MD01;");
    }

    #[test]
    fn cmd_set_mode_a_cw() {
        let cmd = cmd_set_mode_a(&Mode::CW);
        assert_eq!(cmd, b"MD03;");
    }

    #[test]
    fn cmd_set_mode_a_cwr() {
        let cmd = cmd_set_mode_a(&Mode::CWR);
        assert_eq!(cmd, b"MD07;");
    }

    #[test]
    fn cmd_set_mode_a_fm() {
        let cmd = cmd_set_mode_a(&Mode::FM);
        assert_eq!(cmd, b"MD04;");
    }

    #[test]
    fn cmd_set_mode_a_am() {
        let cmd = cmd_set_mode_a(&Mode::AM);
        assert_eq!(cmd, b"MD05;");
    }

    #[test]
    fn cmd_set_mode_a_rtty() {
        let cmd = cmd_set_mode_a(&Mode::RTTY);
        assert_eq!(cmd, b"MD06;");
    }

    #[test]
    fn cmd_set_mode_a_rttyr() {
        let cmd = cmd_set_mode_a(&Mode::RTTYR);
        assert_eq!(cmd, b"MD09;");
    }

    #[test]
    fn cmd_set_mode_a_data_usb() {
        let cmd = cmd_set_mode_a(&Mode::DataUSB);
        assert_eq!(cmd, b"MD0C;");
    }

    #[test]
    fn cmd_set_mode_a_data_lsb() {
        let cmd = cmd_set_mode_a(&Mode::DataLSB);
        assert_eq!(cmd, b"MD08;");
    }

    #[test]
    fn cmd_set_mode_a_data_fm() {
        let cmd = cmd_set_mode_a(&Mode::DataFM);
        assert_eq!(cmd, b"MD0A;");
    }

    #[test]
    fn cmd_set_mode_a_data_am() {
        // DataAM maps to Yaesu AM-N (code D)
        let cmd = cmd_set_mode_a(&Mode::DataAM);
        assert_eq!(cmd, b"MD0D;");
    }

    #[test]
    fn cmd_set_mode_b_usb() {
        let cmd = cmd_set_mode_b(&Mode::USB);
        assert_eq!(cmd, b"MD12;");
    }

    #[test]
    fn cmd_set_mode_b_cw() {
        let cmd = cmd_set_mode_b(&Mode::CW);
        assert_eq!(cmd, b"MD13;");
    }

    #[test]
    fn cmd_read_ptt_bytes() {
        assert_eq!(cmd_read_ptt(), b"TX;");
    }

    #[test]
    fn cmd_set_ptt_on_bytes() {
        assert_eq!(cmd_set_ptt(true), b"TX1;");
    }

    #[test]
    fn cmd_set_ptt_off_bytes() {
        assert_eq!(cmd_set_ptt(false), b"TX0;");
    }

    #[test]
    fn cmd_read_power_bytes() {
        assert_eq!(cmd_read_power(), b"PC;");
    }

    #[test]
    fn cmd_set_power_50w() {
        assert_eq!(cmd_set_power(50), b"PC050;");
    }

    #[test]
    fn cmd_set_power_100w() {
        assert_eq!(cmd_set_power(100), b"PC100;");
    }

    #[test]
    fn cmd_set_power_5w() {
        assert_eq!(cmd_set_power(5), b"PC005;");
    }

    #[test]
    fn cmd_set_power_zero() {
        assert_eq!(cmd_set_power(0), b"PC000;");
    }

    #[test]
    fn cmd_read_s_meter_bytes() {
        assert_eq!(cmd_read_s_meter(), b"SM0;");
    }

    #[test]
    fn cmd_read_swr_bytes() {
        assert_eq!(cmd_read_swr(), b"RM1;");
    }

    #[test]
    fn cmd_read_alc_bytes() {
        assert_eq!(cmd_read_alc(), b"RM5;");
    }

    #[test]
    fn cmd_read_split_bytes() {
        assert_eq!(cmd_read_split(), b"FT;");
    }

    #[test]
    fn cmd_set_split_on_bytes() {
        assert_eq!(cmd_set_split(true), b"FT1;");
    }

    #[test]
    fn cmd_set_split_off_bytes() {
        assert_eq!(cmd_set_split(false), b"FT0;");
    }

    // ---------------------------------------------------------------
    // Response parsing — frequencies
    // ---------------------------------------------------------------

    #[test]
    fn parse_freq_14250() {
        let freq = parse_frequency_response("014250000").unwrap();
        assert_eq!(freq, 14_250_000);
    }

    #[test]
    fn parse_freq_7000() {
        let freq = parse_frequency_response("007000000").unwrap();
        assert_eq!(freq, 7_000_000);
    }

    #[test]
    fn parse_freq_1800() {
        let freq = parse_frequency_response("001800000").unwrap();
        assert_eq!(freq, 1_800_000);
    }

    #[test]
    fn parse_freq_50100() {
        let freq = parse_frequency_response("050100000").unwrap();
        assert_eq!(freq, 50_100_000);
    }

    #[test]
    fn parse_freq_28500() {
        let freq = parse_frequency_response("028500000").unwrap();
        assert_eq!(freq, 28_500_000);
    }

    #[test]
    fn parse_freq_max_9_digits() {
        let freq = parse_frequency_response("999999999").unwrap();
        assert_eq!(freq, 999_999_999);
    }

    #[test]
    fn parse_freq_zero() {
        let freq = parse_frequency_response("000000000").unwrap();
        assert_eq!(freq, 0);
    }

    #[test]
    fn parse_freq_wrong_length_short() {
        assert!(parse_frequency_response("0142500").is_err());
    }

    #[test]
    fn parse_freq_wrong_length_long() {
        assert!(parse_frequency_response("0142500000").is_err());
    }

    #[test]
    fn parse_freq_empty() {
        assert!(parse_frequency_response("").is_err());
    }

    #[test]
    fn parse_freq_non_digit() {
        assert!(parse_frequency_response("01425000A").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing — modes
    // ---------------------------------------------------------------

    #[test]
    fn parse_mode_lsb() {
        assert_eq!(parse_mode_response("1").unwrap(), Mode::LSB);
    }

    #[test]
    fn parse_mode_usb() {
        assert_eq!(parse_mode_response("2").unwrap(), Mode::USB);
    }

    #[test]
    fn parse_mode_cw() {
        assert_eq!(parse_mode_response("3").unwrap(), Mode::CW);
    }

    #[test]
    fn parse_mode_fm() {
        assert_eq!(parse_mode_response("4").unwrap(), Mode::FM);
    }

    #[test]
    fn parse_mode_am() {
        assert_eq!(parse_mode_response("5").unwrap(), Mode::AM);
    }

    #[test]
    fn parse_mode_rtty() {
        assert_eq!(parse_mode_response("6").unwrap(), Mode::RTTY);
    }

    #[test]
    fn parse_mode_cwr() {
        assert_eq!(parse_mode_response("7").unwrap(), Mode::CWR);
    }

    #[test]
    fn parse_mode_data_lsb() {
        assert_eq!(parse_mode_response("8").unwrap(), Mode::DataLSB);
    }

    #[test]
    fn parse_mode_rttyr() {
        assert_eq!(parse_mode_response("9").unwrap(), Mode::RTTYR);
    }

    #[test]
    fn parse_mode_data_fm() {
        assert_eq!(parse_mode_response("A").unwrap(), Mode::DataFM);
    }

    #[test]
    fn parse_mode_fm_n() {
        // FM-N maps to generic FM
        assert_eq!(parse_mode_response("B").unwrap(), Mode::FM);
    }

    #[test]
    fn parse_mode_data_usb() {
        assert_eq!(parse_mode_response("C").unwrap(), Mode::DataUSB);
    }

    #[test]
    fn parse_mode_am_n() {
        // AM-N maps to DataAM
        assert_eq!(parse_mode_response("D").unwrap(), Mode::DataAM);
    }

    #[test]
    fn parse_mode_unknown() {
        assert!(parse_mode_response("F").is_err());
    }

    #[test]
    fn parse_mode_empty() {
        assert!(parse_mode_response("").is_err());
    }

    #[test]
    fn parse_mode_c4fm() {
        // C4FM (code E) is not mapped to a riglib Mode
        assert!(parse_mode_response("E").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing — PTT
    // ---------------------------------------------------------------

    #[test]
    fn parse_ptt_off() {
        assert!(!parse_ptt_response("0").unwrap());
    }

    #[test]
    fn parse_ptt_on_mic() {
        assert!(parse_ptt_response("1").unwrap());
    }

    #[test]
    fn parse_ptt_on_data() {
        assert!(parse_ptt_response("2").unwrap());
    }

    #[test]
    fn parse_ptt_empty() {
        assert!(parse_ptt_response("").is_err());
    }

    #[test]
    fn parse_ptt_invalid() {
        assert!(parse_ptt_response("3").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing — meters
    // ---------------------------------------------------------------

    #[test]
    fn parse_meter_zero() {
        assert_eq!(parse_meter_response("000").unwrap(), 0);
    }

    #[test]
    fn parse_meter_max() {
        assert_eq!(parse_meter_response("255").unwrap(), 255);
    }

    #[test]
    fn parse_meter_mid() {
        assert_eq!(parse_meter_response("128").unwrap(), 128);
    }

    #[test]
    fn parse_meter_s9() {
        // S9 is approximately reading 120
        assert_eq!(parse_meter_response("120").unwrap(), 120);
    }

    #[test]
    fn parse_meter_wrong_length() {
        assert!(parse_meter_response("12").is_err());
        assert!(parse_meter_response("1234").is_err());
    }

    #[test]
    fn parse_meter_non_digit() {
        assert!(parse_meter_response("12A").is_err());
    }

    #[test]
    fn parse_meter_empty() {
        assert!(parse_meter_response("").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing — power
    // ---------------------------------------------------------------

    #[test]
    fn parse_power_100w() {
        assert_eq!(parse_power_response("100").unwrap(), 100);
    }

    #[test]
    fn parse_power_50w() {
        assert_eq!(parse_power_response("050").unwrap(), 50);
    }

    #[test]
    fn parse_power_5w() {
        assert_eq!(parse_power_response("005").unwrap(), 5);
    }

    #[test]
    fn parse_power_zero() {
        assert_eq!(parse_power_response("000").unwrap(), 0);
    }

    #[test]
    fn parse_power_200w() {
        assert_eq!(parse_power_response("200").unwrap(), 200);
    }

    #[test]
    fn parse_power_wrong_length() {
        assert!(parse_power_response("50").is_err());
        assert!(parse_power_response("1000").is_err());
    }

    #[test]
    fn parse_power_non_digit() {
        assert!(parse_power_response("1O0").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing — split
    // ---------------------------------------------------------------

    #[test]
    fn parse_split_off() {
        assert!(!parse_split_response("0").unwrap());
    }

    #[test]
    fn parse_split_on() {
        assert!(parse_split_response("1").unwrap());
    }

    #[test]
    fn parse_split_empty() {
        assert!(parse_split_response("").is_err());
    }

    #[test]
    fn parse_split_invalid() {
        assert!(parse_split_response("2").is_err());
    }

    // ---------------------------------------------------------------
    // Mode round-trip: Mode -> Yaesu code -> Mode
    // ---------------------------------------------------------------

    #[test]
    fn mode_round_trip_all_standard() {
        // Modes that have a perfect round-trip (no lossy mapping)
        let modes = [
            Mode::LSB,
            Mode::USB,
            Mode::CW,
            Mode::CWR,
            Mode::AM,
            Mode::FM,
            Mode::RTTY,
            Mode::RTTYR,
            Mode::DataUSB,
            Mode::DataLSB,
            Mode::DataFM,
        ];
        for mode in modes {
            let code = mode_to_yaesu(&mode);
            let parsed = yaesu_to_mode(code).unwrap();
            assert_eq!(mode, parsed, "round-trip failed for {mode}");
        }
    }

    #[test]
    fn mode_data_am_round_trip() {
        // DataAM maps to AM-N (code D), and AM-N parses back as DataAM
        let code = mode_to_yaesu(&Mode::DataAM);
        assert_eq!(code, "D");
        let parsed = yaesu_to_mode(code).unwrap();
        assert_eq!(parsed, Mode::DataAM);
    }

    // ---------------------------------------------------------------
    // Boundary / edge-case command tests
    // ---------------------------------------------------------------

    #[test]
    fn cmd_set_frequency_a_max_9_digits() {
        let cmd = cmd_set_frequency_a(999_999_999);
        assert_eq!(cmd, b"FA999999999;");
    }

    #[test]
    fn cmd_set_frequency_a_zero() {
        let cmd = cmd_set_frequency_a(0);
        assert_eq!(cmd, b"FA000000000;");
    }

    #[test]
    fn cmd_set_frequency_a_ft8_14074() {
        let cmd = cmd_set_frequency_a(14_074_000);
        assert_eq!(cmd, b"FA014074000;");
    }

    #[test]
    fn cmd_set_power_max_3_digits() {
        let cmd = cmd_set_power(999);
        assert_eq!(cmd, b"PC999;");
    }
}
