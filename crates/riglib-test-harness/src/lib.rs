//! riglib-test-harness: Test utilities, mock transports, and protocol
//! recorders for riglib.
//!
//! This crate provides [`MockTransport`] for deterministic unit testing of
//! protocol engines without requiring real radio hardware, and
//! [`MockTcpServer`] for testing protocol engines that communicate over TCP.

pub mod mock_serial;
pub mod mock_tcp;

pub use mock_serial::MockTransport;
pub use mock_tcp::MockTcpServer;
