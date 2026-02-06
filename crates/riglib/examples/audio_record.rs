//! Record RX audio from a rig to a WAV file.
//!
//! Demonstrates using the [`AudioCapable`] trait to capture receive audio
//! from a rig and write it to a WAV file using the `hound` crate.
//!
//! The example records 10 seconds of audio from VFO-A and saves it as
//! a 32-bit float WAV file. This is useful for recording contest QSOs,
//! capturing signals for later analysis, or building audio monitoring
//! tools.
//!
//! # Requirements
//!
//! - A FlexRadio (uses DAX audio over the network), or
//! - Any serial-connected rig with the `audio` feature enabled and a USB
//!   audio CODEC configured
//!
//! # Usage
//!
//! ```sh
//! cargo run -p riglib --features flex --example audio_record
//! ```

use std::time::Duration;

use riglib::flex::{FlexRadio, FlexRadioBuilder};
use riglib::{AudioCapable, ReceiverId, Rig};

/// Recording duration in seconds.
const RECORD_SECONDS: u64 = 10;

/// Output WAV file path.
const OUTPUT_FILE: &str = "recording.wav";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("FlexRadio Audio Recording Example\n");

    // Discover and connect to a FlexRadio.
    println!("Discovering FlexRadio on LAN...");
    let radios = FlexRadio::discover(Duration::from_secs(3)).await?;

    let radio = radios
        .first()
        .ok_or_else(|| anyhow::anyhow!("No FlexRadio found on the network"))?;

    println!("Connecting to {} at {}...", radio.model, radio.ip);
    let rig = FlexRadioBuilder::new()
        .radio(radio)
        .client_name("riglib-audio-recorder")
        .build()
        .await?;

    let info = rig.info();
    println!("Connected: {} {}", info.manufacturer, info.model_name);

    // Verify audio is supported.
    if !rig.audio_supported() {
        anyhow::bail!("Audio streaming is not supported by this rig configuration");
    }

    // Get the native audio configuration.
    let audio_config = rig.native_audio_config();
    println!(
        "Audio format: {} Hz, {} channel(s), {:?}\n",
        audio_config.sample_rate, audio_config.channels, audio_config.sample_format
    );

    // Start receiving RX audio from VFO-A (slice 0).
    let rx = ReceiverId::VFO_A;
    println!("Starting RX audio on {}...", rx);
    let mut audio_rx = rig.start_rx_audio(rx, None).await?;

    // Set up the WAV writer.
    let spec = hound::WavSpec {
        channels: audio_config.channels,
        sample_rate: audio_config.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(OUTPUT_FILE, spec)?;

    println!(
        "Recording {} seconds of audio to {}...",
        RECORD_SECONDS, OUTPUT_FILE
    );

    let start = tokio::time::Instant::now();
    let deadline = start + Duration::from_secs(RECORD_SECONDS);
    let mut total_frames: u64 = 0;

    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }

        let remaining = deadline - tokio::time::Instant::now();

        // Receive audio buffers with a timeout to handle the deadline.
        match tokio::time::timeout(remaining, audio_rx.recv()).await {
            Ok(Some(buffer)) => {
                // Write all samples to the WAV file.
                for &sample in &buffer.samples {
                    writer.write_sample(sample)?;
                }
                total_frames += buffer.frame_count() as u64;

                // Print progress every second.
                let elapsed = start.elapsed().as_secs();
                if total_frames % (audio_config.sample_rate as u64) < buffer.frame_count() as u64 {
                    println!(
                        "  [{}/{}s] {} frames recorded",
                        elapsed, RECORD_SECONDS, total_frames
                    );
                }
            }
            Ok(None) => {
                println!("Audio stream ended unexpectedly.");
                break;
            }
            Err(_) => {
                // Timeout reached -- recording duration elapsed.
                break;
            }
        }
    }

    // Finalize the WAV file.
    writer.finalize()?;

    let duration_secs = total_frames as f64 / audio_config.sample_rate as f64;
    println!(
        "\nRecording complete: {} frames ({:.1}s) written to {}",
        total_frames, duration_secs, OUTPUT_FILE
    );

    // Stop audio and disconnect.
    rig.stop_audio().await?;
    rig.disconnect().await?;
    println!("Disconnected.");

    Ok(())
}
