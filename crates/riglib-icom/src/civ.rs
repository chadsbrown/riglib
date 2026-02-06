//! CI-V frame encoder/decoder.
//!
//! The Icom CI-V (Communication Interface V) protocol uses binary frames on a
//! half-duplex bus. This module handles the pure byte-level encoding and
//! decoding of CI-V frames, BCD frequency conversion, and collision detection.
//!
//! # Frame format
//!
//! ```text
//! 0xFE 0xFE <dst> <src> <cmd> [<sub>] [<data>...] 0xFD
//! ```
//!
//! - Preamble: two `0xFE` bytes
//! - `dst`: target CI-V address (e.g. `0x98` for IC-7610)
//! - `src`: controller address (typically `0xE0`)
//! - `cmd`: command byte
//! - `sub`: optional sub-command byte
//! - `data`: variable-length payload (BCD-encoded for frequencies)
//! - Terminator: `0xFD`

use bytes::{BufMut, BytesMut};
use riglib_core::Error;

/// Preamble byte repeated twice at the start of every CI-V frame.
pub const PREAMBLE: u8 = 0xFE;

/// Frame terminator byte.
pub const TERMINATOR: u8 = 0xFD;

/// Standard PC controller CI-V address.
pub const CONTROLLER_ADDR: u8 = 0xE0;

/// ACK command byte — positive acknowledgement from the rig.
pub const ACK: u8 = 0xFB;

/// NAK command byte — negative acknowledgement from the rig.
pub const NAK: u8 = 0xFA;

/// Collision indicator byte on the CI-V bus.
///
/// When two devices transmit simultaneously on the shared CI-V bus, the
/// echoed byte may read as `0xFC` instead of the transmitted value. The
/// controller should back off and retry after detecting this.
pub const COLLISION: u8 = 0xFC;

/// A parsed CI-V frame.
///
/// This is the protocol-level representation of a single CI-V message,
/// whether it is a command from the controller or a response from the rig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivFrame {
    /// Destination CI-V address.
    pub dst_addr: u8,
    /// Source CI-V address.
    pub src_addr: u8,
    /// Command byte.
    pub cmd: u8,
    /// Optional sub-command byte. `None` for commands that have no sub-command.
    pub sub_cmd: Option<u8>,
    /// Payload data bytes (may be empty).
    pub data: Vec<u8>,
}

impl CivFrame {
    /// Returns `true` if this frame is a positive acknowledgement (ACK).
    pub fn is_ack(&self) -> bool {
        self.cmd == ACK && self.sub_cmd.is_none() && self.data.is_empty()
    }

    /// Returns `true` if this frame is a negative acknowledgement (NAK).
    pub fn is_nak(&self) -> bool {
        self.cmd == NAK && self.sub_cmd.is_none() && self.data.is_empty()
    }

    /// Returns `true` if this frame is a data response (not ACK/NAK).
    pub fn is_data_response(&self) -> bool {
        !self.is_ack() && !self.is_nak()
    }
}

/// Encode a CI-V frame into raw bytes ready for transmission.
///
/// Produces the full wire format including preamble and terminator.
///
/// # Example
///
/// ```
/// use riglib_icom::civ::{encode_frame, CONTROLLER_ADDR};
///
/// // Read-frequency command to IC-7610 (addr 0x98)
/// let bytes = encode_frame(0x98, CONTROLLER_ADDR, 0x03, None, &[]);
/// assert_eq!(bytes, vec![0xFE, 0xFE, 0x98, 0xE0, 0x03, 0xFD]);
/// ```
pub fn encode_frame(
    dst_addr: u8,
    src_addr: u8,
    cmd: u8,
    sub_cmd: Option<u8>,
    data: &[u8],
) -> Vec<u8> {
    let capacity = 4 + 1 + sub_cmd.is_some() as usize + data.len() + 1;
    let mut buf = BytesMut::with_capacity(capacity);
    buf.put_u8(PREAMBLE);
    buf.put_u8(PREAMBLE);
    buf.put_u8(dst_addr);
    buf.put_u8(src_addr);
    buf.put_u8(cmd);
    if let Some(sub) = sub_cmd {
        buf.put_u8(sub);
    }
    buf.put_slice(data);
    buf.put_u8(TERMINATOR);
    buf.to_vec()
}

