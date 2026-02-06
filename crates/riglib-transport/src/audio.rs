//! cpal-based USB audio backend for riglib.
//!
//! This module provides [`CpalAudioBackend`] which opens USB audio devices
//! via the [`cpal`] crate and bridges its callback-based model to tokio
//! async channels, producing [`AudioBuffer`]s compatible with the
//! [`AudioCapable`](riglib_core::AudioCapable) trait.
//!
//! # Architecture
//!
//! cpal uses a callback model: the operating system's audio subsystem invokes
//! a closure on a high-priority audio thread whenever a buffer of samples is
//! available (input) or needed (output). This module bridges that callback
//! model to tokio async channels:
//!
//! - **RX (input)**: cpal callback -> `mpsc::Sender`
//!   -> `AudioReceiver`
//! - **TX (output)**: `AudioSender` -> `mpsc::Receiver`
//!   -> cpal callback
//!
//! The channel buffer size is 32 buffers, chosen to absorb scheduling jitter
//! without excessive memory use. At 48 kHz stereo with ~512-sample callbacks,
//! this represents roughly 340 ms of audio.
//!
//! # Platform support
//!
//! | Platform     | Backend    | Build dependency               |
//! |-------------|------------|--------------------------------|
//! | Linux/BSD   | ALSA       | `libasound2-dev` (Debian/Ubuntu) |
//! | macOS/iOS   | CoreAudio  | Xcode toolchain                |
//! | Windows     | WASAPI     | None                           |
//!
//! USB Audio Class 1.0 devices (used by all supported transceivers) appear
//! as normal audio devices in all three backends -- no special configuration
//! is needed.
//!
//! # Feature flag
//!
//! This module is only compiled when the `audio` feature is enabled:
//!
//! ```toml
//! [dependencies]
//! riglib-transport = { version = "0.1", features = ["audio"] }
//! ```

use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use riglib_core::error::{Error, Result};
use riglib_core::{AudioBuffer, AudioReceiver, AudioSampleFormat, AudioSender, AudioStreamConfig};
use tokio::sync::mpsc;

/// Channel buffer capacity for audio streams.
///
/// 32 buffers absorbs scheduling jitter without excessive memory use.
/// At 48 kHz stereo with ~512-sample callbacks, this is roughly 340 ms.
const CHANNEL_BUFFER_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Sample format conversion helpers
// ---------------------------------------------------------------------------

/// Convert a 16-bit signed integer audio sample to f32 in `[-1.0, 1.0]`.
///
/// The conversion divides by `i16::MAX` (32767), which means `i16::MIN`
/// (-32768) maps to approximately `-1.0000305`. This is the standard
/// conversion used by audio software.
#[inline]
pub fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / i16::MAX as f32
}

/// Convert an f32 audio sample to 16-bit signed integer.
///
/// Input values are clamped to `[-1.0, 1.0]` before scaling by `i16::MAX`.
#[inline]
pub fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

// ---------------------------------------------------------------------------
// AudioDeviceInfo
// ---------------------------------------------------------------------------

/// Information about an available audio device.
///
/// Returned by [`list_audio_devices()`] to help the user select the correct
/// USB audio device for their rig.
#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    /// Device name as reported by the OS audio subsystem.
    ///
    /// Typical names for ham radio transceivers:
    /// - Linux: `"USB Audio CODEC"` or `"Burr-Brown from TI USB Audio CODEC"`
    /// - macOS: `"USB Audio CODEC"`
    /// - Windows: `"USB Audio CODEC"`
    pub name: String,

    /// Whether this device supports audio input (microphone / RX audio).
    pub is_input: bool,

    /// Whether this device supports audio output (speaker / TX audio).
    pub is_output: bool,
}

// ---------------------------------------------------------------------------
// Device enumeration
// ---------------------------------------------------------------------------

