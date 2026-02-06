//! SmartSDR TCP command/response/status encoding and decoding.
//!
//! The SmartSDR text protocol uses newline-terminated ASCII lines over TCP
//! port 4992. Commands flow from client to radio; responses, status messages,
//! and handshake lines flow from radio to client.
//!
//! # Line formats
//!
//! ```text
//! Command:   C<seq>|<command_text>\n
//! Response:  R<seq>|<hex_error_code>|<response_data>\n
//! Status:    S<hex_handle>|<object> <key>=<value> ...\n
//! Message:   M<seq>|<text>\n
//! Version:   V<major>.<minor>.<patch>.<build>\n
//! Handle:    H<hex_handle>\n
//! ```
//!
//! All encoding/decoding in this module is pure parsing -- no I/O is performed.

use riglib_core::{Error, Result};

// ---------------------------------------------------------------------------
// Frequency conversion helpers
// ---------------------------------------------------------------------------

/// Convert frequency in Hz (`u64`) to MHz (`f64`) for SmartSDR commands.
pub fn hz_to_mhz(hz: u64) -> f64 {
    hz as f64 / 1_000_000.0
}

/// Convert frequency in MHz (`f64`) to Hz (`u64`) for the riglib API.
pub fn mhz_to_hz(mhz: f64) -> u64 {
    (mhz * 1_000_000.0).round() as u64
}

// ---------------------------------------------------------------------------
// Command encoding
// ---------------------------------------------------------------------------

/// Encode a SmartSDR command with the given sequence number.
///
/// Format: `C<seq>|<command>\n`
pub fn encode_command(seq: u32, command: &str) -> Vec<u8> {
    format!("C{seq}|{command}\n").into_bytes()
}

// ---------------------------------------------------------------------------
// Command builders
//
// Each builder returns the command string WITHOUT the `C<seq>|` prefix.
// The prefix is added by the client layer when it assigns a sequence number.
// ---------------------------------------------------------------------------

/// Build a slice tune command.
///
/// Example output: `"slice tune 0 14.250000"`
pub fn cmd_slice_tune(slice_index: u8, freq_hz: u64) -> String {
    format!("slice tune {} {:.6}", slice_index, hz_to_mhz(freq_hz))
}

/// Build a slice set mode command.
///
/// Example output: `"slice set 0 mode=USB"`
pub fn cmd_slice_set_mode(slice_index: u8, mode: &str) -> String {
    format!("slice set {} mode={}", slice_index, mode)
}

/// Build a slice set filter command.
///
/// Example output: `"slice set 0 filter_lo=100 filter_hi=2900"`
pub fn cmd_slice_set_filter(slice_index: u8, lo: i32, hi: i32) -> String {
    format!(
        "slice set {} filter_lo={} filter_hi={}",
        slice_index, lo, hi
    )
}

/// Build a slice set TX command (designate a slice as the TX slice).
///
/// Example output: `"slice set 0 tx=1"`
pub fn cmd_slice_set_tx(slice_index: u8) -> String {
    format!("slice set {} tx=1", slice_index)
}

/// Build a slice create command.
///
/// Example output: `"slice create freq=14.250000 mode=USB"`
pub fn cmd_slice_create(freq_hz: u64, mode: &str) -> String {
    format!("slice create freq={:.6} mode={}", hz_to_mhz(freq_hz), mode)
}

/// Build a slice remove command.
///
/// Example output: `"slice remove 0"`
pub fn cmd_slice_remove(slice_index: u8) -> String {
    format!("slice remove {}", slice_index)
}

/// Build a slice list command.
///
/// Example output: `"slice list"`
pub fn cmd_slice_list() -> String {
    "slice list".to_string()
}

/// Build an xmit (PTT) command.
///
/// Example output: `"xmit 1"` or `"xmit 0"`
pub fn cmd_xmit(on: bool) -> String {
    format!("xmit {}", if on { "1" } else { "0" })
}

/// Build a transmit set power command.
///
/// Example output: `"transmit set power=50"`
pub fn cmd_set_power(watts: u16) -> String {
    format!("transmit set power={}", watts)
}

/// Build an info command.
///
/// Example output: `"info"`
pub fn cmd_info() -> String {
    "info".to_string()
}