/// Encode a [`CivFrame`] into raw bytes.
///
/// Convenience wrapper around [`encode_frame`] that takes a frame struct.
pub fn encode_civ_frame(frame: &CivFrame) -> Vec<u8> {
    encode_frame(
        frame.dst_addr,
        frame.src_addr,
        frame.cmd,
        frame.sub_cmd,
        &frame.data,
    )
}

/// Result of attempting to decode a frame from a byte buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeResult {
    /// A complete frame was decoded. The `usize` is the number of bytes
    /// consumed from the input buffer (including preamble and terminator).
    Frame(CivFrame, usize),

    /// The buffer does not yet contain a complete frame. More data is needed.
    Incomplete,

    /// A CI-V bus collision was detected in the buffer. The `usize` is the
    /// number of bytes to discard.
    Collision(usize),
}

/// Attempt to decode one CI-V frame from a byte buffer.
///
/// Scans `buf` for a valid preamble (`0xFE 0xFE`) followed by a terminator
/// (`0xFD`). Any bytes before the first preamble are silently skipped
/// (garbage or inter-frame noise on the CI-V bus).
///
/// Returns [`DecodeResult::Frame`] on success with the number of bytes
/// consumed (caller should drain these from the buffer),
/// [`DecodeResult::Incomplete`] if no complete frame is available yet,
/// or [`DecodeResult::Collision`] if a collision marker is found.
///
/// # Example
///
/// ```
/// use riglib_icom::civ::{decode_frame, DecodeResult, ACK};
///
/// // ACK from IC-7610 (0x98) to controller (0xE0)
/// let buf = vec![0xFE, 0xFE, 0xE0, 0x98, 0xFB, 0xFD];
/// match decode_frame(&buf) {
///     DecodeResult::Frame(frame, consumed) => {
///         assert!(frame.is_ack());
///         assert_eq!(consumed, 6);
///     }
///     _ => panic!("expected a frame"),
/// }
/// ```
pub fn decode_frame(buf: &[u8]) -> DecodeResult {
    // Find the start of a preamble (two consecutive 0xFE bytes).
    let preamble_pos = match find_preamble(buf) {
        Some(pos) => pos,
        None => return DecodeResult::Incomplete,
    };

    // Check for collision marker anywhere between preamble and terminator.
    let after_preamble = preamble_pos + 2;
    if after_preamble >= buf.len() {
        return DecodeResult::Incomplete;
    }

    // Find the terminator after the preamble.
    let search_start = after_preamble;
    let term_pos = match buf[search_start..].iter().position(|&b| b == TERMINATOR) {
        Some(rel) => search_start + rel,
        None => {
            // Check for collision marker in the partial data.
            if buf[search_start..].contains(&COLLISION) {
                return DecodeResult::Collision(buf.len());
            }
            return DecodeResult::Incomplete;
        }
    };

    // Check for collision marker in the frame body.
    if buf[search_start..term_pos].contains(&COLLISION) {
        let consumed = term_pos + 1;
        return DecodeResult::Collision(consumed);
    }

    // We need at least dst + src + cmd (3 bytes) between the preamble and terminator.
    let body = &buf[after_preamble..term_pos];
    if body.len() < 3 {
        // Malformed frame — skip past the terminator and try again.
        let consumed = term_pos + 1;
        return DecodeResult::Collision(consumed);
    }

    let dst_addr = body[0];
    let src_addr = body[1];
    let cmd = body[2];

    // ACK and NAK frames have no sub-command or data.
    let (sub_cmd, data) = if cmd == ACK || cmd == NAK {
        (None, Vec::new())
    } else if body.len() > 3 {
        // The remaining bytes could be sub_cmd + data, or just data.
        // CI-V protocol ambiguity: we store the first extra byte as sub_cmd
        // and the rest as data. Command builders and response parsers know
        // which commands have sub-commands and which do not.
        //
        // For commands without a sub-command (like 0x00 set-freq, 0x03 read-freq,
        // 0x04 read-mode), the sub_cmd/data split is handled by the higher-level
        // command/response parsers. Here we just split generically: if there's
        // extra payload beyond the command byte, first byte goes to sub_cmd,
        // rest to data.
        (Some(body[3]), body[4..].to_vec())
    } else {
        // Only cmd byte, no sub-command or data.
        (None, Vec::new())
    };

    let frame = CivFrame {
        dst_addr,
        src_addr,
        cmd,
        sub_cmd,
        data,
    };

    let consumed = term_pos + 1;
    DecodeResult::Frame(frame, consumed)
}

