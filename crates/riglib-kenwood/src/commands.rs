//! Kenwood CAT command builders and response parsers.
//!
//! This module provides functions to construct CAT command byte sequences for
//! common transceiver operations (frequency, mode, PTT, metering, split, power)
//! and to parse the corresponding responses from the rig.
//!
//! All functions are pure -- they produce or consume byte vectors / string slices
//! without performing any I/O. The caller is responsible for sending the bytes
//! over a transport and feeding received data back into the response parsers.
//!
//! # Kenwood CAT command reference
//!
//! Based on the TS-590S/SG, TS-890S, and TS-990S CAT protocol manuals.
//! Frequencies are always 11 ASCII digits in hertz, zero-padded on the left.
//! Mode codes are single-digit characters (`1`-`9`).

use riglib_core::{Error, Mode, Result};

use crate::protocol::encode_command;

// ---------------------------------------------------------------
// Kenwood mode code mapping
// ---------------------------------------------------------------

/// Kenwood CAT mode code for LSB.
const KENWOOD_MODE_LSB: &str = "1";
/// Kenwood CAT mode code for USB.
const KENWOOD_MODE_USB: &str = "2";
/// Kenwood CAT mode code for CW.
const KENWOOD_MODE_CW: &str = "3";
/// Kenwood CAT mode code for FM.
const KENWOOD_MODE_FM: &str = "4";
/// Kenwood CAT mode code for AM.
const KENWOOD_MODE_AM: &str = "5";
/// Kenwood CAT mode code for FSK (RTTY).
const KENWOOD_MODE_RTTY: &str = "6";
/// Kenwood CAT mode code for CW-R (reverse beat).
const KENWOOD_MODE_CWR: &str = "7";
/// Kenwood CAT mode code for FSK-R (RTTY reverse).
const KENWOOD_MODE_RTTYR: &str = "9";

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

/// Build a "set VFO-A frequency" command (`FA{freq:011};`).
///
/// The frequency is encoded as exactly 11 zero-padded ASCII digits in hertz.
///
/// # Arguments
///
/// * `freq_hz` - Frequency in hertz (e.g. `14_250_000` for 14.250 MHz).
pub fn cmd_set_frequency_a(freq_hz: u64) -> Vec<u8> {
    encode_command("FA", &format!("{freq_hz:011}"))
}

/// Build a "set VFO-B frequency" command (`FB{freq:011};`).
///
/// Same 11-digit format as VFO-A.
pub fn cmd_set_frequency_b(freq_hz: u64) -> Vec<u8> {
    encode_command("FB", &format!("{freq_hz:011}"))
}

/// Build a "read operating mode" command (`MD;`).
pub fn cmd_read_mode() -> Vec<u8> {
    encode_command("MD", "")
}

/// Build a "set operating mode" command (`MD{code};`).
///
/// Maps the generic [`Mode`] enum to the corresponding Kenwood CAT mode code.
///
/// Data modes (DataUSB, DataLSB, DataFM, DataAM) are mapped to the nearest
/// base mode code since most Kenwood rigs do not have dedicated data mode
/// numbers in the `MD` command. Models with `has_data_modes` may use the
/// `DA` command separately.
///
/// # Arguments
///
/// * `mode` - The operating mode to set.
pub fn cmd_set_mode(mode: &Mode) -> Vec<u8> {
    let code = mode_to_kenwood(mode);
    encode_command("MD", code)
}

/// Build a "read PTT state" command (`TX;`).
pub fn cmd_read_ptt() -> Vec<u8> {
    encode_command("TX", "")
}

/// Build a "set PTT" command.
///
/// - `TX1;` keys the transmitter.
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

