//! Audio streaming types and the [`AudioCapable`] trait.
//!
//! This module defines the core audio abstractions for riglib. All types are
//! pure Rust with no platform-specific I/O -- the actual audio backends (cpal
//! for USB audio, VITA-49 for FlexRadio DAX) live in their respective crates.
//!
//! Audio samples are always normalized to `f32` at the riglib API boundary,
//! regardless of the native format used by the hardware (16-bit signed integer
//! for USB audio CODECs, 32-bit float for FlexRadio DAX). This simplifies
//! consumer code: callers always work with `f32` samples in the range
//! `[-1.0, 1.0]`.
//!
//! # Channel-based streaming
//!
//! Audio data flows through [`tokio::sync::mpsc`] channels wrapped in
//! [`AudioReceiver`] and [`AudioSender`]. This design bridges the callback-based
//! cpal API to the async world cleanly, and matches how FlexRadio DAX audio
//! arrives over UDP.
//!
//! - **RX audio** (from rig): backend pushes [`AudioBuffer`]s into a channel;
//!   the application reads them via [`AudioReceiver::recv()`].
//! - **TX audio** (to rig): the application pushes [`AudioBuffer`]s via
//!   [`AudioSender::send()`]; the backend drains the channel.

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::types::ReceiverId;

// ---------------------------------------------------------------------------
// AudioSampleFormat
// ---------------------------------------------------------------------------

/// Audio sample format at the hardware level.
///
/// At the riglib API boundary, audio is always delivered as `f32`. This enum
/// describes the *native* format of the underlying audio device, which is
/// useful for diagnostics and configuration purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSampleFormat {
    /// 32-bit IEEE 754 floating point, range `[-1.0, 1.0]`.
    ///
    /// Native format for FlexRadio DAX audio.
    F32,

    /// 16-bit signed integer, range `[-32768, 32767]`.
    ///
    /// Native format for USB Audio Class 1.0 CODECs (Icom, Yaesu, Kenwood,
    /// Elecraft).
    I16,
}

// ---------------------------------------------------------------------------
// AudioStreamConfig
// ---------------------------------------------------------------------------

/// Configuration for an audio stream.
///
/// Describes the sample rate, channel count, and sample format of an audio
/// stream. Used both to report a rig's native audio configuration and to
/// request a specific configuration when starting a stream.
///
/// # Typical configurations
///
/// | Backend       | Sample Rate | Channels | Format |
/// |---------------|-------------|----------|--------|
/// | USB Audio     | 48,000 Hz   | 2        | F32    |
/// | FlexRadio DAX | 24,000 Hz   | 2        | F32    |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStreamConfig {
    /// Sample rate in hertz (e.g., 48000 for USB audio, 24000 for FlexRadio DAX).
    pub sample_rate: u32,

    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u16,

    /// Sample format at the API boundary (always F32 at the riglib API level).
    pub sample_format: AudioSampleFormat,
}

// ---------------------------------------------------------------------------
// AudioBuffer
// ---------------------------------------------------------------------------

/// A buffer of interleaved audio samples.
///
/// Samples are normalized `f32` values in the range `[-1.0, 1.0]`. For
/// multi-channel audio, samples are interleaved:
///
/// ```text
/// [L0, R0, L1, R1, ...]   // stereo
/// [S0, S1, S2, ...]        // mono
/// ```
///
/// Each [`AudioBuffer`] carries its own channel count and sample rate so that
/// consumers can verify they match the expected stream configuration.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Interleaved f32 samples, normalized to `[-1.0, 1.0]`.
    pub samples: Vec<f32>,

    /// Number of audio channels.
    pub channels: u16,

    /// Sample rate in hertz.
    pub sample_rate: u32,
}

impl AudioBuffer {
    /// Create a new `AudioBuffer`.
    ///
    /// # Arguments
    ///
    /// * `samples` -- interleaved f32 samples
    /// * `channels` -- number of audio channels (1 for mono, 2 for stereo)
    /// * `sample_rate` -- sample rate in hertz
    pub fn new(samples: Vec<f32>, channels: u16, sample_rate: u32) -> Self {
        AudioBuffer {
            samples,
            channels,
            sample_rate,
        }
    }

    /// Number of audio frames in this buffer.
    ///
    /// A frame contains one sample per channel. For stereo audio with 100
    /// interleaved samples, there are 50 frames.
    pub fn frame_count(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.samples.len() / self.channels as usize
    }

