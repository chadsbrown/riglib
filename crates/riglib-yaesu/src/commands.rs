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

/// Build a "read CW keyer speed" command (`KS;`).
///
/// Response is `KS{speed:03};` where speed is 000-060 WPM.
pub fn cmd_read_cw_speed() -> Vec<u8> {
    encode_command("KS", "")
}

/// Build a "set CW keyer speed" command (`KS{speed:03};`).
///
/// Speed is encoded as exactly 3 zero-padded ASCII digits in WPM.
///
/// # Arguments
///
/// * `wpm` - Keyer speed in words per minute (typically 4-60).
pub fn cmd_set_cw_speed(wpm: u8) -> Vec<u8> {
    encode_command("KS", &format!("{wpm:03}"))
}

/// Build a "VFO A=B" command (`AB;`).
///
/// Copies the active VFO frequency, mode, and filter to the inactive VFO.
pub fn cmd_vfo_a_eq_b() -> Vec<u8> {
    encode_command("AB", "")
}

/// Build a "VFO swap" command (`SV;`).
///
/// Exchanges VFO A and VFO B frequencies.
pub fn cmd_vfo_swap() -> Vec<u8> {
    encode_command("SV", "")
}

/// Build a "read antenna" command (`AN0;`).
///
/// Reads the currently selected antenna for the main receiver.
/// Response is `AN0{ant};` where ant is 1-3.
pub fn cmd_read_antenna() -> Vec<u8> {
    encode_command("AN0", "")
}

/// Build a "set antenna" command (`AN0{ant};`).
///
/// # Arguments
///
/// * `ant` - Antenna number (1-3).
pub fn cmd_set_antenna(ant: u8) -> Vec<u8> {
    encode_command("AN0", &format!("{ant}"))
}

/// Build a "read AGC mode" command (`GT0;`).
///
/// Reads the current AGC mode. Response is `GT0{mode};` where mode is:
/// 0=Off, 1=Fast, 2=Mid, 3=Slow.
pub fn cmd_read_agc() -> Vec<u8> {
    encode_command("GT", "0")
}

/// Build a "set AGC mode" command (`GT0{value};`).
///
/// # Arguments
///
/// * `value` - AGC mode: 0=Off, 1=Fast, 2=Mid, 3=Slow
pub fn cmd_set_agc(value: u8) -> Vec<u8> {
    encode_command("GT", &format!("0{value}"))
}

/// Build a "read preamp" command (`PA0;`).
///
/// Reads the current preamp setting for the main receiver.
/// Response is `PA0{level:02};` where level is: 00=Off, 01=Amp1, 02=Amp2.
pub fn cmd_read_preamp() -> Vec<u8> {
    encode_command("PA0", "")
}

/// Build a "set preamp" command (`PA0{level:02};`).
///
/// # Arguments
///
/// * `level` - Preamp level: 0=Off, 1=Amp1, 2=Amp2
pub fn cmd_set_preamp(level: u8) -> Vec<u8> {
    encode_command("PA0", &format!("{level:02}"))
}

/// Build a "read attenuator" command (`RA0;`).
///
/// Reads the current attenuator setting for the main receiver.
/// Response is `RA0{level:02};` where level is: 00=Off, 01=On.
pub fn cmd_read_attenuator() -> Vec<u8> {
    encode_command("RA0", "")
}

