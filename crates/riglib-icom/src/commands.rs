//! CI-V command builders and response parsers.
//!
//! This module provides functions to construct CI-V command frames for common
//! transceiver operations (frequency, mode, PTT, metering, split, VFO selection)
//! and to parse the corresponding responses from the rig.
//!
//! All functions are pure — they produce or consume byte vectors without
//! performing any I/O. The caller is responsible for sending the bytes over
//! a transport and feeding received bytes back into the response parsers.
//!
//! The controller address is always [`CONTROLLER_ADDR`]
//! (`0xE0`), the standard PC controller address on the CI-V bus.

use riglib_core::{Error, Mode, ReceiverId, Result};

use crate::civ::{CONTROLLER_ADDR, bcd_to_freq, encode_frame, freq_to_bcd, validate_bcd};

// ---------------------------------------------------------------
// CI-V command/sub-command constants
// ---------------------------------------------------------------

/// Write operating frequency (cmd 0x05). Data: 5-byte BCD frequency, no sub-command.
///
/// Note: CI-V command 0x00 is the *transceive* command (Transfer operating
/// frequency data), which is what the rig broadcasts when the user turns
/// the VFO knob. Command 0x05 is the correct controller→rig "set frequency"
/// command used by hamlib, flrig, and other rig control software.
const CMD_SET_FREQ: u8 = 0x05;

/// Read operating frequency (cmd 0x03). No sub-command, no data.
const CMD_READ_FREQ: u8 = 0x03;

/// Read operating mode (cmd 0x04). No sub-command, no data.
const CMD_READ_MODE: u8 = 0x04;

/// Set operating mode (cmd 0x06). Data: mode byte + optional filter byte.
const CMD_SET_MODE: u8 = 0x06;

/// VFO/memory select and band stacking commands (cmd 0x07).
const CMD_VFO_SELECT: u8 = 0x07;

/// Split operation (cmd 0x0F).
const CMD_SPLIT: u8 = 0x0F;

/// Read/set various level settings (cmd 0x14).
const CMD_LEVEL: u8 = 0x14;

/// Read meter values (cmd 0x15).
const CMD_METER: u8 = 0x15;

/// Various read/set operations (cmd 0x1A).
const CMD_MISC: u8 = 0x1A;

/// PTT and transmit control (cmd 0x1C).
const CMD_PTT: u8 = 0x1C;

// Sub-command constants for CMD_VFO_SELECT (0x07)
const SUB_VFO_A: u8 = 0x00;
const SUB_VFO_B: u8 = 0x01;
const SUB_SELECT_MAIN: u8 = 0xD0;
const SUB_SELECT_SUB: u8 = 0xD1;

// Sub-command constants for CMD_SPLIT (0x0F)
const SUB_SPLIT_OFF: u8 = 0x00;
const SUB_SPLIT_ON: u8 = 0x01;

/// Function control (cmd 0x16) — various on/off functions including AGC mode.
const CMD_FUNC: u8 = 0x16;

// Sub-command constants for CMD_LEVEL (0x14)
const SUB_RF_POWER: u8 = 0x0A;
const SUB_CW_SPEED: u8 = 0x0C;

// Sub-command constants for CMD_VFO_SELECT (0x07) — operations
const SUB_VFO_A_EQ_B: u8 = 0xA0;
const SUB_VFO_SWAP: u8 = 0xB0;

/// Attenuator command (cmd 0x11).
const CMD_ATTENUATOR: u8 = 0x11;

/// Antenna connector command (cmd 0x12).
const CMD_ANTENNA: u8 = 0x12;

// Sub-command constants for CMD_METER (0x15)
const SUB_S_METER: u8 = 0x02;
const SUB_SWR_METER: u8 = 0x12;
const SUB_ALC_METER: u8 = 0x13;

// Sub-command constants for CMD_FUNC (0x16)
const SUB_PREAMP: u8 = 0x02;
const SUB_AGC_MODE: u8 = 0x12;

// Sub-command constants for CMD_MISC (0x1A)
const SUB_IF_FILTER: u8 = 0x03;
const SUB_AGC_TIME_CONSTANT: u8 = 0x04;

// Sub-command for CMD_PTT (0x1C)
const SUB_PTT: u8 = 0x00;

/// RIT/XIT control (cmd 0x21).
const CMD_RIT_XIT: u8 = 0x21;

/// Send CW message (cmd 0x17).
const CMD_SEND_CW_MESSAGE: u8 = 0x17;

// Sub-command constants for CMD_RIT_XIT (0x21)
// Icom uses a single shared offset register for both RIT and XIT.
const SUB_RIT_XIT_OFFSET: u8 = 0x00;
const SUB_RIT_ON_OFF: u8 = 0x01;
const SUB_XIT_ON_OFF: u8 = 0x02;

// ---------------------------------------------------------------
// CI-V mode byte mapping
// ---------------------------------------------------------------

/// CI-V mode byte for LSB.
const CIV_MODE_LSB: u8 = 0x00;
/// CI-V mode byte for USB.
const CIV_MODE_USB: u8 = 0x01;
/// CI-V mode byte for AM.
const CIV_MODE_AM: u8 = 0x02;
/// CI-V mode byte for CW.
const CIV_MODE_CW: u8 = 0x03;
/// CI-V mode byte for RTTY.
const CIV_MODE_RTTY: u8 = 0x04;
/// CI-V mode byte for FM.
const CIV_MODE_FM: u8 = 0x05;
/// CI-V mode byte for CW-R (reverse).
const CIV_MODE_CWR: u8 = 0x07;
/// CI-V mode byte for RTTY-R (reverse).
const CIV_MODE_RTTYR: u8 = 0x08;

/// CI-V filter number indicating data mode (filter 3).
///
/// On Icom rigs, setting filter 3 with USB or LSB selects the data sub-mode
/// (USB-D / LSB-D), which uses the DATA modulator input.
const CIV_FILTER_DATA: u8 = 0x03;

/// Default filter number for non-data modes (filter 1 = widest).
const CIV_FILTER_DEFAULT: u8 = 0x01;

// ---------------------------------------------------------------
// Command builders
// ---------------------------------------------------------------

/// Build a CI-V "set frequency" command.
///
/// Encodes `freq_hz` as 5-byte BCD and sends command 0x05 to the rig.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig (e.g. `0x98` for IC-7610)
/// * `freq_hz` - Frequency in hertz
pub fn cmd_set_frequency(addr: u8, freq_hz: u64) -> Vec<u8> {
    let bcd = freq_to_bcd(freq_hz);
    encode_frame(addr, CONTROLLER_ADDR, CMD_SET_FREQ, None, &bcd)
}

/// Build a CI-V "read frequency" command.
///
/// Sends command 0x03 with no data. The rig responds with the current
/// frequency as 5-byte BCD.
pub fn cmd_read_frequency(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_READ_FREQ, None, &[])
}

/// Build a CI-V "set mode" command.
///
/// Translates the generic [`Mode`] enum to the appropriate CI-V mode byte
/// and filter byte, then sends command 0x06.
///
/// Data modes (DataUSB, DataLSB) are encoded as the base sideband mode
/// with filter byte 0x03, which Icom rigs interpret as the data sub-mode.
pub fn cmd_set_mode(addr: u8, mode: Mode) -> Vec<u8> {
    let (mode_byte, filter_byte) = mode_to_civ(mode);
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_SET_MODE,
        None,
        &[mode_byte, filter_byte],
    )
}

/// Build a CI-V "read mode" command.
///
/// Sends command 0x04 with no data. The rig responds with a mode byte
/// and a filter byte.
pub fn cmd_read_mode(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_READ_MODE, None, &[])
}

