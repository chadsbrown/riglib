//! Frequency scan with S-meter readings.
//!
//! Demonstrates scanning across a frequency range on a connected rig,
//! reading the S-meter at each step. This is useful for band surveys,
//! finding open frequencies, or monitoring band activity.
//!
//! The example scans the 20-meter band (14.000 - 14.350 MHz) in 10 kHz
//! steps, pausing briefly at each frequency to let the AGC settle before
//! reading the signal strength.
//!
//! # Requirements
//!
//! - A rig connected via the appropriate interface
//! - Serial port path adjusted for your system
//!
//! # Usage
//!
//! ```sh
//! cargo run -p riglib --features icom --example scan_frequencies
//! ```

use std::time::Duration;

use riglib::icom::IcomBuilder;
use riglib::icom::models::ic_7300;
use riglib::{ReceiverId, Rig};

/// Scan parameters.
const START_FREQ: u64 = 14_000_000; // 14.000 MHz
const END_FREQ: u64 = 14_350_000; // 14.350 MHz
const STEP_HZ: u64 = 10_000; // 10 kHz steps
const SETTLE_MS: u64 = 200; // AGC settle time

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let serial_port = "/dev/ttyUSB0";

    println!("Connecting to IC-7300 on {}...", serial_port);

    let rig = IcomBuilder::new(ic_7300())
        .serial_port(serial_port)
        .baud_rate(115_200)
        .build()
        .await?;

    let info = rig.info();
    println!("Connected: {} {}", info.manufacturer, info.model_name);

    let rx = ReceiverId::VFO_A;

    // Save the current frequency so we can restore it later.
    let original_freq = rig.get_frequency(rx).await?;
    println!(
        "Original frequency: {:.3} MHz\n",
        original_freq as f64 / 1_000_000.0
    );

    println!(
        "Scanning {:.3} - {:.3} MHz in {} kHz steps...\n",
        START_FREQ as f64 / 1_000_000.0,
        END_FREQ as f64 / 1_000_000.0,
        STEP_HZ / 1_000
    );

    println!("{:<14} {:>10}", "Frequency", "S-meter");
    println!("{:-<14} {:-<10}", "", "");

    let mut freq = START_FREQ;
    while freq <= END_FREQ {
        // Tune to the frequency.
        rig.set_frequency(rx, freq).await?;

        // Let the AGC settle before reading the meter.
        tokio::time::sleep(Duration::from_millis(SETTLE_MS)).await;

        // Read the S-meter.
        let dbm = rig.get_s_meter(rx).await?;

        // Format a simple bar graph.
        let bar_len = ((dbm + 127.0) / 2.0).max(0.0) as usize;
        let bar: String = "#".repeat(bar_len.min(40));

        println!(
            "{:>10.3} MHz {:>+7.1} dBm  {}",
            freq as f64 / 1_000_000.0,
            dbm,
            bar
        );

        freq += STEP_HZ;
    }

    // Restore original frequency.
    println!("\nRestoring original frequency...");
    rig.set_frequency(rx, original_freq).await?;
    println!("Restored: {:.3} MHz", original_freq as f64 / 1_000_000.0);

    Ok(())
}