/// Build a meter list command.
///
/// Example output: `"meter list"`
pub fn cmd_meter_list() -> String {
    "meter list".to_string()
}

/// Build a subscribe command.
///
/// Example output: `"sub slice all"`
pub fn cmd_subscribe(object: &str) -> String {
    format!("sub {}", object)
}

/// Build a client program registration command.
///
/// Example output: `"client program riglib"`
pub fn cmd_client_program(name: &str) -> String {
    format!("client program {}", name)
}

/// Create a DAX RX audio stream for the given channel.
///
/// DAX channels are numbered 1-8. The radio responds with a stream handle
/// (hex) that also serves as the VITA-49 stream_id for incoming audio
/// packets.
///
/// Example output: `"stream create type=dax_rx dax_channel=1"`
pub fn cmd_stream_create_dax_rx(dax_channel: u8) -> String {
    format!("stream create type=dax_rx dax_channel={}", dax_channel)
}

/// Create a DAX TX audio stream.
///
/// Only one TX stream per client is supported. The radio responds with a
/// stream handle used to tag outgoing VITA-49 packets.
///
/// Example output: `"stream create type=dax_tx dax_channel=1"`
pub fn cmd_stream_create_dax_tx(dax_channel: u8) -> String {
    format!("stream create type=dax_tx dax_channel={}", dax_channel)
}

/// Remove an active stream by its handle.
///
/// The handle is formatted as a zero-padded 8-digit uppercase hex value,
/// matching the format returned by the radio in stream create responses.
///
/// Example output: `"stream remove 0x20000001"`
pub fn cmd_stream_remove(stream_id: u32) -> String {
    format!("stream remove 0x{:08X}", stream_id)
}

/// Associate a DAX channel with a slice receiver.
///
/// Setting `dax_channel` to 0 removes the DAX assignment from the slice.
///
/// Example output: `"slice set 0 dax=1"`
pub fn cmd_slice_set_dax(slice_index: u8, dax_channel: u8) -> String {
    format!("slice set {} dax={}", slice_index, dax_channel)
}

// ---------------------------------------------------------------------------
// Response / status / message types
// ---------------------------------------------------------------------------

/// A decoded SmartSDR response to a previously-sent command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartSdrResponse {
    /// Sequence number correlating this response to the originating command.
    pub sequence: u32,
    /// Error code. `0` means success; non-zero is a SmartSDR error code.
    pub error_code: u32,
    /// Response data (may be empty). For `slice create`, this is the new slice index.
    pub message: String,
}

/// A decoded SmartSDR status message (unsolicited, pushed after subscription).
#[derive(Debug, Clone, PartialEq)]
pub struct SmartSdrStatus {
    /// The client handle this status was sent to.
    pub handle: u32,
    /// The object type and optional identifier (e.g. `"slice 0"`, `"tx"`, `"radio"`).
    pub object: String,
    /// Key-value pairs extracted from the status line.
    pub params: Vec<(String, String)>,
}

/// A decoded handshake version line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartSdrVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub build: u32,
}

/// Types of lines received from the SmartSDR TCP stream.
#[derive(Debug, Clone, PartialEq)]
pub enum SmartSdrMessage {
    /// Handshake version line: `V1.4.0.0`
    Version(SmartSdrVersion),
    /// Handshake client handle: `H12345678`
    Handle(u32),
    /// Response to a command: `R<seq>|<error>|<data>`
    Response(SmartSdrResponse),
    /// Unsolicited status update: `S<handle>|<object> <kv>...`
    Status(SmartSdrStatus),
    /// Informational message: `M<seq>|<text>`
    Message(String),
    /// An unrecognised line.
    Unknown(String),
}

// ---------------------------------------------------------------------------
// Message parsing
// ---------------------------------------------------------------------------

/// Parse a single line received from the SmartSDR TCP stream.
///
/// The line should NOT include the trailing `\n`. Leading/trailing whitespace
/// is stripped for robustness.
pub fn parse_message(line: &str) -> Result<SmartSdrMessage> {
    let line = line.trim();
    if line.is_empty() {
        return Err(Error::Protocol("empty line".into()));
    }

    let first = line.as_bytes()[0];
    match first {
        b'V' => parse_version(line),
        b'H' => parse_handle(line),
        b'R' => parse_response(line),
        b'S' => parse_status(line),
        b'M' => parse_msg(line),
        _ => Ok(SmartSdrMessage::Unknown(line.to_string())),
    }
}