/// Find the position of the first CI-V preamble (`0xFE 0xFE`) in a buffer.
fn find_preamble(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    buf.windows(2)
        .position(|w| w[0] == PREAMBLE && w[1] == PREAMBLE)
}

/// Convert a frequency in hertz to 5-byte BCD encoding (LSB first).
///
/// Icom CI-V represents frequencies as 10-digit BCD with the least
/// significant byte transmitted first. Each byte holds two BCD digits.
///
/// # Example
///
/// ```
/// use riglib_icom::civ::freq_to_bcd;
///
/// // 14.250 MHz = 14,250,000 Hz
/// let bcd = freq_to_bcd(14_250_000);
/// assert_eq!(bcd, [0x00, 0x00, 0x25, 0x14, 0x00]);
///
/// // 7.000 MHz
/// let bcd = freq_to_bcd(7_000_000);
/// assert_eq!(bcd, [0x00, 0x00, 0x00, 0x07, 0x00]);
/// ```
pub fn freq_to_bcd(freq_hz: u64) -> [u8; 5] {
    let mut result = [0u8; 5];
    let mut freq = freq_hz;

    for byte in &mut result {
        let lo = (freq % 10) as u8;
        freq /= 10;
        let hi = (freq % 10) as u8;
        freq /= 10;
        *byte = (hi << 4) | lo;
    }

    result
}

/// Convert 5-byte BCD encoding (LSB first) back to frequency in hertz.
///
/// This is the inverse of [`freq_to_bcd`].
///
/// # Example
///
/// ```
/// use riglib_icom::civ::bcd_to_freq;
///
/// let freq = bcd_to_freq(&[0x00, 0x00, 0x25, 0x14, 0x00]);
/// assert_eq!(freq, 14_250_000);
/// ```
pub fn bcd_to_freq(bcd: &[u8; 5]) -> u64 {
    let mut freq: u64 = 0;
    let mut multiplier: u64 = 1;

    for &byte in bcd {
        let lo = (byte & 0x0F) as u64;
        let hi = ((byte >> 4) & 0x0F) as u64;
        freq += lo * multiplier;
        multiplier *= 10;
        freq += hi * multiplier;
        multiplier *= 10;
    }

    freq
}

/// Check whether a byte sequence contains a CI-V bus collision indicator.
///
/// On the half-duplex CI-V bus, when two devices transmit simultaneously
/// the echoed data becomes corrupted. The byte `0xFC` in a frame body
/// (between preamble and terminator) indicates such a collision. The
/// controller should discard the frame and retry after a random backoff.
pub fn has_collision(buf: &[u8]) -> bool {
    buf.contains(&COLLISION)
}

