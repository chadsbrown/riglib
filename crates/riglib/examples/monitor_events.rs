//! Monitor real-time rig events.
//!
//! Demonstrates subscribing to the rig event stream and printing all
//! events as they arrive. This is useful for building real-time displays,
//! logging band activity, or debugging rig communication.
//!
//! Events include frequency changes, mode changes, PTT transitions,
//! S-meter readings, SWR readings, and connection status changes.
//!
//! # Requirements
//!
//! - A rig connected via the appropriate interface
//! - Serial port path adjusted for your system
//!
//! # Usage
//!
//! ```sh
//! cargo run -p riglib --features icom --example monitor_events
//! ```

use std::time::Duration;

use riglib::icom::IcomBuilder;
use riglib::icom::models::ic_7610;
use riglib::{ReceiverId, Rig, RigEvent};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let serial_port = "/dev/ttyUSB0";

    println!("Connecting to IC-7610 on {}...", serial_port);

    let rig = IcomBuilder::new(ic_7610())
        .serial_port(serial_port)
        .baud_rate(115_200)
        .build()
        .await?;

    let info = rig.info();
    println!("Connected: {} {}\n", info.manufacturer, info.model_name);

    // Subscribe to rig events.
    let mut events = rig.subscribe()?;
    println!("Subscribed to rig events. Monitoring for 60 seconds...");
    println!("(Turn the VFO knob, change modes, or key PTT to generate events)\n");

    // Poll for events for 60 seconds.
    // In a real application, you would do this in a background task.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);

    // Kick off some initial reads to generate events.
    let rx = ReceiverId::VFO_A;
    let freq = rig.get_frequency(rx).await?;
    let mode = rig.get_mode(rx).await?;
    println!(
        "Initial state: {} {:.3} MHz {}\n",
        rx,
        freq as f64 / 1_000_000.0,
        mode
    );

    println!("{:<12} Event", "Timestamp");
    println!("{:-<12} {:-<50}", "", "");

    let start = tokio::time::Instant::now();

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(event)) => {
                let elapsed = start.elapsed();
                let timestamp = format!("{:>6}.{:03}s", elapsed.as_secs(), elapsed.subsec_millis());

                match event {
                    RigEvent::FrequencyChanged { receiver, freq_hz } => {
                        println!(
                            "{} FrequencyChanged  {} -> {:.3} MHz ({} Hz)",
                            timestamp,
                            receiver,
                            freq_hz as f64 / 1_000_000.0,
                            freq_hz
                        );
                    }
                    RigEvent::ModeChanged { receiver, mode } => {
                        println!("{} ModeChanged       {} -> {}", timestamp, receiver, mode);
                    }
                    RigEvent::PttChanged { on } => {
                        let state = if on { "TX" } else { "RX" };
                        println!("{} PttChanged        -> {}", timestamp, state);
                    }
                    RigEvent::SplitChanged { on } => {
                        let state = if on { "ON" } else { "OFF" };
                        println!("{} SplitChanged      -> {}", timestamp, state);
                    }
                    RigEvent::SmeterReading { receiver, dbm } => {
                        println!(
                            "{} SmeterReading     {} = {:.1} dBm",
                            timestamp, receiver, dbm
                        );
                    }
                    RigEvent::PowerReading { watts } => {
                        println!("{} PowerReading      {:.1} W", timestamp, watts);
                    }
                    RigEvent::SwrReading { swr } => {
                        println!("{} SwrReading        {:.1}:1", timestamp, swr);
                    }
                    RigEvent::Connected => {
                        println!("{} Connected", timestamp);
                    }
                    RigEvent::Disconnected => {
                        println!("{} Disconnected", timestamp);
                        break;
                    }
                    RigEvent::Reconnecting { attempt } => {
                        println!("{} Reconnecting      attempt {}", timestamp, attempt);
                    }
                    RigEvent::AgcChanged { receiver, mode } => {
                        println!("{} AgcChanged        {} -> {}", timestamp, receiver, mode);
                    }
                    RigEvent::CwSpeedChanged { wpm } => {
                        println!("{} CwSpeedChanged    {} WPM", timestamp, wpm);
                    }
                    RigEvent::RitChanged { enabled, offset_hz } => {
                        let state = if enabled { "ON" } else { "OFF" };
                        println!(
                            "{} RitChanged        {} offset {} Hz",
                            timestamp, state, offset_hz
                        );
                    }
                    RigEvent::XitChanged { enabled, offset_hz } => {
                        let state = if enabled { "ON" } else { "OFF" };
                        println!(
                            "{} XitChanged        {} offset {} Hz",
                            timestamp, state, offset_hz
                        );
                    }
                    RigEvent::PreampChanged { receiver, level } => {
                        println!("{} PreampChanged     {} -> {}", timestamp, receiver, level);
                    }
                    RigEvent::AttenuatorChanged { receiver, db } => {
                        println!("{} AttenuatorChanged {} -> {} dB", timestamp, receiver, db);
                    }
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                println!("(missed {} events due to lag)", n);
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                println!("Event channel closed.");
                break;
            }
            Err(_) => {
                // Timeout -- monitoring period elapsed.
                break;
            }
        }
    }

    println!("\nMonitoring complete.");
    Ok(())
}