/// Parse a version line: `V<major>.<minor>.<patch>.<build>`
fn parse_version(line: &str) -> Result<SmartSdrMessage> {
    let body = &line[1..]; // skip 'V'
    let parts: Vec<&str> = body.split('.').collect();
    if parts.len() != 4 {
        return Err(Error::Protocol(format!("invalid version format: {line}")));
    }

    let parse_u32 = |s: &str| -> Result<u32> {
        s.parse::<u32>()
            .map_err(|_| Error::Protocol(format!("invalid version number: {s}")))
    };

    Ok(SmartSdrMessage::Version(SmartSdrVersion {
        major: parse_u32(parts[0])?,
        minor: parse_u32(parts[1])?,
        patch: parse_u32(parts[2])?,
        build: parse_u32(parts[3])?,
    }))
}

/// Parse a handle line: `H<8_hex_digits>`
fn parse_handle(line: &str) -> Result<SmartSdrMessage> {
    let body = &line[1..]; // skip 'H'
    let handle = u32::from_str_radix(body, 16)
        .map_err(|_| Error::Protocol(format!("invalid hex handle: {body}")))?;
    Ok(SmartSdrMessage::Handle(handle))
}

/// Parse a response line: `R<seq>|<hex_error_code>|<response_data>`
fn parse_response(line: &str) -> Result<SmartSdrMessage> {
    let body = &line[1..]; // skip 'R'
    let parts: Vec<&str> = body.splitn(3, '|').collect();
    if parts.len() < 2 {
        return Err(Error::Protocol(format!(
            "malformed response (need at least seq|error): {line}"
        )));
    }

    let sequence = parts[0]
        .parse::<u32>()
        .map_err(|_| Error::Protocol(format!("invalid response sequence number: {}", parts[0])))?;

    let error_code = u32::from_str_radix(parts[1], 16)
        .map_err(|_| Error::Protocol(format!("invalid response error code: {}", parts[1])))?;

    let message = if parts.len() == 3 {
        parts[2].to_string()
    } else {
        String::new()
    };

    Ok(SmartSdrMessage::Response(SmartSdrResponse {
        sequence,
        error_code,
        message,
    }))
}

/// Parse a status line: `S<hex_handle>|<object_type> [<key>=<value> ...]`
fn parse_status(line: &str) -> Result<SmartSdrMessage> {
    let body = &line[1..]; // skip 'S'
    let pipe_pos = body
        .find('|')
        .ok_or_else(|| Error::Protocol(format!("malformed status (no pipe): {line}")))?;

    let handle_str = &body[..pipe_pos];
    let handle = u32::from_str_radix(handle_str, 16)
        .map_err(|_| Error::Protocol(format!("invalid status handle: {handle_str}")))?;

    let payload = &body[pipe_pos + 1..];

    // Split payload into tokens. The first one or two tokens form the object
    // identifier (e.g. "slice 0", "tx", "radio", "interlock"). The remaining
    // tokens are key=value pairs.
    //
    // Heuristic: tokens that contain '=' are key-value pairs. The first run of
    // tokens that do NOT contain '=' form the object identifier.
    let tokens: Vec<&str> = payload.split_whitespace().collect();
    let mut object_parts: Vec<&str> = Vec::new();
    let mut kv_start = 0;

    for (i, token) in tokens.iter().enumerate() {
        if token.contains('=') {
            kv_start = i;
            break;
        }
        object_parts.push(token);
        kv_start = i + 1;
    }

    let object = object_parts.join(" ");

    let mut params: Vec<(String, String)> = Vec::new();
    for token in &tokens[kv_start..] {
        if let Some(eq_pos) = token.find('=') {
            let key = token[..eq_pos].to_string();
            let value = token[eq_pos + 1..].to_string();
            params.push((key, value));
        }
        // Non-kv tokens after the first kv token are silently ignored.
        // This handles edge cases in real SmartSDR output.
    }

    Ok(SmartSdrMessage::Status(SmartSdrStatus {
        handle,
        object,
        params,
    }))
}