/// Validate that raw BCD bytes contain only valid BCD digits (0-9 in each nibble).
///
/// Returns an error if any nibble contains a value greater than 9.
pub fn validate_bcd(bcd: &[u8]) -> riglib_core::Result<()> {
    for (i, &byte) in bcd.iter().enumerate() {
        let lo = byte & 0x0F;
        let hi = (byte >> 4) & 0x0F;
        if lo > 9 || hi > 9 {
            return Err(Error::Protocol(format!(
                "invalid BCD digit at byte {i}: 0x{byte:02X}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // BCD frequency encoding/decoding
    // ---------------------------------------------------------------

    #[test]
    fn bcd_14_250_mhz() {
        // 14,250,000 Hz => 10 digits: 0014250000
        // BCD pairs (MSB to LSB digit-wise): 00 14 25 00 00
        // LSB-first wire order: [0x00, 0x00, 0x25, 0x14, 0x00]
        let bcd = freq_to_bcd(14_250_000);
        assert_eq!(bcd, [0x00, 0x00, 0x25, 0x14, 0x00]);
        assert_eq!(bcd_to_freq(&bcd), 14_250_000);
    }

    #[test]
    fn bcd_7_000_mhz() {
        let bcd = freq_to_bcd(7_000_000);
        assert_eq!(bcd, [0x00, 0x00, 0x00, 0x07, 0x00]);
        assert_eq!(bcd_to_freq(&bcd), 7_000_000);
    }

    #[test]
    fn bcd_28_500_mhz() {
        let bcd = freq_to_bcd(28_500_000);
        assert_eq!(bcd, [0x00, 0x00, 0x50, 0x28, 0x00]);
        assert_eq!(bcd_to_freq(&bcd), 28_500_000);
    }

    #[test]
    fn bcd_50_100_mhz() {
        let bcd = freq_to_bcd(50_100_000);
        assert_eq!(bcd, [0x00, 0x00, 0x10, 0x50, 0x00]);
        assert_eq!(bcd_to_freq(&bcd), 50_100_000);
    }

    #[test]
    fn bcd_1_800_mhz() {
        let bcd = freq_to_bcd(1_800_000);
        assert_eq!(bcd, [0x00, 0x00, 0x80, 0x01, 0x00]);
        assert_eq!(bcd_to_freq(&bcd), 1_800_000);
    }

    #[test]
    fn bcd_432_100_mhz() {
        // 432,100,000 Hz => 10 digits: 0432100000
        // BCD pairs: 04 32 10 00 00
        // LSB-first: [0x00, 0x00, 0x10, 0x32, 0x04]
        let bcd = freq_to_bcd(432_100_000);
        assert_eq!(bcd, [0x00, 0x00, 0x10, 0x32, 0x04]);
        assert_eq!(bcd_to_freq(&bcd), 432_100_000);
    }

    #[test]
    fn bcd_round_trip_zero() {
        let bcd = freq_to_bcd(0);
        assert_eq!(bcd, [0x00, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(bcd_to_freq(&bcd), 0);
    }

    #[test]
    fn bcd_round_trip_max_10_digit() {
        // 9,999,999,999 Hz — maximum representable in 10 BCD digits
        let freq = 9_999_999_999u64;
        let bcd = freq_to_bcd(freq);
        assert_eq!(bcd, [0x99, 0x99, 0x99, 0x99, 0x99]);
        assert_eq!(bcd_to_freq(&bcd), freq);
    }

    #[test]
    fn bcd_with_1hz_resolution() {
        // 14,074,123 Hz — FT8 frequency with sub-kHz precision
        let freq = 14_074_123;
        let bcd = freq_to_bcd(freq);
        assert_eq!(bcd_to_freq(&bcd), freq);
    }

    // ---------------------------------------------------------------
    // BCD validation
    // ---------------------------------------------------------------

    #[test]
    fn validate_bcd_valid() {
        assert!(validate_bcd(&[0x00, 0x00, 0x25, 0x14, 0x00]).is_ok());
        assert!(validate_bcd(&[0x99, 0x99, 0x99, 0x99, 0x99]).is_ok());
    }

    #[test]
    fn validate_bcd_invalid_nibble() {
        // 0xAB has invalid digits A and B
        assert!(validate_bcd(&[0xAB]).is_err());
        // 0x1A has invalid low nibble
        assert!(validate_bcd(&[0x1A]).is_err());
        // 0xF0 has invalid high nibble
        assert!(validate_bcd(&[0xF0]).is_err());
    }

    // ---------------------------------------------------------------
    // Frame encoding
    // ---------------------------------------------------------------

    #[test]
    fn encode_read_frequency() {
        let bytes = encode_frame(0x98, CONTROLLER_ADDR, 0x03, None, &[]);
        assert_eq!(bytes, vec![0xFE, 0xFE, 0x98, 0xE0, 0x03, 0xFD]);
    }

    #[test]
    fn encode_set_frequency() {
        let bcd = freq_to_bcd(14_250_000);
        let bytes = encode_frame(0x98, CONTROLLER_ADDR, 0x00, None, &bcd);
        assert_eq!(
            bytes,
            vec![
                0xFE, 0xFE, 0x98, 0xE0, 0x00, 0x00, 0x00, 0x25, 0x14, 0x00, 0xFD
            ]
        );
    }

    #[test]
    fn encode_with_sub_command() {
        // Set PTT on: cmd=0x1C, sub=0x00, data=0x01
        let bytes = encode_frame(0x98, CONTROLLER_ADDR, 0x1C, Some(0x00), &[0x01]);
        assert_eq!(bytes, vec![0xFE, 0xFE, 0x98, 0xE0, 0x1C, 0x00, 0x01, 0xFD]);
    }

    #[test]
    fn encode_ack() {
        let bytes = encode_frame(CONTROLLER_ADDR, 0x98, ACK, None, &[]);
        assert_eq!(bytes, vec![0xFE, 0xFE, 0xE0, 0x98, 0xFB, 0xFD]);
    }

    #[test]
    fn encode_civ_frame_struct() {
        let frame = CivFrame {
            dst_addr: 0x98,
            src_addr: CONTROLLER_ADDR,
            cmd: 0x03,
            sub_cmd: None,
            data: vec![],
        };
        let bytes = encode_civ_frame(&frame);
        assert_eq!(bytes, vec![0xFE, 0xFE, 0x98, 0xE0, 0x03, 0xFD]);
    }

    // ---------------------------------------------------------------
    // Frame decoding — valid frames
    // ---------------------------------------------------------------

    #[test]
    fn decode_ack_frame() {
        let buf = vec![0xFE, 0xFE, 0xE0, 0x98, 0xFB, 0xFD];
        match decode_frame(&buf) {
            DecodeResult::Frame(frame, consumed) => {
                assert!(frame.is_ack());
                assert!(!frame.is_nak());
                assert_eq!(frame.dst_addr, 0xE0);
                assert_eq!(frame.src_addr, 0x98);
                assert_eq!(consumed, 6);
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn decode_nak_frame() {
        let buf = vec![0xFE, 0xFE, 0xE0, 0x98, 0xFA, 0xFD];
        match decode_frame(&buf) {
            DecodeResult::Frame(frame, consumed) => {
                assert!(frame.is_nak());
                assert!(!frame.is_ack());
                assert_eq!(consumed, 6);
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn decode_data_response_with_sub_cmd() {
        // Frequency response: cmd=0x03, then 5 BCD bytes for 14.250 MHz
        // The rig echoes the command byte back, followed by data.
        // For cmd 0x03 (read freq), the response has no sub-cmd — just data.
        // But our generic decoder splits: first extra byte = sub_cmd, rest = data.
        let buf = vec![
            0xFE, 0xFE, 0xE0, 0x98, 0x03, 0x00, 0x00, 0x25, 0x14, 0x00, 0xFD,
        ];
        match decode_frame(&buf) {
            DecodeResult::Frame(frame, consumed) => {
                assert!(frame.is_data_response());
                assert_eq!(frame.cmd, 0x03);
                // Generic decoder puts first payload byte as sub_cmd
                assert_eq!(frame.sub_cmd, Some(0x00));
                assert_eq!(frame.data, vec![0x00, 0x25, 0x14, 0x00]);
                assert_eq!(consumed, 11);
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn decode_command_only_no_payload() {
        // Read frequency command (no sub-cmd, no data)
        let buf = vec![0xFE, 0xFE, 0x98, 0xE0, 0x03, 0xFD];
        match decode_frame(&buf) {
            DecodeResult::Frame(frame, consumed) => {
                assert_eq!(frame.cmd, 0x03);
                assert_eq!(frame.sub_cmd, None);
                assert!(frame.data.is_empty());
                assert_eq!(consumed, 6);
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Frame decoding — edge cases
    // ---------------------------------------------------------------

    #[test]
    fn decode_incomplete_no_terminator() {
        let buf = vec![0xFE, 0xFE, 0xE0, 0x98, 0x03];
        assert_eq!(decode_frame(&buf), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_incomplete_only_preamble() {
        let buf = vec![0xFE, 0xFE];
        assert_eq!(decode_frame(&buf), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_incomplete_empty() {
        assert_eq!(decode_frame(&[]), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_incomplete_single_byte() {
        assert_eq!(decode_frame(&[0xFE]), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_no_preamble() {
        // Random data with no 0xFE 0xFE sequence
        let buf = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        assert_eq!(decode_frame(&buf), DecodeResult::Incomplete);
    }

    #[test]
    fn decode_garbage_before_preamble() {
        // Garbage bytes before a valid frame
        let buf = vec![0x00, 0x01, 0xFE, 0xFE, 0xE0, 0x98, 0xFB, 0xFD];
        match decode_frame(&buf) {
            DecodeResult::Frame(frame, consumed) => {
                assert!(frame.is_ack());
                // Consumed includes garbage + frame
                assert_eq!(consumed, 8);
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn decode_multiple_frames_in_buffer() {
        // Two ACK frames back to back
        let buf = vec![
            0xFE, 0xFE, 0xE0, 0x98, 0xFB, 0xFD, // first frame
            0xFE, 0xFE, 0xE0, 0x98, 0xFA, 0xFD, // second frame
        ];

        // First decode should return the first frame
        match decode_frame(&buf) {
            DecodeResult::Frame(frame, consumed) => {
                assert!(frame.is_ack());
                assert_eq!(consumed, 6);

                // Decode the remainder to get the second frame
                match decode_frame(&buf[consumed..]) {
                    DecodeResult::Frame(frame2, consumed2) => {
                        assert!(frame2.is_nak());
                        assert_eq!(consumed2, 6);
                    }
                    other => panic!("expected second Frame, got {other:?}"),
                }
            }
            other => panic!("expected first Frame, got {other:?}"),
        }
    }

    // ---------------------------------------------------------------
    // Collision detection
    // ---------------------------------------------------------------

    #[test]
    fn decode_collision_in_frame() {
        // Frame with collision marker in the body
        let buf = vec![0xFE, 0xFE, 0xE0, 0x98, 0xFC, 0xFD];
        match decode_frame(&buf) {
            DecodeResult::Collision(consumed) => {
                assert_eq!(consumed, 6);
            }
            other => panic!("expected Collision, got {other:?}"),
        }
    }

    #[test]
    fn decode_collision_in_data() {
        // Collision marker embedded in data portion
        let buf = vec![0xFE, 0xFE, 0xE0, 0x98, 0x03, 0xFC, 0x00, 0xFD];
        match decode_frame(&buf) {
            DecodeResult::Collision(consumed) => {
                assert_eq!(consumed, 8);
            }
            other => panic!("expected Collision, got {other:?}"),
        }
    }

    #[test]
    fn decode_collision_no_terminator() {
        // Collision marker in incomplete frame (no terminator yet)
        let buf = vec![0xFE, 0xFE, 0xE0, 0x98, 0xFC];
        match decode_frame(&buf) {
            DecodeResult::Collision(consumed) => {
                assert_eq!(consumed, 5);
            }
            other => panic!("expected Collision, got {other:?}"),
        }
    }

    #[test]
    fn has_collision_true() {
        assert!(has_collision(&[0xFE, 0xFE, 0xFC, 0xFD]));
    }

    #[test]
    fn has_collision_false() {
        assert!(!has_collision(&[0xFE, 0xFE, 0xFB, 0xFD]));
    }

    // ---------------------------------------------------------------
    // CivFrame helper methods
    // ---------------------------------------------------------------

    #[test]
    fn frame_is_data_response() {
        let frame = CivFrame {
            dst_addr: 0xE0,
            src_addr: 0x98,
            cmd: 0x03,
            sub_cmd: Some(0x00),
            data: vec![0x00, 0x25, 0x14, 0x00],
        };
        assert!(frame.is_data_response());
        assert!(!frame.is_ack());
        assert!(!frame.is_nak());
    }

    // ---------------------------------------------------------------
    // Round-trip encode/decode
    // ---------------------------------------------------------------

    #[test]
    fn round_trip_simple_command() {
        let original = CivFrame {
            dst_addr: 0x98,
            src_addr: CONTROLLER_ADDR,
            cmd: 0x03,
            sub_cmd: None,
            data: vec![],
        };
        let encoded = encode_civ_frame(&original);
        match decode_frame(&encoded) {
            DecodeResult::Frame(decoded, consumed) => {
                assert_eq!(decoded, original);
                assert_eq!(consumed, encoded.len());
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_with_sub_cmd_and_data() {
        let original = CivFrame {
            dst_addr: 0x98,
            src_addr: CONTROLLER_ADDR,
            cmd: 0x1C,
            sub_cmd: Some(0x00),
            data: vec![0x01],
        };
        let encoded = encode_civ_frame(&original);
        match decode_frame(&encoded) {
            DecodeResult::Frame(decoded, consumed) => {
                assert_eq!(decoded, original);
                assert_eq!(consumed, encoded.len());
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }
}
