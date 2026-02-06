//! TCP transport for rig communication.
//!
//! This module provides [`TcpTransport`], which implements the [`Transport`]
//! trait for network-connected transceivers that accept TCP connections.
//!
//! Notable rigs with TCP/IP interfaces:
//! - FlexRadio SmartSDR (command and status streams on TCP)
//! - Elecraft K4 (direct Ethernet connectivity with Kenwood-style CAT)
//! - Remote rig servers (e.g., rigctld, flrig)
//!
//! # Example
//!
//! ```no_run
//! use riglib_transport::TcpTransport;
//! use riglib_core::transport::Transport;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! // Connect to a FlexRadio SmartSDR command port
//! let mut transport = TcpTransport::connect("192.168.1.100:4992").await?;
//!
//! // Send a command
//! transport.send(b"sub slice all\n").await?;
//!
//! // Receive response with 2 second timeout
//! let mut buf = [0u8; 4096];
//! let n = transport.receive(&mut buf, Duration::from_secs(2)).await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use riglib_core::error::{Error, Result};
use riglib_core::transport::Transport;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Default connection timeout (5 seconds).
///
/// This is generous enough for LAN connections and most internet links,
/// but short enough to avoid hanging during contest operation if a rig
/// is unreachable.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// TCP transport for rig communication.
///
/// Implements the [`Transport`] trait for network-connected transceivers.
/// The connection is established eagerly via [`connect`](TcpTransport::connect)
/// or [`connect_with_timeout`](TcpTransport::connect_with_timeout).
#[derive(Debug)]
pub struct TcpTransport {
    /// The underlying TCP stream, `None` after `close()` is called.
    stream: Option<TcpStream>,
    /// The address string for logging/debugging.
    addr: String,
}

impl TcpTransport {
    /// Connect to a TCP endpoint using the default timeout.
    ///
    /// The `addr` parameter should be a `host:port` string, e.g.,
    /// `"192.168.1.100:4992"` or `"localhost:4532"`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use riglib_transport::TcpTransport;
    /// # async fn example() -> riglib_core::Result<()> {
    /// let transport = TcpTransport::connect("192.168.1.100:4992").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(addr: &str) -> Result<Self> {
        Self::connect_with_timeout(addr, DEFAULT_CONNECT_TIMEOUT).await
    }

    /// Connect to a TCP endpoint with a specified timeout.
    ///
    /// # Arguments
    ///
    /// * `addr` - A `host:port` string (e.g., `"192.168.1.100:4992"`)
    /// * `timeout` - Maximum time to wait for the connection to be established
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use riglib_transport::TcpTransport;
    /// # use std::time::Duration;
    /// # async fn example() -> riglib_core::Result<()> {
    /// let transport = TcpTransport::connect_with_timeout(
    ///     "192.168.1.100:4992",
    ///     Duration::from_secs(10),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect_with_timeout(addr: &str, timeout: Duration) -> Result<Self> {
        tracing::debug!(
            addr = %addr,
            timeout_ms = timeout.as_millis(),
            "Connecting to TCP endpoint"
        );

        let stream = tokio::time::timeout(timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| {
                tracing::error!(addr = %addr, "TCP connection timed out");
                Error::Timeout
            })?
            .map_err(|e| {
                tracing::error!(addr = %addr, error = %e, "TCP connection failed");
                map_connect_error(e, addr)
            })?;

        // Disable Nagle's algorithm for low-latency rig control.
        // Rig commands are typically small and latency-sensitive.
        if let Err(e) = stream.set_nodelay(true) {
            tracing::warn!(
                addr = %addr,
                error = %e,
                "Failed to set TCP_NODELAY (continuing anyway)"
            );
        }

        tracing::info!(addr = %addr, "TCP connection established");

        Ok(Self {
            stream: Some(stream),
            addr: addr.to_string(),
        })
    }

    /// Wrap an existing `TcpStream` as a `TcpTransport`.
    ///
    /// This is useful when a TCP connection has already been established
    /// externally (e.g., accepted from a listener in tests).
    ///
    /// # Arguments
    ///
    /// * `stream` - An already-connected `TcpStream`
    /// * `addr` - A label for logging (typically the peer address)
    pub fn from_stream(stream: TcpStream, addr: String) -> Self {
        tracing::debug!(addr = %addr, "Wrapping existing TCP stream");
        Self {
            stream: Some(stream),
            addr,
        }
    }

    /// Get the address string this transport was connected to.
    pub fn addr(&self) -> &str {
        &self.addr
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        let stream = self.stream.as_mut().ok_or(Error::NotConnected)?;

        tracing::trace!(
            addr = %self.addr,
            bytes = data.len(),
            data = ?data,
            "Sending data"
        );

        stream.write_all(data).await.map_err(|e| {
            tracing::error!(
                addr = %self.addr,
                error = %e,
                "Failed to send data"
            );
            map_io_error(e)
        })?;

        // Flush to ensure data is transmitted immediately
        stream.flush().await.map_err(|e| {
            tracing::error!(
                addr = %self.addr,
                error = %e,
                "Failed to flush TCP stream"
            );
            map_io_error(e)
        })?;

        tracing::trace!(addr = %self.addr, "Data sent successfully");

        Ok(())
    }

    async fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        let stream = self.stream.as_mut().ok_or(Error::NotConnected)?;

        tracing::trace!(
            addr = %self.addr,
            buf_len = buf.len(),
            timeout_ms = timeout.as_millis(),
            "Waiting for data"
        );

        let result = tokio::time::timeout(timeout, stream.read(buf)).await;

        match result {
            Ok(Ok(0)) => {
                // TCP: 0 bytes read means the peer has closed the connection.
                tracing::warn!(addr = %self.addr, "Peer closed connection (0 bytes read)");
                Err(Error::ConnectionLost)
            }
            Ok(Ok(n)) => {
                tracing::trace!(
                    addr = %self.addr,
                    bytes = n,
                    data = ?&buf[..n],
                    "Received data"
                );
                Ok(n)
            }
            Ok(Err(e)) => {
                tracing::error!(
                    addr = %self.addr,
                    error = %e,
                    "Failed to receive data"
                );
                Err(map_io_error(e))
            }
            Err(_) => {
                tracing::trace!(
                    addr = %self.addr,
                    timeout_ms = timeout.as_millis(),
                    "Timeout waiting for data"
                );
                Err(Error::Timeout)
            }
        }
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            tracing::debug!(addr = %self.addr, "Closing TCP connection");

            // Flush any pending data before shutdown
            if let Err(e) = stream.flush().await {
                tracing::warn!(
                    addr = %self.addr,
                    error = %e,
                    "Failed to flush before closing (continuing anyway)"
                );
            }

            // Gracefully shut down the TCP connection
            if let Err(e) = stream.shutdown().await {
                tracing::warn!(
                    addr = %self.addr,
                    error = %e,
                    "Failed to shutdown TCP stream (continuing anyway)"
                );
            }

            tracing::info!(addr = %self.addr, "TCP connection closed");
        }

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
}

