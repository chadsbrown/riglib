//! # riglib -- Modern Rig Control for Amateur Radio
//!
//! `riglib` is an asynchronous Rust library for controlling amateur radio
//! transceivers from Icom, Yaesu, Kenwood, Elecraft, and FlexRadio. It is
//! designed for contest logging software, panadapter displays, and station
//! automation where low-latency, reliable rig control is essential.
//!
//! ## Quick Start
//!
//! Add `riglib` to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! riglib = { version = "0.1", features = ["icom"] }
//! tokio = { version = "1", features = ["full"] }
//! ```
//!
//! Connect to a rig and read its frequency:
//!
//! ```no_run
//! use riglib::{Rig, ReceiverId};
//! use riglib::icom::{IcomBuilder, models::ic_7610};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let rig = IcomBuilder::new(ic_7610())
//!         .serial_port("/dev/ttyUSB0")
//!         .baud_rate(115_200)
//!         .build()
//!         .await?;
//!
//!     let freq = rig.get_frequency(ReceiverId::VFO_A).await?;
//!     println!("VFO-A: {} Hz", freq);
//!     Ok(())
//! }
//! ```
//!
//! ## Architecture
//!
//! The library is organized as a workspace of focused crates:
//!
//! | Crate               | Purpose                                        |
//! |----------------------|------------------------------------------------|
//! | `riglib-core`        | Traits ([`Rig`], [`AudioCapable`]), types, errors |
//! | `riglib-transport`   | Serial, TCP, UDP transport implementations     |
//! | `riglib-icom`        | Icom CI-V binary protocol driver               |
//! | `riglib-yaesu`       | Yaesu CAT text protocol driver                 |
//! | `riglib-kenwood`     | Kenwood CAT text protocol driver               |
//! | `riglib-elecraft`    | Elecraft extended Kenwood protocol driver       |
//! | `riglib-flex`        | FlexRadio SmartSDR TCP/IP + VITA-49 driver     |
//! | **`riglib`**         | This facade crate -- re-exports everything     |
//!
//! All manufacturer drivers implement the [`Rig`] trait, so application code
//! can work with `dyn Rig` and remain manufacturer-agnostic.
//!
//! ## Feature Flags
//!
//! Each manufacturer backend is gated behind a feature flag:
//!
//! | Feature    | Enables                          | Default |
//! |------------|----------------------------------|---------|
//! | `icom`     | [`icom`] module (CI-V protocol)  | yes     |
//! | `yaesu`    | [`yaesu`] module (CAT protocol)  | yes     |
//! | `kenwood`  | [`kenwood`] module (CAT protocol)| yes     |
//! | `elecraft` | [`elecraft`] module (extended Kenwood) | yes |
//! | `flex`     | [`flex`] module (SmartSDR)       | yes     |
//! | `audio`    | USB audio streaming via cpal     | no      |
//! | `full`     | All manufacturer backends        | no      |
//!
//! ## The `Rig` Trait
//!
//! The [`Rig`] trait is the central abstraction. It provides async methods
//! for all common transceiver operations:
//!
//! - **Frequency**: [`get_frequency`](Rig::get_frequency), [`set_frequency`](Rig::set_frequency)
//! - **Mode**: [`get_mode`](Rig::get_mode), [`set_mode`](Rig::set_mode)
//! - **PTT**: [`get_ptt`](Rig::get_ptt), [`set_ptt`](Rig::set_ptt)
//! - **Metering**: [`get_s_meter`](Rig::get_s_meter), [`get_swr`](Rig::get_swr)
//! - **Split**: [`get_split`](Rig::get_split), [`set_split`](Rig::set_split)
//! - **Events**: [`subscribe`](Rig::subscribe) for real-time state change notifications
//!
//! ## Event Subscription
//!
//! All rig drivers emit [`RigEvent`]s through a broadcast channel. Subscribe
//! to receive frequency changes, mode changes, PTT transitions, and meter
//! readings without polling:
//!
//! ```no_run
//! use riglib::{Rig, RigEvent};
//! # async fn example(rig: &dyn Rig) -> riglib::Result<()> {
//! let mut events = rig.subscribe()?;
//! loop {
//!     match events.recv().await {
//!         Ok(RigEvent::FrequencyChanged { receiver, freq_hz }) => {
//!             println!("{}: {} Hz", receiver, freq_hz);
//!         }
//!         Ok(event) => println!("{:?}", event),
//!         Err(_) => break,
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Supported Rigs
//!
//! - **Icom**: IC-7300, IC-7610, IC-7851, IC-9700, IC-705, IC-905, and 8 more
//! - **Yaesu**: FT-DX10, FT-DX101D, FT-DX101MP, FT-991A, FT-891, FT-710
//! - **Kenwood**: TS-590S, TS-590SG, TS-990S, TS-890S
//! - **Elecraft**: K3, K3S, KX3, KX2, K4
//! - **FlexRadio**: FLEX-6400, 6400M, 6600, 6600M, 6700, 8600