/// Build a "force receive" command (`RX;`).
///
/// Kenwood rigs accept `RX;` as an alternative to `TX0;` to force a return
/// to receive mode.
pub fn cmd_force_receive() -> Vec<u8> {
    encode_command("RX", "")
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
/// The `0` parameter selects the main receiver meter. The response returns
/// `SM0nnnn;` where `nnnn` is a 4-digit value (0000-0030 typical for
/// S-meter, though the exact range is model-dependent).
pub fn cmd_read_s_meter() -> Vec<u8> {
    encode_command("SM", "0")
}

/// Build a "read SWR meter" command (`RM1;`).
///
/// Kenwood uses `RM1` for the SWR meter. Only meaningful while transmitting.
/// Response is `RM1nnnn;` where `nnnn` is a 4-digit value.
pub fn cmd_read_swr() -> Vec<u8> {
    encode_command("RM", "1")
}

/// Build a "read ALC meter" command (`RM2;`).
///
/// Kenwood uses `RM2` for the ALC meter. Only meaningful while transmitting.
/// Response is `RM2nnnn;` where `nnnn` is a 4-digit value.
pub fn cmd_read_alc() -> Vec<u8> {
    encode_command("RM", "2")
}

/// Build a "read RX VFO select" command (`FR;`).
///
/// Response is `FR0;` (VFO A) or `FR1;` (VFO B).
pub fn cmd_read_rx_vfo() -> Vec<u8> {
    encode_command("FR", "")
}

/// Build a "read TX VFO select" command (`FT;`).
///
/// Response is `FT0;` (VFO A) or `FT1;` (VFO B).
pub fn cmd_read_tx_vfo() -> Vec<u8> {
    encode_command("FT", "")
}

/// Build commands to set split on or off.
///
/// Split on: `FR0;FT1;` (RX on VFO A, TX on VFO B).
/// Split off: `FR0;FT0;` (both RX and TX on VFO A).
///
/// Returns a pair of command byte vectors to be sent sequentially.
///
/// # Arguments
///
/// * `on` - `true` to enable split, `false` to disable.
pub fn cmd_set_split(on: bool) -> (Vec<u8>, Vec<u8>) {
    let fr = encode_command("FR", "0");
    let ft = if on {
        encode_command("FT", "1")
    } else {
        encode_command("FT", "0")
    };
    (fr, ft)
}

/// Build a "set TX VFO" command.
///
/// `FT0;` = TX on VFO A, `FT1;` = TX on VFO B.
pub fn cmd_set_tx_vfo(vfo_b: bool) -> Vec<u8> {
    if vfo_b {
        encode_command("FT", "1")
    } else {
        encode_command("FT", "0")
    }
}

/// Build a "set AI (Auto Information) mode" command.
///
/// - `AI2;` enables AI mode (rig pushes state changes).
/// - `AI0;` disables AI mode.
///
/// # Arguments
///
/// * `on` - `true` to enable AI mode, `false` to disable.
pub fn cmd_set_ai(on: bool) -> Vec<u8> {
    if on {
        encode_command("AI", "2")
    } else {
        encode_command("AI", "0")
    }
}

/// Build a "read passband width" command (`SH;`).
///
/// Kenwood rigs return the filter/passband width via the `SH` command.
/// The response format varies by model.
pub fn cmd_read_passband() -> Vec<u8> {
    encode_command("SH", "")
}

/// Build a "set passband width" command (`SH{value:02};`).
///
/// The value encoding is model-dependent. Common Kenwood convention
/// uses a 2-digit index that maps to a filter width.
///
/// # Arguments
///
/// * `value` - Passband index (model-dependent, typically 00-31).
pub fn cmd_set_passband(value: u8) -> Vec<u8> {
    encode_command("SH", &format!("{value:02}"))
}

// ---------------------------------------------------------------
// Response parsers
// ---------------------------------------------------------------

/// Parse a frequency response from the data portion of an `FA` or `FB` response.
///
/// Expects exactly 11 ASCII digits representing the frequency in hertz.
///
/// # Arguments
///
/// * `data` - The data field from a decoded `FA` or `FB` response
///   (e.g. `"00014250000"`).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is not exactly 11 digits or
/// cannot be parsed as a valid integer.
pub fn parse_frequency_response(data: &str) -> Result<u64> {
    if data.len() != 11 {
        return Err(Error::Protocol(format!(
            "expected 11 digits for frequency, got {} characters: {:?}",
            data.len(),
            data
        )));
    }
    data.parse::<u64>()
        .map_err(|e| Error::Protocol(format!("invalid frequency digits: {data:?} ({e})")))
}

/// Parse a mode response from the data portion of an `MD` response.
///
/// Expects a single character (digit `1`-`9`) representing the Kenwood
/// mode code.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the mode code is unrecognised.
pub fn parse_mode_response(data: &str) -> Result<Mode> {
    kenwood_to_mode(data)
}

/// Parse a PTT response from the data portion of a `TX` response.
///
/// - `"0"` = receive
/// - `"1"` = transmit
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
        "1" => Ok(true),
        _ => Err(Error::Protocol(format!("unexpected TX state: {data:?}"))),
    }
}

