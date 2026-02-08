//! Elecraft CAT command builders and response parsers.
//!
//! This module provides functions to construct CAT command byte sequences for
//! common transceiver operations (frequency, mode, PTT, metering, split, power,
//! bandwidth) and to parse the corresponding responses from the rig.
//!
//! All functions are pure -- they produce or consume byte vectors / string slices
//! without performing any I/O. The caller is responsible for sending the bytes
//! over a transport and feeding received data back into the response parsers.
//!
//! # Elecraft CAT command reference
//!
//! Based on the Elecraft K3/K3S/KX3/KX2/K4 programmer's reference. Elecraft
//! radios use an extended Kenwood text protocol. Frequencies are always 11 ASCII
//! digits in hertz, zero-padded on the left. Mode codes are single-digit
//! characters (`1`-`9`).
//!
//! # Elecraft-specific commands
//!
//! - `BW` -- IF bandwidth in Hz (K4 primary, also works on K3 for reading)
//! - `FW` -- Filter width in Hz (K3/K3S/KX3/KX2 primary bandwidth command)
//! - `K3;` / `K4;` -- Model identification
//! - `KS` -- Keyer speed
//! - `AP` -- Audio peaking filter
//! - `DS` -- Display read
//! - `DT` -- Data sub-mode

use riglib_core::{Error, Mode, Result};

use crate::protocol::encode_command;

// ---------------------------------------------------------------
// Elecraft mode code mapping
// ---------------------------------------------------------------

/// Elecraft CAT mode code for LSB.
const ELECRAFT_MODE_LSB: &str = "1";
/// Elecraft CAT mode code for USB.
const ELECRAFT_MODE_USB: &str = "2";
/// Elecraft CAT mode code for CW.
const ELECRAFT_MODE_CW: &str = "3";
/// Elecraft CAT mode code for FM.
const ELECRAFT_MODE_FM: &str = "4";
/// Elecraft CAT mode code for AM.
const ELECRAFT_MODE_AM: &str = "5";
/// Elecraft CAT mode code for DATA (data mode on current sideband).
const ELECRAFT_MODE_DATA: &str = "6";
/// Elecraft CAT mode code for CW-R (reverse beat).
const ELECRAFT_MODE_CWR: &str = "7";
/// Elecraft CAT mode code for DATA-R (data mode reverse sideband).
const ELECRAFT_MODE_DATAR: &str = "9";