/// Build a CI-V "set PTT" command.
///
/// Sends command 0x1C sub-command 0x00 with data 0x01 (transmit) or
/// 0x00 (receive).
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `on` - `true` to key the transmitter, `false` to return to receive
pub fn cmd_set_ptt(addr: u8, on: bool) -> Vec<u8> {
    let data = if on { 0x01u8 } else { 0x00u8 };
    encode_frame(addr, CONTROLLER_ADDR, CMD_PTT, Some(SUB_PTT), &[data])
}

/// Build a CI-V "read PTT" command.
///
/// Sends command 0x1C sub-command 0x00 with no data. The rig responds
/// with 0x00 (receive) or 0x01 (transmit).
pub fn cmd_read_ptt(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_PTT, Some(SUB_PTT), &[])
}

/// Build a CI-V "read S-meter" command.
///
/// Sends command 0x15 sub-command 0x02. The rig responds with a 2-byte
/// BCD meter value (0000-0255).
pub fn cmd_read_s_meter(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_METER, Some(SUB_S_METER), &[])
}

/// Build a CI-V "read SWR meter" command.
///
/// Sends command 0x15 sub-command 0x12. Only meaningful while transmitting.
pub fn cmd_read_swr(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_METER, Some(SUB_SWR_METER), &[])
}

/// Build a CI-V "read ALC meter" command.
///
/// Sends command 0x15 sub-command 0x13. Only meaningful while transmitting.
pub fn cmd_read_alc(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_METER, Some(SUB_ALC_METER), &[])
}

/// Build a CI-V "read RF power level" command.
///
/// Sends command 0x14 sub-command 0x0A. The rig responds with a 2-byte
/// value representing the current power setting (0x0000-0x0255).
pub fn cmd_read_power(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_LEVEL, Some(SUB_RF_POWER), &[])
}

/// Build a CI-V "set RF power level" command.
///
/// Sends command 0x14 sub-command 0x0A with a 2-byte BCD value.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `level_0_255` - Power level from 0 to 255 (mapped to rig's power range)
pub fn cmd_set_power(addr: u8, level_0_255: u16) -> Vec<u8> {
    let hi = ((level_0_255 / 100) & 0x0F) as u8;
    let mid = (((level_0_255 % 100) / 10) & 0x0F) as u8;
    let lo = ((level_0_255 % 10) & 0x0F) as u8;
    // Icom 2-byte BCD: high byte = hundreds digit (in low nibble),
    // low byte = tens|ones digits.
    let byte_hi = hi;
    let byte_lo = (mid << 4) | lo;
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_LEVEL,
        Some(SUB_RF_POWER),
        &[byte_hi, byte_lo],
    )
}

/// Build a CI-V "set split" command.
///
/// Sends command 0x0F with sub-command 0x01 (on) or 0x00 (off).
pub fn cmd_set_split(addr: u8, on: bool) -> Vec<u8> {
    let sub = if on { SUB_SPLIT_ON } else { SUB_SPLIT_OFF };
    encode_frame(addr, CONTROLLER_ADDR, CMD_SPLIT, Some(sub), &[])
}

/// Build a CI-V "read split" command.
///
/// Sends command 0x0F with no sub-command or data. The rig responds with
/// the split state.
pub fn cmd_read_split(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_SPLIT, None, &[])
}

/// Build a CI-V "select VFO" command.
///
/// Sends command 0x07 with sub-command 0x00 (VFO A) or 0x01 (VFO B).
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `rx` - [`ReceiverId::VFO_A`] or [`ReceiverId::VFO_B`]
pub fn cmd_select_vfo(addr: u8, rx: ReceiverId) -> Vec<u8> {
    let sub = match rx.index() {
        0 => SUB_VFO_A,
        _ => SUB_VFO_B,
    };
    encode_frame(addr, CONTROLLER_ADDR, CMD_VFO_SELECT, Some(sub), &[])
}

/// Build a CI-V "select main/sub receiver" command.
///
/// For dual-receiver rigs like the IC-7610, this switches the active
/// receiver between main and sub. Sends command 0x07 with sub-command
/// 0xD0 (main) or 0xD1 (sub).
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `rx` - [`ReceiverId::VFO_A`] selects main, [`ReceiverId::VFO_B`] selects sub
pub fn cmd_select_main_sub(addr: u8, rx: ReceiverId) -> Vec<u8> {
    let sub = match rx.index() {
        0 => SUB_SELECT_MAIN,
        _ => SUB_SELECT_SUB,
    };
    encode_frame(addr, CONTROLLER_ADDR, CMD_VFO_SELECT, Some(sub), &[])
}

/// Build a CI-V "read IF filter" command.
///
/// Sends command 0x1A sub-command 0x03. The rig responds with filter data.
pub fn cmd_read_if_filter(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_MISC, Some(SUB_IF_FILTER), &[])
}

/// Build a CI-V "read CW keyer speed" command.
///
/// Sends command 0x14 sub-command 0x0C with no data. The rig responds with
/// a 2-byte BCD value (0000-0255) representing the keyer speed level.
pub fn cmd_read_cw_speed(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_LEVEL, Some(SUB_CW_SPEED), &[])
}

/// Build a CI-V "set CW keyer speed" command.
///
/// Sends command 0x14 sub-command 0x0C with a 2-byte BCD value.
/// The level maps linearly: 0 = minimum speed (6 WPM), 255 = maximum
/// speed (48 WPM) on most modern Icom rigs.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `level_0_255` - Speed level from 0 to 255
pub fn cmd_set_cw_speed(addr: u8, level_0_255: u16) -> Vec<u8> {
    let hi = ((level_0_255 / 100) & 0x0F) as u8;
    let mid = (((level_0_255 % 100) / 10) & 0x0F) as u8;
    let lo = ((level_0_255 % 10) & 0x0F) as u8;
    let byte_hi = hi;
    let byte_lo = (mid << 4) | lo;
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_LEVEL,
        Some(SUB_CW_SPEED),
        &[byte_hi, byte_lo],
    )
}

/// Build a CI-V "VFO A=B" command (copy active VFO to inactive).
///
/// Sends command 0x07 sub-command 0xA0.
pub fn cmd_vfo_a_eq_b(addr: u8) -> Vec<u8> {
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_VFO_SELECT,
        Some(SUB_VFO_A_EQ_B),
        &[],
    )
}

/// Build a CI-V "VFO swap" command (exchange A and B frequencies).
///
/// Sends command 0x07 sub-command 0xB0.
pub fn cmd_vfo_swap(addr: u8) -> Vec<u8> {
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_VFO_SELECT,
        Some(SUB_VFO_SWAP),
        &[],
    )
}

/// Build a CI-V "read antenna" command.
///
/// Sends command 0x12 with no data. The rig responds with a single byte
/// indicating the selected antenna connector (0x01 = ANT1, 0x02 = ANT2, etc.).
pub fn cmd_read_antenna(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_ANTENNA, None, &[])
}

/// Build a CI-V "set antenna" command.
///
/// Sends command 0x12 with a single data byte for the antenna connector.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `ant` - Antenna number (0x01 = ANT1, 0x02 = ANT2)
pub fn cmd_set_antenna(addr: u8, ant: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_ANTENNA, None, &[ant])
}

/// Build a CI-V "read AGC mode" command.
///
/// Sends command 0x16 sub-command 0x12 with no data. The rig responds with
/// a single byte: 0x01 = fast, 0x02 = mid, 0x03 = slow.
///
/// Note: On SDR-generation rigs (IC-7300, IC-7610, IC-9700, IC-705, etc.),
/// AGC off is not reported via this command — see [`cmd_read_agc_time_constant`].
pub fn cmd_read_agc_mode(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_FUNC, Some(SUB_AGC_MODE), &[])
}