/// Build a "set attenuator" command (`RA0{level:02};`).
///
/// # Arguments
///
/// * `level` - Attenuator level: 0=Off, 1=On
pub fn cmd_set_attenuator(level: u8) -> Vec<u8> {
    encode_command("RA0", &format!("{level:02}"))
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
// RIT / XIT command builders
// ---------------------------------------------------------------

/// Build a "read RIT state" command (`RT0;`).
///
/// The response returns the RIT on/off state and offset in hertz.
pub fn cmd_read_rit() -> Vec<u8> {
    encode_command("RT0", "")
}

/// Build a "set RIT on/off" command.
///
/// - `RT01;` enables RIT.
/// - `RT00;` disables RIT.
///
/// This does not change the offset value, only the on/off state.
///
/// # Arguments
///
/// * `on` - `true` to enable RIT, `false` to disable.
pub fn cmd_set_rit_on(on: bool) -> Vec<u8> {
    if on {
        encode_command("RT0", "1")
    } else {
        encode_command("RT0", "0")
    }
}

/// Build a "read XIT state" command (`XT0;`).
///
/// The response returns the XIT on/off state and offset in hertz.
pub fn cmd_read_xit() -> Vec<u8> {
    encode_command("XT0", "")
}

/// Build a "set XIT on/off" command.
///
/// - `XT01;` enables XIT.
/// - `XT00;` disables XIT.
///
/// This does not change the offset value, only the on/off state.
///
/// # Arguments
///
/// * `on` - `true` to enable XIT, `false` to disable.
pub fn cmd_set_xit_on(on: bool) -> Vec<u8> {
    if on {
        encode_command("XT0", "1")
    } else {
        encode_command("XT0", "0")
    }
}

/// Build a "RIT/XIT offset up" command (`RU{hz:04};`).
///
/// Increments the shared RIT/XIT offset register by the specified number of
/// hertz. Yaesu shares a single offset register between RIT and XIT.
///
/// # Arguments
///
/// * `hz` - Number of hertz to increment (encoded as 4 zero-padded digits).
pub fn cmd_rit_up(hz: u32) -> Vec<u8> {
    encode_command("RU", &format!("{hz:04}"))
}

/// Build a "RIT/XIT offset down" command (`RD{hz:04};`).
///
/// Decrements the shared RIT/XIT offset register by the specified number of
/// hertz. Yaesu shares a single offset register between RIT and XIT.
///
/// # Arguments
///
/// * `hz` - Number of hertz to decrement (encoded as 4 zero-padded digits).
pub fn cmd_rit_down(hz: u32) -> Vec<u8> {
    encode_command("RD", &format!("{hz:04}"))
}

/// Build a "RIT/XIT clear" command (`RC;`).
///
/// Resets the shared RIT/XIT offset register to zero. To set an absolute
/// offset, send `RC;` followed by `RU` or `RD` with the desired value.
pub fn cmd_rit_clear() -> Vec<u8> {
    encode_command("RC", "")
}

// ---------------------------------------------------------------
// CW message command builders
// ---------------------------------------------------------------

/// Build a "send CW message" command (`KY {text};`).
///
/// Sends a CW message string to the rig's keyer buffer. The text is
/// truncated to 24 characters if longer. A single space separates the
/// `KY` prefix from the message text.
///
/// # Arguments
///
/// * `text` - The CW message text to send (max 24 characters).
pub fn cmd_send_cw_message(text: &str) -> Vec<u8> {
    let truncated: String = text.chars().take(24).collect();
    encode_command("KY", &format!(" {truncated}"))
}

/// Build a "read CW buffer status" command (`KY;`).
///
/// Queries whether the CW keyer buffer is full or has room for more
/// characters. The response data is `0` (buffer ready) or `1` (buffer full).
pub fn cmd_read_cw_buffer() -> Vec<u8> {
    encode_command("KY", "")
}

/// Build a "stop CW message" command.
///
/// Sends the `KY` command with 24 spaces as the text payload, which
/// flushes and aborts any CW message currently being sent. The total
/// params field is 25 spaces (1 separator + 24 payload spaces).
pub fn cmd_stop_cw_message() -> Vec<u8> {
    encode_command("KY", &" ".repeat(25))
}

// ---------------------------------------------------------------
// Auto Information (AI) command builders
// ---------------------------------------------------------------

/// Build a "set Auto Information mode" command.
///
/// - `AI0;` disables auto information (no unsolicited messages).
/// - `AI2;` enables auto information (rig pushes state changes).
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

/// Parse a CW speed response from the data portion of a `KS` response.
///
/// Expects a 3-character numeric string (000-060) representing speed in WPM.
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

/// Parse an antenna response from the data portion of an `AN0` response.
///
/// Expects a single digit (1-3) representing the antenna port number.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_antenna_response(data: &str) -> Result<u8> {
    if data.len() != 1 {
        return Err(Error::Protocol(format!(
            "expected 1 digit for antenna, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u8 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid antenna digit: {data:?} ({e})")))?;
    Ok(val)
}

/// Parse an AGC mode response from the data portion of a `GT` response.
///
/// Expects 2 characters: the fixed `0` prefix followed by a mode digit.
/// Mode values: 0=Off, 1=Fast, 2=Mid, 3=Slow.
///
/// Returns the raw mode value (0-3).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_agc_response(data: &str) -> Result<u8> {
    if data.len() != 2 {
        return Err(Error::Protocol(format!(
            "expected 2 characters for AGC response, got {} characters: {data:?}",
            data.len()
        )));
    }
    let mode_char = &data[1..2];
    let val: u8 = mode_char
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid AGC mode digit: {mode_char:?} ({e})")))?;
    Ok(val)
}