// ---------------------------------------------------------------
// Command builders -- Kenwood-compatible base commands
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
/// Maps the generic [`Mode`] enum to the corresponding Elecraft CAT mode code.
///
/// Unlike standard Kenwood, Elecraft uses mode 6 (DATA) for digital modes
/// and mode 9 (DATA-R) for reverse-sideband digital modes. This provides
/// proper round-trip mapping for DataUSB/DataLSB and RTTY/RTTYR.
///
/// # Arguments
///
/// * `mode` - The operating mode to set.
pub fn cmd_set_mode(mode: &Mode) -> Vec<u8> {
    let code = mode_to_elecraft(mode);
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
/// Elecraft rigs accept `RX;` as an alternative to `TX0;` to force a return
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
/// * `watts` - Output power in watts (0-999, though most rigs cap at 100).
pub fn cmd_set_power(watts: u16) -> Vec<u8> {
    encode_command("PC", &format!("{watts:03}"))
}

/// Build a "read S-meter" command (`SM0;`).
///
/// The `0` parameter selects the main receiver meter. The response returns
/// `SM0nnnn;` where `nnnn` is a 4-digit value.
pub fn cmd_read_s_meter() -> Vec<u8> {
    encode_command("SM", "0")
}

/// Build a "read SWR meter" command (`RM1;`).
///
/// Uses `RM1` for the SWR meter. Only meaningful while transmitting.
/// Response is `RM1nnnn;` where `nnnn` is a 4-digit value.
pub fn cmd_read_swr() -> Vec<u8> {
    encode_command("RM", "1")
}

/// Build a "read ALC meter" command (`RM2;`).
///
/// Uses `RM2` for the ALC meter. Only meaningful while transmitting.
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

/// Build a "read CW keyer speed" command (`KS;`).
///
/// Response is `KS{speed:03};` where speed is 008-060 WPM.
pub fn cmd_read_cw_speed() -> Vec<u8> {
    encode_command("KS", "")
}

/// Build a "set CW keyer speed" command (`KS{speed:03};`).
///
/// Speed is encoded as exactly 3 zero-padded ASCII digits in WPM.
///
/// # Arguments
///
/// * `wpm` - Keyer speed in words per minute (typically 8-60).
pub fn cmd_set_cw_speed(wpm: u8) -> Vec<u8> {
    encode_command("KS", &format!("{wpm:03}"))
}

/// Build a "VFO A=B" command (`SWT11;`).
///
/// On Elecraft K3/K4, `SWT11;` triggers the A=B function (copies the
/// active VFO to the inactive VFO). This is equivalent to pressing the
/// A=B button on the front panel.
pub fn cmd_vfo_a_eq_b() -> Vec<u8> {
    encode_command("SWT", "11")
}

/// Build a "VFO swap" command (`SWT00;`).
///
/// On Elecraft K3/K4, `SWT00;` triggers the A/B swap function (exchanges
/// VFO A and VFO B). This is equivalent to pressing the A/B button.
pub fn cmd_vfo_swap() -> Vec<u8> {
    encode_command("SWT", "00")
}

// ---------------------------------------------------------------
// Command builders -- AGC
// ---------------------------------------------------------------

/// Build a "read AGC" command for K3-family (`GT;`).
///
/// Response is `GTnnnx;` where:
/// - `nnn` = 3-digit speed value (002=fast, 004=slow)
/// - `x` = AGC on/off flag (0=off, 1=on) in K22 extended mode
///
/// On non-K22 firmware, the response may be `GTnnn;` (3 digits only).
pub fn cmd_read_agc_k3() -> Vec<u8> {
    encode_command("GT", "")
}

/// Build a "set AGC" command for K3-family (`GTnnnx;`).
///
/// Uses K22 extended format with AGC on/off flag.
///
/// # Arguments
///
/// * `speed` - Speed value: 002=fast, 004=slow
/// * `agc_on` - `true` for AGC on, `false` for AGC off
pub fn cmd_set_agc_k3(speed: u16, agc_on: bool) -> Vec<u8> {
    let on_off = if agc_on { "1" } else { "0" };
    encode_command("GT", &format!("{speed:03}{on_off}"))
}

/// Build a "read AGC" command for K4 (`GT$;`).
///
/// Response is `GT$n;` where n is: 0=Off, 1=Slow, 2=Fast.
pub fn cmd_read_agc_k4() -> Vec<u8> {
    encode_command("GT", "$")
}

/// Build a "set AGC" command for K4 (`GT${value};`).
///
/// # Arguments
///
/// * `value` - AGC mode: 0=Off, 1=Slow, 2=Fast
pub fn cmd_set_agc_k4(value: u8) -> Vec<u8> {
    encode_command("GT", &format!("${value}"))
}

// ---------------------------------------------------------------
// Command builders -- Preamp / Attenuator
// ---------------------------------------------------------------

/// Build a "read preamp" command (`PA;`).
///
/// Reads the current preamp setting. Response is `PA{level};` where level
/// is: 0=Off, 1=On.
pub fn cmd_read_preamp() -> Vec<u8> {
    encode_command("PA", "")
}

/// Build a "set preamp" command (`PA{level};`).
///
/// # Arguments
///
/// * `level` - Preamp level: 0=Off, 1=On
pub fn cmd_set_preamp(level: u8) -> Vec<u8> {
    encode_command("PA", &format!("{level}"))
}

/// Build a "read attenuator" command for K3-family (`RA;`).
///
/// Response is `RA{level:02};` where level is a 2-digit value.
pub fn cmd_read_attenuator_k3() -> Vec<u8> {
    encode_command("RA", "")
}

/// Build a "set attenuator" command for K3-family (`RA{level:02};`).
///
/// # Arguments
///
/// * `level` - Attenuator level as a 2-digit value (e.g. 0=Off, model-dependent)
pub fn cmd_set_attenuator_k3(level: u8) -> Vec<u8> {
    encode_command("RA", &format!("{level:02}"))
}

/// Build a "read attenuator" command for K4 (`RA$;`).
///
/// Response is `RA${level};` where level is a single digit.
pub fn cmd_read_attenuator_k4() -> Vec<u8> {
    encode_command("RA", "$")
}

/// Build a "set attenuator" command for K4 (`RA${level};`).
///
/// # Arguments
///
/// * `level` - Attenuator level as a single digit value
pub fn cmd_set_attenuator_k4(level: u8) -> Vec<u8> {
    encode_command("RA", &format!("${level}"))
}

// ---------------------------------------------------------------
// Command builders -- RIT / XIT
// ---------------------------------------------------------------

/// Build a "read RIT state" command (`RT;`).
///
/// Response is `RT0;` (RIT off) or `RT1;` (RIT on).
pub fn cmd_read_rit() -> Vec<u8> {
    encode_command("RT", "")
}

/// Build a "set RIT on/off" command.
///
/// - `RT1;` enables RIT.
/// - `RT0;` disables RIT.
///
/// # Arguments
///
/// * `on` - `true` to enable RIT, `false` to disable.
pub fn cmd_set_rit_on(on: bool) -> Vec<u8> {
    if on {
        encode_command("RT", "1")
    } else {
        encode_command("RT", "0")
    }
}

/// Build a "read XIT state" command (`XT;`).
///
/// Response is `XT0;` (XIT off) or `XT1;` (XIT on).
pub fn cmd_read_xit() -> Vec<u8> {
    encode_command("XT", "")
}

/// Build a "set XIT on/off" command.
///
/// - `XT1;` enables XIT.
/// - `XT0;` disables XIT.
///
/// # Arguments
///
/// * `on` - `true` to enable XIT, `false` to disable.
pub fn cmd_set_xit_on(on: bool) -> Vec<u8> {
    if on {
        encode_command("XT", "1")
    } else {
        encode_command("XT", "0")
    }
}

/// Build a "read RIT/XIT offset" command (`RO;`).
///
/// Response is `RO{+/-}{offset:05};` where the offset is a signed
/// 5-digit value in hertz. K3/K3S range is +/-9999 Hz.
pub fn cmd_read_rit_xit_offset() -> Vec<u8> {
    encode_command("RO", "")
}

/// Build a "RIT up" command (`RU;`).
///
/// Increments the RIT/XIT offset by one step.
pub fn cmd_rit_up() -> Vec<u8> {
    encode_command("RU", "")
}

/// Build a "RIT down" command (`RD;`).
///
/// Decrements the RIT/XIT offset by one step.
pub fn cmd_rit_down() -> Vec<u8> {
    encode_command("RD", "")
}

/// Build a "RIT clear" command (`RC;`).
///
/// Resets the RIT/XIT offset to zero.
pub fn cmd_rit_clear() -> Vec<u8> {
    encode_command("RC", "")
}

// ---------------------------------------------------------------
// CW message commands
// ---------------------------------------------------------------

/// Build a "send CW message" command (`KY {text};`).
///
/// The text field is a fixed 24-character field, right-padded with spaces.
/// If `text` is longer than 24 characters it is truncated to 24. A single
/// space separator precedes the 24-character payload (total parameter length
/// is 25 characters).
///
/// # Arguments
///
/// * `text` - The CW message text to send (up to 24 characters).
pub fn cmd_send_cw_message(text: &str) -> Vec<u8> {
    let truncated = if text.len() > 24 { &text[..24] } else { text };
    let padded = format!("{truncated:<24}");
    encode_command("KY", &format!(" {padded}"))
}

/// Build a "read CW buffer status" command (`KY;`).
///
/// Queries whether the rig's CW message buffer can accept more text.
/// Response data is `"0"` (buffer ready) or `"1"` (buffer full).
pub fn cmd_read_cw_buffer() -> Vec<u8> {
    encode_command("KY", "")
}

/// Build a "stop CW message" command.
///
/// Sends `KY` followed by 25 spaces (1 separator + 24 space payload),
/// which instructs the rig to stop sending the current CW message.
pub fn cmd_stop_cw_message() -> Vec<u8> {
    encode_command("KY", &" ".repeat(25))
}

// ---------------------------------------------------------------
// Command builders -- Elecraft extensions
// ---------------------------------------------------------------

/// Build a "read bandwidth" command for K4 (`BW;`).
///
/// The K4 uses the `BW` command for IF bandwidth. Response is `BW{hz:04};`
/// where the value is the bandwidth in Hz (4 digits, zero-padded).
pub fn cmd_read_bandwidth_bw() -> Vec<u8> {
    encode_command("BW", "")
}

/// Build a "set bandwidth" command for K4 (`BW{hz:04};`).
///
/// # Arguments
///
/// * `hz` - Bandwidth in hertz (e.g. 500, 2700).
pub fn cmd_set_bandwidth_bw(hz: u32) -> Vec<u8> {
    encode_command("BW", &format!("{hz:04}"))
}

/// Build a "read filter width" command for K3/K3S/KX3/KX2 (`FW;`).
///
/// The K3-family uses the `FW` command for filter bandwidth. Response is
/// `FW{hz:04};` where the value is the bandwidth in Hz (4 digits).
pub fn cmd_read_bandwidth_fw() -> Vec<u8> {
    encode_command("FW", "")
}

/// Build a "set filter width" command for K3/K3S/KX3/KX2 (`FW{hz:04};`).
///
/// # Arguments
///
/// * `hz` - Filter width in hertz (e.g. 500, 2700).
pub fn cmd_set_bandwidth_fw(hz: u32) -> Vec<u8> {
    encode_command("FW", &format!("{hz:04}"))
}

/// Build an Elecraft identification command (`K3;`).
///
/// Sends `K3;` which returns `K3` on K3/K3S/KX3/KX2. A K4 will return
/// `?;` to this command. Use with [`cmd_identify_k4()`] for auto-detection.
pub fn cmd_identify_k3() -> Vec<u8> {
    encode_command("K3", "")
}

/// Build an Elecraft K4 identification command (`K4;`).
///
/// Sends `K4;` which returns `K4` on K4 models. A K3/KX3/KX2 will return
/// `?;` to this command.
pub fn cmd_identify_k4() -> Vec<u8> {
    encode_command("K4", "")
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
/// Expects a single character (digit `1`-`9`) representing the Elecraft
/// mode code.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the mode code is unrecognised.
pub fn parse_mode_response(data: &str) -> Result<Mode> {
    elecraft_to_mode(data)
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
/// Meter responses include a 1-digit meter selector followed by a 4-digit
/// value. For example, `SM0` response data is `"00015"` (selector `0`,
/// value `0015`). For `RM1`, the data is `"10045"` (selector `1`,
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

/// Parse a CW speed response from the data portion of a `KS` response.
///
/// Expects a 3-character numeric string (008-060) representing speed in WPM.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_cw_speed_response(data: &str) -> Result<u8> {
    if data.len() != 3 {
        return Err(Error::Protocol(format!(
            "expected 3 digits for CW speed, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u8 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid CW speed digits: {data:?} ({e})")))?;
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

/// Parse an AGC response from the data portion of a K3-family `GT` response.
///
/// Handles both 3-character (`GTnnn;`) and 4-character K22 (`GTnnnx;`)
/// formats.
///
/// Returns `(speed, agc_on)`:
/// - `speed` - 3-digit value (002=fast, 004=slow)
/// - `agc_on` - `true` if AGC is on. For non-K22 (3-char) responses,
///   always returns `true` (AGC assumed on since off is not indicated).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_agc_response_k3(data: &str) -> Result<(u16, bool)> {
    if data.len() != 3 && data.len() != 4 {
        return Err(Error::Protocol(format!(
            "expected 3 or 4 characters for K3 AGC response, got {} characters: {data:?}",
            data.len()
        )));
    }
    let speed: u16 = data[0..3].parse().map_err(|e| {
        Error::Protocol(format!("invalid AGC speed value: {:?} ({e})", &data[0..3]))
    })?;
    let agc_on = if data.len() == 4 {
        &data[3..4] == "1"
    } else {
        true
    };
    Ok((speed, agc_on))
}

/// Parse an AGC response from the data portion of a K4 `GT` response.
///
/// Expects `$n` where n is: 0=Off, 1=Slow, 2=Fast.
///
/// Returns the raw mode value (0-2).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_agc_response_k4(data: &str) -> Result<u8> {
    if data.len() != 2 || !data.starts_with('$') {
        return Err(Error::Protocol(format!(
            "expected '$n' for K4 AGC response, got: {data:?}"
        )));
    }
    let val: u8 = data[1..2].parse().map_err(|e| {
        Error::Protocol(format!(
            "invalid K4 AGC mode digit: {:?} ({e})",
            &data[1..2]
        ))
    })?;
    Ok(val)
}

/// Parse a preamp response from the data portion of a `PA` response.
///
/// Expects a single character (digit `0` or `1`) representing the preamp state:
/// 0=Off, 1=On.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_preamp_response(data: &str) -> Result<u8> {
    if data.len() != 1 {
        return Err(Error::Protocol(format!(
            "expected 1 digit for preamp response, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u8 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid preamp level: {data:?} ({e})")))?;
    Ok(val)
}

/// Parse an attenuator response from the data portion of a K3-family `RA` response.
///
/// Expects 2 characters representing the attenuator level.
///
/// Returns the raw level value.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_attenuator_response_k3(data: &str) -> Result<u8> {
    if data.len() != 2 {
        return Err(Error::Protocol(format!(
            "expected 2 characters for K3 attenuator response, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u8 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid K3 attenuator level: {data:?} ({e})")))?;
    Ok(val)
}

/// Parse an attenuator response from the data portion of a K4 `RA` response.
///
/// Expects `$n` where n is a single digit attenuator level.
///
/// Returns the raw level value.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_attenuator_response_k4(data: &str) -> Result<u8> {
    if data.len() != 2 || !data.starts_with('$') {
        return Err(Error::Protocol(format!(
            "expected '$n' for K4 attenuator response, got: {data:?}"
        )));
    }
    let val: u8 = data[1..2].parse().map_err(|e| {
        Error::Protocol(format!(
            "invalid K4 attenuator level: {:?} ({e})",
            &data[1..2]
        ))
    })?;
    Ok(val)
}

/// Parse a bandwidth response from the data portion of a `BW` or `FW` response.
///
/// Elecraft returns bandwidth as a 4-digit Hz value (e.g. `"0500"` = 500 Hz,
/// `"2700"` = 2700 Hz).
///
/// # Arguments
///
/// * `data` - The data field from a decoded `BW` or `FW` response.
///
/// # Returns
///
/// The bandwidth in hertz.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_bandwidth_response(data: &str) -> Result<u32> {
    if data.len() != 4 {
        return Err(Error::Protocol(format!(
            "expected 4 digits for bandwidth, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u32 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid bandwidth digits: {data:?} ({e})")))?;
    Ok(val)
}

/// Parse a RIT response from the data portion of an `RT` response.
///
/// Expects a single character: `"0"` (RIT off) or `"1"` (RIT on).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is empty or not a valid state.
pub fn parse_rit_response(data: &str) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected RIT state digit, got empty data".into(),
        ));
    }
    match data {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(Error::Protocol(format!("unexpected RIT state: {data:?}"))),
    }
}