/// Build a CI-V "set AGC mode" command.
///
/// Sends command 0x16 sub-command 0x12 with a single data byte.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `mode` - AGC mode byte: 0x01 = fast, 0x02 = mid, 0x03 = slow.
///   On older rigs (IC-785x, IC-7800, IC-7700, IC-7600) 0x00 = off.
///   On SDR-generation rigs 0x00 is not supported via this command.
pub fn cmd_set_agc_mode(addr: u8, mode: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_FUNC, Some(SUB_AGC_MODE), &[mode])
}

/// Build a CI-V "read AGC time constant" command.
///
/// Sends command 0x1A sub-command 0x04 with no data. On SDR-generation
/// Icom rigs, a value of 0x00 means AGC is off.
///
/// This is the only way to read AGC-off state on IC-7300, IC-7610,
/// IC-9700, IC-705, IC-7300MK2, and IC-905.
pub fn cmd_read_agc_time_constant(addr: u8) -> Vec<u8> {
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_MISC,
        Some(SUB_AGC_TIME_CONSTANT),
        &[],
    )
}

/// Build a CI-V "set AGC time constant" command.
///
/// Sends command 0x1A sub-command 0x04 with a single data byte.
/// Setting value to 0x00 turns AGC off on SDR-generation Icom rigs.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `value` - Time constant value (0x00 = AGC off on SDR rigs)
pub fn cmd_set_agc_time_constant(addr: u8, value: u8) -> Vec<u8> {
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_MISC,
        Some(SUB_AGC_TIME_CONSTANT),
        &[value],
    )
}

// ---------------------------------------------------------------
// RIT/XIT command builders
// ---------------------------------------------------------------

/// Build a CI-V "read RIT on/off" command.
///
/// Sends command 0x21 sub-command 0x01 with no data. The rig responds with
/// a single byte: 0x00 = off, 0x01 = on.
pub fn cmd_read_rit_on(addr: u8) -> Vec<u8> {
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_RIT_XIT,
        Some(SUB_RIT_ON_OFF),
        &[],
    )
}

/// Build a CI-V "set RIT on/off" command.
///
/// Sends command 0x21 sub-command 0x01 with data 0x01 (on) or 0x00 (off).
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `on` - `true` to enable RIT, `false` to disable
pub fn cmd_set_rit_on(addr: u8, on: bool) -> Vec<u8> {
    let data = if on { 0x01u8 } else { 0x00u8 };
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_RIT_XIT,
        Some(SUB_RIT_ON_OFF),
        &[data],
    )
}

/// Build a CI-V "read RIT/XIT offset" command.
///
/// Sends command 0x21 sub-command 0x00 with no data. Icom rigs have a single
/// shared offset register for both RIT and XIT. The rig responds with 2 bytes
/// of little-endian BCD magnitude followed by a sign byte.
///
/// Example: +150 Hz = `[0x50, 0x01, 0x00]` (LE-BCD `0150`, sign positive)
pub fn cmd_read_rit_offset(addr: u8) -> Vec<u8> {
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_RIT_XIT,
        Some(SUB_RIT_XIT_OFFSET),
        &[],
    )
}

/// Build a CI-V "set RIT/XIT offset" command.
///
/// Sends command 0x21 sub-command 0x00 with 2 bytes of little-endian BCD
/// magnitude and a sign byte. The offset range is typically +/-9999 Hz.
/// Icom rigs have a single shared offset register for both RIT and XIT.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `offset_hz` - RIT/XIT offset in hertz (positive or negative)
pub fn cmd_set_rit_offset(addr: u8, offset_hz: i32) -> Vec<u8> {
    let data = encode_rit_xit_offset(offset_hz);
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_RIT_XIT,
        Some(SUB_RIT_XIT_OFFSET),
        &data,
    )
}

/// Build a CI-V "read XIT on/off" command.
///
/// Sends command 0x21 sub-command 0x02 with no data. The rig responds with
/// a single byte: 0x00 = off, 0x01 = on.
pub fn cmd_read_xit_on(addr: u8) -> Vec<u8> {
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_RIT_XIT,
        Some(SUB_XIT_ON_OFF),
        &[],
    )
}

/// Build a CI-V "set XIT on/off" command.
///
/// Sends command 0x21 sub-command 0x02 with data 0x01 (on) or 0x00 (off).
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `on` - `true` to enable XIT, `false` to disable
pub fn cmd_set_xit_on(addr: u8, on: bool) -> Vec<u8> {
    let data = if on { 0x01u8 } else { 0x00u8 };
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_RIT_XIT,
        Some(SUB_XIT_ON_OFF),
        &[data],
    )
}

/// Build a CI-V "read XIT offset" command.
///
/// Reads the shared RIT/XIT offset. This is the same register as RIT offset
/// (sub-command 0x00). Provided as a separate function for API clarity.
pub fn cmd_read_xit_offset(addr: u8) -> Vec<u8> {
    cmd_read_rit_offset(addr)
}

/// Build a CI-V "set XIT offset" command.
///
/// Sets the shared RIT/XIT offset. This is the same register as RIT offset
/// (sub-command 0x00). Provided as a separate function for API clarity.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `offset_hz` - RIT/XIT offset in hertz (positive or negative)
pub fn cmd_set_xit_offset(addr: u8, offset_hz: i32) -> Vec<u8> {
    cmd_set_rit_offset(addr, offset_hz)
}

// ---------------------------------------------------------------
// CW message command builders
// ---------------------------------------------------------------

/// Build a CI-V "send CW message" command.
///
/// Sends command 0x17 with the text payload as raw ASCII bytes. Each
/// character is sent as its standard ASCII byte value (e.g., `'C'` = 0x43,
/// `'Q'` = 0x51). The payload is truncated to 30 characters if longer;
/// the rig wiring layer handles chunking for longer messages.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig (e.g. `0x98` for IC-7610)
/// * `text` - ASCII text to send as CW (max 30 characters per frame)
pub fn cmd_send_cw_message(addr: u8, text: &str) -> Vec<u8> {
    let text_bytes: Vec<u8> = text.as_bytes().iter().copied().take(30).collect();
    encode_frame(
        addr,
        CONTROLLER_ADDR,
        CMD_SEND_CW_MESSAGE,
        None,
        &text_bytes,
    )
}

/// Build a CI-V "stop CW message" command.
///
/// Sends command 0x17 with a single `0xFF` byte to abort a CW message
/// that is currently being sent by the rig's keyer.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
pub fn cmd_stop_cw_message(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_SEND_CW_MESSAGE, None, &[0xFF])
}

// ---------------------------------------------------------------
// RIT/XIT response parsers
// ---------------------------------------------------------------

/// Parse a RIT on/off response from CI-V data bytes.
///
/// Expects 1 byte: 0x00 = off, 0x01 = on.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_rit_on_response(data: &[u8]) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for RIT on/off response, got 0".into(),
        ));
    }
    Ok(data[0] != 0x00)
}

/// Parse a RIT offset response from CI-V data bytes.
///
/// Expects 3 bytes: a sign byte (0x00 = positive, 0x01 = negative) followed
/// by 2 bytes of BCD-encoded magnitude.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data length is wrong or contains
/// invalid BCD digits.
pub fn parse_rit_offset_response(data: &[u8]) -> Result<i32> {
    parse_rit_xit_offset_response(data, "RIT")
}

/// Parse a XIT on/off response from CI-V data bytes.
///
/// Expects 1 byte: 0x00 = off, 0x01 = on.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_xit_on_response(data: &[u8]) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for XIT on/off response, got 0".into(),
        ));
    }
    Ok(data[0] != 0x00)
}

/// Parse a XIT offset response from CI-V data bytes.
///
/// Expects 3 bytes: a sign byte (0x00 = positive, 0x01 = negative) followed
/// by 2 bytes of BCD-encoded magnitude.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data length is wrong or contains
/// invalid BCD digits.
pub fn parse_xit_offset_response(data: &[u8]) -> Result<i32> {
    parse_rit_xit_offset_response(data, "XIT")
}

