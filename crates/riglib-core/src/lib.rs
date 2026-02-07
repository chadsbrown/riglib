//! riglib-core: Core traits, types, and error definitions for riglib.
//!
//! This crate defines the manufacturer-agnostic abstractions that all riglib
//! backends implement. Contest loggers and other applications depend on these
//! types without pulling in any specific rig driver.
//!
//! # Key types
//!
//! - [`Rig`] -- the unified trait for controlling any transceiver
//! - [`Transport`] -- byte-level communication channel
//! - [`RigEvent`] -- asynchronous state change notifications
//! - [`Error`] / [`Result`] -- error handling

pub mod audio;
pub mod band;
pub mod error;
pub mod events;
pub mod helpers;
pub mod rig;
pub mod transport;
pub mod types;

// Re-export key types at crate root for ergonomic `use riglib_core::*`.
pub use audio::{
    AudioBuffer, AudioCapable, AudioReceiver, AudioSampleFormat, AudioSender, AudioStreamConfig,
    usb_audio_config,
};
pub use band::{Band, ParseBandError};
pub use error::{Error, Result};
pub use events::RigEvent;
pub use helpers::{format_freq_mhz, s_units_from_dbm};
pub use rig::Rig;
pub use transport::Transport;
pub use types::*;