/// Parse a XIT response from the data portion of an `XT` response.
///
/// Expects a single character: `"0"` (XIT off) or `"1"` (XIT on).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is empty or not a valid state.
pub fn parse_xit_response(data: &str) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected XIT state digit, got empty data".into(),
        ));
    }
    match data {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(Error::Protocol(format!("unexpected XIT state: {data:?}"))),
    }
}

/// Parse a RIT/XIT offset response from the data portion of an `RO` response.
///
/// Elecraft returns the shared RIT/XIT offset as `+XXXXX` or `-XXXXX` where
/// `XXXXX` is a 5-digit absolute offset in hertz. K3/K3S range is +/-9999 Hz.
///
/// After prefix splitting by the protocol decoder, the data portion has the
/// format `+XXXXX` or `-XXXXX` (6 characters total: sign + 5 digits).
///
/// # Returns
///
/// The signed offset in hertz.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data does not match the expected format.
pub fn parse_rit_xit_offset_response(data: &str) -> Result<i32> {
    if data.len() != 6 {
        return Err(Error::Protocol(format!(
            "expected 6 characters for RIT/XIT offset response, got {} characters: {data:?}",
            data.len()
        )));
    }

    let sign = match &data[0..1] {
        "+" => 1i32,
        "-" => -1i32,
        other => {
            return Err(Error::Protocol(format!(
                "expected + or - for RIT/XIT offset sign, got {other:?}"
            )));
        }
    };

    let digits = &data[1..6];
    let abs_offset: i32 = digits
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid RIT/XIT offset digits: {digits:?} ({e})")))?;

    Ok(sign * abs_offset)
}