// ---------------------------------------------------------------
// RIT/XIT offset encoding/decoding helpers
// ---------------------------------------------------------------

/// Encode a signed RIT/XIT offset as little-endian BCD + sign byte.
///
/// The offset is clamped to +/-9999 Hz. The CI-V encoding is:
/// - Byte 0: low BCD byte (tens and ones digits)
/// - Byte 1: high BCD byte (thousands and hundreds digits)
/// - Byte 2: sign (0x00 = positive, 0x01 = negative)
///
/// Example: +150 Hz = `[0x50, 0x01, 0x00]`, -300 Hz = `[0x00, 0x03, 0x01]`
fn encode_rit_xit_offset(offset_hz: i32) -> [u8; 3] {
    let sign_byte: u8 = if offset_hz < 0 { 0x01 } else { 0x00 };
    let magnitude = offset_hz.unsigned_abs().min(9999);

    let thousands = ((magnitude / 1000) % 10) as u8;
    let hundreds = ((magnitude / 100) % 10) as u8;
    let tens = ((magnitude / 10) % 10) as u8;
    let ones = (magnitude % 10) as u8;

    let bcd_hi = (thousands << 4) | hundreds;
    let bcd_lo = (tens << 4) | ones;

    [bcd_lo, bcd_hi, sign_byte]
}

/// Parse a signed RIT/XIT offset from CI-V response data.
///
/// Expects 3 bytes in little-endian BCD format: `[BCD_lo, BCD_hi, sign]`.
fn parse_rit_xit_offset_response(data: &[u8], label: &str) -> Result<i32> {
    if data.len() != 3 {
        return Err(Error::Protocol(format!(
            "expected 3 bytes for {label} offset response, got {}",
            data.len()
        )));
    }

    let lo = data[0];
    let hi = data[1];
    let sign_byte = data[2];

    validate_bcd(&[lo, hi])?;

    let thousands = ((hi >> 4) & 0x0F) as i32;
    let hundreds = (hi & 0x0F) as i32;
    let tens = ((lo >> 4) & 0x0F) as i32;
    let ones = (lo & 0x0F) as i32;

    let magnitude = thousands * 1000 + hundreds * 100 + tens * 10 + ones;

    if sign_byte == 0x01 {
        Ok(-magnitude)
    } else {
        Ok(magnitude)
    }
}

/// Parse an AGC mode response from CI-V data bytes.
///
/// Expects 1 byte: 0x01 = fast, 0x02 = mid, 0x03 = slow.
/// On older rigs, 0x00 = off.
///
/// Returns the raw mode byte for the rig layer to interpret.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_agc_mode_response(data: &[u8]) -> Result<u8> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for AGC mode response, got 0".into(),
        ));
    }
    Ok(data[0])
}

/// Parse an AGC time constant response from CI-V data bytes.
///
/// Expects 1 byte representing the AGC time constant. A value of 0x00
/// indicates AGC off on SDR-generation rigs.
///
/// Returns the raw time constant byte.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_agc_time_constant_response(data: &[u8]) -> Result<u8> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for AGC time constant response, got 0".into(),
        ));
    }
    Ok(data[0])
}

/// Build a CI-V "read preamp" command.
///
/// Sends command 0x16 sub-command 0x02 with no data. The rig responds with
/// a single byte: 0x00 = off, 0x01 = preamp 1, 0x02 = preamp 2.
pub fn cmd_read_preamp(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_FUNC, Some(SUB_PREAMP), &[])
}

/// Build a CI-V "set preamp" command.
///
/// Sends command 0x16 sub-command 0x02 with a single data byte.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `level` - Preamp level: 0x00 = off, 0x01 = preamp 1, 0x02 = preamp 2.
///   Available levels are model-dependent (gated in the rig layer).
pub fn cmd_set_preamp(addr: u8, level: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_FUNC, Some(SUB_PREAMP), &[level])
}

/// Parse a preamp response from CI-V data bytes.
///
/// Expects 1 byte: 0x00 = off, 0x01 = preamp 1, 0x02 = preamp 2.
///
/// Returns the raw preamp level byte for the rig layer to interpret.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_preamp_response(data: &[u8]) -> Result<u8> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for preamp response, got 0".into(),
        ));
    }
    Ok(data[0])
}

/// Build a CI-V "read attenuator" command.
///
/// Sends command 0x11 with no sub-command or data. The rig responds with a
/// single byte indicating the attenuator setting (0x00 = off, 0x20 = 20 dB
/// on most models).
pub fn cmd_read_attenuator(addr: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_ATTENUATOR, None, &[])
}

/// Build a CI-V "set attenuator" command.
///
/// Sends command 0x11 with a single data byte.
///
/// # Arguments
///
/// * `addr` - CI-V address of the target rig
/// * `level` - Attenuator level: 0x00 = off, 0x20 = 20 dB on most models.
pub fn cmd_set_attenuator(addr: u8, level: u8) -> Vec<u8> {
    encode_frame(addr, CONTROLLER_ADDR, CMD_ATTENUATOR, None, &[level])
}

/// Parse an attenuator response from CI-V data bytes.
///
/// Expects 1 byte: 0x00 = off, 0x20 = 20 dB on most models.
///
/// Returns the raw attenuator level byte.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_attenuator_response(data: &[u8]) -> Result<u8> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for attenuator response, got 0".into(),
        ));
    }
    Ok(data[0])
}

// ---------------------------------------------------------------
// Response parsers
// ---------------------------------------------------------------

/// Parse a frequency response from CI-V data bytes.
///
/// Expects exactly 5 bytes of BCD-encoded frequency data (the payload
/// after the command byte in a frequency response frame). This is what
/// you get from the `sub_cmd` + `data` fields of a decoded [`CivFrame`](crate::civ::CivFrame)
/// with `cmd == 0x03`.
///
/// # Arguments
///
/// * `data` - The raw BCD data bytes from the response frame. Accepts
///   either 5 bytes (pure BCD) for cases where the caller has already
///   stripped the command echo, or the full payload.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data length is wrong or contains
/// invalid BCD digits.
pub fn parse_frequency_response(data: &[u8]) -> Result<u64> {
    if data.len() != 5 {
        return Err(Error::Protocol(format!(
            "expected 5 BCD bytes for frequency, got {}",
            data.len()
        )));
    }
    validate_bcd(data)?;
    let bcd: [u8; 5] = data.try_into().unwrap();
    Ok(bcd_to_freq(&bcd))
}

/// Parse a mode response from CI-V data bytes.
///
/// Expects 2 bytes: the CI-V mode byte and the filter byte.
/// Data modes are identified by filter byte 0x03 combined with
/// a USB (0x01) or LSB (0x00) mode byte.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is too short or the mode
/// byte is unrecognized.
pub fn parse_mode_response(data: &[u8]) -> Result<Mode> {
    if data.len() < 2 {
        return Err(Error::Protocol(format!(
            "expected at least 2 bytes for mode response, got {}",
            data.len()
        )));
    }
    let mode_byte = data[0];
    let filter_byte = data[1];
    civ_to_mode(mode_byte, filter_byte)
}

/// Parse a PTT response from CI-V data bytes.
///
/// Expects 1 byte: 0x00 = receive, 0x01 = transmit.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_ptt_response(data: &[u8]) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for PTT response, got 0".into(),
        ));
    }
    Ok(data[0] != 0x00)
}

