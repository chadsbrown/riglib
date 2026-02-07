//! Yaesu CAT text-protocol encoder/decoder.
//!
//! The Yaesu CAT protocol uses semicolon-terminated ASCII commands over a
//! serial link. Unlike Icom CI-V there is no binary preamble, no addressing
//! scheme, and no BCD encoding. Commands are two-letter prefixes followed
//! by ASCII parameters, terminated with `;`.
//!
//! # Command format
//!
//! ```text
//! <prefix><params>;
//! ```
//!
//! - `prefix`: Two (or more) uppercase ASCII characters identifying the command
//!   (e.g. `FA`, `MD0`, `TX`, `RM1`).
//! - `params`: Zero or more ASCII characters (digits, hex letters, etc.).
//! - Terminator: `;` (0x3B).
//!
//! # Response format
//!
//! Responses echo the command prefix, followed by data, terminated with `;`.
//! The error response for an unrecognised or invalid command is `?;`.

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
    /// - `prefix`: the command prefix (e.g. `"FA"`, `"MD0"`, `"TX"`).
    /// - `data`: the parameter/value portion after the prefix, before `;`.
    /// - The `usize` is the number of bytes consumed from the input buffer
    ///   (including the terminator).
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
/// * `prefix` - The command prefix (e.g. `"FA"`, `"MD0"`, `"TX"`).
/// * `params` - Parameter string (may be empty for read commands).
///
/// # Example
///
/// ```
/// use riglib_yaesu::protocol::encode_command;
///
/// let cmd = encode_command("FA", "");
/// assert_eq!(cmd, b"FA;");
///
/// let cmd = encode_command("FA", "014250000");
/// assert_eq!(cmd, b"FA014250000;");
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
/// Scans `buf` for a semicolon terminator. Any bytes before the first
/// printable ASCII character are silently skipped (noise on the serial line).
///
/// Returns [`DecodeResult::Response`] on success with the number of bytes
/// consumed, [`DecodeResult::Error`] if the response is `?;`, or
/// [`DecodeResult::Incomplete`] if no complete response is available yet.
///
/// # Prefix/data split
///
/// The prefix is extracted as the leading alphabetic (plus digit suffixes that
/// are part of the command name, e.g. `MD0`, `RM1`, `SM0`) characters of the
/// response. The data is everything after the prefix up to (but not including)
/// the terminator.
///
/// # Example
///
/// ```
/// use riglib_yaesu::protocol::{decode_response, DecodeResult};
///
/// let buf = b"FA014250000;";
/// match decode_response(buf) {
///     DecodeResult::Response { prefix, data, consumed } => {
///         assert_eq!(prefix, "FA");
///         assert_eq!(data, "014250000");
///         assert_eq!(consumed, 12);
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
            // Malformed data — skip past the terminator.
            return DecodeResult::Error(consumed);
        }
    };

    // Split into prefix and data.
    //
    // The prefix consists of leading uppercase ASCII letters, optionally
    // followed by digits that are part of the command name (e.g. MD0, MD1,
    // RM1, RM5, SM0, SH0, NA0, AI). We determine where the "command name"
    // ends and where the "data" begins.
    //
    // Heuristic: the prefix is the leading letters, plus any digits that
    // immediately follow the letters but are part of the command name.
    // Yaesu command names have at most one trailing digit in the prefix
    // (e.g. MD0, MD1, RM1, RM5, SM0, SH0, NA0).
    //
    // For commands like FA, FB, TX, FT, PC, AI — the prefix is purely
    // alphabetic and the data portion contains all digits.
    //
    // For commands like MD0, MD1, RM1, RM5, SM0 — the digit is part of
    // the prefix and identifies which sub-function is being addressed.
    //
    // Strategy: take all leading alpha characters. Then, if the resulting
    // prefix is one of the known multi-character prefixes that take a
    // digit suffix (MD, RM, SM, SH, NA), consume one more digit as part
    // of the prefix.
    let alpha_end = body_str
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(body_str.len());

    let alpha_prefix = &body_str[..alpha_end];

    // Known command prefixes that include a trailing digit as part of the
    // command name.
    const DIGIT_SUFFIX_PREFIXES: &[&str] =
        &["MD", "RM", "SM", "SH", "NA", "AN", "PA", "RA", "RT", "XT"];

    let prefix_end = if DIGIT_SUFFIX_PREFIXES.contains(&alpha_prefix)
        && alpha_end < body_str.len()
        && body_str.as_bytes()[alpha_end].is_ascii_digit()
    {
        alpha_end + 1
    } else {
        alpha_end
    };

    let prefix = body_str[..prefix_end].to_string();
    let data = body_str[prefix_end..].to_string();

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
        let cmd = encode_command("FA", "014250000");
        assert_eq!(cmd, b"FA014250000;");
    }

    #[test]
    fn encode_read_mode_a() {
        let cmd = encode_command("MD0", "");
        assert_eq!(cmd, b"MD0;");
    }

    #[test]
    fn encode_set_mode_a() {
        let cmd = encode_command("MD0", "2");
        assert_eq!(cmd, b"MD02;");
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
        let cmd = encode_command("SM0", "");
        assert_eq!(cmd, b"SM0;");
    }

    #[test]
    fn encode_split_on() {
        let cmd = encode_command("FT", "1");
        assert_eq!(cmd, b"FT1;");
    }

    #[test]
    fn encode_auto_info_on() {
        let cmd = encode_command("AI", "2");
        assert_eq!(cmd, b"AI2;");
    }

    // ---------------------------------------------------------------
    // Response decoding — valid responses
    // ---------------------------------------------------------------

    #[test]
    fn decode_frequency_response() {
        let buf = b"FA014250000;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FA");
                assert_eq!(data, "014250000");
                assert_eq!(consumed, 12);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_frequency_b_response() {
        let buf = b"FB007000000;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FB");
                assert_eq!(data, "007000000");
                assert_eq!(consumed, 12);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_mode_response() {
        let buf = b"MD02;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "MD0");
                assert_eq!(data, "2");
                assert_eq!(consumed, 5);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_mode_b_response() {
        let buf = b"MD11;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "MD1");
                assert_eq!(data, "1");
                assert_eq!(consumed, 5);
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
        let buf = b"SM0128;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "SM0");
                assert_eq!(data, "128");
                assert_eq!(consumed, 7);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_swr_meter_response() {
        let buf = b"RM1045;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "RM1");
                assert_eq!(data, "045");
                assert_eq!(consumed, 7);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_alc_meter_response() {
        let buf = b"RM5100;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "RM5");
                assert_eq!(data, "100");
                assert_eq!(consumed, 7);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_split_response() {
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

    // ---------------------------------------------------------------
    // Response decoding — error and edge cases
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
        let buf = b"FA014250000";
        assert_eq!(decode_response(buf), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_incomplete_empty() {
        assert_eq!(decode_response(b""), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_multiple_responses_in_buffer() {
        let buf = b"FA014250000;MD02;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "FA");
                assert_eq!(data, "014250000");
                assert_eq!(consumed, 12);

                // Decode the remainder to get the second response.
                match decode_response(&buf[consumed..]) {
                    DecodeResult::Response {
                        prefix: p2,
                        data: d2,
                        consumed: c2,
                    } => {
                        assert_eq!(p2, "MD0");
                        assert_eq!(d2, "2");
                        assert_eq!(c2, 5);
                    }
                    other => panic!("expected second Response, got {other:?}"),
                }
            }
            other => panic!("expected first Response, got {other:?}"),
        }
    }

    #[test]
    fn decode_response_with_hex_mode_code() {
        // Mode C = DATA-USB on Yaesu
        let buf = b"MD0C;";
        match decode_response(buf) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                assert_eq!(prefix, "MD0");
                assert_eq!(data, "C");
                assert_eq!(consumed, 5);
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Round-trip: encode then decode
    // ---------------------------------------------------------------

    #[test]
    fn round_trip_read_command() {
        let cmd = encode_command("FA", "");
        // A read command wouldn't normally come back as a response with
        // empty data, but encoding is still valid.
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
        let cmd = encode_command("FA", "014250000");
        match decode_response(&cmd) {
            DecodeResult::Response { prefix, data, .. } => {
                assert_eq!(prefix, "FA");
                assert_eq!(data, "014250000");
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_set_mode() {
        let cmd = encode_command("MD0", "2");
        match decode_response(&cmd) {
            DecodeResult::Response { prefix, data, .. } => {
                assert_eq!(prefix, "MD0");
                assert_eq!(data, "2");
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }
}
