//! VITA-49.0 binary frame parser for FlexRadio SmartSDR.
//!
//! FlexRadio uses VITA-49.0 (NOT VITA-49.2) to stream real-time data over
//! UDP port 4991. This module provides a pure parser with no I/O dependencies.
//! All functions operate on raw byte slices and return parsed structures or
//! errors.
//!
//! The parser handles the specific packet types FlexRadio produces: meter
//! data, DAX audio, FFT, waterfall, Opus audio, and DAX IQ streams.

use riglib_core::{Error, Result};

/// VITA-49 header size in bytes.
pub const HEADER_SIZE: usize = 28;

/// FlexRadio OUI (Organizationally Unique Identifier).
pub const FLEXRADIO_OUI: u32 = 0x001C2D;

/// Stream type identified by the Packet Class Code in the VITA-49 header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    /// Meter data (S-meter, power, SWR, ALC, etc.) -- class code 0x8002.
    MeterData,
    /// FFT / panadapter spectral data -- class code 0x8003.
    Fft,
    /// Waterfall display data -- class code 0x8004.
    Waterfall,
    /// Opus compressed audio -- class code 0x8005.
    OpusAudio,
    /// DAX IQ at 24 ksps -- class code 0x02E3.
    DaxIq24,
    /// DAX IQ at 48 ksps -- class code 0x02E4.
    DaxIq48,
    /// DAX IQ at 96 ksps -- class code 0x02E5.
    DaxIq96,
    /// DAX IQ at 192 ksps -- class code 0x02E6.
    DaxIq192,
    /// DAX demodulated audio, 24 ksps stereo float32 -- class code 0x03E3.
    DaxAudio,
    /// Discovery broadcast -- class code 0xFFFF.
    Discovery,
    /// Unrecognized class code.
    Unknown(u16),
}

/// Derive the stream type from a VITA-49 Packet Class Code.
pub fn stream_type_from_class_code(code: u16) -> StreamType {
    match code {
        0x8002 => StreamType::MeterData,
        0x8003 => StreamType::Fft,
        0x8004 => StreamType::Waterfall,
        0x8005 => StreamType::OpusAudio,
        0x02E3 => StreamType::DaxIq24,
        0x02E4 => StreamType::DaxIq48,
        0x02E5 => StreamType::DaxIq96,
        0x02E6 => StreamType::DaxIq192,
        0x03E3 => StreamType::DaxAudio,
        0xFFFF => StreamType::Discovery,
        other => StreamType::Unknown(other),
    }
}

/// Parsed VITA-49 packet header (28 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vita49Header {
    /// Packet type from bits 31-28 of the header word.
    /// 0x3 = Extension Data with Stream ID for FlexRadio packets.
    pub packet_type: u8,
    /// Whether a Class ID is present (bit 27). True for FlexRadio packets.
    pub class_id_present: bool,
    /// Whether a trailer is present (bit 26). Usually false for FlexRadio.
    pub trailer_present: bool,
    /// 4-bit rolling packet counter (bits 15-12).
    pub packet_count: u8,
    /// Total packet size in 32-bit words, including header (bits 11-0).
    pub packet_size_words: u16,
    /// Stream ID identifying the specific stream instance.
    pub stream_id: u32,
    /// 24-bit OUI from the Class ID field. 0x001C2D for FlexRadio.
    pub class_oui: u32,
    /// Information Class Code (upper byte from offset 11, lower byte from
    /// the upper 16 bits at offset 12).
    pub info_class_code: u16,
    /// Packet Class Code identifying the stream type (lower 16 bits at
    /// offset 12).
    pub packet_class_code: u16,
    /// The stream type derived from the packet class code.
    pub stream_type: StreamType,
    /// Integer timestamp in seconds since Unix epoch.
    pub timestamp_int: u32,
    /// Fractional timestamp in picoseconds.
    pub timestamp_frac: u64,
}

/// A parsed VITA-49 packet: header plus a reference to the payload bytes.
#[derive(Debug, PartialEq)]
pub struct Vita49Packet<'a> {
    /// The parsed 28-byte header.
    pub header: Vita49Header,
    /// Payload bytes after the header. This is a slice into the original
    /// buffer, so no copying occurs.
    pub payload: &'a [u8],
}

/// A single meter reading extracted from a meter data packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterReading {
    /// 16-bit meter ID (assigned dynamically by the radio).
    pub meter_id: u16,
    /// Signed 16-bit meter value in internal units.
    pub value: i16,
}

