//! FlexRadio LAN discovery example.
//!
//! Demonstrates discovering FlexRadio transceivers on the local network
//! using VITA-49 UDP broadcast packets. Once a radio is found, connects
//! to it and prints basic information.
//!
//! FlexRadio units continuously broadcast their presence on UDP port 4992.
//! This example listens for those broadcasts for a few seconds, then
//! connects to the first radio found.
//!
//! # Requirements
//!
//! - A FlexRadio FLEX-6000 or FLEX-8000 series radio on the same LAN
//! - UDP port 4992 accessible (not blocked by firewall)
//!
//! # Usage
//!
//! ```sh
//! cargo run -p riglib --features flex --example flexradio_discover
//! ```

use std::time::Duration;

use riglib::Rig;
use riglib::flex::{FlexRadio, FlexRadioBuilder};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Searching for FlexRadio units on the LAN (3 seconds)...\n");

    let radios = FlexRadio::discover(Duration::from_secs(3)).await?;

    if radios.is_empty() {
        println!("No FlexRadio units found on the network.");
        println!("\nTroubleshooting:");
        println!("  - Verify the radio is powered on and connected to the LAN");
        println!("  - Check that UDP port 4992 is not blocked by a firewall");
        println!("  - Ensure your computer is on the same subnet as the radio");
        return Ok(());
    }

    println!("Found {} radio(s):\n", radios.len());

    for (i, radio) in radios.iter().enumerate() {
        println!("  [{}] {} (S/N: {})", i + 1, radio.model, radio.serial);
        println!("      IP: {}:{}", radio.ip, radio.port);
        println!("      Nickname: {}", radio.nickname);
        println!("      Firmware: {}", radio.firmware_version);
        println!();
    }

    // Connect to the first discovered radio.
    let radio = &radios[0];
    println!(
        "Connecting to {} at {}:{}...",
        radio.model, radio.ip, radio.port
    );

    let rig = FlexRadioBuilder::new()
        .radio(radio)
        .client_name("riglib-example")
        .build()
        .await?;

    let info = rig.info();
    println!(
        "Connected: {} {} (id: {})\n",
        info.manufacturer, info.model_name, info.model_id
    );

    // Print capabilities.
    let caps = rig.capabilities();
    println!("Capabilities:");
    println!("  Max slices (receivers): {}", caps.max_receivers);
    println!("  Max power: {} W", caps.max_power_watts);
    println!("  Has split: {}", caps.has_split);
    println!("  Has I/Q output: {}", caps.has_iq_output);
    println!("  Audio streaming: {}", caps.has_audio_streaming);

    // List active slices/receivers.
    let receivers = rig.receivers().await?;
    println!("\nActive receivers: {}", receivers.len());
    for rx in &receivers {
        let freq = rig.get_frequency(*rx).await?;
        let mode = rig.get_mode(*rx).await?;
        println!("  {}: {:.3} MHz {}", rx, freq as f64 / 1_000_000.0, mode);
    }

    println!("\nDisconnecting...");
    rig.disconnect().await?;
    println!("Done.");

    Ok(())
}
