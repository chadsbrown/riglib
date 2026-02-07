//! Transport trait for rig communication.
//!
//! The [`Transport`] trait abstracts over the physical link to a transceiver.
//! Implementations exist for serial ports (CI-V, CAT), TCP sockets (FlexRadio
//! SmartSDR), and mock transports for testing.
//!
//! Protocol engines (e.g. the CI-V codec in `riglib-icom`) operate on a
//! `Transport` rather than directly on a serial port, enabling both real
//! hardware control and deterministic unit testing with `MockTransport`
//! from the `riglib-test-harness` crate.

use async_trait::async_trait;
use std::time::Duration;

use crate::error::Result;

/// Asynchronous byte-level transport to a rig.
///
/// Implementations handle framing, buffering, and error recovery at the
/// physical layer. Protocol-level concerns (CI-V addressing, CAT command
/// structure) are handled by the protocol engines that consume this trait.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send raw bytes to the rig.
    ///
    /// Implementations should block until all bytes have been written to
    /// the underlying transport (serial TX buffer, TCP socket, etc.).
    async fn send(&mut self, data: &[u8]) -> Result<()>;

    /// Receive bytes from the rig into the provided buffer.
    ///
    /// Returns the number of bytes actually read. Will wait up to `timeout`
    /// for data to arrive; returns [`Error::Timeout`](crate::error::Error::Timeout)
    /// if no data is received within the deadline.
    async fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize>;

    /// Close the transport connection.
    ///
    /// After calling `close()`, subsequent `send()` and `receive()` calls
    /// should return [`Error::NotConnected`](crate::error::Error::NotConnected).
    async fn close(&mut self) -> Result<()>;

    /// Check whether the transport is currently connected.
    fn is_connected(&self) -> bool;

    /// Assert or de-assert the DTR (Data Terminal Ready) serial line.
    ///
    /// Used for hardware PTT or CW keying on serial transports.
    /// Returns [`Error::Unsupported`](crate::error::Error::Unsupported) on
    /// transports that don't have serial control lines (TCP, UDP, mock).
    async fn set_dtr(&mut self, _on: bool) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "DTR control not supported by this transport".into(),
        ))
    }

    /// Assert or de-assert the RTS (Request To Send) serial line.
    ///
    /// Used for hardware PTT or CW keying on serial transports.
    /// Returns [`Error::Unsupported`](crate::error::Error::Unsupported) on
    /// transports that don't have serial control lines (TCP, UDP, mock).
    async fn set_rts(&mut self, _on: bool) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "RTS control not supported by this transport".into(),
        ))
    }
}
