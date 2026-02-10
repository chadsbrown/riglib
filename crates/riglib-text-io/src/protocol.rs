//! Shared text-protocol decode/encode for Kenwood, Elecraft, and Yaesu backends.
//!
//! All three manufacturers use semicolon-terminated ASCII command/response frames.
//! The key difference is in prefix parsing: Kenwood and Elecraft use purely
//! alphabetic prefixes (`FA`, `MD`), while Yaesu includes a trailing digit for
//! certain commands (`MD0`, `RM5`, `SM0`). This module parameterizes that
//! difference via `digit_suffix_prefixes`.

/// The semicolon byte that terminates every text-protocol response.
pub const TERMINATOR: u8 = b';';

/// The error response sent by text-protocol rigs.
pub const ERROR_RESPONSE: &[u8] = b"?;";

/// Result of attempting to decode one response from a byte buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeResult {
    /// A complete response was decoded.
    Response {
        /// Command prefix echoed in the response (e.g. `"FA"`, `"MD0"`).
        prefix: String,
        /// Data payload (everything between the prefix and the terminator).
        data: String,
        /// Number of bytes consumed from the input buffer.
        consumed: usize,
    },

    /// The rig returned the error response `?;`.
    Error(usize),

    /// The buffer does not yet contain a complete response. More data is needed.
    Incomplete,
}

/// Decode one semicolon-terminated response from a byte buffer.
///
/// `digit_suffix_prefixes` lists command prefixes (e.g. `["MD", "RM", "SM"]`)
/// that include a trailing digit as part of the command name. Pass `&[]` for
/// Kenwood/Elecraft (purely alphabetic prefixes) or the Yaesu list for Yaesu.
///
/// Returns the first complete response found, or [`DecodeResult::Incomplete`]
/// if no terminator is present yet.
pub fn decode_response(buf: &[u8], digit_suffix_prefixes: &[&str]) -> DecodeResult {
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
            return DecodeResult::Error(consumed);
        }
    };

    // Split into prefix and data.
    //
    // The prefix consists of leading uppercase ASCII letters, optionally
    // followed by a digit that is part of the command name (for Yaesu
    // commands like MD0, RM5, SM0, SH0, NA0).
    let alpha_end = body_str
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(body_str.len());

    let alpha_prefix = &body_str[..alpha_end];

    // If this prefix is in the digit-suffix list and the next char is a digit,
    // include that digit in the prefix.
    let prefix_end = if digit_suffix_prefixes.contains(&alpha_prefix)
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