/// Parse a preamp response from the data portion of a `PA0` response.
///
/// Expects 2 characters representing the preamp level.
/// Values: 00=Off, 01=Amp1, 02=Amp2.
///
/// Returns the raw level value.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_preamp_response(data: &str) -> Result<u8> {
    if data.len() != 2 {
        return Err(Error::Protocol(format!(
            "expected 2 characters for preamp response, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u8 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid preamp level: {data:?} ({e})")))?;
    Ok(val)
}

/// Parse an attenuator response from the data portion of an `RA0` response.
///
/// Expects 2 characters representing the attenuator level.
/// Values: 00=Off, 01=On.
///
/// Returns the raw level value.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data cannot be parsed.
pub fn parse_attenuator_response(data: &str) -> Result<u8> {
    if data.len() != 2 {
        return Err(Error::Protocol(format!(
            "expected 2 characters for attenuator response, got {} characters: {data:?}",
            data.len()
        )));
    }
    let val: u8 = data
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid attenuator level: {data:?} ({e})")))?;
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

/// Parse a RIT response from the data portion of an `RT0` response.
///
/// After prefix splitting by the protocol decoder, the data portion has the
/// format `P+XXXX` or `P-XXXX` where:
/// - `P` is `0` (RIT off) or `1` (RIT on)
/// - `+` or `-` is the sign of the offset
/// - `XXXX` is a 4-digit absolute offset in hertz
///
/// # Returns
///
/// A tuple of `(on, offset_hz)` where `on` is the RIT enabled state and
/// `offset_hz` is the signed offset in hertz.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data does not match the expected format.
pub fn parse_rit_response(data: &str) -> Result<(bool, i32)> {
    parse_rit_xit_response(data, "RIT")
}

/// Parse a XIT response from the data portion of an `XT0` response.
///
/// Same format as RIT: `P+XXXX` or `P-XXXX` where P is 0/1, sign is +/-,
/// and XXXX is a 4-digit absolute offset in hertz.
///
/// # Returns
///
/// A tuple of `(on, offset_hz)` where `on` is the XIT enabled state and
/// `offset_hz` is the signed offset in hertz.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data does not match the expected format.
pub fn parse_xit_response(data: &str) -> Result<(bool, i32)> {
    parse_rit_xit_response(data, "XIT")
}

/// Internal helper to parse the data portion of an RT0 or XT0 response.
///
/// Expected format: `P+XXXX` or `P-XXXX` (6 characters total).
fn parse_rit_xit_response(data: &str, label: &str) -> Result<(bool, i32)> {
    if data.len() != 6 {
        return Err(Error::Protocol(format!(
            "expected 6 characters for {label} response, got {} characters: {data:?}",
            data.len()
        )));
    }

    let on = match &data[0..1] {
        "0" => false,
        "1" => true,
        other => {
            return Err(Error::Protocol(format!(
                "expected 0 or 1 for {label} on/off state, got {other:?}"
            )));
        }
    };

    let sign = match &data[1..2] {
        "+" => 1i32,
        "-" => -1i32,
        other => {
            return Err(Error::Protocol(format!(
                "expected + or - for {label} offset sign, got {other:?}"
            )));
        }
    };

    let digits = &data[2..6];
    let abs_offset: i32 = digits
        .parse()
        .map_err(|e| Error::Protocol(format!("invalid {label} offset digits: {digits:?} ({e})")))?;

    Ok((on, sign * abs_offset))
}

/// Parse a CW buffer status response from the data portion of a `KY` response.
///
/// - `"0"` = buffer ready (not full)
/// - `"1"` = buffer full
///
/// # Returns
///
/// `true` if the buffer is full, `false` if it has room for more characters.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if `data` is not `"0"` or `"1"`.
pub fn parse_cw_buffer_response(data: &str) -> Result<bool> {
    match data {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(Error::Protocol(format!(
            "unexpected CW buffer state: {data:?}"
        ))),
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
    fn cmd_read_cw_speed_bytes() {
        assert_eq!(cmd_read_cw_speed(), b"KS;");
    }

    #[test]
    fn cmd_set_cw_speed_25wpm() {
        assert_eq!(cmd_set_cw_speed(25), b"KS025;");
    }

    #[test]
    fn cmd_set_cw_speed_zero() {
        assert_eq!(cmd_set_cw_speed(0), b"KS000;");
    }

    #[test]
    fn cmd_vfo_a_eq_b_bytes() {
        assert_eq!(cmd_vfo_a_eq_b(), b"AB;");
    }

    #[test]
    fn cmd_vfo_swap_bytes() {
        assert_eq!(cmd_vfo_swap(), b"SV;");
    }

    #[test]
    fn cmd_read_antenna_bytes() {
        assert_eq!(cmd_read_antenna(), b"AN0;");
    }

    #[test]
    fn cmd_set_antenna_1() {
        assert_eq!(cmd_set_antenna(1), b"AN01;");
    }

    #[test]
    fn cmd_set_antenna_2() {
        assert_eq!(cmd_set_antenna(2), b"AN02;");
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
    // AGC commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_agc_bytes() {
        assert_eq!(cmd_read_agc(), b"GT0;");
    }

    #[test]
    fn cmd_set_agc_off() {
        assert_eq!(cmd_set_agc(0), b"GT00;");
    }

    #[test]
    fn cmd_set_agc_fast() {
        assert_eq!(cmd_set_agc(1), b"GT01;");
    }

    #[test]
    fn cmd_set_agc_mid() {
        assert_eq!(cmd_set_agc(2), b"GT02;");
    }

    #[test]
    fn cmd_set_agc_slow() {
        assert_eq!(cmd_set_agc(3), b"GT03;");
    }

    #[test]
    fn parse_agc_off() {
        assert_eq!(parse_agc_response("00").unwrap(), 0);
    }

    #[test]
    fn parse_agc_fast() {
        assert_eq!(parse_agc_response("01").unwrap(), 1);
    }

    #[test]
    fn parse_agc_mid() {
        assert_eq!(parse_agc_response("02").unwrap(), 2);
    }

    #[test]
    fn parse_agc_slow() {
        assert_eq!(parse_agc_response("03").unwrap(), 3);
    }

    #[test]
    fn parse_agc_wrong_length() {
        assert!(parse_agc_response("0").is_err());
        assert!(parse_agc_response("001").is_err());
    }

    // ---------------------------------------------------------------
    // Preamp commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_preamp_bytes() {
        assert_eq!(cmd_read_preamp(), b"PA0;");
    }

    #[test]
    fn cmd_set_preamp_off() {
        assert_eq!(cmd_set_preamp(0), b"PA000;");
    }

    #[test]
    fn cmd_set_preamp_amp1() {
        assert_eq!(cmd_set_preamp(1), b"PA001;");
    }

    #[test]
    fn cmd_set_preamp_amp2() {
        assert_eq!(cmd_set_preamp(2), b"PA002;");
    }

    #[test]
    fn parse_preamp_off() {
        assert_eq!(parse_preamp_response("00").unwrap(), 0);
    }

    #[test]
    fn parse_preamp_amp1() {
        assert_eq!(parse_preamp_response("01").unwrap(), 1);
    }

    #[test]
    fn parse_preamp_amp2() {
        assert_eq!(parse_preamp_response("02").unwrap(), 2);
    }

    #[test]
    fn parse_preamp_wrong_length() {
        assert!(parse_preamp_response("0").is_err());
        assert!(parse_preamp_response("001").is_err());
    }

    // ---------------------------------------------------------------
    // Attenuator commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_attenuator_bytes() {
        assert_eq!(cmd_read_attenuator(), b"RA0;");
    }

    #[test]
    fn cmd_set_attenuator_off() {
        assert_eq!(cmd_set_attenuator(0), b"RA000;");
    }

    #[test]
    fn cmd_set_attenuator_on() {
        assert_eq!(cmd_set_attenuator(1), b"RA001;");
    }

    #[test]
    fn parse_attenuator_off() {
        assert_eq!(parse_attenuator_response("00").unwrap(), 0);
    }

    #[test]
    fn parse_attenuator_on() {
        assert_eq!(parse_attenuator_response("01").unwrap(), 1);
    }

    #[test]
    fn parse_attenuator_wrong_length() {
        assert!(parse_attenuator_response("0").is_err());
        assert!(parse_attenuator_response("001").is_err());
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
    // Response parsing — CW speed
    // ---------------------------------------------------------------

    #[test]
    fn parse_cw_speed_25() {
        assert_eq!(parse_cw_speed_response("025").unwrap(), 25);
    }

    #[test]
    fn parse_cw_speed_zero() {
        assert_eq!(parse_cw_speed_response("000").unwrap(), 0);
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
    // Response parsing — antenna
    // ---------------------------------------------------------------

    #[test]
    fn parse_antenna_1() {
        assert_eq!(parse_antenna_response("1").unwrap(), 1);
    }

    #[test]
    fn parse_antenna_2() {
        assert_eq!(parse_antenna_response("2").unwrap(), 2);
    }

    #[test]
    fn parse_antenna_wrong_length() {
        assert!(parse_antenna_response("12").is_err());
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
    // RIT / XIT command builders
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_rit_bytes() {
        assert_eq!(cmd_read_rit(), b"RT0;");
    }

    #[test]
    fn cmd_set_rit_on_bytes() {
        assert_eq!(cmd_set_rit_on(true), b"RT01;");
    }

    #[test]
    fn cmd_set_rit_off_bytes() {
        assert_eq!(cmd_set_rit_on(false), b"RT00;");
    }

    #[test]
    fn cmd_read_xit_bytes() {
        assert_eq!(cmd_read_xit(), b"XT0;");
    }

    #[test]
    fn cmd_set_xit_on_bytes() {
        assert_eq!(cmd_set_xit_on(true), b"XT01;");
    }

    #[test]
    fn cmd_set_xit_off_bytes() {
        assert_eq!(cmd_set_xit_on(false), b"XT00;");
    }

    #[test]
    fn cmd_rit_up_50hz() {
        assert_eq!(cmd_rit_up(50), b"RU0050;");
    }

    #[test]
    fn cmd_rit_up_zero() {
        assert_eq!(cmd_rit_up(0), b"RU0000;");
    }

    #[test]
    fn cmd_rit_up_9999() {
        assert_eq!(cmd_rit_up(9999), b"RU9999;");
    }

    #[test]
    fn cmd_rit_up_1hz() {
        assert_eq!(cmd_rit_up(1), b"RU0001;");
    }

    #[test]
    fn cmd_rit_down_50hz() {
        assert_eq!(cmd_rit_down(50), b"RD0050;");
    }

    #[test]
    fn cmd_rit_down_zero() {
        assert_eq!(cmd_rit_down(0), b"RD0000;");
    }

    #[test]
    fn cmd_rit_down_9999() {
        assert_eq!(cmd_rit_down(9999), b"RD9999;");
    }

    #[test]
    fn cmd_rit_down_1hz() {
        assert_eq!(cmd_rit_down(1), b"RD0001;");
    }

    #[test]
    fn cmd_rit_clear_bytes() {
        assert_eq!(cmd_rit_clear(), b"RC;");
    }

    // ---------------------------------------------------------------
    // Response parsing — RIT
    // ---------------------------------------------------------------

    #[test]
    fn parse_rit_on_positive_offset() {
        let (on, offset) = parse_rit_response("1+0050").unwrap();
        assert!(on);
        assert_eq!(offset, 50);
    }

    #[test]
    fn parse_rit_on_negative_offset() {
        let (on, offset) = parse_rit_response("1-0050").unwrap();
        assert!(on);
        assert_eq!(offset, -50);
    }

    #[test]
    fn parse_rit_off_zero_offset() {
        let (on, offset) = parse_rit_response("0+0000").unwrap();
        assert!(!on);
        assert_eq!(offset, 0);
    }

    #[test]
    fn parse_rit_off_negative_zero() {
        // -0000 should parse as 0
        let (on, offset) = parse_rit_response("0-0000").unwrap();
        assert!(!on);
        assert_eq!(offset, 0);
    }

    #[test]
    fn parse_rit_max_positive_offset() {
        let (on, offset) = parse_rit_response("1+9999").unwrap();
        assert!(on);
        assert_eq!(offset, 9999);
    }

    #[test]
    fn parse_rit_max_negative_offset() {
        let (on, offset) = parse_rit_response("1-9999").unwrap();
        assert!(on);
        assert_eq!(offset, -9999);
    }

    #[test]
    fn parse_rit_off_with_residual_offset() {
        // RIT disabled but offset register still has a value
        let (on, offset) = parse_rit_response("0+0120").unwrap();
        assert!(!on);
        assert_eq!(offset, 120);
    }

    #[test]
    fn parse_rit_wrong_length_short() {
        assert!(parse_rit_response("1+050").is_err());
    }

    #[test]
    fn parse_rit_wrong_length_long() {
        assert!(parse_rit_response("1+00500").is_err());
    }

    #[test]
    fn parse_rit_empty() {
        assert!(parse_rit_response("").is_err());
    }

    #[test]
    fn parse_rit_invalid_on_off() {
        assert!(parse_rit_response("2+0050").is_err());
    }

    #[test]
    fn parse_rit_invalid_sign() {
        assert!(parse_rit_response("1*0050").is_err());
    }

    #[test]
    fn parse_rit_invalid_digits() {
        assert!(parse_rit_response("1+00AB").is_err());
    }

    // ---------------------------------------------------------------
    // Response parsing — XIT
    // ---------------------------------------------------------------

    #[test]
    fn parse_xit_on_positive_offset() {
        let (on, offset) = parse_xit_response("1+0050").unwrap();
        assert!(on);
        assert_eq!(offset, 50);
    }

    #[test]
    fn parse_xit_on_negative_offset() {
        let (on, offset) = parse_xit_response("1-0050").unwrap();
        assert!(on);
        assert_eq!(offset, -50);
    }

    #[test]
    fn parse_xit_off_zero_offset() {
        let (on, offset) = parse_xit_response("0+0000").unwrap();
        assert!(!on);
        assert_eq!(offset, 0);
    }

    #[test]
    fn parse_xit_max_positive_offset() {
        let (on, offset) = parse_xit_response("1+9999").unwrap();
        assert!(on);
        assert_eq!(offset, 9999);
    }

    #[test]
    fn parse_xit_max_negative_offset() {
        let (on, offset) = parse_xit_response("1-9999").unwrap();
        assert!(on);
        assert_eq!(offset, -9999);
    }

    #[test]
    fn parse_xit_wrong_length() {
        assert!(parse_xit_response("1+050").is_err());
    }

    #[test]
    fn parse_xit_empty() {
        assert!(parse_xit_response("").is_err());
    }

    #[test]
    fn parse_xit_invalid_on_off() {
        assert!(parse_xit_response("3+0050").is_err());
    }

    #[test]
    fn parse_xit_invalid_sign() {
        assert!(parse_xit_response("1=0050").is_err());
    }

    #[test]
    fn parse_xit_invalid_digits() {
        assert!(parse_xit_response("1+ABCD").is_err());
    }

    // ---------------------------------------------------------------
    // CW message commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_send_cw_message_bytes() {
        assert_eq!(cmd_send_cw_message("TEST"), b"KY TEST;");
    }

    #[test]
    fn cmd_send_cw_message_empty() {
        assert_eq!(cmd_send_cw_message(""), b"KY ;");
    }

    #[test]
    fn cmd_send_cw_message_truncates_at_24() {
        let long_text = "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234";
        assert_eq!(long_text.len(), 30);
        let cmd = cmd_send_cw_message(long_text);
        assert_eq!(cmd, b"KY ABCDEFGHIJKLMNOPQRSTUVWX;");
    }

    #[test]
    fn cmd_read_cw_buffer_bytes() {
        assert_eq!(cmd_read_cw_buffer(), b"KY;");
    }

    #[test]
    fn cmd_stop_cw_message_bytes() {
        let cmd = cmd_stop_cw_message();
        // KY (2) + 25 spaces (1 separator + 24 payload) + ; (1) = 28 bytes
        assert_eq!(cmd.len(), 28);
        assert!(cmd.starts_with(b"KY "));
        assert!(cmd.ends_with(b";"));
        // Verify all middle bytes are spaces
        assert!(cmd[2..27].iter().all(|&b| b == b' '));
    }

    #[test]
    fn parse_cw_buffer_ready() {
        assert_eq!(parse_cw_buffer_response("0").unwrap(), false);
    }

    #[test]
    fn parse_cw_buffer_full() {
        assert_eq!(parse_cw_buffer_response("1").unwrap(), true);
    }

    #[test]
    fn parse_cw_buffer_invalid() {
        assert!(parse_cw_buffer_response("2").is_err());
    }

    #[test]
    fn parse_cw_buffer_empty() {
        assert!(parse_cw_buffer_response("").is_err());
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

    // ---------------------------------------------------------------
    // AI commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_set_ai_on() {
        assert_eq!(cmd_set_ai(true), b"AI2;");
    }

    #[test]
    fn cmd_set_ai_off() {
        assert_eq!(cmd_set_ai(false), b"AI0;");
    }
}