/// Parse a meter response from the data portion of an `RM` or `SM` response.
///
/// Kenwood meter responses include a 1-digit meter selector followed by a
/// 4-digit value. For example, `SM0` response data is `"00015"` (selector
/// `0`, value `0015`). For `RM1`, the data is `"10045"` (selector `1`,
/// value `0045`).
///
/// This parser extracts the 4-digit numeric value, skipping the leading
/// selector digit.
///
/// # Returns
///
/// The raw meter value as a `u16`.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed as a valid integer.
pub fn parse_meter_response(data: &str) -> Result<u16> {
    if data.len() != 5 {
        return Err(Error::Protocol(format!(
            "expected 5 characters for meter (selector + 4 digits), got {} characters: {data:?}",
            data.len()
        )));
    }
    // Skip the selector digit (first char), parse the remaining 4 digits.
    let value_str = &data[1..];
    let val: u16 = value_str
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid meter digits: {value_str:?} ({e})")))?;
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

/// Parse a VFO select response from the data portion of an `FR` or `FT` response.
///
/// - `"0"` = VFO A
/// - `"1"` = VFO B
///
/// Returns `true` if VFO B is selected.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is empty or not a valid VFO state.
pub fn parse_vfo_response(data: &str) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected VFO state digit, got empty data".into(),
        ));
    }
    match data {
        "0" => Ok(false), // VFO A
        "1" => Ok(true),  // VFO B
        _ => Err(Error::Protocol(format!("unexpected VFO state: {data:?}"))),
    }
}

// ---------------------------------------------------------------
// Mode conversion helpers
// ---------------------------------------------------------------

/// Convert a generic [`Mode`] to the Kenwood CAT mode code string.
///
/// Data modes are mapped to the nearest base mode since most Kenwood rigs
/// do not have separate mode codes for data sub-modes in the `MD` command.
fn mode_to_kenwood(mode: &Mode) -> &'static str {
    match mode {
        Mode::LSB => KENWOOD_MODE_LSB,
        Mode::USB => KENWOOD_MODE_USB,
        Mode::CW => KENWOOD_MODE_CW,
        Mode::CWR => KENWOOD_MODE_CWR,
        Mode::AM => KENWOOD_MODE_AM,
        Mode::FM => KENWOOD_MODE_FM,
        Mode::RTTY => KENWOOD_MODE_RTTY,
        Mode::RTTYR => KENWOOD_MODE_RTTYR,
        // Data modes map to base sideband modes.
        Mode::DataUSB => KENWOOD_MODE_USB,
        Mode::DataLSB => KENWOOD_MODE_LSB,
        Mode::DataFM => KENWOOD_MODE_FM,
        Mode::DataAM => KENWOOD_MODE_AM,
    }
}