/// Parse a meter reading response from CI-V data bytes.
///
/// Icom meter responses are 2 bytes of BCD representing a value from
/// 0000 to 0255. This function decodes the BCD and normalizes it to
/// a `f32` in the range 0.0 to 1.0 (where 1.0 = 0255 = full scale).
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data length is wrong or contains
/// invalid BCD digits.
pub fn parse_meter_response(data: &[u8]) -> Result<f32> {
    if data.len() != 2 {
        return Err(Error::Protocol(format!(
            "expected 2 bytes for meter response, got {}",
            data.len()
        )));
    }
    validate_bcd(data)?;

    // 2-byte BCD: byte[0] = high digits, byte[1] = low digits.
    // Example: [0x01, 0x28] = 0128 decimal.
    let hi_byte = data[0];
    let lo_byte = data[1];
    let hundreds = ((hi_byte >> 4) & 0x0F) as u32 * 1000
        + (hi_byte & 0x0F) as u32 * 100
        + ((lo_byte >> 4) & 0x0F) as u32 * 10
        + (lo_byte & 0x0F) as u32;
    Ok(hundreds as f32 / 255.0)
}

/// Parse a split response from CI-V data bytes.
///
/// The split response is a single sub-command byte:
/// 0x00 = split off, 0x01 = split on.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_split_response(data: &[u8]) -> Result<bool> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for split response, got 0".into(),
        ));
    }
    Ok(data[0] != 0x00)
}

/// Parse an antenna response from CI-V data bytes.
///
/// Expects 1 byte: 0x01 = ANT1, 0x02 = ANT2, etc.
///
/// # Errors
///
/// Returns [`Error::Protocol`] if the data is empty.
pub fn parse_antenna_response(data: &[u8]) -> Result<u8> {
    if data.is_empty() {
        return Err(Error::Protocol(
            "expected 1 byte for antenna response, got 0".into(),
        ));
    }
    Ok(data[0])
}

// ---------------------------------------------------------------
// Mode conversion helpers
// ---------------------------------------------------------------

/// Convert a generic [`Mode`] to CI-V mode byte and filter byte.
fn mode_to_civ(mode: Mode) -> (u8, u8) {
    match mode {
        Mode::LSB => (CIV_MODE_LSB, CIV_FILTER_DEFAULT),
        Mode::USB => (CIV_MODE_USB, CIV_FILTER_DEFAULT),
        Mode::AM => (CIV_MODE_AM, CIV_FILTER_DEFAULT),
        Mode::CW => (CIV_MODE_CW, CIV_FILTER_DEFAULT),
        Mode::CWR => (CIV_MODE_CWR, CIV_FILTER_DEFAULT),
        Mode::RTTY => (CIV_MODE_RTTY, CIV_FILTER_DEFAULT),
        Mode::RTTYR => (CIV_MODE_RTTYR, CIV_FILTER_DEFAULT),
        Mode::FM => (CIV_MODE_FM, CIV_FILTER_DEFAULT),
        Mode::DataUSB => (CIV_MODE_USB, CIV_FILTER_DATA),
        Mode::DataLSB => (CIV_MODE_LSB, CIV_FILTER_DATA),
        Mode::DataFM => (CIV_MODE_FM, CIV_FILTER_DATA),
        Mode::DataAM => (CIV_MODE_AM, CIV_FILTER_DATA),
    }
}