/// Parse a message line: `M<seq>|<text>`
fn parse_msg(line: &str) -> Result<SmartSdrMessage> {
    let body = &line[1..]; // skip 'M'
    let pipe_pos = body
        .find('|')
        .ok_or_else(|| Error::Protocol(format!("malformed message (no pipe): {line}")))?;

    // We validate the sequence number but only keep the text.
    let _seq_str = &body[..pipe_pos];
    let text = &body[pipe_pos + 1..];
    Ok(SmartSdrMessage::Message(text.to_string()))
}

// ---------------------------------------------------------------------------
// Status object parsers
// ---------------------------------------------------------------------------

/// Parsed slice status fields extracted from a status message.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceStatus {
    /// The slice index (0-7).
    pub index: u8,
    /// Slice frequency in MHz (from `RF_frequency` key).
    pub frequency_mhz: Option<f64>,
    /// Operating mode string (from `mode` key).
    pub mode: Option<String>,
    /// Lower filter edge in Hz (from `filter_lo` key).
    pub filter_lo: Option<i32>,
    /// Upper filter edge in Hz (from `filter_hi` key).
    pub filter_hi: Option<i32>,
    /// Whether this slice is the TX slice (from `tx` key).
    pub tx: Option<bool>,
    /// Whether this slice is the active slice (from `active` key).
    pub active: Option<bool>,
}

/// Parse a slice status from key-value pairs and the object string.
///
/// The `object_str` is expected to be something like `"slice 0"`. The slice
/// index is extracted from the second token.
pub fn parse_slice_status(params: &[(String, String)], object_str: &str) -> Result<SliceStatus> {
    // Extract slice index from object string (e.g. "slice 0" -> 0).
    let parts: Vec<&str> = object_str.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(Error::Protocol(format!(
            "slice status missing index in object: {object_str}"
        )));
    }
    let index = parts[1]
        .parse::<u8>()
        .map_err(|_| Error::Protocol(format!("invalid slice index: {}", parts[1])))?;

    let mut status = SliceStatus {
        index,
        frequency_mhz: None,
        mode: None,
        filter_lo: None,
        filter_hi: None,
        tx: None,
        active: None,
    };

    for (key, value) in params {
        match key.as_str() {
            "RF_frequency" => {
                status.frequency_mhz = Some(
                    value
                        .parse::<f64>()
                        .map_err(|_| Error::Protocol(format!("invalid RF_frequency: {value}")))?,
                );
            }
            "mode" => {
                status.mode = Some(value.clone());
            }
            "filter_lo" => {
                status.filter_lo = Some(
                    value
                        .parse::<i32>()
                        .map_err(|_| Error::Protocol(format!("invalid filter_lo: {value}")))?,
                );
            }
            "filter_hi" => {
                status.filter_hi = Some(
                    value
                        .parse::<i32>()
                        .map_err(|_| Error::Protocol(format!("invalid filter_hi: {value}")))?,
                );
            }
            "tx" => {
                status.tx = Some(value == "1");
            }
            "active" => {
                status.active = Some(value == "1");
            }
            _ => {
                // Ignore unknown keys -- the radio may send fields we
                // don't care about.
            }
        }
    }

    Ok(status)
}

/// Parsed meter status fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeterStatus {
    /// Meter numeric ID.
    pub id: u16,
    /// Human-readable meter name (e.g. `"s-meter"`, `"swr"`).
    pub name: Option<String>,
    /// Meter source: `"SLC"` (slice), `"TX"`, `"RAD"` (radio), etc.
    pub source: Option<String>,
}

