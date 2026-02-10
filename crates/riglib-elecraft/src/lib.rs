//! Elecraft extended Kenwood protocol backend for riglib.
//!
//! This crate implements the Elecraft CAT (Computer Aided Transceiver) protocol
//! used by Elecraft K3, K3S, KX3, KX2, and K4 transceivers. It provides:
//!
//! - **Protocol codec** ([`protocol`]) -- encode and decode semicolon-terminated
//!   CAT commands and responses, with prefix/data splitting and error detection.
//! - **Command builders** ([`commands`]) -- construct correctly-formatted CAT
//!   commands for common operations (frequency, mode, PTT, metering, split,
//!   power, bandwidth) and parse the corresponding responses.
//! - **Model definitions** ([`models`]) -- static capability data for supported
//!   Elecraft rigs (K3, K3S, KX3, KX2, K4).
//! - **Rig driver** ([`rig`]) -- full [`Rig`](riglib_core::Rig) trait
//!   implementation with transport abstraction, retry logic, and event emission.
//! - **Builder** ([`builder`]) -- fluent builder API for constructing
//!   [`ElecraftRig`] instances with smart defaults.
//!
//! # Elecraft vs standard Kenwood protocol differences
//!
//! Elecraft radios use an extended Kenwood text protocol:
//! - Same 11-digit frequency fields and semicolon terminator
//! - Mode 6 = DATA (digital modes), Mode 9 = DATA-R (reverse digital)
//! - `BW` command for bandwidth on K4, `FW` command on K3-family
//! - `K3;` / `K4;` for model identification
//! - AI (Auto Information) mode uses `AI2;` to enable
//!
//! # Example
//!
//! ```
//! use riglib_elecraft::protocol::{encode_command, decode_response, DecodeResult};
//! use riglib_elecraft::commands::{cmd_read_frequency_a, parse_frequency_response};
//!
//! // Build a "read VFO-A frequency" command
//! let cmd = cmd_read_frequency_a();
//! assert_eq!(cmd, b"FA;");
//!
//! // Simulate receiving a frequency response from the rig
//! let response = b"FA00014250000;";
//! if let DecodeResult::Response { prefix, data, .. } = decode_response(response) {
//!     assert_eq!(prefix, "FA");
//!     let freq = parse_frequency_response(&data).unwrap();
//!     assert_eq!(freq, 14_250_000);
//! }
//! ```

pub mod builder;
pub mod commands;
pub mod models;
pub mod protocol;
pub mod rig;

// Re-export the primary types for ergonomic `use riglib_elecraft::*`.
pub use builder::ElecraftBuilder;
pub use models::ElecraftModel;
pub use rig::ElecraftRig;
