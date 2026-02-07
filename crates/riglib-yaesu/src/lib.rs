//! Yaesu CAT protocol backend for riglib.
//!
//! This crate implements the Yaesu CAT (Computer Aided Transceiver) text-based
//! protocol used by modern Yaesu HF transceivers. It provides:
//!
//! - **Protocol codec** ([`protocol`]) -- encode and decode semicolon-terminated
//!   CAT commands and responses, with prefix/data splitting and error detection.
//! - **Command builders** ([`commands`]) -- construct correctly-formatted CAT
//!   commands for common operations (frequency, mode, PTT, metering, split,
//!   power) and parse the corresponding responses.
//! - **Model definitions** ([`models`]) -- static capability data for supported
//!   Yaesu rigs (FT-DX10, FT-891, FT-991A, FT-DX101D, FT-DX101MP, FT-710).
//! - **Rig implementation** ([`rig`]) -- the [`Rig`](riglib_core::Rig) trait
//!   implementation for Yaesu transceivers ([`YaesuRig`]).
//! - **Builder** ([`builder`]) -- fluent builder for constructing [`YaesuRig`]
//!   instances with configurable transport and retry policies ([`YaesuBuilder`]).
//!
//! # Example
//!
//! ```
//! use riglib_yaesu::protocol::{encode_command, decode_response, DecodeResult};
//! use riglib_yaesu::commands::{cmd_read_frequency_a, parse_frequency_response};
//!
//! // Build a "read VFO-A frequency" command
//! let cmd = cmd_read_frequency_a();
//! assert_eq!(cmd, b"FA;");
//!
//! // Simulate receiving a frequency response from the rig
//! let response = b"FA014250000;";
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

pub use builder::YaesuBuilder;
pub use rig::YaesuRig;
