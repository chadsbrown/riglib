//! FlexRadio SmartSDR protocol backend for riglib.
//!
//! This crate implements the FlexRadio SmartSDR TCP/IP protocol used by all
//! FLEX-6000/8000 series software-defined radios. It provides:
//!
//! - **SmartSDR client** ([`client`]) -- TCP command/status protocol with
//!   sequence-numbered commands, asynchronous status parsing, and VITA-49
//!   UDP data stream routing.
//! - **VITA-49 codec** ([`vita49`]) -- parse and build VITA-49 Extension Data
//!   packets for meter data, DAX audio, and discovery broadcasts.
//! - **DAX audio** ([`dax`]) -- receive and send audio via FlexRadio DAX
//!   channels (24 kHz stereo f32 over VITA-49 UDP).
//! - **Discovery** ([`discovery`]) -- find FlexRadio units on the LAN by
//!   listening for VITA-49 discovery broadcasts on UDP port 4992.
//! - **Model definitions** ([`models`]) -- static capability data for all
//!   supported FlexRadio models (FLEX-6400, 6400M, 6600, 6600M, 6700, 8600).
//! - **FlexRadio** ([`rig`]) -- the [`Rig`](riglib_core::Rig) and
//!   [`AudioCapable`](riglib_core::AudioCapable) trait implementations.
//! - **FlexRadioBuilder** ([`builder`]) -- fluent builder for constructing
//!   `FlexRadio` instances with configurable network parameters.
//!
//! # Architecture
//!
//! Unlike serial-protocol rigs, FlexRadio uses a split transport:
//! - **TCP** for commands (`C<seq>|<cmd>\n`) and responses/status messages
//! - **UDP** for real-time data: meter readings, DAX audio, and I/Q streams
//!
//! The [`SmartSdrClient`](client::SmartSdrClient) manages both connections and
//! maintains a cached [`RadioState`](state::RadioState) updated by status
//! messages. Most `Rig` trait getters read from this cache rather than
//! sending a command, making them very fast.
//!
//! # Example
//!
//! ```no_run
//! use riglib_flex::{FlexRadio, FlexRadioBuilder};
//! use riglib_flex::models::flex_6600;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! // Discover radios on the LAN
//! let radios = FlexRadio::discover(Duration::from_secs(3)).await?;
//! if let Some(radio) = radios.first() {
//!     let rig = FlexRadioBuilder::new()
//!         .radio(radio)
//!         .build()
//!         .await?;
//!     // Use rig via the Rig trait...
//! }
//! # Ok(())
//! # }
//! ```

pub mod builder;
pub mod client;
pub mod codec;
pub mod dax;
pub mod discovery;
pub mod meters;
pub mod mode;
pub mod models;
pub mod rig;
pub mod state;
pub mod vita49;

pub use builder::{FlexRadioBuilder, FlexTransports};
pub use dax::{DAX_CHANNELS, DAX_SAMPLE_RATE, DaxStream, dax_audio_config};
pub use rig::FlexRadio;
