//! Mock TCP server for protocol-level testing.
//!
//! [`MockTcpServer`] provides a lightweight TCP listener pre-loaded with
//! scripted responses, enabling deterministic testing of protocol engines
//! (e.g., Elecraft K4 over Ethernet, FlexRadio SmartSDR) without real
//! hardware or network infrastructure.
//!
//! # Example
//!
//! ```
//! use riglib_test_harness::MockTcpServer;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! let mut server = MockTcpServer::new().await?;
//!
//! // When the client sends "FA;", respond with "FA00014060000;"
//! server.expect(b"FA;", b"FA00014060000;");
//!
//! // Get the address to connect a TcpTransport to
//! let addr = server.addr();
//! // ... connect and test ...
//! # Ok(())
//! # }
//! ```

use riglib_core::error::{Error, Result};
use std::collections::VecDeque;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// A pre-loaded request/response pair for the mock TCP server.
#[derive(Debug, Clone)]
struct TcpExpectation {
    /// The exact bytes we expect the client to send.
    request: Vec<u8>,
    /// The bytes to send back when the matching request is received.
    response: Vec<u8>,
}

/// A mock TCP server for testing protocol engines over the network.
///
/// The server listens on a random available port on localhost. Once
/// [`start`](MockTcpServer::start) is called, it accepts a single
/// connection and processes expectations in order: for each expected
/// request, it reads from the client and writes back the corresponding
/// response.
///
/// If the client sends data that does not match the next expectation,
/// the server logs the mismatch and closes the connection.
pub struct MockTcpServer {
    /// The address the server is listening on (e.g., "127.0.0.1:54321").
    addr: String,
    /// Ordered queue of expected request/response pairs.
    expectations: VecDeque<TcpExpectation>,
    /// Handle to the server task once started.
    server_handle: Option<JoinHandle<std::result::Result<(), String>>>,
}

impl MockTcpServer {
    /// Create a new mock TCP server listening on a random port.
    ///
    /// The server does not accept connections until [`start`](MockTcpServer::start)
    /// is called, allowing expectations to be loaded first.
    pub async fn new() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| Error::Transport(format!("failed to bind mock TCP server: {}", e)))?;
        let addr = listener.local_addr().map_err(Error::Io)?.to_string();

        // We need to keep the listener alive but transfer it to the server
        // task later. Store it via a oneshot channel.
        // Actually, we'll re-bind in start(). For now, drop and record the port.
        // Better approach: hold the listener.
        // We'll store the listener in an Option and move it into the task on start().
        Ok(Self {
            addr,
            expectations: VecDeque::new(),
            server_handle: None,
        })
    }

    /// Add an expected request/response pair.
    ///
    /// Expectations are consumed in order. When the connected client sends
    /// bytes matching `request`, the server replies with `response`.
    pub fn expect(&mut self, request: &[u8], response: &[u8]) {
        self.expectations.push_back(TcpExpectation {
            request: request.to_vec(),
            response: response.to_vec(),
        });
    }

    /// Get the address the server is listening on.
    ///
    /// Use this to connect a `TcpTransport` to the mock server.
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Start the server, accepting a single client connection and processing
    /// all expectations.
    ///
    /// This spawns a background task. Call [`wait`](MockTcpServer::wait) to
    /// block until all expectations have been processed and check for errors.
    pub fn start(&mut self) {
        let addr = self.addr.clone();
        let expectations: Vec<TcpExpectation> = self.expectations.drain(..).collect();

        let handle = tokio::spawn(async move {
            let listener = TcpListener::bind(&addr)
                .await
                .map_err(|e| format!("failed to re-bind mock TCP server on {}: {}", addr, e))?;

            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("failed to accept connection: {}", e))?;

            for (i, expectation) in expectations.iter().enumerate() {
                let mut buf = vec![0u8; expectation.request.len()];
                let mut total_read = 0;

                // Read exactly the expected number of bytes
                while total_read < expectation.request.len() {
                    let n = stream
                        .read(&mut buf[total_read..])
                        .await
                        .map_err(|e| format!("expectation {}: read error: {}", i, e))?;
                    if n == 0 {
                        return Err(format!(
                            "expectation {}: client disconnected after {} bytes (expected {})",
                            i,
                            total_read,
                            expectation.request.len()
                        ));
                    }
                    total_read += n;
                }

                if buf != expectation.request {
                    return Err(format!(
                        "expectation {}: request mismatch: expected {:02X?}, got {:02X?}",
                        i, expectation.request, buf
                    ));
                }

                stream
                    .write_all(&expectation.response)
                    .await
                    .map_err(|e| format!("expectation {}: write error: {}", i, e))?;

                stream
                    .flush()
                    .await
                    .map_err(|e| format!("expectation {}: flush error: {}", i, e))?;
            }

            Ok(())
        });

        self.server_handle = Some(handle);
    }

    /// Start the server and return a channel that signals when the listener
    /// is ready to accept connections.
    ///
    /// This avoids race conditions where the client tries to connect before
    /// the server has re-bound to the port.
    pub fn start_with_ready(&mut self) -> oneshot::Receiver<()> {
        let addr = self.addr.clone();
        let expectations: Vec<TcpExpectation> = self.expectations.drain(..).collect();
        let (ready_tx, ready_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            let listener = TcpListener::bind(&addr)
                .await
                .map_err(|e| format!("failed to re-bind mock TCP server on {}: {}", addr, e))?;

            // Signal that the listener is ready
            let _ = ready_tx.send(());

            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("failed to accept connection: {}", e))?;

            for (i, expectation) in expectations.iter().enumerate() {
                let mut buf = vec![0u8; expectation.request.len()];
                let mut total_read = 0;

                while total_read < expectation.request.len() {
                    let n = stream
                        .read(&mut buf[total_read..])
                        .await
                        .map_err(|e| format!("expectation {}: read error: {}", i, e))?;
                    if n == 0 {
                        return Err(format!(
                            "expectation {}: client disconnected after {} bytes (expected {})",
                            i,
                            total_read,
                            expectation.request.len()
                        ));
                    }
                    total_read += n;
                }

                if buf != expectation.request {
                    return Err(format!(
                        "expectation {}: request mismatch: expected {:02X?}, got {:02X?}",
                        i, expectation.request, buf
                    ));
                }

                stream
                    .write_all(&expectation.response)
                    .await
                    .map_err(|e| format!("expectation {}: write error: {}", i, e))?;

                stream
                    .flush()
                    .await
                    .map_err(|e| format!("expectation {}: flush error: {}", i, e))?;
            }

            Ok(())
        });

        self.server_handle = Some(handle);
        ready_rx
    }

    /// Wait for the server task to complete and return any errors.
    ///
    /// Call this after the client has finished its interactions to verify
    /// that all expectations were met.
    pub async fn wait(self) -> std::result::Result<(), String> {
        if let Some(handle) = self.server_handle {
            handle
                .await
                .map_err(|e| format!("server task panicked: {}", e))?
        } else {
            Ok(())
        }
    }
}