/// Parse a CW buffer status response from the data portion of a `KY` response.
///
/// The rig responds with a single character indicating buffer state:
/// - `"0"` = buffer can accept more text (ready).
/// - `"1"` = buffer is full.
///
/// Returns `true` if the buffer is ready to accept text, `false` if full.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is empty or contains an unexpected value.
pub fn parse_cw_buffer_response(data: &str) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected CW buffer status digit, got empty data".into(),
        ));
    }
    match data {
        "0" => Ok(true),  // buffer ready
        "1" => Ok(false), // buffer full
        _ => Err(Error::Protocol(format!(
            "unexpected CW buffer status: {data:?}"
        ))),
    }
}

// ---------------------------------------------------------------
// Mode conversion helpers
// ---------------------------------------------------------------

/// Convert a generic [`Mode`] to the Elecraft CAT mode code string.
///
/// Elecraft mode mapping differs from standard Kenwood:
/// - Mode 6 (DATA) is used for digital modes (RTTY, DataUSB)
/// - Mode 9 (DATA-R) is used for reverse digital modes (RTTYR, DataLSB)
/// - DataFM and DataAM map to their base modes (FM=4, AM=5) since
///   Elecraft does not have distinct data sub-modes for FM/AM.
fn mode_to_elecraft(mode: &Mode) -> &'static str {
    match mode {
        Mode::LSB => ELECRAFT_MODE_LSB,
        Mode::USB => ELECRAFT_MODE_USB,
        Mode::CW => ELECRAFT_MODE_CW,
        Mode::CWR => ELECRAFT_MODE_CWR,
        Mode::AM => ELECRAFT_MODE_AM,
        Mode::FM => ELECRAFT_MODE_FM,
        // Elecraft uses mode 6 (DATA) for digital modes on normal sideband.
        Mode::RTTY => ELECRAFT_MODE_DATA,
        Mode::DataUSB => ELECRAFT_MODE_DATA,
        // Elecraft uses mode 9 (DATA-R) for digital modes on reverse sideband.
        Mode::RTTYR => ELECRAFT_MODE_DATAR,
        Mode::DataLSB => ELECRAFT_MODE_DATAR,
        // DataFM and DataAM map to base FM/AM since Elecraft does not
        // distinguish data sub-modes for these.
        Mode::DataFM => ELECRAFT_MODE_FM,
        Mode::DataAM => ELECRAFT_MODE_AM,
    }
}

