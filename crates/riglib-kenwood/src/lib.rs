//! Kenwood CAT protocol backend for riglib.
//!
//! This crate implements the Kenwood CAT (Computer Aided Transceiver) text-based
//! protocol used by modern Kenwood HF transceivers. It provides:
//!
//! - **Protocol codec** ([`protocol`]) -- encode and decode semicolon-terminated
//!   CAT commands and responses, with prefix/data splitting and error detection.
//! - **Command builders** ([`commands`]) -- construct correctly-formatted CAT
//!   commands for common operations (frequency, mode, PTT, metering, split,
//!   power) and parse the corresponding responses.
//! - **Model definitions** ([`models`]) -- static capability data for supported
//!   Kenwood rigs (TS-590S, TS-590SG, TS-990S, TS-890S).
//! - **Rig driver** ([`rig`]) -- full [`Rig`](riglib_core::Rig) trait
//!   implementation with transport abstraction, retry logic, and event emission.
//! - **Builder** ([`builder`]) -- fluent builder API for constructing
//!   [`KenwoodRig`] instances with smart defaults.
//!
//! # Kenwood vs Yaesu protocol differences
//!
//! Kenwood and Yaesu both use semicolon-terminated text commands, but:
//! - Kenwood uses 11-digit frequency fields (vs Yaesu's 9-digit)
//! - Kenwood mode codes: 1=LSB, 2=USB, 3=CW, 4=FM, 5=AM, 6=FSK, 7=CW-R, 9=FSK-R
//! - Split is controlled via independent `FR`/`FT` commands (RX/TX VFO select)
//! - AI (Auto Information) mode uses `AI2;` to enable, pushes unsolicited state
//!
//! # Example
//!
//! ```
//! use riglib_kenwood::protocol::{encode_command, decode_response, DecodeResult};
//! use riglib_kenwood::commands::{cmd_read_frequency_a, parse_frequency_response};
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
pub mod transceive;

// Re-export the primary types for ergonomic `use riglib_kenwood::*`.
pub use builder::KenwoodBuilder;
pub use models::KenwoodModel;
pub use rig::KenwoodRig;
