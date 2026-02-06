//! DAX (Digital Audio eXchange) stream management for FlexRadio.
//!
//! DAX provides network-based audio streaming over VITA-49 UDP packets,
//! replacing the USB audio interface used by traditional transceivers.
//! Each DAX channel (1-8) can be associated with a specific slice receiver
//! for independent RX audio streams.
//!
//! DAX audio is always 24 ksps, stereo, IEEE 754 float32 -- no sample rate
//! or format conversion is needed between the VITA-49 payload and the riglib
//! [`AudioBuffer`] type.
//!
//! This module does NOT require the `audio` feature flag or cpal, since DAX
//! is purely network-based.

use riglib_core::AudioBuffer;
use riglib_core::audio::{AudioSampleFormat, AudioStreamConfig};
use tokio::sync::mpsc;

/// Native DAX audio sample rate in hertz.
pub const DAX_SAMPLE_RATE: u32 = 24_000;

/// Number of audio channels in a DAX stream (always stereo).
pub const DAX_CHANNELS: u16 = 2;

/// Get the native DAX audio stream configuration.
///
/// Returns a configuration describing 24 kHz, stereo, f32 -- the fixed
/// format used by all FlexRadio DAX audio streams.
pub fn dax_audio_config() -> AudioStreamConfig {
    AudioStreamConfig {
        sample_rate: DAX_SAMPLE_RATE,
        channels: DAX_CHANNELS,
        sample_format: AudioSampleFormat::F32,
    }
}

/// State of an active DAX audio stream.
///
/// Each `DaxStream` represents a single RX audio stream created via the
/// `stream create type=dax_rx` SmartSDR command. The `stream_id` doubles
/// as the VITA-49 stream identifier used to route incoming UDP packets
/// to the correct consumer.
#[derive(Debug)]
pub struct DaxStream {
    /// Stream handle returned by SmartSDR (also the VITA-49 stream_id).
    pub stream_id: u32,
    /// DAX channel number (1-8).
    pub dax_channel: u8,
    /// Associated slice index, if known.
    pub slice_index: Option<u8>,
    /// Sender for routing parsed audio buffers to the `AudioReceiver`.
    pub audio_tx: mpsc::Sender<AudioBuffer>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dax_audio_config() {
        let config = dax_audio_config();
        assert_eq!(config.sample_rate, 24_000);
        assert_eq!(config.channels, 2);
        assert_eq!(config.sample_format, AudioSampleFormat::F32);
    }

    #[test]
    fn test_dax_constants() {
        assert_eq!(DAX_SAMPLE_RATE, 24_000);
        assert_eq!(DAX_CHANNELS, 2);
    }

    #[test]
    fn test_dax_stream_creation() {
        let (tx, _rx) = mpsc::channel(8);
        let stream = DaxStream {
            stream_id: 0x2000_0001,
            dax_channel: 1,
            slice_index: Some(0),
            audio_tx: tx,
        };
        assert_eq!(stream.stream_id, 0x2000_0001);
        assert_eq!(stream.dax_channel, 1);
        assert_eq!(stream.slice_index, Some(0));
    }

    #[test]
    fn test_dax_stream_no_slice() {
        let (tx, _rx) = mpsc::channel(8);
        let stream = DaxStream {
            stream_id: 0x42,
            dax_channel: 3,
            slice_index: None,
            audio_tx: tx,
        };
        assert_eq!(stream.slice_index, None);
        assert_eq!(stream.dax_channel, 3);
    }
}
