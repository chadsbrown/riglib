//! Basic Icom rig control example.
//!
//! Demonstrates connecting to an Icom IC-7610 via CI-V, reading the
//! current frequency and mode, and setting a new frequency.
//!
//! # Requirements
//!
//! - An Icom IC-7610 (or other CI-V rig) connected via USB
//! - The serial port path adjusted for your system (e.g., `/dev/ttyUSB0`
//!   on Linux, `COM3` on Windows)
//!
//! # Usage
//!
//! ```sh
//! cargo run -p riglib --features icom --example basic_icom
//! ```

use std::time::Duration;

use riglib::icom::IcomBuilder;
use riglib::icom::models::ic_7610;
use riglib::{Mode, ReceiverId, Rig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Adjust this to match your system's serial port.
    let serial_port = "/dev/ttyUSB0";

    println!("Connecting to IC-7610 on {}...", serial_port);

    let rig = IcomBuilder::new(ic_7610())
        .serial_port(serial_port)
        .baud_rate(115_200)
        .command_timeout(Duration::from_millis(300))
        .build()
        .await?;

    // Print rig info.
    let info = rig.info();
    println!(
        "Connected: {} {} (id: {})",
        info.manufacturer, info.model_name, info.model_id
    );

    // Print capabilities.
    let caps = rig.capabilities();
    println!("Max receivers: {}", caps.max_receivers);
    println!("Max power: {} W", caps.max_power_watts);
    println!("Has split: {}", caps.has_split);

    // Read current frequency on VFO-A.
    let rx = ReceiverId::VFO_A;
    let freq = rig.get_frequency(rx).await?;
    println!("{}: {} Hz ({:.3} MHz)", rx, freq, freq as f64 / 1_000_000.0);

    // Read current mode.
    let mode = rig.get_mode(rx).await?;
    println!("Mode: {}", mode);

    // Read S-meter.
    let s_meter = rig.get_s_meter(rx).await?;
    println!("S-meter: {:.1} dBm", s_meter);

    // Set frequency to 14.074 MHz (FT8 on 20 meters).
    let new_freq = 14_074_000;
    println!("\nSetting frequency to {} Hz...", new_freq);
    rig.set_frequency(rx, new_freq).await?;

    // Verify the change.
    let freq = rig.get_frequency(rx).await?;
    println!(
        "Frequency now: {} Hz ({:.3} MHz)",
        freq,
        freq as f64 / 1_000_000.0
    );

    // Set mode to USB.
    println!("Setting mode to USB...");
    rig.set_mode(rx, Mode::USB).await?;

    let mode = rig.get_mode(rx).await?;
    println!("Mode now: {}", mode);

    println!("\nDone.");
    Ok(())
}