/// List all available audio input and output devices.
///
/// Uses the platform's default audio host (ALSA on Linux, CoreAudio on macOS,
/// WASAPI on Windows) to enumerate devices. USB Audio Class 1.0 devices
/// (used by all supported transceivers) appear alongside built-in audio
/// devices.
///
/// # Note
///
/// On Linux with ALSA, devices plugged in after the program started may not
/// appear until re-enumeration. Call this function again to refresh.
///
/// # Errors
///
/// Returns [`Error::Transport`] if the audio host cannot enumerate devices.
pub fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();

    // Collect input device names into a set for cross-referencing.
    let mut input_names = std::collections::HashSet::new();
    if let Ok(inputs) = host.input_devices() {
        for device in inputs {
            if let Ok(desc) = device.description() {
                input_names.insert(desc.name().to_string());
            }
        }
    }

    // Collect output device names into a set.
    let mut output_names = std::collections::HashSet::new();
    if let Ok(outputs) = host.output_devices() {
        for device in outputs {
            if let Ok(desc) = device.description() {
                output_names.insert(desc.name().to_string());
            }
        }
    }

    // Merge into a single list with is_input / is_output flags.
    let all_names: std::collections::HashSet<&str> = input_names
        .iter()
        .chain(output_names.iter())
        .map(|s| s.as_str())
        .collect();

    let mut devices: Vec<AudioDeviceInfo> = all_names
        .into_iter()
        .map(|name| AudioDeviceInfo {
            name: name.to_string(),
            is_input: input_names.contains(name),
            is_output: output_names.contains(name),
        })
        .collect();

    // Sort by name for deterministic output.
    devices.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(devices)
}

// ---------------------------------------------------------------------------
// Internal helper: find a device by name
// ---------------------------------------------------------------------------

/// Find an audio device by name, searching input or output devices.
fn find_device(name: &str, for_input: bool) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let devices = if for_input {
        host.input_devices()
            .map_err(|e| Error::Transport(format!("failed to enumerate input devices: {e}")))?
    } else {
        host.output_devices()
            .map_err(|e| Error::Transport(format!("failed to enumerate output devices: {e}")))?
    };

    for device in devices {
        if let Ok(desc) = device.description() {
            if desc.name() == name {
                return Ok(device);
            }
        }
    }

    Err(Error::Transport(format!("audio device not found: {name}")))
}

/// Convert a cpal [`SampleFormat`] to a riglib [`AudioSampleFormat`].
///
/// Only `I16` and `F32` are mapped directly; all other formats are reported
/// as `I16` since we convert them internally anyway.
fn to_audio_sample_format(fmt: SampleFormat) -> AudioSampleFormat {
    match fmt {
        SampleFormat::F32 => AudioSampleFormat::F32,
        _ => AudioSampleFormat::I16,
    }
}

// ---------------------------------------------------------------------------
// CpalAudioBackend
// ---------------------------------------------------------------------------

/// Backend that bridges cpal audio streams to tokio async channels.
///
/// This struct opens USB audio devices via the operating system's audio
/// subsystem (ALSA, CoreAudio, or WASAPI) and converts between cpal's
/// callback model and the [`AudioReceiver`] / [`AudioSender`] channel
/// model used by the rest of riglib.
///
/// # Lifecycle
///
/// The cpal [`Stream`](cpal::Stream) objects are held inside this struct.
/// **Dropping the streams stops audio capture/playback**, so the backend
/// must be kept alive for the duration of audio streaming. Call
/// [`stop()`](CpalAudioBackend::stop) to explicitly release streams.
///
/// # Example
///
/// ```no_run
/// use riglib_transport::audio::CpalAudioBackend;
///
/// # fn example() -> riglib_core::Result<()> {
/// let mut backend = CpalAudioBackend::new("USB Audio CODEC");
/// let rx = backend.start_input()?;
/// // rx is an AudioReceiver -- use rx.recv().await in async code
/// # Ok(())
/// # }
/// ```
pub struct CpalAudioBackend {
    device_name: String,
    /// Held alive to keep the input stream running. Dropping stops capture.
    input_stream: Option<cpal::Stream>,
    /// Held alive to keep the output stream running. Dropping stops playback.
    output_stream: Option<cpal::Stream>,
}

impl CpalAudioBackend {
    /// Create a new backend targeting the named audio device.
    ///
    /// No audio streams are opened until [`start_input()`](Self::start_input)
    /// or [`start_output()`](Self::start_output) is called.
    ///
    /// The `device_name` should match the name reported by
    /// [`list_audio_devices()`] (e.g., `"USB Audio CODEC"` for most
    /// ham radio transceivers).
    pub fn new(device_name: &str) -> Self {
        CpalAudioBackend {
            device_name: device_name.to_string(),
            input_stream: None,
            output_stream: None,
        }
    }