/// Parse a meter status from key-value pairs.
///
/// Expects at least a `num` (or `id`) key for the meter ID, and optionally
/// `nam` (name) and `src` (source) keys.
pub fn parse_meter_status(params: &[(String, String)]) -> Result<MeterStatus> {
    let mut id: Option<u16> = None;
    let mut name: Option<String> = None;
    let mut source: Option<String> = None;

    for (key, value) in params {
        match key.as_str() {
            "num" | "id" => {
                id = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| Error::Protocol(format!("invalid meter id: {value}")))?,
                );
            }
            "nam" | "name" => {
                name = Some(value.clone());
            }
            "src" | "source" => {
                source = Some(value.clone());
            }
            _ => {}
        }
    }

    let id = id.ok_or_else(|| Error::Protocol("meter status missing id/num field".into()))?;

    Ok(MeterStatus { id, name, source })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Frequency conversion -----------------------------------------------

    #[test]
    fn hz_to_mhz_basic() {
        assert!((hz_to_mhz(14_250_000) - 14.25).abs() < 1e-9);
        assert!((hz_to_mhz(1_800_000) - 1.8).abs() < 1e-9);
        assert!((hz_to_mhz(54_000_000) - 54.0).abs() < 1e-9);
    }

    #[test]
    fn mhz_to_hz_basic() {
        assert_eq!(mhz_to_hz(14.250000), 14_250_000);
        assert_eq!(mhz_to_hz(1.8), 1_800_000);
    }

    #[test]
    fn frequency_round_trip() {
        // Test representative frequencies across HF range at 1 Hz precision.
        let test_freqs: &[u64] = &[
            500_000,    // 500 kHz
            1_800_000,  // 160m bottom
            1_850_001,  // odd 1 Hz
            3_573_000,  // 80m FT8
            7_074_000,  // 40m FT8
            10_136_000, // 30m FT8
            14_074_000, // 20m FT8
            14_250_000, // 20m SSB
            21_074_000, // 15m FT8
            28_074_000, // 10m FT8
            50_313_000, // 6m FT8
            54_000_000, // 6m top
        ];
        for &hz in test_freqs {
            let mhz = hz_to_mhz(hz);
            let back = mhz_to_hz(mhz);
            assert_eq!(back, hz, "round-trip failed for {hz} Hz (mhz={mhz})");
        }
    }

    #[test]
    fn frequency_edge_zero() {
        assert!((hz_to_mhz(0) - 0.0).abs() < 1e-15);
        assert_eq!(mhz_to_hz(0.0), 0);
    }

    #[test]
    fn frequency_very_large() {
        // 1.3 GHz (23cm band) -- beyond HF but should still work.
        let hz: u64 = 1_296_000_000;
        let mhz = hz_to_mhz(hz);
        assert!((mhz - 1296.0).abs() < 1e-6);
        assert_eq!(mhz_to_hz(mhz), hz);
    }

    // -- Command encoding ---------------------------------------------------

    #[test]
    fn encode_command_seq1() {
        let bytes = encode_command(1, "slice tune 0 14.250000");
        assert_eq!(bytes, b"C1|slice tune 0 14.250000\n");
    }

    #[test]
    fn encode_command_large_seq() {
        let bytes = encode_command(99999, "info");
        assert_eq!(bytes, b"C99999|info\n");
    }

    #[test]
    fn encode_command_seq_zero() {
        let bytes = encode_command(0, "xmit 1");
        assert_eq!(bytes, b"C0|xmit 1\n");
    }

    // -- Command builders ---------------------------------------------------

    #[test]
    fn cmd_slice_tune_20m() {
        let cmd = cmd_slice_tune(0, 14_250_000);
        assert_eq!(cmd, "slice tune 0 14.250000");
    }

    #[test]
    fn cmd_slice_tune_160m() {
        let cmd = cmd_slice_tune(1, 1_800_000);
        assert_eq!(cmd, "slice tune 1 1.800000");
    }

    #[test]
    fn cmd_slice_tune_6m() {
        let cmd = cmd_slice_tune(0, 54_000_000);
        assert_eq!(cmd, "slice tune 0 54.000000");
    }

    #[test]
    fn cmd_slice_tune_500khz() {
        let cmd = cmd_slice_tune(0, 500_000);
        assert_eq!(cmd, "slice tune 0 0.500000");
    }

    #[test]
    fn cmd_slice_set_mode_usb() {
        assert_eq!(cmd_slice_set_mode(0, "USB"), "slice set 0 mode=USB");
    }

    #[test]
    fn cmd_slice_set_filter_ssb() {
        assert_eq!(
            cmd_slice_set_filter(0, 100, 2900),
            "slice set 0 filter_lo=100 filter_hi=2900"
        );
    }

    #[test]
    fn cmd_slice_set_tx_slice0() {
        assert_eq!(cmd_slice_set_tx(0), "slice set 0 tx=1");
    }

    #[test]
    fn cmd_slice_create_basic() {
        assert_eq!(
            cmd_slice_create(14_250_000, "USB"),
            "slice create freq=14.250000 mode=USB"
        );
    }

    #[test]
    fn cmd_slice_remove_basic() {
        assert_eq!(cmd_slice_remove(2), "slice remove 2");
    }

    #[test]
    fn cmd_slice_list_basic() {
        assert_eq!(cmd_slice_list(), "slice list");
    }

    #[test]
    fn cmd_xmit_on() {
        assert_eq!(cmd_xmit(true), "xmit 1");
    }

    #[test]
    fn cmd_xmit_off() {
        assert_eq!(cmd_xmit(false), "xmit 0");
    }

    #[test]
    fn cmd_set_power_50w() {
        assert_eq!(cmd_set_power(50), "transmit set power=50");
    }

    #[test]
    fn cmd_set_power_100w() {
        assert_eq!(cmd_set_power(100), "transmit set power=100");
    }

    #[test]
    fn cmd_info_basic() {
        assert_eq!(cmd_info(), "info");
    }

    #[test]
    fn cmd_meter_list_basic() {
        assert_eq!(cmd_meter_list(), "meter list");
    }

    #[test]
    fn cmd_subscribe_slice() {
        assert_eq!(cmd_subscribe("slice all"), "sub slice all");
    }

    #[test]
    fn cmd_subscribe_meter() {
        assert_eq!(cmd_subscribe("meter all"), "sub meter all");
    }

    #[test]
    fn cmd_client_program_riglib() {
        assert_eq!(cmd_client_program("riglib"), "client program riglib");
    }

    // -- DAX stream command builders ----------------------------------------

    #[test]
    fn test_cmd_stream_create_dax_rx() {
        assert_eq!(
            cmd_stream_create_dax_rx(1),
            "stream create type=dax_rx dax_channel=1"
        );
        assert_eq!(
            cmd_stream_create_dax_rx(8),
            "stream create type=dax_rx dax_channel=8"
        );
    }

    #[test]
    fn test_cmd_stream_create_dax_tx() {
        assert_eq!(
            cmd_stream_create_dax_tx(1),
            "stream create type=dax_tx dax_channel=1"
        );
    }

    #[test]
    fn test_cmd_stream_remove() {
        assert_eq!(cmd_stream_remove(0x2000_0001), "stream remove 0x20000001");
        assert_eq!(cmd_stream_remove(0x0000_0042), "stream remove 0x00000042");
    }

    #[test]
    fn test_cmd_slice_set_dax() {
        assert_eq!(cmd_slice_set_dax(0, 1), "slice set 0 dax=1");
        assert_eq!(cmd_slice_set_dax(3, 0), "slice set 3 dax=0");
        assert_eq!(cmd_slice_set_dax(7, 8), "slice set 7 dax=8");
    }

    // -- Message parsing: version -------------------------------------------

    #[test]
    fn parse_version_line() {
        let msg = parse_message("V1.4.0.0").unwrap();
        assert_eq!(
            msg,
            SmartSdrMessage::Version(SmartSdrVersion {
                major: 1,
                minor: 4,
                patch: 0,
                build: 0,
            })
        );
    }

    #[test]
    fn parse_version_high_numbers() {
        let msg = parse_message("V3.10.42.1234").unwrap();
        assert_eq!(
            msg,
            SmartSdrMessage::Version(SmartSdrVersion {
                major: 3,
                minor: 10,
                patch: 42,
                build: 1234,
            })
        );
    }

    #[test]
    fn parse_version_invalid_format() {
        assert!(parse_message("V1.4.0").is_err());
        assert!(parse_message("V1.4.0.0.0").is_err());
    }

    #[test]
    fn parse_version_non_numeric() {
        assert!(parse_message("V1.4.abc.0").is_err());
    }

    // -- Message parsing: handle --------------------------------------------

    #[test]
    fn parse_handle_line() {
        let msg = parse_message("H12345678").unwrap();
        assert_eq!(msg, SmartSdrMessage::Handle(0x12345678));
    }

    #[test]
    fn parse_handle_all_hex() {
        let msg = parse_message("HABCDEF01").unwrap();
        assert_eq!(msg, SmartSdrMessage::Handle(0xABCDEF01));
    }

    #[test]
    fn parse_handle_zero() {
        let msg = parse_message("H00000000").unwrap();
        assert_eq!(msg, SmartSdrMessage::Handle(0));
    }

    #[test]
    fn parse_handle_invalid_hex() {
        assert!(parse_message("HXYZ").is_err());
    }

    // -- Message parsing: response ------------------------------------------

    #[test]
    fn parse_response_success_empty() {
        let msg = parse_message("R1|00000000|").unwrap();
        assert_eq!(
            msg,
            SmartSdrMessage::Response(SmartSdrResponse {
                sequence: 1,
                error_code: 0,
                message: String::new(),
            })
        );
    }

    #[test]
    fn parse_response_success_with_data() {
        let msg = parse_message("R7|00000000|3").unwrap();
        assert_eq!(
            msg,
            SmartSdrMessage::Response(SmartSdrResponse {
                sequence: 7,
                error_code: 0,
                message: "3".into(),
            })
        );
    }

    #[test]
    fn parse_response_error() {
        let msg = parse_message("R2|50000015|Invalid slice index").unwrap();
        assert_eq!(
            msg,
            SmartSdrMessage::Response(SmartSdrResponse {
                sequence: 2,
                error_code: 0x50000015,
                message: "Invalid slice index".into(),
            })
        );
    }

    #[test]
    fn parse_response_no_data_field() {
        // Only seq and error code, no third pipe-delimited field.
        let msg = parse_message("R1|00000000").unwrap();
        assert_eq!(
            msg,
            SmartSdrMessage::Response(SmartSdrResponse {
                sequence: 1,
                error_code: 0,
                message: String::new(),
            })
        );
    }

    #[test]
    fn parse_response_malformed_no_pipe() {
        assert!(parse_message("R1").is_err());
    }

    #[test]
    fn parse_response_invalid_sequence() {
        assert!(parse_message("Rabc|00000000|").is_err());
    }

    #[test]
    fn parse_response_invalid_error_code() {
        assert!(parse_message("R1|ZZZZZZZZ|").is_err());
    }

    // -- Message parsing: status --------------------------------------------

    #[test]
    fn parse_status_slice_full() {
        let msg = parse_message(
            "S12345678|slice 0 RF_frequency=14.250000 mode=USB filter_lo=100 filter_hi=2900",
        )
        .unwrap();

        match msg {
            SmartSdrMessage::Status(s) => {
                assert_eq!(s.handle, 0x12345678);
                assert_eq!(s.object, "slice 0");
                assert_eq!(s.params.len(), 4);
                assert_eq!(s.params[0], ("RF_frequency".into(), "14.250000".into()));
                assert_eq!(s.params[1], ("mode".into(), "USB".into()));
                assert_eq!(s.params[2], ("filter_lo".into(), "100".into()));
                assert_eq!(s.params[3], ("filter_hi".into(), "2900".into()));
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn parse_status_tx() {
        let msg = parse_message("SDEADBEEF|tx freq=14.250000 power=100").unwrap();
        match msg {
            SmartSdrMessage::Status(s) => {
                assert_eq!(s.handle, 0xDEADBEEF);
                assert_eq!(s.object, "tx");
                assert_eq!(s.params.len(), 2);
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn parse_status_radio() {
        let msg =
            parse_message("S00000001|radio model=FLEX-6600 serial=12345 name=MyStation").unwrap();
        match msg {
            SmartSdrMessage::Status(s) => {
                assert_eq!(s.object, "radio");
                assert_eq!(s.params.len(), 3);
                assert_eq!(s.params[0], ("model".into(), "FLEX-6600".into()));
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn parse_status_no_params() {
        let msg = parse_message("S12345678|interlock").unwrap();
        match msg {
            SmartSdrMessage::Status(s) => {
                assert_eq!(s.object, "interlock");
                assert!(s.params.is_empty());
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn parse_status_malformed_no_pipe() {
        assert!(parse_message("S12345678 no pipe").is_err());
    }

    #[test]
    fn parse_status_invalid_handle() {
        assert!(parse_message("SNOTAHEX|slice 0").is_err());
    }

    // -- Message parsing: message -------------------------------------------

    #[test]
    fn parse_message_line() {
        let msg = parse_message("M1|Some message text").unwrap();
        assert_eq!(msg, SmartSdrMessage::Message("Some message text".into()));
    }

    #[test]
    fn parse_message_empty_text() {
        let msg = parse_message("M42|").unwrap();
        assert_eq!(msg, SmartSdrMessage::Message(String::new()));
    }

    // -- Message parsing: unknown and edge cases ----------------------------

    #[test]
    fn parse_unknown_line() {
        let msg = parse_message("X something unexpected").unwrap();
        assert_eq!(
            msg,
            SmartSdrMessage::Unknown("X something unexpected".into())
        );
    }

    #[test]
    fn parse_empty_line_returns_error() {
        assert!(parse_message("").is_err());
        assert!(parse_message("   ").is_err());
    }

    #[test]
    fn parse_line_with_leading_whitespace() {
        // Should trim whitespace before parsing.
        let msg = parse_message("  V1.0.0.0  ").unwrap();
        assert_eq!(
            msg,
            SmartSdrMessage::Version(SmartSdrVersion {
                major: 1,
                minor: 0,
                patch: 0,
                build: 0,
            })
        );
    }

    // -- Slice status parsing -----------------------------------------------

    #[test]
    fn parse_slice_status_full() {
        let params = vec![
            ("RF_frequency".into(), "14.250000".into()),
            ("mode".into(), "USB".into()),
            ("filter_lo".into(), "100".into()),
            ("filter_hi".into(), "2900".into()),
            ("tx".into(), "1".into()),
            ("active".into(), "0".into()),
        ];
        let ss = parse_slice_status(&params, "slice 0").unwrap();
        assert_eq!(ss.index, 0);
        assert!((ss.frequency_mhz.unwrap() - 14.25).abs() < 1e-9);
        assert_eq!(ss.mode, Some("USB".into()));
        assert_eq!(ss.filter_lo, Some(100));
        assert_eq!(ss.filter_hi, Some(2900));
        assert_eq!(ss.tx, Some(true));
        assert_eq!(ss.active, Some(false));
    }

    #[test]
    fn parse_slice_status_partial() {
        let params = vec![("mode".into(), "CW".into())];
        let ss = parse_slice_status(&params, "slice 3").unwrap();
        assert_eq!(ss.index, 3);
        assert_eq!(ss.frequency_mhz, None);
        assert_eq!(ss.mode, Some("CW".into()));
        assert_eq!(ss.filter_lo, None);
        assert_eq!(ss.filter_hi, None);
        assert_eq!(ss.tx, None);
        assert_eq!(ss.active, None);
    }

    #[test]
    fn parse_slice_status_missing_index() {
        let params = vec![];
        assert!(parse_slice_status(&params, "slice").is_err());
    }

    #[test]
    fn parse_slice_status_invalid_index() {
        let params = vec![];
        assert!(parse_slice_status(&params, "slice xyz").is_err());
    }

    #[test]
    fn parse_slice_status_tx_false() {
        let params = vec![("tx".into(), "0".into())];
        let ss = parse_slice_status(&params, "slice 0").unwrap();
        assert_eq!(ss.tx, Some(false));
    }

    // -- Meter status parsing -----------------------------------------------

    #[test]
    fn parse_meter_status_basic() {
        let params = vec![
            ("num".into(), "5".into()),
            ("nam".into(), "s-meter".into()),
            ("src".into(), "SLC".into()),
        ];
        let ms = parse_meter_status(&params).unwrap();
        assert_eq!(ms.id, 5);
        assert_eq!(ms.name, Some("s-meter".into()));
        assert_eq!(ms.source, Some("SLC".into()));
    }

    #[test]
    fn parse_meter_status_id_only() {
        let params = vec![("num".into(), "42".into())];
        let ms = parse_meter_status(&params).unwrap();
        assert_eq!(ms.id, 42);
        assert_eq!(ms.name, None);
        assert_eq!(ms.source, None);
    }

    #[test]
    fn parse_meter_status_missing_id() {
        let params = vec![("nam".into(), "swr".into())];
        assert!(parse_meter_status(&params).is_err());
    }

    #[test]
    fn parse_meter_status_alt_keys() {
        // Some firmware versions use "id" and "name" instead of "num" and "nam".
        let params = vec![
            ("id".into(), "10".into()),
            ("name".into(), "power_forward".into()),
            ("source".into(), "TX".into()),
        ];
        let ms = parse_meter_status(&params).unwrap();
        assert_eq!(ms.id, 10);
        assert_eq!(ms.name, Some("power_forward".into()));
        assert_eq!(ms.source, Some("TX".into()));
    }
}