/// A single stereo audio sample from a DAX audio packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioSample {
    /// Left channel sample (IEEE 754 float32).
    pub left: f32,
    /// Right channel sample (IEEE 754 float32).
    pub right: f32,
}

/// Parse a VITA-49 packet from a raw UDP datagram buffer.
///
/// The buffer must contain at least [`HEADER_SIZE`] bytes. The
/// `packet_size_words` field in the header is validated against the buffer
/// length: the declared size must not exceed the buffer.
///
/// Returns a [`Vita49Packet`] with the parsed header and a payload slice
/// referencing the bytes after the 28-byte header.
pub fn parse_packet(data: &[u8]) -> Result<Vita49Packet<'_>> {
    if data.len() < HEADER_SIZE {
        return Err(Error::Protocol(format!(
            "VITA-49 packet too short: {} bytes, minimum is {}",
            data.len(),
            HEADER_SIZE
        )));
    }

    // -- Header word (offset 0-3, big-endian) --
    let header_word = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let packet_type = ((header_word >> 28) & 0x0F) as u8;
    let class_id_present = (header_word >> 27) & 1 == 1;
    let trailer_present = (header_word >> 26) & 1 == 1;
    let packet_count = ((header_word >> 12) & 0x0F) as u8;
    let packet_size_words = (header_word & 0x0FFF) as u16;

    // Validate packet size against buffer length.
    let packet_size_bytes = packet_size_words as usize * 4;
    if packet_size_bytes > data.len() {
        return Err(Error::Protocol(format!(
            "VITA-49 packet_size ({} words = {} bytes) exceeds buffer length ({} bytes)",
            packet_size_words,
            packet_size_bytes,
            data.len()
        )));
    }

    // -- Stream ID (offset 4-7) --
    let stream_id = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

    // -- Class ID upper (offset 8-11): OUI (24 bits) + info class upper byte --
    let class_upper = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let class_oui = (class_upper >> 8) & 0x00FF_FFFF;
    let info_class_upper = (class_upper & 0xFF) as u16;

    if class_oui != FLEXRADIO_OUI {
        tracing::warn!(
            oui = class_oui,
            expected = FLEXRADIO_OUI,
            "VITA-49 packet OUI does not match FlexRadio"
        );
    }

    // -- Class ID lower (offset 12-15): info class code (upper 16) + packet class code (lower 16) --
    let class_lower = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
    let info_class_lower = ((class_lower >> 16) & 0xFFFF) as u16;
    let packet_class_code = (class_lower & 0xFFFF) as u16;

    // The full Information Class Code spans the low byte of offset 8-11 and
    // the high 16 bits of offset 12-15.  Per the spec description the
    // "Information Class Code" field at offset 12 is 0x534C for FlexRadio,
    // so we store the 16-bit value from offset 12-13 as info_class_code.
    let info_class_code = info_class_lower;
    let _ = info_class_upper; // consumed as part of OUI-adjacent byte

    let stream_type = stream_type_from_class_code(packet_class_code);

    // -- Integer Timestamp (offset 16-19) --
    let timestamp_int = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);

    // -- Fractional Timestamp (offset 20-27, 64-bit) --
    let timestamp_frac = u64::from_be_bytes([
        data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
    ]);

    let header = Vita49Header {
        packet_type,
        class_id_present,
        trailer_present,
        packet_count,
        packet_size_words,
        stream_id,
        class_oui,
        info_class_code,
        packet_class_code,
        stream_type,
        timestamp_int,
        timestamp_frac,
    };

    // Payload is everything after the header, up to the declared packet size.
    let payload = &data[HEADER_SIZE..packet_size_bytes];

    Ok(Vita49Packet { header, payload })
}

/// Extract meter readings from a meter data packet's payload.
///
/// Each reading is 4 bytes: a 16-bit unsigned meter ID followed by a 16-bit
/// signed value, both big-endian. The payload length must be divisible by 4.
/// An empty payload returns an empty Vec (valid -- no meters reported).
pub fn parse_meter_payload(payload: &[u8]) -> Result<Vec<MeterReading>> {
    if payload.len() % 4 != 0 {
        return Err(Error::Protocol(format!(
            "meter payload length {} is not divisible by 4",
            payload.len()
        )));
    }

    let count = payload.len() / 4;
    let mut readings = Vec::with_capacity(count);

    for i in 0..count {
        let offset = i * 4;
        let meter_id = u16::from_be_bytes([payload[offset], payload[offset + 1]]);
        let value = i16::from_be_bytes([payload[offset + 2], payload[offset + 3]]);
        readings.push(MeterReading { meter_id, value });
    }

    Ok(readings)
}