    /// Open the input (RX) audio stream.
    ///
    /// Returns an [`AudioReceiver`] connected to the cpal input callback.
    /// Audio is captured at the device's native configuration (typically
    /// 48 kHz i16 stereo for USB Audio CODECs) and converted to f32 in
    /// the callback before being sent through the channel.
    ///
    /// The channel holds up to 32 buffers to absorb scheduling jitter.
    /// If the consumer falls behind, buffers are dropped silently (via
    /// `try_send`) rather than blocking the audio thread.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transport`] if the device is not found, the default
    /// input configuration cannot be queried, or the stream fails to start.
    pub fn start_input(&mut self) -> Result<AudioReceiver> {
        let device = find_device(&self.device_name, true)?;
        let supported_config = device
            .default_input_config()
            .map_err(|e| Error::Transport(format!("no default input config: {e}")))?;

        let sample_format = supported_config.sample_format();
        let sample_rate = supported_config.sample_rate();
        let channels = supported_config.channels();
        let stream_config: cpal::StreamConfig = supported_config.into();

        let (tx, rx) = mpsc::channel::<AudioBuffer>(CHANNEL_BUFFER_SIZE);

        let stream = match sample_format {
            SampleFormat::I16 => {
                build_input_stream_i16(&device, &stream_config, channels, sample_rate, tx)?
            }
            SampleFormat::F32 => {
                build_input_stream_f32(&device, &stream_config, channels, sample_rate, tx)?
            }
            other => {
                return Err(Error::Transport(format!(
                    "unsupported input sample format: {other}"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| Error::Transport(format!("failed to start input stream: {e}")))?;

        self.input_stream = Some(stream);

        let config = AudioStreamConfig {
            sample_rate,
            channels,
            sample_format: AudioSampleFormat::F32,
        };

        Ok(AudioReceiver::new(rx, config))
    }

    /// Open the output (TX) audio stream.
    ///
    /// Returns an [`AudioSender`] connected to the cpal output callback.
    /// Audio buffers sent through the sender are expected to contain f32
    /// samples, which are converted to the device's native format (typically
    /// i16) in the output callback.
    ///
    /// When no audio data is available (the channel is empty), the callback
    /// outputs silence to avoid underruns.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transport`] if the device is not found, the default
    /// output configuration cannot be queried, or the stream fails to start.
    pub fn start_output(&mut self) -> Result<AudioSender> {
        let device = find_device(&self.device_name, false)?;
        let supported_config = device
            .default_output_config()
            .map_err(|e| Error::Transport(format!("no default output config: {e}")))?;

        let sample_format = supported_config.sample_format();
        let sample_rate = supported_config.sample_rate();
        let channels = supported_config.channels();
        let stream_config: cpal::StreamConfig = supported_config.into();

        let (tx, rx) = mpsc::channel::<AudioBuffer>(CHANNEL_BUFFER_SIZE);

        let stream = match sample_format {
            SampleFormat::I16 => {
                build_output_stream_i16(&device, &stream_config, channels, sample_rate, rx)?
            }
            SampleFormat::F32 => {
                build_output_stream_f32(&device, &stream_config, channels, sample_rate, rx)?
            }
            other => {
                return Err(Error::Transport(format!(
                    "unsupported output sample format: {other}"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| Error::Transport(format!("failed to start output stream: {e}")))?;

        self.output_stream = Some(stream);

        let config = AudioStreamConfig {
            sample_rate,
            channels,
            sample_format: AudioSampleFormat::F32,
        };

        Ok(AudioSender::new(tx, config))
    }

    /// Stop all audio streams.
    ///
    /// Drops the cpal streams, which causes the OS to release the audio
    /// device. Any connected [`AudioReceiver`] will receive `None` on the
    /// next `recv()`, and any connected [`AudioSender`] will get
    /// [`Error::StreamClosed`] on the next `send()`.
    pub fn stop(&mut self) {
        self.input_stream = None;
        self.output_stream = None;
    }

    /// Get the native audio configuration for the device.
    ///
    /// Queries the device's default input configuration and returns the
    /// corresponding [`AudioStreamConfig`]. If the device also supports
    /// output, the output configuration is typically identical.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Transport`] if the device is not found or its
    /// configuration cannot be queried.
    pub fn native_config(&self) -> Result<AudioStreamConfig> {
        // Try input config first; fall back to output config.
        let config = if let Ok(device) = find_device(&self.device_name, true) {
            device
                .default_input_config()
                .map_err(|e| Error::Transport(format!("no default input config: {e}")))?
        } else {
            let device = find_device(&self.device_name, false)?;
            device
                .default_output_config()
                .map_err(|e| Error::Transport(format!("no default output config: {e}")))?
        };

        Ok(AudioStreamConfig {
            sample_rate: config.sample_rate(),
            channels: config.channels(),
            sample_format: to_audio_sample_format(config.sample_format()),
        })
    }
}

// ---------------------------------------------------------------------------
// Input stream builders (one per sample format)
// ---------------------------------------------------------------------------

/// Build a cpal input stream for i16 sample format.
///
/// Converts i16 samples to f32 before sending through the channel.
fn build_input_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    sample_rate: u32,
    tx: mpsc::Sender<AudioBuffer>,
) -> Result<cpal::Stream> {
    let stream = device
        .build_input_stream(
            config,
            move |data: &[i16], _info: &cpal::InputCallbackInfo| {
                let samples: Vec<f32> = data.iter().map(|&s| i16_to_f32(s)).collect();
                let buffer = AudioBuffer::new(samples, channels, sample_rate);
                // Use try_send to never block the audio thread.
                let _ = tx.try_send(buffer);
            },
            |err| {
                tracing::error!("cpal input stream error: {}", err);
            },
            None,
        )
        .map_err(|e| Error::Transport(format!("failed to build input stream: {e}")))?;

    Ok(stream)
}

/// Build a cpal input stream for f32 sample format.
///
/// No conversion needed -- samples are already f32.
fn build_input_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    sample_rate: u32,
    tx: mpsc::Sender<AudioBuffer>,
) -> Result<cpal::Stream> {
    let stream = device
        .build_input_stream(
            config,
            move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                let samples = data.to_vec();
                let buffer = AudioBuffer::new(samples, channels, sample_rate);
                let _ = tx.try_send(buffer);
            },
            |err| {
                tracing::error!("cpal input stream error: {}", err);
            },
            None,
        )
        .map_err(|e| Error::Transport(format!("failed to build input stream: {e}")))?;

    Ok(stream)
}

// ---------------------------------------------------------------------------
// Output stream builders (one per sample format)
// ---------------------------------------------------------------------------

/// Build a cpal output stream for i16 sample format.
///
/// Converts f32 samples from the channel to i16 before writing to the device.
fn build_output_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    _channels: u16,
    _sample_rate: u32,
    mut rx: mpsc::Receiver<AudioBuffer>,
) -> Result<cpal::Stream> {
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [i16], _info: &cpal::OutputCallbackInfo| {
                match rx.try_recv() {
                    Ok(buffer) => {
                        for (out, &sample) in data.iter_mut().zip(buffer.samples.iter()) {
                            *out = f32_to_i16(sample);
                        }
                        // Zero any remaining samples if the buffer is shorter
                        // than the output request.
                        if buffer.samples.len() < data.len() {
                            for out in &mut data[buffer.samples.len()..] {
                                *out = 0;
                            }
                        }
                    }
                    Err(_) => {
                        // No data available -- output silence.
                        data.fill(0);
                    }
                }
            },
            |err| {
                tracing::error!("cpal output stream error: {}", err);
            },
            None,
        )
        .map_err(|e| Error::Transport(format!("failed to build output stream: {e}")))?;

    Ok(stream)
}

/// Build a cpal output stream for f32 sample format.
///
/// No conversion needed -- samples are already f32.
fn build_output_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    _channels: u16,
    _sample_rate: u32,
    mut rx: mpsc::Receiver<AudioBuffer>,
) -> Result<cpal::Stream> {
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| match rx.try_recv() {
                Ok(buffer) => {
                    for (out, &sample) in data.iter_mut().zip(buffer.samples.iter()) {
                        *out = sample;
                    }
                    if buffer.samples.len() < data.len() {
                        for out in &mut data[buffer.samples.len()..] {
                            *out = 0.0;
                        }
                    }
                }
                Err(_) => {
                    data.fill(0.0);
                }
            },
            |err| {
                tracing::error!("cpal output stream error: {}", err);
            },
            None,
        )
        .map_err(|e| Error::Transport(format!("failed to build output stream: {e}")))?;

    Ok(stream)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Conversion helper tests (run without audio hardware) ---------------

    #[test]
    fn test_i16_to_f32_zero() {
        assert_eq!(i16_to_f32(0), 0.0);
    }

    #[test]
    fn test_i16_to_f32_max() {
        assert_eq!(i16_to_f32(i16::MAX), 1.0);
    }

    #[test]
    fn test_i16_to_f32_min() {
        // i16::MIN is -32768, i16::MAX is 32767, so the result is
        // slightly less than -1.0 (approximately -1.0000305).
        let val = i16_to_f32(i16::MIN);
        assert!(val < -1.0);
        assert!(val > -1.001);
    }

    #[test]
    fn test_i16_to_f32_positive() {
        let val = i16_to_f32(16384);
        assert!((val - 0.5000153).abs() < 0.001);
    }

    #[test]
    fn test_i16_to_f32_negative() {
        let val = i16_to_f32(-16384);
        assert!((val - (-0.5000153)).abs() < 0.001);
    }

    #[test]
    fn test_f32_to_i16_zero() {
        assert_eq!(f32_to_i16(0.0), 0);
    }

    #[test]
    fn test_f32_to_i16_max() {
        assert_eq!(f32_to_i16(1.0), i16::MAX);
    }

    #[test]
    fn test_f32_to_i16_min() {
        assert_eq!(f32_to_i16(-1.0), -i16::MAX);
    }

    #[test]
    fn test_f32_to_i16_clamp_high() {
        // Values above 1.0 should clamp to i16::MAX.
        assert_eq!(f32_to_i16(1.5), i16::MAX);
        assert_eq!(f32_to_i16(100.0), i16::MAX);
    }

    #[test]
    fn test_f32_to_i16_clamp_low() {
        // Values below -1.0 should clamp to -i16::MAX.
        assert_eq!(f32_to_i16(-1.5), -i16::MAX);
        assert_eq!(f32_to_i16(-100.0), -i16::MAX);
    }

    #[test]
    fn test_i16_f32_round_trip() {
        // For most values, converting i16 -> f32 -> i16 should be close
        // to the original. We test a range of values.
        let test_values: Vec<i16> = vec![0, 1, -1, 100, -100, 1000, -1000, 10000, -10000, i16::MAX];
        for &original in &test_values {
            let converted = f32_to_i16(i16_to_f32(original));
            assert!(
                (original - converted).abs() <= 1,
                "round-trip failed for {original}: got {converted}"
            );
        }
    }

    // -- AudioDeviceInfo tests ----------------------------------------------

    #[test]
    fn test_audio_device_info_debug() {
        let info = AudioDeviceInfo {
            name: "USB Audio CODEC".to_string(),
            is_input: true,
            is_output: true,
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("USB Audio CODEC"));
        assert!(debug.contains("is_input: true"));
        assert!(debug.contains("is_output: true"));
    }

    #[test]
    fn test_audio_device_info_clone() {
        let info = AudioDeviceInfo {
            name: "Test Device".to_string(),
            is_input: true,
            is_output: false,
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, "Test Device");
        assert!(cloned.is_input);
        assert!(!cloned.is_output);
    }

    // -- Device enumeration test (requires audio hardware) ------------------

    #[test]
    #[ignore] // requires audio hardware -- will not pass in CI
    fn test_list_devices() {
        let devices = list_audio_devices().unwrap();
        // Just verify it does not panic -- device list may be empty in CI.
        println!("Found {} audio device(s):", devices.len());
        for d in &devices {
            println!(
                "  {} (input={}, output={})",
                d.name, d.is_input, d.is_output
            );
        }
    }

    // -- CpalAudioBackend construction test (no hardware needed) ------------

    #[test]
    fn test_backend_new() {
        let backend = CpalAudioBackend::new("USB Audio CODEC");
        assert_eq!(backend.device_name, "USB Audio CODEC");
        assert!(backend.input_stream.is_none());
        assert!(backend.output_stream.is_none());
    }

    #[test]
    fn test_backend_stop_without_streams() {
        // Calling stop() when no streams are open should not panic.
        let mut backend = CpalAudioBackend::new("nonexistent");
        backend.stop();
        assert!(backend.input_stream.is_none());
        assert!(backend.output_stream.is_none());
    }

    // -- Sample format conversion test --------------------------------------

    #[test]
    fn test_to_audio_sample_format() {
        assert_eq!(
            to_audio_sample_format(SampleFormat::F32),
            AudioSampleFormat::F32
        );
        assert_eq!(
            to_audio_sample_format(SampleFormat::I16),
            AudioSampleFormat::I16
        );
        // Other formats map to I16 since we convert them internally.
        assert_eq!(
            to_audio_sample_format(SampleFormat::I32),
            AudioSampleFormat::I16
        );
        assert_eq!(
            to_audio_sample_format(SampleFormat::U16),
            AudioSampleFormat::I16
        );
    }
}