    /// Duration of this buffer in seconds.
    ///
    /// Computed as `frame_count / sample_rate`. Returns `0.0` if the sample
    /// rate is zero (which would be a misconfigured buffer).
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frame_count() as f64 / self.sample_rate as f64
    }

    /// Check whether this buffer contains no samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

// ---------------------------------------------------------------------------
// AudioReceiver
// ---------------------------------------------------------------------------

/// Receives audio buffers from a rig's RX audio stream.
///
/// Wraps a [`tokio::sync::mpsc::Receiver`] to provide a typed, async interface
/// for reading audio data. The underlying channel is bounded to provide
/// backpressure -- if the consumer falls behind, the producer (cpal callback
/// or DAX UDP receiver) will drop samples rather than accumulate unbounded
/// memory.
///
/// The stream ends when the sender side is dropped (e.g., when
/// [`AudioCapable::stop_audio()`] is called or the rig disconnects), at which
/// point [`recv()`](AudioReceiver::recv) returns `None`.
pub struct AudioReceiver {
    rx: mpsc::Receiver<AudioBuffer>,
    config: AudioStreamConfig,
}

impl AudioReceiver {
    /// Create a new `AudioReceiver` wrapping an mpsc receiver.
    pub fn new(rx: mpsc::Receiver<AudioBuffer>, config: AudioStreamConfig) -> Self {
        AudioReceiver { rx, config }
    }

    /// Receive the next audio buffer.
    ///
    /// Returns `None` when the stream has been closed (sender dropped).
    pub async fn recv(&mut self) -> Option<AudioBuffer> {
        self.rx.recv().await
    }

    /// Get the audio stream configuration.
    pub fn config(&self) -> &AudioStreamConfig {
        &self.config
    }
}

// ---------------------------------------------------------------------------
// AudioSender
// ---------------------------------------------------------------------------

/// Sends audio buffers to a rig's TX audio stream.
///
/// Wraps a [`tokio::sync::mpsc::Sender`] to provide a typed, async interface
/// for writing audio data. The underlying channel is bounded; if the rig's
/// audio output is not draining fast enough, [`send()`](AudioSender::send)
/// will wait until space is available.
///
/// When the receiver side is dropped (e.g., the rig disconnects or
/// [`AudioCapable::stop_audio()`] is called), [`send()`](AudioSender::send)
/// returns [`Error::StreamClosed`].
pub struct AudioSender {
    tx: mpsc::Sender<AudioBuffer>,
    config: AudioStreamConfig,
}

impl AudioSender {
    /// Create a new `AudioSender` wrapping an mpsc sender.
    pub fn new(tx: mpsc::Sender<AudioBuffer>, config: AudioStreamConfig) -> Self {
        AudioSender { tx, config }
    }

    /// Send an audio buffer to the rig for transmission.
    ///
    /// Returns [`Error::StreamClosed`] if the receiving end of the stream
    /// has been dropped.
    pub async fn send(&self, buffer: AudioBuffer) -> Result<()> {
        self.tx.send(buffer).await.map_err(|_| Error::StreamClosed)
    }

    /// Get the audio stream configuration.
    pub fn config(&self) -> &AudioStreamConfig {
        &self.config
    }
}

// ---------------------------------------------------------------------------
// USB Audio Class 1.0 helper
// ---------------------------------------------------------------------------

/// Return the standard USB Audio Class 1.0 configuration used by all serial
/// manufacturers (Icom, Yaesu, Kenwood, Elecraft).
///
/// These transceivers all present a USB Audio Class 1.0 CODEC (typically
/// identified as "USB Audio CODEC" or "Burr-Brown from TI USB Audio CODEC")
/// that operates at 48 kHz, stereo, 16-bit signed integer. The riglib API
/// normalizes samples to f32 internally, so the `sample_format` here
/// reflects the native hardware format for diagnostic purposes.
pub fn usb_audio_config() -> AudioStreamConfig {
    AudioStreamConfig {
        sample_rate: 48000,
        channels: 2,
        sample_format: AudioSampleFormat::I16,
    }
}

// ---------------------------------------------------------------------------
// AudioCapable trait
// ---------------------------------------------------------------------------