/// Convert an Elecraft CAT mode code string to a generic [`Mode`].
///
/// Elecraft mode codes are single-digit characters:
/// - `1` = LSB, `2` = USB, `3` = CW, `4` = FM, `5` = AM
/// - `6` = DATA (digital mode, normal sideband) -> DataUSB
/// - `7` = CW-R, `9` = DATA-R (digital mode, reverse sideband) -> DataLSB
///
/// Note: Mode 6 maps to `DataUSB` and mode 9 maps to `DataLSB` rather than
/// `RTTY`/`RTTYR`, since Elecraft uses these modes for all digital modes
/// (including FT8, RTTY, PSK, etc.) through the sound card interface.
fn elecraft_to_mode(code: &str) -> Result<Mode> {
    match code {
        "1" => Ok(Mode::LSB),
        "2" => Ok(Mode::USB),
        "3" => Ok(Mode::CW),
        "4" => Ok(Mode::FM),
        "5" => Ok(Mode::AM),
        "6" => Ok(Mode::DataUSB),
        "7" => Ok(Mode::CWR),
        "9" => Ok(Mode::DataLSB),
        _ => Err(Error::Protocol(format!(
            "unknown Elecraft mode code: {code:?}"
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
        // Elecraft RTTY maps to DATA mode (6)
        let cmd = cmd_set_mode(&Mode::RTTY);
        assert_eq!(cmd, b"MD6;");
    }

    #[test]
    fn cmd_set_mode_rttyr() {
        // Elecraft RTTYR maps to DATA-R mode (9)
        let cmd = cmd_set_mode(&Mode::RTTYR);
        assert_eq!(cmd, b"MD9;");
    }

    #[test]
    fn cmd_set_mode_data_usb() {
        // DataUSB maps to Elecraft DATA mode (6)
        let cmd = cmd_set_mode(&Mode::DataUSB);
        assert_eq!(cmd, b"MD6;");
    }

    #[test]
    fn cmd_set_mode_data_lsb() {
        // DataLSB maps to Elecraft DATA-R mode (9)
        let cmd = cmd_set_mode(&Mode::DataLSB);
        assert_eq!(cmd, b"MD9;");
    }

    #[test]
    fn cmd_set_mode_data_fm() {
        // DataFM maps to base FM (code 4) on Elecraft
        let cmd = cmd_set_mode(&Mode::DataFM);
        assert_eq!(cmd, b"MD4;");
    }

    #[test]
    fn cmd_set_mode_data_am() {
        // DataAM maps to base AM (code 5) on Elecraft
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
    fn cmd_read_cw_speed_bytes() {
        assert_eq!(cmd_read_cw_speed(), b"KS;");
    }

    #[test]
    fn cmd_set_cw_speed_25wpm() {
        assert_eq!(cmd_set_cw_speed(25), b"KS025;");
    }

    #[test]
    fn cmd_set_cw_speed_8wpm() {
        assert_eq!(cmd_set_cw_speed(8), b"KS008;");
    }

    #[test]
    fn cmd_vfo_a_eq_b_bytes() {
        assert_eq!(cmd_vfo_a_eq_b(), b"SWT11;");
    }

    #[test]
    fn cmd_vfo_swap_bytes() {
        assert_eq!(cmd_vfo_swap(), b"SWT00;");
    }

    // ---------------------------------------------------------------
    // AGC commands — K3-family
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_agc_k3_bytes() {
        assert_eq!(cmd_read_agc_k3(), b"GT;");
    }

    #[test]
    fn cmd_set_agc_k3_fast_on() {
        assert_eq!(cmd_set_agc_k3(2, true), b"GT0021;");
    }

    #[test]
    fn cmd_set_agc_k3_slow_on() {
        assert_eq!(cmd_set_agc_k3(4, true), b"GT0041;");
    }

    #[test]
    fn cmd_set_agc_k3_fast_off() {
        assert_eq!(cmd_set_agc_k3(2, false), b"GT0020;");
    }

    // ---------------------------------------------------------------
    // AGC commands — K4
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_agc_k4_bytes() {
        assert_eq!(cmd_read_agc_k4(), b"GT$;");
    }

    #[test]
    fn cmd_set_agc_k4_off() {
        assert_eq!(cmd_set_agc_k4(0), b"GT$0;");
    }

    #[test]
    fn cmd_set_agc_k4_slow() {
        assert_eq!(cmd_set_agc_k4(1), b"GT$1;");
    }

    #[test]
    fn cmd_set_agc_k4_fast() {
        assert_eq!(cmd_set_agc_k4(2), b"GT$2;");
    }

    // ---------------------------------------------------------------
    // Response parsing — AGC
    // ---------------------------------------------------------------

    #[test]
    fn parse_agc_k3_fast_on() {
        let (speed, on) = parse_agc_response_k3("0021").unwrap();
        assert_eq!(speed, 2);
        assert!(on);
    }

    #[test]
    fn parse_agc_k3_slow_on() {
        let (speed, on) = parse_agc_response_k3("0041").unwrap();
        assert_eq!(speed, 4);
        assert!(on);
    }

    #[test]
    fn parse_agc_k3_fast_off() {
        let (speed, on) = parse_agc_response_k3("0020").unwrap();
        assert_eq!(speed, 2);
        assert!(!on);
    }

    #[test]
    fn parse_agc_k3_non_k22() {
        // Non-K22 firmware: 3 digits only, AGC assumed on
        let (speed, on) = parse_agc_response_k3("002").unwrap();
        assert_eq!(speed, 2);
        assert!(on);
    }

    #[test]
    fn parse_agc_k3_wrong_length() {
        assert!(parse_agc_response_k3("00").is_err());
        assert!(parse_agc_response_k3("00211").is_err());
    }

    #[test]
    fn parse_agc_k4_off() {
        assert_eq!(parse_agc_response_k4("$0").unwrap(), 0);
    }

    #[test]
    fn parse_agc_k4_slow() {
        assert_eq!(parse_agc_response_k4("$1").unwrap(), 1);
    }

    #[test]
    fn parse_agc_k4_fast() {
        assert_eq!(parse_agc_response_k4("$2").unwrap(), 2);
    }

    #[test]
    fn parse_agc_k4_missing_dollar() {
        assert!(parse_agc_response_k4("02").is_err());
    }

    #[test]
    fn parse_agc_k4_wrong_length() {
        assert!(parse_agc_response_k4("$").is_err());
        assert!(parse_agc_response_k4("$01").is_err());
    }

    // ---------------------------------------------------------------
    // Preamp commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_preamp_bytes() {
        assert_eq!(cmd_read_preamp(), b"PA;");
    }

    #[test]
    fn cmd_set_preamp_off() {
        assert_eq!(cmd_set_preamp(0), b"PA0;");
    }

    #[test]
    fn cmd_set_preamp_on() {
        assert_eq!(cmd_set_preamp(1), b"PA1;");
    }

    #[test]
    fn parse_preamp_off() {
        assert_eq!(parse_preamp_response("0").unwrap(), 0);
    }

    #[test]
    fn parse_preamp_on() {
        assert_eq!(parse_preamp_response("1").unwrap(), 1);
    }

    #[test]
    fn parse_preamp_wrong_length() {
        assert!(parse_preamp_response("").is_err());
        assert!(parse_preamp_response("01").is_err());
    }

    // ---------------------------------------------------------------
    // Attenuator commands — K3-family
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_attenuator_k3_bytes() {
        assert_eq!(cmd_read_attenuator_k3(), b"RA;");
    }

    #[test]
    fn cmd_set_attenuator_k3_off() {
        assert_eq!(cmd_set_attenuator_k3(0), b"RA00;");
    }

    #[test]
    fn cmd_set_attenuator_k3_on() {
        assert_eq!(cmd_set_attenuator_k3(10), b"RA10;");
    }

    #[test]
    fn parse_attenuator_k3_off() {
        assert_eq!(parse_attenuator_response_k3("00").unwrap(), 0);
    }

    #[test]
    fn parse_attenuator_k3_on() {
        assert_eq!(parse_attenuator_response_k3("10").unwrap(), 10);
    }

    #[test]
    fn parse_attenuator_k3_wrong_length() {
        assert!(parse_attenuator_response_k3("0").is_err());
        assert!(parse_attenuator_response_k3("001").is_err());
    }

    // ---------------------------------------------------------------
    // Attenuator commands — K4
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_attenuator_k4_bytes() {
        assert_eq!(cmd_read_attenuator_k4(), b"RA$;");
    }

    #[test]
    fn cmd_set_attenuator_k4_off() {
        assert_eq!(cmd_set_attenuator_k4(0), b"RA$0;");
    }

    #[test]
    fn cmd_set_attenuator_k4_on() {
        assert_eq!(cmd_set_attenuator_k4(1), b"RA$1;");
    }

    #[test]
    fn parse_attenuator_k4_off() {
        assert_eq!(parse_attenuator_response_k4("$0").unwrap(), 0);
    }

    #[test]
    fn parse_attenuator_k4_on() {
        assert_eq!(parse_attenuator_response_k4("$1").unwrap(), 1);
    }

    #[test]
    fn parse_attenuator_k4_missing_dollar() {
        assert!(parse_attenuator_response_k4("01").is_err());
    }

    #[test]
    fn parse_attenuator_k4_wrong_length() {
        assert!(parse_attenuator_response_k4("$").is_err());
        assert!(parse_attenuator_response_k4("$01").is_err());
    }

    // ---------------------------------------------------------------
    // Elecraft-specific command builders
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_bandwidth_bw_bytes() {
        assert_eq!(cmd_read_bandwidth_bw(), b"BW;");
    }

    #[test]
    fn cmd_set_bandwidth_bw_500() {
        assert_eq!(cmd_set_bandwidth_bw(500), b"BW0500;");
    }

    #[test]
    fn cmd_set_bandwidth_bw_2700() {
        assert_eq!(cmd_set_bandwidth_bw(2700), b"BW2700;");
    }

    #[test]
    fn cmd_read_bandwidth_fw_bytes() {
        assert_eq!(cmd_read_bandwidth_fw(), b"FW;");
    }

    #[test]
    fn cmd_set_bandwidth_fw_500() {
        assert_eq!(cmd_set_bandwidth_fw(500), b"FW0500;");
    }

    #[test]
    fn cmd_set_bandwidth_fw_2700() {
        assert_eq!(cmd_set_bandwidth_fw(2700), b"FW2700;");
    }

    #[test]
    fn cmd_identify_k3_bytes() {
        assert_eq!(cmd_identify_k3(), b"K3;");
    }

    #[test]
    fn cmd_identify_k4_bytes() {
        assert_eq!(cmd_identify_k4(), b"K4;");
    }

    // ---------------------------------------------------------------
    // Response parsing -- frequencies (11-digit format)
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
    fn parse_mode_data() {
        // Mode 6 = DATA -> DataUSB on Elecraft
        assert_eq!(parse_mode_response("6").unwrap(), Mode::DataUSB);
    }

    #[test]
    fn parse_mode_cwr() {
        assert_eq!(parse_mode_response("7").unwrap(), Mode::CWR);
    }

    #[test]
    fn parse_mode_datar() {
        // Mode 9 = DATA-R -> DataLSB on Elecraft
        assert_eq!(parse_mode_response("9").unwrap(), Mode::DataLSB);
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
        assert_eq!(parse_meter_response("00015").unwrap(), 15);
    }

    #[test]
    fn parse_meter_s9_plus_30() {
        assert_eq!(parse_meter_response("00025").unwrap(), 25);
    }

    #[test]
    fn parse_meter_swr() {
        assert_eq!(parse_meter_response("10045").unwrap(), 45);
    }

    #[test]
    fn parse_meter_alc() {
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
    // Response parsing -- CW speed
    // ---------------------------------------------------------------

    #[test]
    fn parse_cw_speed_25() {
        assert_eq!(parse_cw_speed_response("025").unwrap(), 25);
    }

    #[test]
    fn parse_cw_speed_8() {
        assert_eq!(parse_cw_speed_response("008").unwrap(), 8);
    }

    #[test]
    fn parse_cw_speed_max() {
        assert_eq!(parse_cw_speed_response("060").unwrap(), 60);
    }

    #[test]
    fn parse_cw_speed_wrong_length() {
        assert!(parse_cw_speed_response("25").is_err());
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
    // Response parsing -- bandwidth
    // ---------------------------------------------------------------

    #[test]
    fn parse_bandwidth_500() {
        assert_eq!(parse_bandwidth_response("0500").unwrap(), 500);
    }

    #[test]
    fn parse_bandwidth_2700() {
        assert_eq!(parse_bandwidth_response("2700").unwrap(), 2700);
    }

    #[test]
    fn parse_bandwidth_100() {
        assert_eq!(parse_bandwidth_response("0100").unwrap(), 100);
    }

    #[test]
    fn parse_bandwidth_6000() {
        assert_eq!(parse_bandwidth_response("6000").unwrap(), 6000);
    }

    #[test]
    fn parse_bandwidth_zero() {
        assert_eq!(parse_bandwidth_response("0000").unwrap(), 0);
    }

    #[test]
    fn parse_bandwidth_wrong_length() {
        assert!(parse_bandwidth_response("500").is_err());
        assert!(parse_bandwidth_response("05000").is_err());
    }

    #[test]
    fn parse_bandwidth_non_digit() {
        assert!(parse_bandwidth_response("050A").is_err());
    }

    #[test]
    fn parse_bandwidth_empty() {
        assert!(parse_bandwidth_response("").is_err());
    }

    // ---------------------------------------------------------------
    // RIT/XIT command builders
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_rit_bytes() {
        assert_eq!(cmd_read_rit(), b"RT;");
    }

    #[test]
    fn cmd_set_rit_on_bytes() {
        assert_eq!(cmd_set_rit_on(true), b"RT1;");
    }

    #[test]
    fn cmd_set_rit_off_bytes() {
        assert_eq!(cmd_set_rit_on(false), b"RT0;");
    }

    #[test]
    fn cmd_read_xit_bytes() {
        assert_eq!(cmd_read_xit(), b"XT;");
    }

    #[test]
    fn cmd_set_xit_on_bytes() {
        assert_eq!(cmd_set_xit_on(true), b"XT1;");
    }

    #[test]
    fn cmd_set_xit_off_bytes() {
        assert_eq!(cmd_set_xit_on(false), b"XT0;");
    }

    #[test]
    fn cmd_read_rit_xit_offset_bytes() {
        assert_eq!(cmd_read_rit_xit_offset(), b"RO;");
    }

    #[test]
    fn cmd_rit_up_bytes() {
        assert_eq!(cmd_rit_up(), b"RU;");
    }

    #[test]
    fn cmd_rit_down_bytes() {
        assert_eq!(cmd_rit_down(), b"RD;");
    }

    #[test]
    fn cmd_rit_clear_bytes() {
        assert_eq!(cmd_rit_clear(), b"RC;");
    }

    // ---------------------------------------------------------------
    // Response parsing -- RIT state
    // ---------------------------------------------------------------

    #[test]
    fn parse_rit_off() {
        assert!(!parse_rit_response("0").unwrap());
    }

    #[test]
    fn parse_rit_on() {
        assert!(parse_rit_response("1").unwrap());
    }

    #[test]
    fn parse_rit_empty() {
        assert!(parse_rit_response("").is_err());
    }

    #[test]
    fn parse_rit_invalid() {
        assert!(parse_rit_response("2").is_err());
    }

    #[test]
    fn parse_rit_too_long() {
        assert!(parse_rit_response("01").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing -- XIT state
    // ---------------------------------------------------------------

    #[test]
    fn parse_xit_off() {
        assert!(!parse_xit_response("0").unwrap());
    }

    #[test]
    fn parse_xit_on() {
        assert!(parse_xit_response("1").unwrap());
    }

    #[test]
    fn parse_xit_empty() {
        assert!(parse_xit_response("").is_err());
    }

    #[test]
    fn parse_xit_invalid() {
        assert!(parse_xit_response("2").is_err());
    }

    #[test]
    fn parse_xit_too_long() {
        assert!(parse_xit_response("01").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing -- RIT/XIT offset
    // ---------------------------------------------------------------

    #[test]
    fn parse_offset_positive() {
        assert_eq!(parse_rit_xit_offset_response("+00050").unwrap(), 50);
    }

    #[test]
    fn parse_offset_negative() {
        assert_eq!(parse_rit_xit_offset_response("-00050").unwrap(), -50);
    }

    #[test]
    fn parse_offset_zero_positive() {
        assert_eq!(parse_rit_xit_offset_response("+00000").unwrap(), 0);
    }

    #[test]
    fn parse_offset_zero_negative() {
        // -00000 is still zero
        assert_eq!(parse_rit_xit_offset_response("-00000").unwrap(), 0);
    }

    #[test]
    fn parse_offset_max_positive() {
        assert_eq!(parse_rit_xit_offset_response("+99999").unwrap(), 99999);
    }

    #[test]
    fn parse_offset_max_negative() {
        assert_eq!(parse_rit_xit_offset_response("-99999").unwrap(), -99999);
    }

    #[test]
    fn parse_offset_typical_positive() {
        assert_eq!(parse_rit_xit_offset_response("+00120").unwrap(), 120);
    }

    #[test]
    fn parse_offset_typical_negative() {
        assert_eq!(parse_rit_xit_offset_response("-00300").unwrap(), -300);
    }

    #[test]
    fn parse_offset_one_hz_positive() {
        assert_eq!(parse_rit_xit_offset_response("+00001").unwrap(), 1);
    }

    #[test]
    fn parse_offset_one_hz_negative() {
        assert_eq!(parse_rit_xit_offset_response("-00001").unwrap(), -1);
    }

    #[test]
    fn parse_offset_too_short() {
        assert!(parse_rit_xit_offset_response("+0050").is_err());
    }

    #[test]
    fn parse_offset_too_long() {
        assert!(parse_rit_xit_offset_response("+000500").is_err());
    }

    #[test]
    fn parse_offset_empty() {
        assert!(parse_rit_xit_offset_response("").is_err());
    }

    #[test]
    fn parse_offset_wrong_sign() {
        assert!(parse_rit_xit_offset_response("*00050").is_err());
    }

    #[test]
    fn parse_offset_missing_sign() {
        assert!(parse_rit_xit_offset_response("000050").is_err());
    }

    #[test]
    fn parse_offset_non_digits() {
        assert!(parse_rit_xit_offset_response("+00AB0").is_err());
    }

    #[test]
    fn parse_offset_alpha_string() {
        assert!(parse_rit_xit_offset_response("+hello").is_err());
    }

    // ---------------------------------------------------------------
    // Mode round-trip: base modes go Mode -> Elecraft code -> Mode
    // ---------------------------------------------------------------

    #[test]
    fn mode_round_trip_base_modes() {
        // These base modes have a perfect round-trip.
        let modes = [
            Mode::LSB,
            Mode::USB,
            Mode::CW,
            Mode::CWR,
            Mode::AM,
            Mode::FM,
        ];
        for mode in modes {
            let code = mode_to_elecraft(&mode);
            let parsed = elecraft_to_mode(code).unwrap();
            assert_eq!(mode, parsed, "round-trip failed for {mode}");
        }
    }

    #[test]
    fn mode_round_trip_data_modes() {
        // DataUSB -> "6" (DATA) -> DataUSB: perfect round-trip
        let code = mode_to_elecraft(&Mode::DataUSB);
        assert_eq!(code, "6");
        assert_eq!(elecraft_to_mode(code).unwrap(), Mode::DataUSB);

        // DataLSB -> "9" (DATA-R) -> DataLSB: perfect round-trip
        let code = mode_to_elecraft(&Mode::DataLSB);
        assert_eq!(code, "9");
        assert_eq!(elecraft_to_mode(code).unwrap(), Mode::DataLSB);
    }

    #[test]
    fn mode_rtty_maps_to_data() {
        // RTTY and DataUSB both map to mode 6 (DATA)
        assert_eq!(mode_to_elecraft(&Mode::RTTY), "6");
        assert_eq!(mode_to_elecraft(&Mode::DataUSB), "6");

        // RTTYR and DataLSB both map to mode 9 (DATA-R)
        assert_eq!(mode_to_elecraft(&Mode::RTTYR), "9");
        assert_eq!(mode_to_elecraft(&Mode::DataLSB), "9");
    }

    #[test]
    fn data_modes_map_to_base() {
        // DataFM -> "4" (FM), DataAM -> "5" (AM)
        assert_eq!(mode_to_elecraft(&Mode::DataFM), "4");
        assert_eq!(mode_to_elecraft(&Mode::DataAM), "5");

        // Parsing those codes back yields the base mode
        assert_eq!(elecraft_to_mode("4").unwrap(), Mode::FM);
        assert_eq!(elecraft_to_mode("5").unwrap(), Mode::AM);
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

    #[test]
    fn cmd_set_bandwidth_bw_zero_padded() {
        assert_eq!(cmd_set_bandwidth_bw(50), b"BW0050;");
    }

    #[test]
    fn cmd_set_bandwidth_fw_zero_padded() {
        assert_eq!(cmd_set_bandwidth_fw(50), b"FW0050;");
    }

    // ---------------------------------------------------------------
    // CW message commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_send_cw_message_bytes() {
        // "TEST" + 20 spaces padding = 24 chars, plus 1 space separator
        assert_eq!(cmd_send_cw_message("TEST"), b"KY TEST                    ;");
    }

    #[test]
    fn cmd_send_cw_message_empty() {
        // Empty string pads to 24 spaces
        assert_eq!(cmd_send_cw_message(""), b"KY                         ;");
    }

    #[test]
    fn cmd_send_cw_message_truncates_at_24() {
        // 30-char input should truncate to first 24
        let input = "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234";
        assert_eq!(input.len(), 30);
        let cmd = cmd_send_cw_message(input);
        // "KY" + " " + 24 chars + ";"
        assert_eq!(cmd, b"KY ABCDEFGHIJKLMNOPQRSTUVWX;");
    }

    #[test]
    fn cmd_send_cw_message_exact_24() {
        // Exactly 24-char input has no extra padding
        let input = "ABCDEFGHIJKLMNOPQRSTUVWX";
        assert_eq!(input.len(), 24);
        let cmd = cmd_send_cw_message(input);
        assert_eq!(cmd, b"KY ABCDEFGHIJKLMNOPQRSTUVWX;");
    }

    #[test]
    fn cmd_read_cw_buffer_bytes() {
        assert_eq!(cmd_read_cw_buffer(), b"KY;");
    }

    #[test]
    fn cmd_stop_cw_message_bytes() {
        // "KY" + 25 spaces + ";"
        assert_eq!(cmd_stop_cw_message(), b"KY                         ;");
    }

    #[test]
    fn parse_cw_buffer_ready() {
        assert!(parse_cw_buffer_response("0").unwrap());
    }

    #[test]
    fn parse_cw_buffer_full() {
        assert!(!parse_cw_buffer_response("1").unwrap());
    }

    #[test]
    fn parse_cw_buffer_invalid() {
        assert!(parse_cw_buffer_response("2").is_err());
    }

    #[test]
    fn parse_cw_buffer_empty() {
        assert!(parse_cw_buffer_response("").is_err());
    }
}