/// Convert a Kenwood CAT mode code string to a generic [`Mode`].
///
/// Kenwood mode codes are single-digit characters:
/// - `1` = LSB, `2` = USB, `3` = CW, `4` = FM, `5` = AM
/// - `6` = FSK (RTTY), `7` = CW-R, `9` = FSK-R (RTTYR)
///
/// Note: codes `8` and higher than `9` are not standard on most Kenwood rigs.
fn kenwood_to_mode(code: &str) -> Result<Mode> {
    match code {
        "1" => Ok(Mode::LSB),
        "2" => Ok(Mode::USB),
        "3" => Ok(Mode::CW),
        "4" => Ok(Mode::FM),
        "5" => Ok(Mode::AM),
        "6" => Ok(Mode::RTTY),
        "7" => Ok(Mode::CWR),
        "9" => Ok(Mode::RTTYR),
        _ => Err(Error::Protocol(format!(
            "unknown Kenwood mode code: {code:?}"
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
        assert_eq!(cmd, b"FA00014250000;");
    }

    #[test]
    fn cmd_set_frequency_a_7000() {
        let cmd = cmd_set_frequency_a(7_000_000);
        assert_eq!(cmd, b"FA00007000000;");
    }

    #[test]
    fn cmd_set_frequency_a_zero_padded() {
        let cmd = cmd_set_frequency_a(1_800_000);
        assert_eq!(cmd, b"FA00001800000;");
    }

    #[test]
    fn cmd_set_frequency_b_28500() {
        let cmd = cmd_set_frequency_b(28_500_000);
        assert_eq!(cmd, b"FB00028500000;");
    }

    #[test]
    fn cmd_set_frequency_a_50mhz() {
        let cmd = cmd_set_frequency_a(50_100_000);
        assert_eq!(cmd, b"FA00050100000;");
    }

    #[test]
    fn cmd_read_mode_bytes() {
        assert_eq!(cmd_read_mode(), b"MD;");
    }

    #[test]
    fn cmd_set_mode_usb() {
        let cmd = cmd_set_mode(&Mode::USB);
        assert_eq!(cmd, b"MD2;");
    }

    #[test]
    fn cmd_set_mode_lsb() {
        let cmd = cmd_set_mode(&Mode::LSB);
        assert_eq!(cmd, b"MD1;");
    }

    #[test]
    fn cmd_set_mode_cw() {
        let cmd = cmd_set_mode(&Mode::CW);
        assert_eq!(cmd, b"MD3;");
    }

    #[test]
    fn cmd_set_mode_cwr() {
        let cmd = cmd_set_mode(&Mode::CWR);
        assert_eq!(cmd, b"MD7;");
    }

    #[test]
    fn cmd_set_mode_fm() {
        let cmd = cmd_set_mode(&Mode::FM);
        assert_eq!(cmd, b"MD4;");
    }

    #[test]
    fn cmd_set_mode_am() {
        let cmd = cmd_set_mode(&Mode::AM);
        assert_eq!(cmd, b"MD5;");
    }

    #[test]
    fn cmd_set_mode_rtty() {
        let cmd = cmd_set_mode(&Mode::RTTY);
        assert_eq!(cmd, b"MD6;");
    }

    #[test]
    fn cmd_set_mode_rttyr() {
        let cmd = cmd_set_mode(&Mode::RTTYR);
        assert_eq!(cmd, b"MD9;");
    }

    #[test]
    fn cmd_set_mode_data_usb() {
        // DataUSB maps to base USB (code 2) on Kenwood
        let cmd = cmd_set_mode(&Mode::DataUSB);
        assert_eq!(cmd, b"MD2;");
    }

    #[test]
    fn cmd_set_mode_data_lsb() {
        // DataLSB maps to base LSB (code 1) on Kenwood
        let cmd = cmd_set_mode(&Mode::DataLSB);
        assert_eq!(cmd, b"MD1;");
    }

    #[test]
    fn cmd_set_mode_data_fm() {
        // DataFM maps to base FM (code 4) on Kenwood
        let cmd = cmd_set_mode(&Mode::DataFM);
        assert_eq!(cmd, b"MD4;");
    }

    #[test]
    fn cmd_set_mode_data_am() {
        // DataAM maps to base AM (code 5) on Kenwood
        let cmd = cmd_set_mode(&Mode::DataAM);
        assert_eq!(cmd, b"MD5;");
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
    fn cmd_force_receive_bytes() {
        assert_eq!(cmd_force_receive(), b"RX;");
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
        assert_eq!(cmd_read_alc(), b"RM2;");
    }

    #[test]
    fn cmd_read_rx_vfo_bytes() {
        assert_eq!(cmd_read_rx_vfo(), b"FR;");
    }

    #[test]
    fn cmd_read_tx_vfo_bytes() {
        assert_eq!(cmd_read_tx_vfo(), b"FT;");
    }

    #[test]
    fn cmd_set_split_on() {
        let (fr, ft) = cmd_set_split(true);
        assert_eq!(fr, b"FR0;");
        assert_eq!(ft, b"FT1;");
    }

    #[test]
    fn cmd_set_split_off() {
        let (fr, ft) = cmd_set_split(false);
        assert_eq!(fr, b"FR0;");
        assert_eq!(ft, b"FT0;");
    }

    #[test]
    fn cmd_set_tx_vfo_a() {
        assert_eq!(cmd_set_tx_vfo(false), b"FT0;");
    }

    #[test]
    fn cmd_set_tx_vfo_b() {
        assert_eq!(cmd_set_tx_vfo(true), b"FT1;");
    }

    #[test]
    fn cmd_set_ai_on() {
        assert_eq!(cmd_set_ai(true), b"AI2;");
    }

    #[test]
    fn cmd_set_ai_off() {
        assert_eq!(cmd_set_ai(false), b"AI0;");
    }

    #[test]
    fn cmd_read_passband_bytes() {
        assert_eq!(cmd_read_passband(), b"SH;");
    }

    #[test]
    fn cmd_set_passband_bytes() {
        assert_eq!(cmd_set_passband(12), b"SH12;");
    }

    // ---------------------------------------------------------------
    // Response parsing -- frequencies (11-digit Kenwood format)
    // ---------------------------------------------------------------

    #[test]
    fn parse_freq_14250() {
        let freq = parse_frequency_response("00014250000").unwrap();
        assert_eq!(freq, 14_250_000);
    }

    #[test]
    fn parse_freq_7000() {
        let freq = parse_frequency_response("00007000000").unwrap();
        assert_eq!(freq, 7_000_000);
    }

    #[test]
    fn parse_freq_1800() {
        let freq = parse_frequency_response("00001800000").unwrap();
        assert_eq!(freq, 1_800_000);
    }

    #[test]
    fn parse_freq_50100() {
        let freq = parse_frequency_response("00050100000").unwrap();
        assert_eq!(freq, 50_100_000);
    }

    #[test]
    fn parse_freq_28500() {
        let freq = parse_frequency_response("00028500000").unwrap();
        assert_eq!(freq, 28_500_000);
    }

    #[test]
    fn parse_freq_max_11_digits() {
        let freq = parse_frequency_response("99999999999").unwrap();
        assert_eq!(freq, 99_999_999_999);
    }

    #[test]
    fn parse_freq_zero() {
        let freq = parse_frequency_response("00000000000").unwrap();
        assert_eq!(freq, 0);
    }

    #[test]
    fn parse_freq_wrong_length_short() {
        assert!(parse_frequency_response("0014250000").is_err());
    }

    #[test]
    fn parse_freq_wrong_length_long() {
        assert!(parse_frequency_response("000014250000").is_err());
    }

    #[test]
    fn parse_freq_9_digits_wrong() {
        // Yaesu format (9 digits) should fail for Kenwood (11 digits)
        assert!(parse_frequency_response("014250000").is_err());
    }

    #[test]
    fn parse_freq_empty() {
        assert!(parse_frequency_response("").is_err());
    }

    #[test]
    fn parse_freq_non_digit() {
        assert!(parse_frequency_response("0001425000A").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing -- modes
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
    fn parse_mode_rttyr() {
        assert_eq!(parse_mode_response("9").unwrap(), Mode::RTTYR);
    }

    #[test]
    fn parse_mode_unknown_8() {
        assert!(parse_mode_response("8").is_err());
    }

    #[test]
    fn parse_mode_unknown() {
        assert!(parse_mode_response("A").is_err());
    }

    #[test]
    fn parse_mode_empty() {
        assert!(parse_mode_response("").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing -- PTT
    // ---------------------------------------------------------------

    #[test]
    fn parse_ptt_off() {
        assert!(!parse_ptt_response("0").unwrap());
    }

    #[test]
    fn parse_ptt_on() {
        assert!(parse_ptt_response("1").unwrap());
    }

    #[test]
    fn parse_ptt_empty() {
        assert!(parse_ptt_response("").is_err());
    }

    #[test]
    fn parse_ptt_invalid() {
        assert!(parse_ptt_response("2").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing -- meters (selector + 4 digits)
    // ---------------------------------------------------------------

    #[test]
    fn parse_meter_zero() {
        assert_eq!(parse_meter_response("00000").unwrap(), 0);
    }

    #[test]
    fn parse_meter_s9() {
        // S-meter: SM00015; -> data "00015", value 0015
        assert_eq!(parse_meter_response("00015").unwrap(), 15);
    }

    #[test]
    fn parse_meter_s9_plus_30() {
        // SM00025; -> data "00025", value 0025
        assert_eq!(parse_meter_response("00025").unwrap(), 25);
    }

    #[test]
    fn parse_meter_swr() {
        // RM10045; -> data "10045", selector 1, value 0045
        assert_eq!(parse_meter_response("10045").unwrap(), 45);
    }

    #[test]
    fn parse_meter_alc() {
        // RM20100; -> data "20100", selector 2, value 0100
        assert_eq!(parse_meter_response("20100").unwrap(), 100);
    }

    #[test]
    fn parse_meter_wrong_length_short() {
        assert!(parse_meter_response("012").is_err());
    }

    #[test]
    fn parse_meter_wrong_length_long() {
        assert!(parse_meter_response("001234").is_err());
    }

    #[test]
    fn parse_meter_non_digit() {
        assert!(parse_meter_response("0123A").is_err());
    }

    #[test]
    fn parse_meter_empty() {
        assert!(parse_meter_response("").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing -- power
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
    // Response parsing -- VFO select
    // ---------------------------------------------------------------

    #[test]
    fn parse_vfo_a() {
        assert!(!parse_vfo_response("0").unwrap());
    }

    #[test]
    fn parse_vfo_b() {
        assert!(parse_vfo_response("1").unwrap());
    }

    #[test]
    fn parse_vfo_empty() {
        assert!(parse_vfo_response("").is_err());
    }

    #[test]
    fn parse_vfo_invalid() {
        assert!(parse_vfo_response("2").is_err());
    }

    // ---------------------------------------------------------------
    // Mode round-trip: base modes go Mode -> Kenwood code -> Mode
    // ---------------------------------------------------------------

    #[test]
    fn mode_round_trip_base_modes() {
        // Only the 8 base modes have a perfect round-trip. Data modes
        // are lossy (mapped to base mode on set, read back as base).
        let modes = [
            Mode::LSB,
            Mode::USB,
            Mode::CW,
            Mode::CWR,
            Mode::AM,
            Mode::FM,
            Mode::RTTY,
            Mode::RTTYR,
        ];
        for mode in modes {
            let code = mode_to_kenwood(&mode);
            let parsed = kenwood_to_mode(code).unwrap();
            assert_eq!(mode, parsed, "round-trip failed for {mode}");
        }
    }

    #[test]
    fn data_modes_map_to_base() {
        // DataUSB -> "2" (USB), DataLSB -> "1" (LSB), etc.
        assert_eq!(mode_to_kenwood(&Mode::DataUSB), "2");
        assert_eq!(mode_to_kenwood(&Mode::DataLSB), "1");
        assert_eq!(mode_to_kenwood(&Mode::DataFM), "4");
        assert_eq!(mode_to_kenwood(&Mode::DataAM), "5");

        // Parsing those codes back yields the base mode, not the data variant
        assert_eq!(kenwood_to_mode("2").unwrap(), Mode::USB);
        assert_eq!(kenwood_to_mode("1").unwrap(), Mode::LSB);
        assert_eq!(kenwood_to_mode("4").unwrap(), Mode::FM);
        assert_eq!(kenwood_to_mode("5").unwrap(), Mode::AM);
    }

    // ---------------------------------------------------------------
    // Boundary / edge-case command tests
    // ---------------------------------------------------------------

    #[test]
    fn cmd_set_frequency_a_max_11_digits() {
        let cmd = cmd_set_frequency_a(99_999_999_999);
        assert_eq!(cmd, b"FA99999999999;");
    }

    #[test]
    fn cmd_set_frequency_a_zero() {
        let cmd = cmd_set_frequency_a(0);
        assert_eq!(cmd, b"FA00000000000;");
    }

    #[test]
    fn cmd_set_frequency_a_ft8_14074() {
        let cmd = cmd_set_frequency_a(14_074_000);
        assert_eq!(cmd, b"FA00014074000;");
    }

    #[test]
    fn cmd_set_power_max_3_digits() {
        let cmd = cmd_set_power(999);
        assert_eq!(cmd, b"PC999;");
    }
}