// Implement Drop to log when the transport is dropped while still connected
impl Drop for TcpTransport {
    fn drop(&mut self) {
        if self.stream.is_some() {
            tracing::debug!(addr = %self.addr, "TcpTransport dropped, closing connection");
            // The stream will be automatically closed when dropped
        }
    }
}

/// Map a connection-time I/O error to the appropriate [`Error`] variant.
fn map_connect_error(e: std::io::Error, addr: &str) -> Error {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => {
            Error::Transport(format!("connection refused: {}", addr))
        }
        _ => Error::Io(e),
    }
}

/// Map a data-path I/O error to the appropriate [`Error`] variant.
fn map_io_error(e: std::io::Error) -> Error {
    match e.kind() {
        std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::ConnectionAborted => Error::ConnectionLost,
        _ => Error::Io(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_core::transport::Transport;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Helper: bind a TcpListener on a random available port and return it
    /// along with its address string.
    async fn test_listener() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        (listener, addr)
    }

    #[tokio::test]
    async fn connect_send_receive() {
        let (listener, addr) = test_listener().await;

        // Spawn a server that echoes data back
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 256];
            let n = stream.read(&mut buf).await.unwrap();
            stream.write_all(&buf[..n]).await.unwrap();
            stream.flush().await.unwrap();
        });

        let mut transport = TcpTransport::connect(&addr).await.unwrap();
        assert!(transport.is_connected());

        // Send data
        let data = b"Hello, rig!";
        transport.send(data).await.unwrap();

        // Receive echoed response
        let mut buf = [0u8; 256];
        let n = transport
            .receive(&mut buf, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(&buf[..n], data);

        transport.close().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn connect_timeout_to_nonexistent_host() {
        // RFC 5737: 192.0.2.0/24 is TEST-NET-1, reserved for documentation.
        // Connections to it should time out (packets are black-holed, not refused).
        let result =
            TcpTransport::connect_with_timeout("192.0.2.1:12345", Duration::from_millis(100)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        // Depending on the network stack, this may be Timeout or an Io error.
        // On most systems, connecting to TEST-NET will time out.
        assert!(
            matches!(err, Error::Timeout | Error::Io(_)),
            "expected Timeout or Io, got: {:?}",
            err
        );
    }

    #[tokio::test]
    async fn connect_refused() {
        // Bind a listener and immediately drop it so the port is not listening
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        drop(listener);

        let result = TcpTransport::connect(&addr).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            Error::Transport(msg) => assert!(
                msg.contains("connection refused"),
                "expected 'connection refused' in message, got: {}",
                msg
            ),
            other => panic!("expected Transport error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn receive_timeout() {
        let (listener, addr) = test_listener().await;

        // Server accepts but sends nothing
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            // Hold the connection open but send nothing
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let mut transport = TcpTransport::connect(&addr).await.unwrap();

        let mut buf = [0u8; 256];
        let result = transport
            .receive(&mut buf, Duration::from_millis(100))
            .await;
        assert!(matches!(result, Err(Error::Timeout)));

        transport.close().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn disconnection_detection() {
        let (listener, addr) = test_listener().await;

        // Server accepts then immediately closes the connection
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            drop(stream);
        });

        let mut transport = TcpTransport::connect(&addr).await.unwrap();

        // Wait for the server to close its end
        server.await.unwrap();

        // Give the OS a moment to propagate the FIN
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Receive should detect the closed connection
        let mut buf = [0u8; 256];
        let result = transport.receive(&mut buf, Duration::from_secs(2)).await;
        assert!(
            matches!(result, Err(Error::ConnectionLost)),
            "expected ConnectionLost, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn send_after_close_returns_not_connected() {
        let (listener, addr) = test_listener().await;

        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let mut transport = TcpTransport::connect(&addr).await.unwrap();
        transport.close().await.unwrap();

        let result = transport.send(b"should fail").await;
        assert!(matches!(result, Err(Error::NotConnected)));

        server.abort();
    }

    #[tokio::test]
    async fn receive_after_close_returns_not_connected() {
        let (listener, addr) = test_listener().await;

        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let mut transport = TcpTransport::connect(&addr).await.unwrap();
        transport.close().await.unwrap();

        let mut buf = [0u8; 256];
        let result = transport.receive(&mut buf, Duration::from_secs(1)).await;
        assert!(matches!(result, Err(Error::NotConnected)));

        server.abort();
    }

    #[tokio::test]
    async fn is_connected_state_transitions() {
        let (listener, addr) = test_listener().await;

        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let mut transport = TcpTransport::connect(&addr).await.unwrap();
        assert!(transport.is_connected());

        transport.close().await.unwrap();
        assert!(!transport.is_connected());

        // Closing again is a no-op, should not error
        transport.close().await.unwrap();
        assert!(!transport.is_connected());

        server.abort();
    }

    #[tokio::test]
    async fn addr_accessor() {
        let (listener, addr) = test_listener().await;

        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let mut transport = TcpTransport::connect(&addr).await.unwrap();
        assert_eq!(transport.addr(), addr);

        transport.close().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn from_stream_works() {
        let (listener, _addr) = test_listener().await;
        let listener_addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            let n = stream.read(&mut buf).await.unwrap();
            stream.write_all(&buf[..n]).await.unwrap();
            stream.flush().await.unwrap();
        });

        let raw_stream = TcpStream::connect(listener_addr).await.unwrap();
        let mut transport = TcpTransport::from_stream(raw_stream, listener_addr.to_string());
        assert!(transport.is_connected());

        transport.send(b"test").await.unwrap();

        let mut buf = [0u8; 64];
        let n = transport
            .receive(&mut buf, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(&buf[..n], b"test");

        transport.close().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn multiple_send_receive_cycles() {
        let (listener, addr) = test_listener().await;

        // Server echoes each message back
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 256];
            for _ in 0..3 {
                let n = stream.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                stream.write_all(&buf[..n]).await.unwrap();
                stream.flush().await.unwrap();
            }
        });

        let mut transport = TcpTransport::connect(&addr).await.unwrap();

        for msg in &[b"first" as &[u8], b"second", b"third"] {
            transport.send(msg).await.unwrap();
            let mut buf = [0u8; 256];
            let n = transport
                .receive(&mut buf, Duration::from_secs(2))
                .await
                .unwrap();
            assert_eq!(&buf[..n], *msg);
        }

        transport.close().await.unwrap();
        server.await.unwrap();
    }
}