/// Trait for rigs that support audio streaming.
///
/// This is a separate trait from [`crate::rig::Rig`] because not all rigs
/// support audio streaming. For serial-connected rigs, audio requires a USB
/// audio device to be configured. For FlexRadio, audio uses the DAX network
/// stream.
///
/// Audio is always normalized to `f32` at the API boundary. The native format
/// (16-bit for USB, float32 for DAX) is converted internally by each backend.
///
/// # Usage
///
/// ```ignore
/// use riglib_core::{AudioCapable, ReceiverId};
///
/// async fn capture_audio(rig: &dyn AudioCapable) -> riglib_core::Result<()> {
///     let mut rx_audio = rig.start_rx_audio(ReceiverId::VFO_A, None).await?;
///     while let Some(buf) = rx_audio.recv().await {
///         println!("got {} frames at {} Hz", buf.frame_count(), buf.sample_rate);
///     }
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait AudioCapable: Send + Sync {
    /// Start receiving RX audio from the specified receiver.
    ///
    /// Returns an [`AudioReceiver`] that yields [`AudioBuffer`]s as audio
    /// arrives from the rig. The stream continues until
    /// [`stop_audio()`](AudioCapable::stop_audio) is called or the receiver
    /// is dropped.
    ///
    /// If `config` is `None`, uses the rig's native configuration as reported
    /// by [`native_audio_config()`](AudioCapable::native_audio_config).
    async fn start_rx_audio(
        &self,
        rx: ReceiverId,
        config: Option<AudioStreamConfig>,
    ) -> Result<AudioReceiver>;

    /// Start sending TX audio to the rig.
    ///
    /// Returns an [`AudioSender`] that accepts [`AudioBuffer`]s to transmit.
    /// PTT must be asserted separately via [`Rig::set_ptt(true)`](crate::rig::Rig::set_ptt).
    ///
    /// If `config` is `None`, uses the rig's native configuration as reported
    /// by [`native_audio_config()`](AudioCapable::native_audio_config).
    async fn start_tx_audio(&self, config: Option<AudioStreamConfig>) -> Result<AudioSender>;

    /// Stop all audio streams (RX and TX).
    ///
    /// After this call, any outstanding [`AudioReceiver`]s will return `None`
    /// on the next `recv()`, and any [`AudioSender`]s will return
    /// [`Error::StreamClosed`] on the next `send()`.
    async fn stop_audio(&self) -> Result<()>;

    /// Whether audio streaming is supported and configured for this rig.
    ///
    /// Returns `false` if no audio device has been configured (serial rigs)
    /// or if the rig does not support audio streaming.
    fn audio_supported(&self) -> bool;

    /// Get the native audio configuration for this rig.
    ///
    /// Returns the sample rate, channel count, and format that the rig uses
    /// natively. For USB audio rigs this is typically 48 kHz / 16-bit / stereo.
    /// For FlexRadio DAX this is 24 kHz / f32 / stereo.
    fn native_audio_config(&self) -> AudioStreamConfig;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- AudioBuffer tests --------------------------------------------------

    #[test]
    fn test_new_buffer() {
        let samples = vec![0.0_f32, 0.5, -0.5, 1.0];
        let buf = AudioBuffer::new(samples.clone(), 2, 48000);
        assert_eq!(buf.samples, samples);
        assert_eq!(buf.channels, 2);
        assert_eq!(buf.sample_rate, 48000);
    }

    #[test]
    fn test_frame_count_stereo() {
        // 100 interleaved samples, 2 channels = 50 frames.
        let samples = vec![0.0_f32; 100];
        let buf = AudioBuffer::new(samples, 2, 48000);
        assert_eq!(buf.frame_count(), 50);
    }

    #[test]
    fn test_frame_count_mono() {
        // 100 samples, 1 channel = 100 frames.
        let samples = vec![0.0_f32; 100];
        let buf = AudioBuffer::new(samples, 1, 48000);
        assert_eq!(buf.frame_count(), 100);
    }

    #[test]
    fn test_duration_secs() {
        // 48000 mono samples at 48 kHz = exactly 1.0 second.
        let samples = vec![0.0_f32; 48000];
        let buf = AudioBuffer::new(samples, 1, 48000);
        assert!((buf.duration_secs() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_empty_buffer() {
        let buf = AudioBuffer::new(vec![], 2, 48000);
        assert!(buf.is_empty());
        assert_eq!(buf.frame_count(), 0);
        assert!((buf.duration_secs() - 0.0).abs() < f64::EPSILON);
    }

    // -- AudioStreamConfig tests --------------------------------------------

    #[test]
    fn test_usb_config() {
        let config = AudioStreamConfig {
            sample_rate: 48000,
            channels: 2,
            sample_format: AudioSampleFormat::F32,
        };
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channels, 2);
        assert_eq!(config.sample_format, AudioSampleFormat::F32);
    }

    #[test]
    fn test_dax_config() {
        let config = AudioStreamConfig {
            sample_rate: 24000,
            channels: 2,
            sample_format: AudioSampleFormat::F32,
        };
        assert_eq!(config.sample_rate, 24000);
        assert_eq!(config.channels, 2);
        assert_eq!(config.sample_format, AudioSampleFormat::F32);
    }

    #[test]
    fn test_config_equality() {
        let a = AudioStreamConfig {
            sample_rate: 48000,
            channels: 2,
            sample_format: AudioSampleFormat::F32,
        };
        let b = AudioStreamConfig {
            sample_rate: 48000,
            channels: 2,
            sample_format: AudioSampleFormat::F32,
        };
        assert_eq!(a, b);
    }

    // -- AudioReceiver / AudioSender tests ----------------------------------

    #[tokio::test]
    async fn test_receiver_recv() {
        let config = AudioStreamConfig {
            sample_rate: 48000,
            channels: 2,
            sample_format: AudioSampleFormat::F32,
        };
        let (tx, rx) = mpsc::channel(8);
        let mut receiver = AudioReceiver::new(rx, config);

        let buf = AudioBuffer::new(vec![0.1, -0.1, 0.2, -0.2], 2, 48000);
        tx.send(buf).await.unwrap();

        let received = receiver.recv().await.unwrap();
        assert_eq!(received.samples, vec![0.1, -0.1, 0.2, -0.2]);
        assert_eq!(received.channels, 2);
        assert_eq!(received.sample_rate, 48000);
    }

    #[tokio::test]
    async fn test_receiver_closed() {
        let config = AudioStreamConfig {
            sample_rate: 48000,
            channels: 2,
            sample_format: AudioSampleFormat::F32,
        };
        let (tx, rx) = mpsc::channel::<AudioBuffer>(8);
        let mut receiver = AudioReceiver::new(rx, config);

        // Drop the sender to close the channel.
        drop(tx);

        let result = receiver.recv().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_sender_send() {
        let config = AudioStreamConfig {
            sample_rate: 48000,
            channels: 1,
            sample_format: AudioSampleFormat::F32,
        };
        let (tx, mut rx) = mpsc::channel(8);
        let sender = AudioSender::new(tx, config);

        let buf = AudioBuffer::new(vec![0.5, -0.5, 0.25], 1, 48000);
        sender.send(buf).await.unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.samples, vec![0.5, -0.5, 0.25]);
        assert_eq!(received.channels, 1);
    }

    #[tokio::test]
    async fn test_sender_closed() {
        let config = AudioStreamConfig {
            sample_rate: 48000,
            channels: 2,
            sample_format: AudioSampleFormat::F32,
        };
        let (tx, rx) = mpsc::channel(8);
        let sender = AudioSender::new(tx, config);

        // Drop the receiver to close the channel.
        drop(rx);

        let buf = AudioBuffer::new(vec![0.0; 4], 2, 48000);
        let result = sender.send(buf).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::StreamClosed));
    }

    #[test]
    fn test_config_accessor() {
        let config = AudioStreamConfig {
            sample_rate: 24000,
            channels: 2,
            sample_format: AudioSampleFormat::F32,
        };

        // Verify AudioReceiver::config() returns the correct config.
        let (_tx, rx) = mpsc::channel::<AudioBuffer>(8);
        let receiver = AudioReceiver::new(rx, config.clone());
        assert_eq!(receiver.config().sample_rate, 24000);
        assert_eq!(receiver.config().channels, 2);
        assert_eq!(receiver.config().sample_format, AudioSampleFormat::F32);

        // Verify AudioSender::config() returns the correct config.
        let (tx, _rx) = mpsc::channel::<AudioBuffer>(8);
        let sender = AudioSender::new(tx, config);
        assert_eq!(sender.config().sample_rate, 24000);
        assert_eq!(sender.config().channels, 2);
        assert_eq!(sender.config().sample_format, AudioSampleFormat::F32);
    }

    // -- usb_audio_config() helper -------------------------------------------

    #[test]
    fn test_usb_audio_config() {
        let config = super::usb_audio_config();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channels, 2);
        assert_eq!(config.sample_format, AudioSampleFormat::I16);
    }

    // -- AudioSampleFormat tests --------------------------------------------

    #[test]
    fn test_format_debug() {
        assert_eq!(format!("{:?}", AudioSampleFormat::F32), "F32");
        assert_eq!(format!("{:?}", AudioSampleFormat::I16), "I16");
    }
}