/// Convert CI-V mode byte and filter byte to a generic [`Mode`].
///
/// Filter byte 0x03 indicates a data sub-mode on Icom rigs.
fn civ_to_mode(mode_byte: u8, filter_byte: u8) -> Result<Mode> {
    let is_data = filter_byte == CIV_FILTER_DATA;

    match (mode_byte, is_data) {
        (CIV_MODE_LSB, false) => Ok(Mode::LSB),
        (CIV_MODE_LSB, true) => Ok(Mode::DataLSB),
        (CIV_MODE_USB, false) => Ok(Mode::USB),
        (CIV_MODE_USB, true) => Ok(Mode::DataUSB),
        (CIV_MODE_AM, false) => Ok(Mode::AM),
        (CIV_MODE_AM, true) => Ok(Mode::DataAM),
        (CIV_MODE_CW, _) => Ok(Mode::CW),
        (CIV_MODE_RTTY, _) => Ok(Mode::RTTY),
        (CIV_MODE_FM, false) => Ok(Mode::FM),
        (CIV_MODE_FM, true) => Ok(Mode::DataFM),
        (CIV_MODE_CWR, _) => Ok(Mode::CWR),
        (CIV_MODE_RTTYR, _) => Ok(Mode::RTTYR),
        _ => Err(Error::Protocol(format!(
            "unknown CI-V mode byte: 0x{mode_byte:02X} filter: 0x{filter_byte:02X}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IC7610_ADDR: u8 = 0x98;

    // ---------------------------------------------------------------
    // Command building verification
    // ---------------------------------------------------------------

    #[test]
    fn cmd_set_frequency_bytes() {
        let bytes = cmd_set_frequency(IC7610_ADDR, 14_250_000);
        // FE FE 98 E0 05 <5 BCD bytes> FD
        assert_eq!(bytes[0], 0xFE);
        assert_eq!(bytes[1], 0xFE);
        assert_eq!(bytes[2], IC7610_ADDR);
        assert_eq!(bytes[3], CONTROLLER_ADDR);
        assert_eq!(bytes[4], CMD_SET_FREQ);
        // BCD for 14,250,000: [0x00, 0x00, 0x25, 0x14, 0x00]
        assert_eq!(&bytes[5..10], &[0x00, 0x00, 0x25, 0x14, 0x00]);
        assert_eq!(bytes[10], 0xFD);
        assert_eq!(bytes.len(), 11);
    }

    #[test]
    fn cmd_read_frequency_bytes() {
        let bytes = cmd_read_frequency(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_READ_FREQ,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_mode_usb() {
        let bytes = cmd_set_mode(IC7610_ADDR, Mode::USB);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_SET_MODE,
                CIV_MODE_USB,
                CIV_FILTER_DEFAULT,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_mode_data_usb() {
        let bytes = cmd_set_mode(IC7610_ADDR, Mode::DataUSB);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_SET_MODE,
                CIV_MODE_USB,
                CIV_FILTER_DATA,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_mode_cw() {
        let bytes = cmd_set_mode(IC7610_ADDR, Mode::CW);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_SET_MODE,
                CIV_MODE_CW,
                CIV_FILTER_DEFAULT,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_mode_bytes() {
        let bytes = cmd_read_mode(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_READ_MODE,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_ptt_on() {
        let bytes = cmd_set_ptt(IC7610_ADDR, true);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_PTT,
                SUB_PTT,
                0x01,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_ptt_off() {
        let bytes = cmd_set_ptt(IC7610_ADDR, false);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_PTT,
                SUB_PTT,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_ptt_bytes() {
        let bytes = cmd_read_ptt(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_PTT,
                SUB_PTT,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_s_meter_bytes() {
        let bytes = cmd_read_s_meter(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_METER,
                SUB_S_METER,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_swr_bytes() {
        let bytes = cmd_read_swr(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_METER,
                SUB_SWR_METER,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_alc_bytes() {
        let bytes = cmd_read_alc(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_METER,
                SUB_ALC_METER,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_power_bytes() {
        let bytes = cmd_read_power(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_LEVEL,
                SUB_RF_POWER,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_power_bytes() {
        // Set power to 128 => BCD 0128 => bytes [0x01, 0x28]
        let bytes = cmd_set_power(IC7610_ADDR, 128);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_LEVEL,
                SUB_RF_POWER,
                0x01,
                0x28,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_power_zero() {
        let bytes = cmd_set_power(IC7610_ADDR, 0);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_LEVEL,
                SUB_RF_POWER,
                0x00,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_power_max() {
        // 255 => BCD 0255 => bytes [0x02, 0x55]
        let bytes = cmd_set_power(IC7610_ADDR, 255);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_LEVEL,
                SUB_RF_POWER,
                0x02,
                0x55,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_split_on() {
        let bytes = cmd_set_split(IC7610_ADDR, true);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_SPLIT,
                SUB_SPLIT_ON,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_split_off() {
        let bytes = cmd_set_split(IC7610_ADDR, false);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_SPLIT,
                SUB_SPLIT_OFF,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_split_bytes() {
        let bytes = cmd_read_split(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![0xFE, 0xFE, IC7610_ADDR, CONTROLLER_ADDR, CMD_SPLIT, 0xFD]
        );
    }

    #[test]
    fn cmd_select_vfo_a() {
        let bytes = cmd_select_vfo(IC7610_ADDR, ReceiverId::VFO_A);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_VFO_SELECT,
                SUB_VFO_A,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_select_vfo_b() {
        let bytes = cmd_select_vfo(IC7610_ADDR, ReceiverId::VFO_B);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_VFO_SELECT,
                SUB_VFO_B,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_select_main() {
        let bytes = cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_VFO_SELECT,
                SUB_SELECT_MAIN,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_select_sub() {
        let bytes = cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_B);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_VFO_SELECT,
                SUB_SELECT_SUB,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_if_filter_bytes() {
        let bytes = cmd_read_if_filter(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_MISC,
                SUB_IF_FILTER,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_cw_speed_bytes() {
        let bytes = cmd_read_cw_speed(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_LEVEL,
                SUB_CW_SPEED,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_cw_speed_mid() {
        // Set speed to 128 => BCD 0128 => bytes [0x01, 0x28]
        let bytes = cmd_set_cw_speed(IC7610_ADDR, 128);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_LEVEL,
                SUB_CW_SPEED,
                0x01,
                0x28,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_cw_speed_max() {
        // 255 => BCD 0255 => bytes [0x02, 0x55]
        let bytes = cmd_set_cw_speed(IC7610_ADDR, 255);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_LEVEL,
                SUB_CW_SPEED,
                0x02,
                0x55,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_vfo_a_eq_b_bytes() {
        let bytes = cmd_vfo_a_eq_b(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_VFO_SELECT,
                SUB_VFO_A_EQ_B,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_vfo_swap_bytes() {
        let bytes = cmd_vfo_swap(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_VFO_SELECT,
                SUB_VFO_SWAP,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_antenna_bytes() {
        let bytes = cmd_read_antenna(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![0xFE, 0xFE, IC7610_ADDR, CONTROLLER_ADDR, CMD_ANTENNA, 0xFD]
        );
    }

    #[test]
    fn cmd_set_antenna_ant1() {
        let bytes = cmd_set_antenna(IC7610_ADDR, 0x01);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_ANTENNA,
                0x01,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_antenna_ant2() {
        let bytes = cmd_set_antenna(IC7610_ADDR, 0x02);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_ANTENNA,
                0x02,
                0xFD
            ]
        );
    }

    // ---------------------------------------------------------------
    // Response parsing
    // ---------------------------------------------------------------

    #[test]
    fn parse_freq_response_14250() {
        let data = [0x00u8, 0x00, 0x25, 0x14, 0x00];
        let freq = parse_frequency_response(&data).unwrap();
        assert_eq!(freq, 14_250_000);
    }

    #[test]
    fn parse_freq_response_7000() {
        let data = [0x00u8, 0x00, 0x00, 0x07, 0x00];
        let freq = parse_frequency_response(&data).unwrap();
        assert_eq!(freq, 7_000_000);
    }

    #[test]
    fn parse_freq_response_wrong_length() {
        assert!(parse_frequency_response(&[0x00, 0x00, 0x25]).is_err());
        assert!(parse_frequency_response(&[]).is_err());
    }

    #[test]
    fn parse_freq_response_invalid_bcd() {
        // 0xAB has invalid BCD digits
        assert!(parse_frequency_response(&[0xAB, 0x00, 0x00, 0x00, 0x00]).is_err());
    }

    #[test]
    fn parse_mode_usb() {
        let mode = parse_mode_response(&[CIV_MODE_USB, CIV_FILTER_DEFAULT]).unwrap();
        assert_eq!(mode, Mode::USB);
    }

    #[test]
    fn parse_mode_lsb() {
        let mode = parse_mode_response(&[CIV_MODE_LSB, CIV_FILTER_DEFAULT]).unwrap();
        assert_eq!(mode, Mode::LSB);
    }

    #[test]
    fn parse_mode_cw() {
        let mode = parse_mode_response(&[CIV_MODE_CW, CIV_FILTER_DEFAULT]).unwrap();
        assert_eq!(mode, Mode::CW);
    }

    #[test]
    fn parse_mode_cwr() {
        let mode = parse_mode_response(&[CIV_MODE_CWR, CIV_FILTER_DEFAULT]).unwrap();
        assert_eq!(mode, Mode::CWR);
    }

    #[test]
    fn parse_mode_rtty() {
        let mode = parse_mode_response(&[CIV_MODE_RTTY, CIV_FILTER_DEFAULT]).unwrap();
        assert_eq!(mode, Mode::RTTY);
    }

    #[test]
    fn parse_mode_rttyr() {
        let mode = parse_mode_response(&[CIV_MODE_RTTYR, CIV_FILTER_DEFAULT]).unwrap();
        assert_eq!(mode, Mode::RTTYR);
    }

    #[test]
    fn parse_mode_am() {
        let mode = parse_mode_response(&[CIV_MODE_AM, CIV_FILTER_DEFAULT]).unwrap();
        assert_eq!(mode, Mode::AM);
    }

    #[test]
    fn parse_mode_fm() {
        let mode = parse_mode_response(&[CIV_MODE_FM, CIV_FILTER_DEFAULT]).unwrap();
        assert_eq!(mode, Mode::FM);
    }

    #[test]
    fn parse_mode_data_usb() {
        let mode = parse_mode_response(&[CIV_MODE_USB, CIV_FILTER_DATA]).unwrap();
        assert_eq!(mode, Mode::DataUSB);
    }

    #[test]
    fn parse_mode_data_lsb() {
        let mode = parse_mode_response(&[CIV_MODE_LSB, CIV_FILTER_DATA]).unwrap();
        assert_eq!(mode, Mode::DataLSB);
    }

    #[test]
    fn parse_mode_data_fm() {
        let mode = parse_mode_response(&[CIV_MODE_FM, CIV_FILTER_DATA]).unwrap();
        assert_eq!(mode, Mode::DataFM);
    }

    #[test]
    fn parse_mode_data_am() {
        let mode = parse_mode_response(&[CIV_MODE_AM, CIV_FILTER_DATA]).unwrap();
        assert_eq!(mode, Mode::DataAM);
    }

    #[test]
    fn parse_mode_unknown() {
        assert!(parse_mode_response(&[0xFF, 0x01]).is_err());
    }

    #[test]
    fn parse_mode_too_short() {
        assert!(parse_mode_response(&[0x01]).is_err());
        assert!(parse_mode_response(&[]).is_err());
    }

    #[test]
    fn parse_ptt_on() {
        assert!(parse_ptt_response(&[0x01]).unwrap());
    }

    #[test]
    fn parse_ptt_off() {
        assert!(!parse_ptt_response(&[0x00]).unwrap());
    }

    #[test]
    fn parse_ptt_empty() {
        assert!(parse_ptt_response(&[]).is_err());
    }

    #[test]
    fn parse_meter_full_scale() {
        // 0255 in BCD = [0x02, 0x55]
        let value = parse_meter_response(&[0x02, 0x55]).unwrap();
        assert!((value - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_meter_zero() {
        let value = parse_meter_response(&[0x00, 0x00]).unwrap();
        assert!((value - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_meter_mid() {
        // 0128 in BCD = [0x01, 0x28]
        let value = parse_meter_response(&[0x01, 0x28]).unwrap();
        assert!((value - 128.0 / 255.0).abs() < 0.001);
    }

    #[test]
    fn parse_meter_wrong_length() {
        assert!(parse_meter_response(&[0x01]).is_err());
        assert!(parse_meter_response(&[0x01, 0x02, 0x03]).is_err());
    }

    #[test]
    fn parse_meter_invalid_bcd() {
        assert!(parse_meter_response(&[0x0A, 0x00]).is_err());
    }

    #[test]
    fn parse_split_on() {
        assert!(parse_split_response(&[0x01]).unwrap());
    }

    #[test]
    fn parse_split_off() {
        assert!(!parse_split_response(&[0x00]).unwrap());
    }

    #[test]
    fn parse_split_empty() {
        assert!(parse_split_response(&[]).is_err());
    }

    #[test]
    fn parse_antenna_ant1() {
        assert_eq!(parse_antenna_response(&[0x01]).unwrap(), 0x01);
    }

    #[test]
    fn parse_antenna_ant2() {
        assert_eq!(parse_antenna_response(&[0x02]).unwrap(), 0x02);
    }

    #[test]
    fn parse_antenna_empty() {
        assert!(parse_antenna_response(&[]).is_err());
    }

    // ---------------------------------------------------------------
    // AGC commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_agc_mode_bytes() {
        let bytes = cmd_read_agc_mode(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_FUNC,
                SUB_AGC_MODE,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_agc_mode_fast() {
        let bytes = cmd_set_agc_mode(IC7610_ADDR, 0x01);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_FUNC,
                SUB_AGC_MODE,
                0x01,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_agc_mode_mid() {
        let bytes = cmd_set_agc_mode(IC7610_ADDR, 0x02);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_FUNC,
                SUB_AGC_MODE,
                0x02,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_agc_mode_slow() {
        let bytes = cmd_set_agc_mode(IC7610_ADDR, 0x03);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_FUNC,
                SUB_AGC_MODE,
                0x03,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_agc_time_constant_bytes() {
        let bytes = cmd_read_agc_time_constant(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_MISC,
                SUB_AGC_TIME_CONSTANT,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_agc_time_constant_off() {
        let bytes = cmd_set_agc_time_constant(IC7610_ADDR, 0x00);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_MISC,
                SUB_AGC_TIME_CONSTANT,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn parse_agc_mode_fast() {
        assert_eq!(parse_agc_mode_response(&[0x01]).unwrap(), 0x01);
    }

    #[test]
    fn parse_agc_mode_mid() {
        assert_eq!(parse_agc_mode_response(&[0x02]).unwrap(), 0x02);
    }

    #[test]
    fn parse_agc_mode_slow() {
        assert_eq!(parse_agc_mode_response(&[0x03]).unwrap(), 0x03);
    }

    #[test]
    fn parse_agc_mode_empty() {
        assert!(parse_agc_mode_response(&[]).is_err());
    }

    #[test]
    fn parse_agc_time_constant_off() {
        assert_eq!(parse_agc_time_constant_response(&[0x00]).unwrap(), 0x00);
    }

    #[test]
    fn parse_agc_time_constant_empty() {
        assert!(parse_agc_time_constant_response(&[]).is_err());
    }

    // ---------------------------------------------------------------
    // Preamp commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_preamp_bytes() {
        let bytes = cmd_read_preamp(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_FUNC,
                SUB_PREAMP,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_preamp_off() {
        let bytes = cmd_set_preamp(IC7610_ADDR, 0x00);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_FUNC,
                SUB_PREAMP,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_preamp_1() {
        let bytes = cmd_set_preamp(IC7610_ADDR, 0x01);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_FUNC,
                SUB_PREAMP,
                0x01,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_preamp_2() {
        let bytes = cmd_set_preamp(IC7610_ADDR, 0x02);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_FUNC,
                SUB_PREAMP,
                0x02,
                0xFD
            ]
        );
    }

    #[test]
    fn parse_preamp_off() {
        assert_eq!(parse_preamp_response(&[0x00]).unwrap(), 0x00);
    }

    #[test]
    fn parse_preamp_1() {
        assert_eq!(parse_preamp_response(&[0x01]).unwrap(), 0x01);
    }

    #[test]
    fn parse_preamp_2() {
        assert_eq!(parse_preamp_response(&[0x02]).unwrap(), 0x02);
    }

    #[test]
    fn parse_preamp_empty() {
        assert!(parse_preamp_response(&[]).is_err());
    }

    // ---------------------------------------------------------------
    // Attenuator commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_attenuator_bytes() {
        let bytes = cmd_read_attenuator(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_ATTENUATOR,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_attenuator_off() {
        let bytes = cmd_set_attenuator(IC7610_ADDR, 0x00);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_ATTENUATOR,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_attenuator_20db() {
        let bytes = cmd_set_attenuator(IC7610_ADDR, 0x20);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_ATTENUATOR,
                0x20,
                0xFD
            ]
        );
    }

    #[test]
    fn parse_attenuator_off() {
        assert_eq!(parse_attenuator_response(&[0x00]).unwrap(), 0x00);
    }

    #[test]
    fn parse_attenuator_20db() {
        assert_eq!(parse_attenuator_response(&[0x20]).unwrap(), 0x20);
    }

    #[test]
    fn parse_attenuator_empty() {
        assert!(parse_attenuator_response(&[]).is_err());
    }

    // ---------------------------------------------------------------
    // RIT/XIT commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_read_rit_on_bytes() {
        let bytes = cmd_read_rit_on(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_ON_OFF,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_rit_on_true() {
        let bytes = cmd_set_rit_on(IC7610_ADDR, true);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_ON_OFF,
                0x01,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_rit_on_false() {
        let bytes = cmd_set_rit_on(IC7610_ADDR, false);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_ON_OFF,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_rit_offset_bytes() {
        let bytes = cmd_read_rit_offset(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_XIT_OFFSET,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_rit_offset_positive_150() {
        // +150 Hz = LE-BCD [0x50, 0x01], sign 0x00
        let bytes = cmd_set_rit_offset(IC7610_ADDR, 150);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_XIT_OFFSET,
                0x50,
                0x01,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_rit_offset_negative_300() {
        // -300 Hz = LE-BCD [0x00, 0x03], sign 0x01
        let bytes = cmd_set_rit_offset(IC7610_ADDR, -300);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_XIT_OFFSET,
                0x00,
                0x03,
                0x01,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_rit_offset_zero() {
        // 0 Hz = LE-BCD [0x00, 0x00], sign 0x00
        let bytes = cmd_set_rit_offset(IC7610_ADDR, 0);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_XIT_OFFSET,
                0x00,
                0x00,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_rit_offset_max() {
        // +9999 Hz = LE-BCD [0x99, 0x99], sign 0x00
        let bytes = cmd_set_rit_offset(IC7610_ADDR, 9999);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_XIT_OFFSET,
                0x99,
                0x99,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_rit_offset_min() {
        // -9999 Hz = LE-BCD [0x99, 0x99], sign 0x01
        let bytes = cmd_set_rit_offset(IC7610_ADDR, -9999);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_RIT_XIT_OFFSET,
                0x99,
                0x99,
                0x01,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_xit_on_bytes() {
        let bytes = cmd_read_xit_on(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_XIT_ON_OFF,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_xit_on_true() {
        let bytes = cmd_set_xit_on(IC7610_ADDR, true);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_XIT_ON_OFF,
                0x01,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_set_xit_on_false() {
        let bytes = cmd_set_xit_on(IC7610_ADDR, false);
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                CMD_RIT_XIT,
                SUB_XIT_ON_OFF,
                0x00,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_read_xit_offset_same_as_rit() {
        // XIT offset uses the same shared register as RIT (sub-command 0x00)
        assert_eq!(
            cmd_read_xit_offset(IC7610_ADDR),
            cmd_read_rit_offset(IC7610_ADDR)
        );
    }

    #[test]
    fn cmd_set_xit_offset_same_as_rit() {
        // XIT offset uses the same shared register as RIT (sub-command 0x00)
        assert_eq!(
            cmd_set_xit_offset(IC7610_ADDR, 500),
            cmd_set_rit_offset(IC7610_ADDR, 500)
        );
        assert_eq!(
            cmd_set_xit_offset(IC7610_ADDR, -1234),
            cmd_set_rit_offset(IC7610_ADDR, -1234)
        );
    }

    // ---------------------------------------------------------------
    // RIT/XIT response parsing
    // ---------------------------------------------------------------

    #[test]
    fn parse_rit_on_true() {
        assert!(parse_rit_on_response(&[0x01]).unwrap());
    }

    #[test]
    fn parse_rit_on_false() {
        assert!(!parse_rit_on_response(&[0x00]).unwrap());
    }

    #[test]
    fn parse_rit_on_empty() {
        assert!(parse_rit_on_response(&[]).is_err());
    }

    #[test]
    fn parse_rit_offset_positive_150() {
        // LE-BCD [0x50, 0x01], sign 0x00 (positive) => +150 Hz
        let offset = parse_rit_offset_response(&[0x50, 0x01, 0x00]).unwrap();
        assert_eq!(offset, 150);
    }

    #[test]
    fn parse_rit_offset_negative_300() {
        // LE-BCD [0x00, 0x03], sign 0x01 (negative) => -300 Hz
        let offset = parse_rit_offset_response(&[0x00, 0x03, 0x01]).unwrap();
        assert_eq!(offset, -300);
    }

    #[test]
    fn parse_rit_offset_zero() {
        let offset = parse_rit_offset_response(&[0x00, 0x00, 0x00]).unwrap();
        assert_eq!(offset, 0);
    }

    #[test]
    fn parse_rit_offset_max() {
        // +9999 Hz = LE-BCD [0x99, 0x99], sign 0x00
        let offset = parse_rit_offset_response(&[0x99, 0x99, 0x00]).unwrap();
        assert_eq!(offset, 9999);
    }

    #[test]
    fn parse_rit_offset_min() {
        // -9999 Hz = LE-BCD [0x99, 0x99], sign 0x01
        let offset = parse_rit_offset_response(&[0x99, 0x99, 0x01]).unwrap();
        assert_eq!(offset, -9999);
    }

    #[test]
    fn parse_rit_offset_wrong_length() {
        assert!(parse_rit_offset_response(&[0x50, 0x01]).is_err());
        assert!(parse_rit_offset_response(&[0x50, 0x01, 0x00, 0x00]).is_err());
        assert!(parse_rit_offset_response(&[]).is_err());
    }

    #[test]
    fn parse_rit_offset_invalid_bcd() {
        // 0xAB has invalid BCD digits
        assert!(parse_rit_offset_response(&[0xAB, 0x00, 0x00]).is_err());
    }

    #[test]
    fn parse_xit_on_true() {
        assert!(parse_xit_on_response(&[0x01]).unwrap());
    }

    #[test]
    fn parse_xit_on_false() {
        assert!(!parse_xit_on_response(&[0x00]).unwrap());
    }

    #[test]
    fn parse_xit_on_empty() {
        assert!(parse_xit_on_response(&[]).is_err());
    }

    #[test]
    fn parse_xit_offset_positive_500() {
        // LE-BCD [0x00, 0x05], sign 0x00 => +500 Hz
        let offset = parse_xit_offset_response(&[0x00, 0x05, 0x00]).unwrap();
        assert_eq!(offset, 500);
    }

    #[test]
    fn parse_xit_offset_negative_1234() {
        // LE-BCD [0x34, 0x12], sign 0x01 => -1234 Hz
        let offset = parse_xit_offset_response(&[0x34, 0x12, 0x01]).unwrap();
        assert_eq!(offset, -1234);
    }

    #[test]
    fn parse_xit_offset_wrong_length() {
        assert!(parse_xit_offset_response(&[0x00]).is_err());
        assert!(parse_xit_offset_response(&[]).is_err());
    }

    #[test]
    fn parse_xit_offset_invalid_bcd() {
        assert!(parse_xit_offset_response(&[0xF0, 0x00, 0x00]).is_err());
    }

    // ---------------------------------------------------------------
    // RIT/XIT offset encode/decode round-trip
    // ---------------------------------------------------------------

    #[test]
    fn rit_offset_round_trip() {
        for offset in [-9999, -1234, -300, -1, 0, 1, 150, 500, 9999] {
            let encoded = encode_rit_xit_offset(offset);
            let decoded = parse_rit_xit_offset_response(&encoded, "RIT").unwrap();
            assert_eq!(decoded, offset, "round-trip failed for offset {offset}");
        }
    }

    #[test]
    fn rit_offset_clamp_overflow() {
        // Values beyond +/-9999 should be clamped to 9999
        let encoded = encode_rit_xit_offset(12345);
        let decoded = parse_rit_xit_offset_response(&encoded, "RIT").unwrap();
        assert_eq!(decoded, 9999);

        let encoded = encode_rit_xit_offset(-12345);
        let decoded = parse_rit_xit_offset_response(&encoded, "RIT").unwrap();
        assert_eq!(decoded, -9999);
    }

    // ---------------------------------------------------------------
    // CW message commands
    // ---------------------------------------------------------------

    #[test]
    fn cmd_send_cw_message_bytes() {
        let bytes = cmd_send_cw_message(IC7610_ADDR, "CQ CQ");
        assert_eq!(
            bytes,
            vec![
                0xFE,
                0xFE,
                IC7610_ADDR,
                CONTROLLER_ADDR,
                0x17,
                0x43,
                0x51,
                0x20,
                0x43,
                0x51,
                0xFD
            ]
        );
    }

    #[test]
    fn cmd_send_cw_message_empty() {
        let bytes = cmd_send_cw_message(IC7610_ADDR, "");
        assert_eq!(
            bytes,
            vec![0xFE, 0xFE, IC7610_ADDR, CONTROLLER_ADDR, 0x17, 0xFD]
        );
    }

    #[test]
    fn cmd_send_cw_message_truncates_at_30() {
        // 40 characters — should be truncated to 30
        let long_text = "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890ABCD";
        assert_eq!(long_text.len(), 40);
        let bytes = cmd_send_cw_message(IC7610_ADDR, long_text);
        // Frame: preamble (2) + addr (1) + ctrl (1) + cmd (1) + data (30) + end (1) = 36
        assert_eq!(bytes.len(), 36);
        // Data portion is bytes[5..35] — exactly 30 bytes
        let data_portion = &bytes[5..35];
        assert_eq!(data_portion.len(), 30);
        assert_eq!(data_portion, b"ABCDEFGHIJKLMNOPQRSTUVWXYZ1234");
    }

    #[test]
    fn cmd_stop_cw_message_bytes() {
        let bytes = cmd_stop_cw_message(IC7610_ADDR);
        assert_eq!(
            bytes,
            vec![0xFE, 0xFE, IC7610_ADDR, CONTROLLER_ADDR, 0x17, 0xFF, 0xFD]
        );
    }

    // ---------------------------------------------------------------
    // Mode round-trip: Mode -> CI-V bytes -> Mode
    // ---------------------------------------------------------------

    #[test]
    fn mode_round_trip_all() {
        let modes = [
            Mode::LSB,
            Mode::USB,
            Mode::AM,
            Mode::CW,
            Mode::CWR,
            Mode::RTTY,
            Mode::RTTYR,
            Mode::FM,
            Mode::DataUSB,
            Mode::DataLSB,
            Mode::DataFM,
            Mode::DataAM,
        ];
        for mode in modes {
            let (mode_byte, filter_byte) = mode_to_civ(mode);
            let parsed = civ_to_mode(mode_byte, filter_byte).unwrap();
            assert_eq!(mode, parsed, "round-trip failed for {mode}");
        }
    }
}