pub use riglib_core::*;

/// Icom CI-V protocol backend.
///
/// Provides [`IcomRig`](icom::IcomRig) and [`IcomBuilder`](icom::IcomBuilder)
/// for controlling Icom transceivers over the CI-V binary protocol.
/// Supports 14 models from the IC-7300 through the IC-905.
#[cfg(feature = "icom")]
pub mod icom {
    pub use riglib_icom::*;
}

/// Yaesu CAT protocol backend.
///
/// Provides [`YaesuRig`](yaesu::YaesuRig) and [`YaesuBuilder`](yaesu::YaesuBuilder)
/// for controlling Yaesu transceivers over the semicolon-terminated CAT text
/// protocol. Supports modern HF transceivers including the FT-DX101 series.
#[cfg(feature = "yaesu")]
pub mod yaesu {
    pub use riglib_yaesu::*;
}

/// Elecraft extended Kenwood protocol backend.
///
/// Provides [`ElecraftRig`](elecraft::ElecraftRig) and
/// [`ElecraftBuilder`](elecraft::ElecraftBuilder) for controlling Elecraft
/// transceivers. Uses an extended version of the Kenwood text protocol with
/// Elecraft-specific commands for bandwidth, AI mode, and model identification.
#[cfg(feature = "elecraft")]
pub mod elecraft {
    pub use riglib_elecraft::*;
}

/// Kenwood CAT protocol backend.
///
/// Provides [`KenwoodRig`](kenwood::KenwoodRig) and
/// [`KenwoodBuilder`](kenwood::KenwoodBuilder) for controlling Kenwood
/// transceivers over the standard Kenwood CAT text protocol with
/// 11-digit frequency fields and AI (Auto Information) mode.
#[cfg(feature = "kenwood")]
pub mod kenwood {
    pub use riglib_kenwood::*;
}

/// FlexRadio SmartSDR protocol backend.
///
/// Provides [`FlexRadio`](flex::FlexRadio) and
/// [`FlexRadioBuilder`](flex::FlexRadioBuilder) for controlling FlexRadio
/// software-defined radios over TCP/IP. Includes LAN discovery, VITA-49
/// I/Q streaming, and DAX audio integration.
#[cfg(feature = "flex")]
pub mod flex {
    pub use riglib_flex::*;
}

/// Returns a flat list of all supported rig models across all enabled
/// manufacturer backends.
///
/// This is the primary entry point for applications that need to enumerate
/// supported rigs (e.g. for a model picker dropdown). Each manufacturer
/// backend is gated behind its feature flag -- only models from enabled
/// backends are included.
///
/// # Example
///
/// ```
/// let rigs = riglib::supported_rigs();
/// for rig in &rigs {
///     println!("{} {} ({:?})", rig.manufacturer, rig.model_name, rig.connection);
/// }
/// ```
pub fn supported_rigs() -> Vec<RigDefinition> {
    let mut rigs = Vec::new();

    #[cfg(feature = "icom")]
    {
        rigs.extend(
            icom::models::all_icom_models()
                .iter()
                .map(RigDefinition::from),
        );
    }

    #[cfg(feature = "yaesu")]
    {
        rigs.extend(
            yaesu::models::all_yaesu_models()
                .iter()
                .map(RigDefinition::from),
        );
    }

    #[cfg(feature = "kenwood")]
    {
        rigs.extend(
            kenwood::models::all_kenwood_models()
                .iter()
                .map(RigDefinition::from),
        );
    }

    #[cfg(feature = "elecraft")]
    {
        rigs.extend(
            elecraft::models::all_elecraft_models()
                .iter()
                .map(RigDefinition::from),
        );
    }

    #[cfg(feature = "flex")]
    {
        rigs.extend(
            flex::models::all_flex_models()
                .iter()
                .map(RigDefinition::from),
        );
    }

    rigs
}