/// Extract the command prefix from a command byte sequence.
///
/// The prefix is the leading alphabetic characters, optionally followed by
/// a digit if the prefix is in `digit_suffix_prefixes`. This matches the
/// prefix that [`decode_response`] would extract from the rig's reply.
///
/// # Examples
///
/// ```
/// use riglib_text_io::protocol::extract_command_prefix;
///
/// // Kenwood/Elecraft (no digit suffixes):
/// assert_eq!(extract_command_prefix(b"FA00014074000;", &[]), "FA");
/// assert_eq!(extract_command_prefix(b"MD2;", &[]), "MD");
///
/// // Yaesu (with digit suffixes):
/// let dsp = &["MD", "RM", "SM", "SH", "NA", "AN", "PA", "RA"];
/// assert_eq!(extract_command_prefix(b"MD02;", dsp), "MD0");
/// assert_eq!(extract_command_prefix(b"FA014250000;", dsp), "FA");
/// ```
pub fn extract_command_prefix(cmd: &[u8], digit_suffix_prefixes: &[&str]) -> String {
    let s = std::str::from_utf8(cmd).unwrap_or("");
    let alpha_end = s
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());

    let alpha_prefix = &s[..alpha_end];

    if digit_suffix_prefixes.contains(&alpha_prefix)
        && alpha_end < s.len()
        && s.as_bytes()[alpha_end].is_ascii_digit()
    {
        s[..alpha_end + 1].to_string()
    } else {
        alpha_prefix.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Yaesu digit-suffix prefixes for parameterized tests.
    const YAESU_DSP: &[&str] = &["MD", "RM", "SM", "SH", "NA", "AN", "PA", "RA"];

    // -----------------------------------------------------------------------
    // decode_response — basic cases
    // -----------------------------------------------------------------------

    #[test]
    fn decode_empty_buffer() {
        assert_eq!(decode_response(b"", &[]), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_no_terminator() {
        assert_eq!(
            decode_response(b"FA00014074000", &[]),
            DecodeResult::Incomplete
        );
    }

    #[test]
    fn decode_error_response() {
        assert_eq!(decode_response(b"?;", &[]), DecodeResult::Error(2));
    }

    // -----------------------------------------------------------------------
    // decode_response — Kenwood/Elecraft style (no digit suffixes)
    // -----------------------------------------------------------------------

    #[test]
    fn decode_kenwood_frequency() {
        assert_eq!(
            decode_response(b"FA00014074000;", &[]),
            DecodeResult::Response {
                prefix: "FA".into(),
                data: "00014074000".into(),
                consumed: 14,
            }
        );
    }

    #[test]
    fn decode_kenwood_mode() {
        assert_eq!(
            decode_response(b"MD3;", &[]),
            DecodeResult::Response {
                prefix: "MD".into(),
                data: "3".into(),
                consumed: 4,
            }
        );
    }

    #[test]
    fn decode_kenwood_ptt() {
        assert_eq!(
            decode_response(b"TX1;", &[]),
            DecodeResult::Response {
                prefix: "TX".into(),
                data: "1".into(),
                consumed: 4,
            }
        );
    }

    #[test]
    fn decode_kenwood_alpha_only_prefix() {
        // A query response with no data portion.
        assert_eq!(
            decode_response(b"TX;", &[]),
            DecodeResult::Response {
                prefix: "TX".into(),
                data: "".into(),
                consumed: 3,
            }
        );
    }

    // -----------------------------------------------------------------------
    // decode_response — Yaesu style (with digit suffixes)
    // -----------------------------------------------------------------------

    #[test]
    fn decode_yaesu_mode_with_digit_suffix() {
        assert_eq!(
            decode_response(b"MD03;", YAESU_DSP),
            DecodeResult::Response {
                prefix: "MD0".into(),
                data: "3".into(),
                consumed: 5,
            }
        );
    }

    #[test]
    fn decode_yaesu_smeter_with_digit_suffix() {
        assert_eq!(
            decode_response(b"SM0015;", YAESU_DSP),
            DecodeResult::Response {
                prefix: "SM0".into(),
                data: "015".into(),
                consumed: 7,
            }
        );
    }

    #[test]
    fn decode_yaesu_non_dsp_prefix() {
        // FA is not in the digit suffix list, so digits are data.
        assert_eq!(
            decode_response(b"FA014250000;", YAESU_DSP),
            DecodeResult::Response {
                prefix: "FA".into(),
                data: "014250000".into(),
                consumed: 12,
            }
        );
    }

    #[test]
    fn decode_yaesu_ptt() {
        // TX is not in the digit suffix list.
        assert_eq!(
            decode_response(b"TX1;", YAESU_DSP),
            DecodeResult::Response {
                prefix: "TX".into(),
                data: "1".into(),
                consumed: 4,
            }
        );
    }

    // -----------------------------------------------------------------------
    // decode_response — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn decode_non_utf8_is_error() {
        let buf = [0xFF, 0xFE, b';'];
        assert_eq!(decode_response(&buf, &[]), DecodeResult::Error(3));
    }

    #[test]
    fn decode_multiple_in_buffer() {
        // Two complete responses — only the first is returned.
        let buf = b"FA00014074000;MD3;";
        assert_eq!(
            decode_response(buf, &[]),
            DecodeResult::Response {
                prefix: "FA".into(),
                data: "00014074000".into(),
                consumed: 14,
            }
        );
    }

    #[test]
    fn decode_complete_plus_incomplete() {
        let buf = b"FA00014074000;MD";
        assert_eq!(
            decode_response(buf, &[]),
            DecodeResult::Response {
                prefix: "FA".into(),
                data: "00014074000".into(),
                consumed: 14,
            }
        );
    }

    // -----------------------------------------------------------------------
    // extract_command_prefix
    // -----------------------------------------------------------------------

    #[test]
    fn extract_prefix_kenwood_fa() {
        assert_eq!(extract_command_prefix(b"FA00014074000;", &[]), "FA");
    }

    #[test]
    fn extract_prefix_kenwood_md() {
        assert_eq!(extract_command_prefix(b"MD2;", &[]), "MD");
    }

    #[test]
    fn extract_prefix_kenwood_tx_query() {
        assert_eq!(extract_command_prefix(b"TX;", &[]), "TX");
    }

    #[test]
    fn extract_prefix_yaesu_mode() {
        assert_eq!(extract_command_prefix(b"MD02;", YAESU_DSP), "MD0");
    }

    #[test]
    fn extract_prefix_yaesu_non_dsp() {
        assert_eq!(extract_command_prefix(b"FA014250000;", YAESU_DSP), "FA");
    }

    #[test]
    fn extract_prefix_yaesu_smeter() {
        assert_eq!(extract_command_prefix(b"SM0;", YAESU_DSP), "SM0");
    }

    #[test]
    fn extract_prefix_empty() {
        assert_eq!(extract_command_prefix(b"", &[]), "");
    }

    #[test]
    fn extract_prefix_non_utf8() {
        assert_eq!(extract_command_prefix(&[0xFF, 0xFE], &[]), "");
    }
}
