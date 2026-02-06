//! Elecraft CAT text-protocol encoder/decoder.
//!
//! Elecraft radios (K3, K3S, KX3, KX2, K4) use an extended Kenwood-style
//! text protocol. Commands are two-letter prefixes followed by ASCII
//! parameters, terminated with `;`. This module provides the low-level
//! encode/decode layer that is identical in format to the Kenwood protocol
//! but re-implemented independently for crate isolation.
//!
//! # Command format
//!
//! ```text
//! <prefix><params>;
//! ```
//!
//! - `prefix`: Two uppercase ASCII characters identifying the command
//!   (e.g. `FA`, `MD`, `TX`, `BW`, `FW`).
//! - `params`: Zero or more ASCII characters (digits, etc.).
//! - Terminator: `;` (0x3B).
//!
//! # Response format
//!
//! Responses echo the command prefix, followed by data, terminated with `;`.
//! The error response for an unrecognised or invalid command is `?;`.
//!
//! # AI (Auto Information) mode
//!
//! When AI mode is enabled (`AI2;`), the rig pushes unsolicited state-change
//! messages using the same response format (e.g. `FA00014250000;`, `MD2;`).
//! These can be decoded with [`decode_response`] just like polled responses.

use bytes::{BufMut, BytesMut};

/// CAT command/response terminator byte.
pub const TERMINATOR: u8 = b';';

/// Error response from the rig: `?;`.
pub const ERROR_RESPONSE: &[u8] = b"?;";

/// Result of attempting to decode a CAT response from a byte buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeResult {
    /// A complete response was decoded.
    ///
    /// - `prefix`: the command prefix (e.g. `"FA"`, `"MD"`, `"BW"`).
    /// - `data`: the parameter/value portion after the prefix, before `;`.
    /// - The `consumed` field is the number of bytes consumed from the input
    ///   buffer (including the terminator).
    Response {
        /// Command prefix echoed in the response.
        prefix: String,
        /// Data payload (everything between the prefix and the terminator).
        data: String,
        /// Number of bytes consumed from the input buffer.
        consumed: usize,
    },

    /// The rig returned the error response `?;`.
    ///
    /// The `usize` is the number of bytes consumed from the input buffer.
    Error(usize),

    /// The buffer does not yet contain a complete response. More data is needed.
    Incomplete,
}

/// Encode a CAT command into raw bytes ready for transmission.
///
/// Concatenates the command prefix, parameters, and the terminator `;`.
///
/// # Arguments
///
/// * `prefix` - The command prefix (e.g. `"FA"`, `"MD"`, `"BW"`, `"FW"`).
/// * `params` - Parameter string (may be empty for read commands).
///
/// # Example
///
/// ```
/// use riglib_elecraft::protocol::encode_command;
///
/// let cmd = encode_command("FA", "");
/// assert_eq!(cmd, b"FA;");
///
/// let cmd = encode_command("FA", "00014250000");
/// assert_eq!(cmd, b"FA00014250000;");
///
/// let cmd = encode_command("BW", "0500");
/// assert_eq!(cmd, b"BW0500;");
/// ```
pub fn encode_command(prefix: &str, params: &str) -> Vec<u8> {
    let capacity = prefix.len() + params.len() + 1;
    let mut buf = BytesMut::with_capacity(capacity);
    buf.put_slice(prefix.as_bytes());
    buf.put_slice(params.as_bytes());
    buf.put_u8(TERMINATOR);
    buf.to_vec()
}