/// Extract stereo audio samples from a DAX audio packet's payload.
///
/// Samples are interleaved left/right IEEE 754 float32 values in
/// **little-endian** byte order (FlexRadio convention within the big-endian
/// VITA-49 framing). Each stereo frame is 8 bytes. The payload length must
/// be divisible by 8.
pub fn parse_dax_audio_payload(payload: &[u8]) -> Result<Vec<AudioSample>> {
    if payload.len() % 8 != 0 {
        return Err(Error::Protocol(format!(
            "DAX audio payload length {} is not divisible by 8",
            payload.len()
        )));
    }

    let count = payload.len() / 8;
    let mut samples = Vec::with_capacity(count);

    for i in 0..count {
        let offset = i * 8;
        let left = f32::from_le_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]);
        let right = f32::from_le_bytes([
            payload[offset + 4],
            payload[offset + 5],
            payload[offset + 6],
            payload[offset + 7],
        ]);
        samples.push(AudioSample { left, right });
    }

    Ok(samples)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a valid VITA-49 header as a 28-byte array.
    ///
    /// Uses defaults that match typical FlexRadio meter data packets unless
    /// overridden by the caller.
    #[allow(clippy::too_many_arguments)]
    fn build_header(
        packet_type: u8,
        class_id_present: bool,
        trailer_present: bool,
        packet_count: u8,
        packet_size_words: u16,
        stream_id: u32,
        oui: u32,
        info_class_code: u16,
        packet_class_code: u16,
        timestamp_int: u32,
        timestamp_frac: u64,
    ) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];

        // Header word
        let mut hw: u32 = 0;
        hw |= (packet_type as u32 & 0x0F) << 28;
        hw |= (class_id_present as u32) << 27;
        hw |= (trailer_present as u32) << 26;
        // bits 25-24: TSI = 01 (UTC)
        hw |= 0x01 << 20;
        // bits 19-18: TSF = 01 (sample count)
        hw |= 0x01 << 18;
        hw |= (packet_count as u32 & 0x0F) << 12;
        hw |= packet_size_words as u32 & 0x0FFF;
        buf[0..4].copy_from_slice(&hw.to_be_bytes());

        // Stream ID
        buf[4..8].copy_from_slice(&stream_id.to_be_bytes());

        // Class OUI + info class upper byte
        let class_upper: u32 = oui << 8;
        buf[8..12].copy_from_slice(&class_upper.to_be_bytes());

        // Info class code (upper 16) + packet class code (lower 16)
        let class_lower: u32 = ((info_class_code as u32) << 16) | packet_class_code as u32;
        buf[12..16].copy_from_slice(&class_lower.to_be_bytes());

        // Integer timestamp
        buf[16..20].copy_from_slice(&timestamp_int.to_be_bytes());

        // Fractional timestamp
        buf[20..28].copy_from_slice(&timestamp_frac.to_be_bytes());

        buf
    }

    /// Build a complete packet: header + payload bytes.
    fn build_packet(
        packet_class_code: u16,
        stream_id: u32,
        payload: &[u8],
        timestamp_int: u32,
        timestamp_frac: u64,
    ) -> Vec<u8> {
        let total_bytes = HEADER_SIZE + payload.len();
        assert!(total_bytes % 4 == 0, "total packet must be word-aligned");
        let size_words = (total_bytes / 4) as u16;

        let header = build_header(
            0x3, // Extension Data with Stream ID
            true,
            false,
            0,
            size_words,
            stream_id,
            FLEXRADIO_OUI,
            0x534C,
            packet_class_code,
            timestamp_int,
            timestamp_frac,
        );

        let mut pkt = Vec::with_capacity(total_bytes);
        pkt.extend_from_slice(&header);
        pkt.extend_from_slice(payload);
        pkt
    }

    // -- stream_type_from_class_code --

    #[test]
    fn class_code_meter_data() {
        assert_eq!(stream_type_from_class_code(0x8002), StreamType::MeterData);
    }

    #[test]
    fn class_code_fft() {
        assert_eq!(stream_type_from_class_code(0x8003), StreamType::Fft);
    }

    #[test]
    fn class_code_waterfall() {
        assert_eq!(stream_type_from_class_code(0x8004), StreamType::Waterfall);
    }

    #[test]
    fn class_code_opus_audio() {
        assert_eq!(stream_type_from_class_code(0x8005), StreamType::OpusAudio);
    }

    #[test]
    fn class_code_dax_iq24() {
        assert_eq!(stream_type_from_class_code(0x02E3), StreamType::DaxIq24);
    }

    #[test]
    fn class_code_dax_iq48() {
        assert_eq!(stream_type_from_class_code(0x02E4), StreamType::DaxIq48);
    }

    #[test]
    fn class_code_dax_iq96() {
        assert_eq!(stream_type_from_class_code(0x02E5), StreamType::DaxIq96);
    }

    #[test]
    fn class_code_dax_iq192() {
        assert_eq!(stream_type_from_class_code(0x02E6), StreamType::DaxIq192);
    }

    #[test]
    fn class_code_dax_audio() {
        assert_eq!(stream_type_from_class_code(0x03E3), StreamType::DaxAudio);
    }

    #[test]
    fn class_code_discovery() {
        assert_eq!(stream_type_from_class_code(0xFFFF), StreamType::Discovery);
    }

    #[test]
    fn class_code_unknown() {
        assert_eq!(
            stream_type_from_class_code(0x1234),
            StreamType::Unknown(0x1234)
        );
    }

    // -- parse_packet: valid packets --

    #[test]
    fn parse_valid_meter_packet() {
        // Two meter readings: meter 5 = 1000, meter 12 = -200
        let mut payload = Vec::new();
        payload.extend_from_slice(&5u16.to_be_bytes());
        payload.extend_from_slice(&1000i16.to_be_bytes());
        payload.extend_from_slice(&12u16.to_be_bytes());
        payload.extend_from_slice(&(-200i16).to_be_bytes());

        let pkt = build_packet(0x8002, 0xAABBCCDD, &payload, 1700000000, 500_000_000_000);
        let parsed = parse_packet(&pkt).unwrap();

        assert_eq!(parsed.header.packet_type, 0x3);
        assert!(parsed.header.class_id_present);
        assert!(!parsed.header.trailer_present);
        assert_eq!(parsed.header.stream_id, 0xAABBCCDD);
        assert_eq!(parsed.header.class_oui, FLEXRADIO_OUI);
        assert_eq!(parsed.header.info_class_code, 0x534C);
        assert_eq!(parsed.header.packet_class_code, 0x8002);
        assert_eq!(parsed.header.stream_type, StreamType::MeterData);
        assert_eq!(parsed.header.timestamp_int, 1700000000);
        assert_eq!(parsed.header.timestamp_frac, 500_000_000_000);
        assert_eq!(parsed.payload.len(), 8);
    }

    #[test]
    fn parse_valid_dax_audio_packet() {
        // One stereo frame: left = 0.5, right = -0.25 (little-endian float32)
        let mut payload = Vec::new();
        payload.extend_from_slice(&0.5f32.to_le_bytes());
        payload.extend_from_slice(&(-0.25f32).to_le_bytes());

        let pkt = build_packet(0x03E3, 0x00000042, &payload, 1700000001, 0);
        let parsed = parse_packet(&pkt).unwrap();

        assert_eq!(parsed.header.stream_type, StreamType::DaxAudio);
        assert_eq!(parsed.header.stream_id, 0x42);
        assert_eq!(parsed.payload.len(), 8);
    }

    #[test]
    fn parse_header_only_packet() {
        let pkt = build_packet(0x8002, 0x00000001, &[], 1700000002, 0);
        assert_eq!(pkt.len(), HEADER_SIZE);

        let parsed = parse_packet(&pkt).unwrap();
        assert_eq!(parsed.payload.len(), 0);
        assert_eq!(parsed.header.packet_size_words, 7); // 28 / 4
    }

    // -- parse_packet: error cases --

    #[test]
    fn reject_truncated_packet() {
        let data = [0u8; 27]; // one byte short
        let err = parse_packet(&data).unwrap_err();
        assert!(err.to_string().contains("too short"));
    }

    #[test]
    fn reject_empty_buffer() {
        let err = parse_packet(&[]).unwrap_err();
        assert!(err.to_string().contains("too short"));
    }

    #[test]
    fn reject_packet_size_exceeds_buffer() {
        // Build a valid header claiming 100 words (400 bytes) but only provide 28 bytes.
        let header = build_header(
            0x3,
            true,
            false,
            0,
            100, // 100 words = 400 bytes
            0x00000001,
            FLEXRADIO_OUI,
            0x534C,
            0x8002,
            0,
            0,
        );
        let err = parse_packet(&header).unwrap_err();
        assert!(err.to_string().contains("exceeds buffer"));
    }

    // -- parse_packet: various stream types --

    #[test]
    fn parse_each_stream_type() {
        let class_codes: &[(u16, StreamType)] = &[
            (0x8002, StreamType::MeterData),
            (0x8003, StreamType::Fft),
            (0x8004, StreamType::Waterfall),
            (0x8005, StreamType::OpusAudio),
            (0x02E3, StreamType::DaxIq24),
            (0x02E4, StreamType::DaxIq48),
            (0x02E5, StreamType::DaxIq96),
            (0x02E6, StreamType::DaxIq192),
            (0x03E3, StreamType::DaxAudio),
            (0xFFFF, StreamType::Discovery),
        ];

        for &(code, ref expected_type) in class_codes {
            let pkt = build_packet(code, 0x00000001, &[0u8; 4], 0, 0);
            let parsed = parse_packet(&pkt).unwrap();
            assert_eq!(
                &parsed.header.stream_type, expected_type,
                "class code 0x{:04X}",
                code
            );
        }
    }

    // -- parse_packet: timestamp extraction --

    #[test]
    fn timestamp_extraction() {
        let ts_int: u32 = 1_700_000_000; // 2023-11-14T22:13:20Z
        let ts_frac: u64 = 999_999_999_999; // just under 1 second in picoseconds

        let pkt = build_packet(0x8002, 0, &[], ts_int, ts_frac);
        let parsed = parse_packet(&pkt).unwrap();

        assert_eq!(parsed.header.timestamp_int, ts_int);
        assert_eq!(parsed.header.timestamp_frac, ts_frac);
    }

    // -- parse_packet: stream_id extraction --

    #[test]
    fn stream_id_extraction() {
        let pkt = build_packet(0x8002, 0xDEADBEEF, &[], 0, 0);
        let parsed = parse_packet(&pkt).unwrap();
        assert_eq!(parsed.header.stream_id, 0xDEADBEEF);
    }

    // -- parse_packet: packet_count --

    #[test]
    fn packet_count_extraction() {
        for count in 0..16u8 {
            let header = build_header(
                0x3,
                true,
                false,
                count,
                7,
                0,
                FLEXRADIO_OUI,
                0x534C,
                0x8002,
                0,
                0,
            );
            let parsed = parse_packet(&header).unwrap();
            assert_eq!(parsed.header.packet_count, count, "packet_count {}", count);
        }
    }

    // -- parse_packet: buffer larger than declared size --

    #[test]
    fn buffer_larger_than_declared_size() {
        // Build a header-only packet (7 words = 28 bytes), but provide 64 bytes.
        let pkt = build_packet(0x8002, 0x01, &[], 0, 0);
        let mut oversized = pkt.clone();
        oversized.extend_from_slice(&[0xFFu8; 36]); // extra trailing garbage

        let parsed = parse_packet(&oversized).unwrap();
        // Payload should be empty because the header says 7 words (28 bytes).
        assert_eq!(parsed.payload.len(), 0);
    }

    // -- Round-trip test --

    #[test]
    fn round_trip_meter_packet() {
        let stream_id = 0x12345678u32;
        let ts_int = 1_700_100_200u32;
        let ts_frac = 42_000_000_000u64;

        let mut payload = Vec::new();
        // meter 1 = 500
        payload.extend_from_slice(&1u16.to_be_bytes());
        payload.extend_from_slice(&500i16.to_be_bytes());
        // meter 2 = -32768
        payload.extend_from_slice(&2u16.to_be_bytes());
        payload.extend_from_slice(&(-32768i16).to_be_bytes());
        // meter 65535 = 32767 (max values)
        payload.extend_from_slice(&0xFFFFu16.to_be_bytes());
        payload.extend_from_slice(&32767i16.to_be_bytes());

        let pkt = build_packet(0x8002, stream_id, &payload, ts_int, ts_frac);
        let parsed = parse_packet(&pkt).unwrap();

        assert_eq!(parsed.header.packet_type, 0x3);
        assert!(parsed.header.class_id_present);
        assert!(!parsed.header.trailer_present);
        assert_eq!(parsed.header.packet_count, 0);
        assert_eq!(parsed.header.stream_id, stream_id);
        assert_eq!(parsed.header.class_oui, FLEXRADIO_OUI);
        assert_eq!(parsed.header.packet_class_code, 0x8002);
        assert_eq!(parsed.header.stream_type, StreamType::MeterData);
        assert_eq!(parsed.header.timestamp_int, ts_int);
        assert_eq!(parsed.header.timestamp_frac, ts_frac);

        let readings = parse_meter_payload(parsed.payload).unwrap();
        assert_eq!(readings.len(), 3);
        assert_eq!(
            readings[0],
            MeterReading {
                meter_id: 1,
                value: 500
            }
        );
        assert_eq!(
            readings[1],
            MeterReading {
                meter_id: 2,
                value: -32768
            }
        );
        assert_eq!(
            readings[2],
            MeterReading {
                meter_id: 0xFFFF,
                value: 32767
            }
        );
    }

    #[test]
    fn round_trip_dax_audio_packet() {
        let mut payload = Vec::new();
        let samples_in = [(0.0f32, 0.0f32), (1.0, -1.0), (0.123_456_79, 0.987_654_3)];
        for &(l, r) in &samples_in {
            payload.extend_from_slice(&l.to_le_bytes());
            payload.extend_from_slice(&r.to_le_bytes());
        }

        let pkt = build_packet(0x03E3, 0x42, &payload, 1_700_000_000, 0);
        let parsed = parse_packet(&pkt).unwrap();

        assert_eq!(parsed.header.stream_type, StreamType::DaxAudio);

        let samples = parse_dax_audio_payload(parsed.payload).unwrap();
        assert_eq!(samples.len(), 3);
        for (i, &(l, r)) in samples_in.iter().enumerate() {
            assert_eq!(samples[i].left, l, "sample {} left", i);
            assert_eq!(samples[i].right, r, "sample {} right", i);
        }
    }

    // -- parse_meter_payload --

    #[test]
    fn meter_payload_single_reading() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&42u16.to_be_bytes());
        payload.extend_from_slice(&(-100i16).to_be_bytes());

        let readings = parse_meter_payload(&payload).unwrap();
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].meter_id, 42);
        assert_eq!(readings[0].value, -100);
    }

    #[test]
    fn meter_payload_multiple_readings() {
        let mut payload = Vec::new();
        for i in 0..10u16 {
            payload.extend_from_slice(&i.to_be_bytes());
            payload.extend_from_slice(&(i as i16 * 100).to_be_bytes());
        }

        let readings = parse_meter_payload(&payload).unwrap();
        assert_eq!(readings.len(), 10);
        for (i, r) in readings.iter().enumerate() {
            assert_eq!(r.meter_id, i as u16);
            assert_eq!(r.value, i as i16 * 100);
        }
    }

    #[test]
    fn meter_payload_empty() {
        let readings = parse_meter_payload(&[]).unwrap();
        assert!(readings.is_empty());
    }

    #[test]
    fn meter_payload_bad_length() {
        let err = parse_meter_payload(&[0u8; 5]).unwrap_err();
        assert!(err.to_string().contains("not divisible by 4"));
    }

    // -- parse_dax_audio_payload --

    #[test]
    fn dax_audio_single_sample() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&0.75f32.to_le_bytes());
        payload.extend_from_slice(&(-0.5f32).to_le_bytes());

        let samples = parse_dax_audio_payload(&payload).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].left, 0.75);
        assert_eq!(samples[0].right, -0.5);
    }

    #[test]
    fn dax_audio_multiple_samples() {
        let mut payload = Vec::new();
        for i in 0..128 {
            let val = i as f32 / 128.0;
            payload.extend_from_slice(&val.to_le_bytes());
            payload.extend_from_slice(&(-val).to_le_bytes());
        }

        let samples = parse_dax_audio_payload(&payload).unwrap();
        assert_eq!(samples.len(), 128);
        for (i, s) in samples.iter().enumerate() {
            let expected = i as f32 / 128.0;
            assert_eq!(s.left, expected, "sample {} left", i);
            assert_eq!(s.right, -expected, "sample {} right", i);
        }
    }

    #[test]
    fn dax_audio_empty_payload() {
        let samples = parse_dax_audio_payload(&[]).unwrap();
        assert!(samples.is_empty());
    }

    #[test]
    fn dax_audio_bad_length() {
        let err = parse_dax_audio_payload(&[0u8; 7]).unwrap_err();
        assert!(err.to_string().contains("not divisible by 8"));
    }

    #[test]
    fn dax_audio_bad_length_not_multiple_of_8() {
        // 12 bytes: divisible by 4 but not by 8
        let err = parse_dax_audio_payload(&[0u8; 12]).unwrap_err();
        assert!(err.to_string().contains("not divisible by 8"));
    }
}
