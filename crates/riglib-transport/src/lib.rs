//! Transport implementations for riglib.
//!
//! This crate provides concrete implementations of the [`Transport`](riglib_core::Transport) trait
//! from `riglib-core` for various physical connection types:
//!
//! - [`SerialTransport`]: USB virtual COM ports and RS-232 serial connections
//! - [`TcpTransport`]: TCP connections for FlexRadio SmartSDR, Elecraft K4,
//!   and other network-based rigs
//! - [`UdpTransport`]: UDP datagrams for FlexRadio discovery and VITA-49
//!   I/Q data streams
//!
//! # Example
//!
//! ```no_run
//! use riglib_transport::SerialTransport;
//! use riglib_core::transport::Transport;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! // Connect to an Icom rig via CI-V
//! let mut transport = SerialTransport::open("/dev/ttyUSB0", 19200).await?;
//!
//! // Send a command
//! transport.send(&[0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD]).await?;
//!
//! // Receive response
//! let mut buf = [0u8; 256];
//! let n = transport.receive(&mut buf, Duration::from_secs(1)).await?;
//! # Ok(())
//! # }
//! ```

pub mod serial;
pub mod tcp;
pub mod udp;

#[cfg(feature = "audio")]
pub mod audio;

pub use serial::{DataBits, FlowControl, Parity, SerialConfig, SerialTransport, StopBits};
pub use tcp::TcpTransport;
pub use udp::UdpTransport;

#[cfg(feature = "audio")]
pub use audio::{AudioDeviceInfo, CpalAudioBackend};