/// Attempt to decode one CAT response from a byte buffer.
///
/// Scans `buf` for a semicolon terminator. Returns [`DecodeResult::Response`]
/// on success with the number of bytes consumed, [`DecodeResult::Error`] if
/// the response is `?;`, or [`DecodeResult::Incomplete`] if no complete
/// response is available yet.
///
/// # Prefix/data split
///
/// The prefix is extracted as the leading alphabetic characters. Elecraft
/// uses the same convention as Kenwood: two-letter alphabetic prefixes
/// (FA, FB, MD, BW, FW, etc.) followed by numeric or alphanumeric data.
///
/// # Example
///
/// ```
/// use riglib_elecraft::protocol::{decode_response, DecodeResult};
///
/// let buf = b"FA00014250000;";
/// match decode_response(buf) {
///     DecodeResult::Response { prefix, data, consumed } => {
///         assert_eq!(prefix, "FA");
///         assert_eq!(data, "00014250000");
///         assert_eq!(consumed, 14);
///     }
///     _ => panic!("expected Response"),
/// }
/// ```
pub fn decode_response(buf: &[u8]) -> DecodeResult {
    if buf.is_empty() {
        return DecodeResult::Incomplete;
    }

    // Find the terminator.
    let term_pos = match buf.iter().position(|&b| b == TERMINATOR) {
        Some(pos) => pos,
        None => return DecodeResult::Incomplete,
    };

    let consumed = term_pos + 1;
    let body = &buf[..term_pos];

    // Check for error response: body is just `?`.
    if body == b"?" {
        return DecodeResult::Error(consumed);
    }

    // Convert body to a string. If it contains non-UTF-8 bytes, treat as error.
    let body_str = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(_) => {
            // Malformed data -- skip past the terminator.
            return DecodeResult::Error(consumed);
        }
    };

    // Split into prefix and data.
    //
    // Elecraft command prefixes are purely alphabetic (FA, FB, MD, TX, RX,
    // PC, SM, RM, FR, FT, AI, BW, FW, KS, AP, DS, DT, etc.). The data
    // portion starts at the first non-alpha character.
    let alpha_end = body_str
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(body_str.len());

    let prefix = body_str[..alpha_end].to_string();
    let data = body_str[alpha_end..].to_string();

    DecodeResult::Response {
        prefix,
        data,
        consumed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Command encoding
    // ---------------------------------------------------------------

    #[test]
    fn encode_read_frequency_a() {
        let cmd = encode_command("FA", "");
        assert_eq!(cmd, b"FA;");
    }

    #[test]
    fn encode_set_frequency_a() {
        let cmd = encode_command("FA", "00014250000");
        assert_eq!(cmd, b"FA00014250000;");
    }

    #[test]
    fn encode_read_mode() {
        let cmd = encode_command("MD", "");
        assert_eq!(cmd, b"MD;");
    }

    #[test]
    fn encode_set_mode() {
        let cmd = encode_command("MD", "2");
        assert_eq!(cmd, b"MD2;");
    }

    #[test]
    fn encode_ptt_on() {
        let cmd = encode_command("TX", "1");
        assert_eq!(cmd, b"TX1;");
    }

    #[test]
    fn encode_ptt_off() {
        let cmd = encode_command("TX", "0");
        assert_eq!(cmd, b"TX0;");
    }

    #[test]
    fn encode_read_ptt() {
        let cmd = encode_command("TX", "");
        assert_eq!(cmd, b"TX;");
    }

    #[test]
    fn encode_set_power() {
        let cmd = encode_command("PC", "050");
        assert_eq!(cmd, b"PC050;");
    }

    #[test]
    fn encode_read_s_meter() {
        let cmd = encode_command("SM", "0");
        assert_eq!(cmd, b"SM0;");
    }

    #[test]
    fn encode_split_on() {
        let cmd1 = encode_command("FR", "0");
        let cmd2 = encode_command("FT", "1");
        assert_eq!(cmd1, b"FR0;");
        assert_eq!(cmd2, b"FT1;");
    }

    #[test]
    fn encode_auto_info_on() {
        let cmd = encode_command("AI", "2");
        assert_eq!(cmd, b"AI2;");
    }

    #[test]
    fn encode_bandwidth_bw() {
        let cmd = encode_command("BW", "0500");
        assert_eq!(cmd, b"BW0500;");
    }

    #[test]
    fn encode_bandwidth_fw() {
        let cmd = encode_command("FW", "0500");
        assert_eq!(cmd, b"FW0500;");
    }

    #[test]
    fn encode_k3_identify() {
        let cmd = encode_command("K3", "");
        assert_eq!(cmd, b"K3;");
    }

    #[test]
    fn encode_k4_identify() {
        let cmd = encode_command("K4", "");
        assert_eq!(cmd, b"K4;");
    }

    // ---------------------------------------------------------------
    // Response decoding -- valid responses
    // ---------------------------------------------------------------

    #[test]
    fn decode_frequency_response() {
        let buf = b"FA00014250000;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FA");
                assert_eq!(data, "00014250000");
                assert_eq!(consumed, 14);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_frequency_b_response() {
        let buf = b"FB00007000000;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FB");
                assert_eq!(data, "00007000000");
                assert_eq!(consumed, 14);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_mode_response() {
        let buf = b"MD2;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "MD");
                assert_eq!(data, "2");
                assert_eq!(consumed, 4);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_ptt_response() {
        let buf = b"TX1;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "TX");
                assert_eq!(data, "1");
                assert_eq!(consumed, 4);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_power_response() {
        let buf = b"PC050;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "PC");
                assert_eq!(data, "050");
                assert_eq!(consumed, 6);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_s_meter_response() {
        let buf = b"SM00015;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "SM");
                assert_eq!(data, "00015");
                assert_eq!(consumed, 8);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_swr_meter_response() {
        let buf = b"RM10045;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "RM");
                assert_eq!(data, "10045");
                assert_eq!(consumed, 8);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_bandwidth_bw_response() {
        let buf = b"BW0500;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "BW");
                assert_eq!(data, "0500");
                assert_eq!(consumed, 7);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_bandwidth_fw_response() {
        let buf = b"FW0500;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FW");
                assert_eq!(data, "0500");
                assert_eq!(consumed, 7);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_split_fr_response() {
        let buf = b"FR0;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FR");
                assert_eq!(data, "0");
                assert_eq!(consumed, 4);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_split_ft_response() {
        let buf = b"FT1;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FT");
                assert_eq!(data, "1");
                assert_eq!(consumed, 4);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_auto_info_response() {
        let buf = b"AI2;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "AI");
                assert_eq!(data, "2");
                assert_eq!(consumed, 4);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_k3_identify_response() {
        // K3; returns just "K3" with empty data (prefix-only response)
        let buf = b"K3;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "K");
                assert_eq!(data, "3");
                assert_eq!(consumed, 3);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Response decoding -- error and edge cases
    // ---------------------------------------------------------------

    #[test]
    fn decode_error_response() {
        let buf = b"?;";
        match decode_response(buf) {
            DecodeResult::Error(consumed) => {
                assert_eq!(consumed, 2);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn decode_incomplete_no_terminator() {
        let buf = b"FA00014250000";
        assert_eq!(decode_response(buf), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_incomplete_empty() {
        assert_eq!(decode_response(b""), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_multiple_responses_in_buffer() {
        let buf = b"FA00014250000;MD2;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FA");
                assert_eq!(data, "00014250000");
                assert_eq!(consumed, 14);

                // Decode the remainder to get the second response.
                match decode_response(&buf[consumed..]) {
                    DecodeResult::Response {
                        prefix: p2,
                        data: d2,
                        consumed: c2,
                    } => {
                        assert_eq!(p2, "MD");
                        assert_eq!(d2, "2");
                        assert_eq!(c2, 4);
                    }
                    other => panic!("expected second Response, got {other:?}"),
                }
            }
            other => panic!("expected first Response, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Round-trip: encode then decode
    // ---------------------------------------------------------------

    #[test]
    fn round_trip_read_command() {
        let cmd = encode_command("FA", "");
        match decode_response(&cmd) {
            DecodeResult::Response { prefix, data, .. } => {
                assert_eq!(prefix, "FA");
                assert_eq!(data, "");
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_set_frequency() {
        let cmd = encode_command("FA", "00014250000");
        match decode_response(&cmd) {
            DecodeResult::Response { prefix, data, .. } => {
                assert_eq!(prefix, "FA");
                assert_eq!(data, "00014250000");
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_set_mode() {
        let cmd = encode_command("MD", "2");
        match decode_response(&cmd) {
            DecodeResult::Response { prefix, data, .. } => {
                assert_eq!(prefix, "MD");
                assert_eq!(data, "2");
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_bandwidth() {
        let cmd = encode_command("BW", "2700");
        match decode_response(&cmd) {
            DecodeResult::Response { prefix, data, .. } => {
                assert_eq!(prefix, "BW");
                assert_eq!(data, "2700");
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }
}
