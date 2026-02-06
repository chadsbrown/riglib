//! Icom CI-V protocol backend for riglib.
//!
//! This crate implements the Icom CI-V (Communication Interface V) binary
//! protocol used by all modern Icom transceivers. It provides:
//!
//! - **Frame codec** ([`civ`]) -- encode and decode CI-V frames, BCD frequency
//!   conversion, and collision detection on the half-duplex CI-V bus.
//! - **Command builders** ([`commands`]) -- construct correctly-formatted CI-V
//!   commands for common operations (frequency, mode, PTT, metering, split,
//!   VFO selection) and parse the corresponding responses.
//! - **Model definitions** ([`models`]) -- static capability data for all
//!   supported Icom rigs (IC-7300 through IC-905, 14 models total).
//! - **IcomRig** ([`rig`]) -- the [`Rig`](riglib_core::Rig) trait implementation
//!   that ties the CI-V protocol engine to a [`Transport`](riglib_core::Transport).
//! - **IcomBuilder** ([`builder`]) -- fluent builder for constructing `IcomRig`
//!   instances with configurable retry, timeout, and CI-V address settings.
//!
//! # Example
//!
//! ```
//! use riglib_icom::civ::{encode_frame, decode_frame, DecodeResult, CONTROLLER_ADDR};
//! use riglib_icom::commands::cmd_read_frequency;
//!
//! // Build a "read frequency" command for IC-7610
//! let cmd = cmd_read_frequency(0x98);
//! assert_eq!(cmd, vec![0xFE, 0xFE, 0x98, 0xE0, 0x03, 0xFD]);
//!
//! // Simulate receiving an ACK from the rig
//! let response = vec![0xFE, 0xFE, 0xE0, 0x98, 0xFB, 0xFD];
//! if let DecodeResult::Frame(frame, _) = decode_frame(&response) {
//!     assert!(frame.is_ack());
//! }
//! ```

pub mod builder;
pub mod civ;
pub mod commands;
pub mod models;
pub mod rig;

pub use builder::IcomBuilder;
pub use rig::IcomRig;
