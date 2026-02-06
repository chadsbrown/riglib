//! UDP transport for rig communication.
//!
//! This module provides [`UdpTransport`], a datagram-oriented transport for
//! use cases where UDP is the appropriate protocol. Unlike [`super::TcpTransport`],
//! this does **not** implement the [`Transport`](riglib_core::Transport) trait because UDP is
//! connectionless and datagram-based rather than stream-oriented.
//!
//! Notable use cases in amateur radio:
//! - FlexRadio SmartSDR discovery (broadcast UDP on port 4992)
//! - VITA-49 I/Q data streams from FlexRadio
//! - Custom UDP-based rig control protocols
//!
//! # Example
//!
//! ```no_run
//! use riglib_transport::UdpTransport;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! // Bind to any available port for FlexRadio discovery
//! let transport = UdpTransport::bind("0.0.0.0:0").await?;
//! transport.set_broadcast(true)?;
//!
//! // Send a discovery packet
//! let discovery_msg = b"discovery";
//! let broadcast_addr = "255.255.255.255:4992".parse().unwrap();
//! transport.send_to(discovery_msg, broadcast_addr).await?;
//!
//! // Wait for a response
//! let mut buf = [0u8; 4096];
//! let (n, src) = transport.recv_from(&mut buf, Duration::from_secs(5)).await?;
//! println!("Received {} bytes from {}", n, src);
//! # Ok(())
//! # }
//! ```

use riglib_core::error::{Error, Result};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;

/// UDP transport for datagram-based rig communication.
///
/// Wraps a [`tokio::net::UdpSocket`] with error mapping consistent with the
/// rest of the riglib transport layer. This is used for broadcast discovery
/// (FlexRadio SmartSDR) and VITA-49 I/Q data streams.
#[derive(Debug)]
pub struct UdpTransport {
    /// The underlying UDP socket.
    socket: UdpSocket,
    /// The local address the socket is bound to.
    local_addr: SocketAddr,
}

impl UdpTransport {
    /// Bind to a local address.
    ///
    /// Use `"0.0.0.0:0"` to bind to any available port on all interfaces,
    /// or specify a port like `"0.0.0.0:4992"` for a well-known service.
    ///
    /// # Arguments
    ///
    /// * `addr` - A `host:port` string (e.g., `"0.0.0.0:0"` or `"192.168.1.50:4992"`)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use riglib_transport::UdpTransport;
    /// # async fn example() -> riglib_core::Result<()> {
    /// let transport = UdpTransport::bind("0.0.0.0:0").await?;
    /// println!("Bound to {}", transport.local_addr());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn bind(addr: &str) -> Result<Self> {
        tracing::debug!(addr = %addr, "Binding UDP socket");

        let socket = UdpSocket::bind(addr).await.map_err(|e| {
            tracing::error!(addr = %addr, error = %e, "Failed to bind UDP socket");
            Error::Io(e)
        })?;

        let local_addr = socket.local_addr().map_err(|e| {
            tracing::error!(error = %e, "Failed to get local address");
            Error::Io(e)
        })?;

        tracing::debug!(local_addr = %local_addr, "UDP socket bound");

        Ok(Self { socket, local_addr })
    }

    /// Bind to a specific port on all interfaces.
    ///
    /// Convenience method equivalent to `bind(&format!("0.0.0.0:{port}"))`.
    ///
    /// # Arguments
    ///
    /// * `port` - The port number to bind to (0 for any available port)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use riglib_transport::UdpTransport;
    /// # async fn example() -> riglib_core::Result<()> {
    /// let transport = UdpTransport::bind_port(0).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn bind_port(port: u16) -> Result<Self> {
        Self::bind(&format!("0.0.0.0:{}", port)).await
    }

    /// Get the local address this socket is bound to.
    ///
    /// This is useful when binding to port 0 to discover the assigned port.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Send a datagram to the specified address.
    ///
    /// The entire `data` slice is sent as a single datagram. UDP does not
    /// guarantee delivery or ordering, but each datagram is atomic -- it
    /// either arrives in full or not at all.
    ///
    /// # Arguments
    ///
    /// * `data` - The bytes to send
    /// * `addr` - The destination socket address
    pub async fn send_to(&self, data: &[u8], addr: SocketAddr) -> Result<()> {
        tracing::trace!(
            local = %self.local_addr,
            remote = %addr,
            bytes = data.len(),
            "Sending datagram"
        );

        self.socket.send_to(data, addr).await.map_err(|e| {
            tracing::error!(
                local = %self.local_addr,
                remote = %addr,
                error = %e,
                "Failed to send datagram"
            );
            Error::Io(e)
        })?;

        tracing::trace!(
            local = %self.local_addr,
            remote = %addr,
            bytes = data.len(),
            "Datagram sent"
        );

        Ok(())
    }

    /// Receive a datagram with timeout. Returns `(bytes_read, source_addr)`.
    ///
    /// The buffer should be large enough to hold an entire datagram. Any
    /// bytes beyond `buf.len()` are silently discarded (standard UDP
    /// behavior). For VITA-49 frames, 8192 bytes is typically sufficient.
    ///
    /// # Arguments
    ///
    /// * `buf` - Buffer to receive the datagram into
    /// * `timeout` - Maximum time to wait for a datagram
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] if no datagram arrives within `timeout`.
    pub async fn recv_from(
        &self,
        buf: &mut [u8],
        timeout: Duration,
    ) -> Result<(usize, SocketAddr)> {
        tracing::trace!(
            local = %self.local_addr,
            buf_len = buf.len(),
            timeout_ms = timeout.as_millis(),
            "Waiting for datagram"
        );

        let result = tokio::time::timeout(timeout, self.socket.recv_from(buf)).await;

        match result {
            Ok(Ok((n, src))) => {
                tracing::trace!(
                    local = %self.local_addr,
                    remote = %src,
                    bytes = n,
                    "Received datagram"
                );
                Ok((n, src))
            }
            Ok(Err(e)) => {
                tracing::error!(
                    local = %self.local_addr,
                    error = %e,
                    "Failed to receive datagram"
                );
                Err(Error::Io(e))
            }
            Err(_) => {
                tracing::trace!(
                    local = %self.local_addr,
                    timeout_ms = timeout.as_millis(),
                    "Timeout waiting for datagram"
                );
                Err(Error::Timeout)
            }
        }
    }

    /// Receive a datagram with timeout, ignoring the source address.
    ///
    /// Returns the number of bytes read. This is a convenience wrapper
    /// around [`recv_from`](Self::recv_from) for cases where the source
    /// address is not needed (e.g., after [`connect`](Self::connect)).
    ///
    /// # Arguments
    ///
    /// * `buf` - Buffer to receive the datagram into
    /// * `timeout` - Maximum time to wait for a datagram
    pub async fn recv(&self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        let (n, _src) = self.recv_from(buf, timeout).await?;
        Ok(n)
    }

    /// Enable or disable broadcast on this socket.
    ///
    /// This must be enabled before sending to broadcast addresses
    /// (e.g., `255.255.255.255`). Required for FlexRadio SmartSDR
    /// discovery.
    ///
    /// # Arguments
    ///
    /// * `enable` - `true` to enable broadcast, `false` to disable
    pub fn set_broadcast(&self, enable: bool) -> Result<()> {
        tracing::debug!(
            local = %self.local_addr,
            enable = enable,
            "Setting broadcast"
        );

        self.socket.set_broadcast(enable).map_err(|e| {
            tracing::error!(
                local = %self.local_addr,
                error = %e,
                "Failed to set broadcast"
            );
            Error::Io(e)
        })
    }

    /// Connect this socket to a specific remote address.
    ///
    /// After connecting, datagrams can only be sent to and received from
    /// the specified address. This is useful for VITA-49 data streams
    /// where the radio's address is known and we want to filter out
    /// packets from other sources.
    ///
    /// Note: UDP "connect" does not perform a handshake. It merely sets
    /// a default destination and filters incoming datagrams.
    ///
    /// # Arguments
    ///
    /// * `addr` - The remote address to connect to
    pub async fn connect(&self, addr: SocketAddr) -> Result<()> {
        tracing::debug!(
            local = %self.local_addr,
            remote = %addr,
            "Connecting UDP socket to remote address"
        );

        self.socket.connect(addr).await.map_err(|e| {
            tracing::error!(
                local = %self.local_addr,
                remote = %addr,
                error = %e,
                "Failed to connect UDP socket"
            );
            Error::Io(e)
        })?;

        tracing::debug!(
            local = %self.local_addr,
            remote = %addr,
            "UDP socket connected"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bind_and_local_addr() {
        let transport = UdpTransport::bind("127.0.0.1:0").await.unwrap();
        let addr = transport.local_addr();

        assert_eq!(addr.ip(), std::net::Ipv4Addr::LOCALHOST);
        assert_ne!(addr.port(), 0, "OS should assign a nonzero port");
    }

    #[tokio::test]
    async fn send_recv_loopback() {
        let sender = UdpTransport::bind("127.0.0.1:0").await.unwrap();
        let receiver = UdpTransport::bind("127.0.0.1:0").await.unwrap();

        let data = b"CQ CQ CQ DE W1AW";
        sender.send_to(data, receiver.local_addr()).await.unwrap();

        let mut buf = [0u8; 256];
        let n = receiver
            .recv(&mut buf, Duration::from_secs(2))
            .await
            .unwrap();

        assert_eq!(&buf[..n], data);
    }

    #[tokio::test]
    async fn recv_timeout() {
        let transport = UdpTransport::bind("127.0.0.1:0").await.unwrap();

        let mut buf = [0u8; 256];
        let result = transport
            .recv_from(&mut buf, Duration::from_millis(50))
            .await;

        assert!(
            matches!(result, Err(Error::Timeout)),
            "expected Timeout, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn recv_from_returns_source() {
        let socket_a = UdpTransport::bind("127.0.0.1:0").await.unwrap();
        let socket_b = UdpTransport::bind("127.0.0.1:0").await.unwrap();

        let data = b"73 de W1AW";
        socket_a.send_to(data, socket_b.local_addr()).await.unwrap();

        let mut buf = [0u8; 256];
        let (n, src) = socket_b
            .recv_from(&mut buf, Duration::from_secs(2))
            .await
            .unwrap();

        assert_eq!(&buf[..n], data);
        assert_eq!(src, socket_a.local_addr(), "source should be socket A");
    }

    #[tokio::test]
    async fn broadcast() {
        // Verify that set_broadcast succeeds and the socket can send to
        // a broadcast address. Receiving broadcast datagrams on loopback
        // is not reliable across all OS/kernel configurations, so we use
        // a connected pair to verify the flag takes effect without errors.
        let sender = UdpTransport::bind("0.0.0.0:0").await.unwrap();
        sender.set_broadcast(true).unwrap();

        // Bind receiver to 0.0.0.0 so it can receive broadcast datagrams.
        let receiver = UdpTransport::bind("0.0.0.0:0").await.unwrap();
        let recv_port = receiver.local_addr().port();

        let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", recv_port).parse().unwrap();

        let data = b"SmartSDR discovery";
        // Sending to broadcast may fail in CI environments (no route to
        // 255.255.255.255, restricted networking, etc.). The important thing
        // is that set_broadcast(true) succeeded above.
        if sender.send_to(data, broadcast_addr).await.is_ok() {
            let mut buf = [0u8; 256];
            // On many CI/test environments, broadcast may not be received on
            // loopback. We accept either success or timeout as valid outcomes.
            match receiver.recv(&mut buf, Duration::from_millis(200)).await {
                Ok(n) => assert_eq!(&buf[..n], data),
                Err(Error::Timeout) => {
                    // Broadcast not delivered on this host -- still valid.
                }
                Err(e) => panic!("unexpected error: {:?}", e),
            }
        }
    }

    #[tokio::test]
    async fn multiple_datagrams() {
        let sender = UdpTransport::bind("127.0.0.1:0").await.unwrap();
        let receiver = UdpTransport::bind("127.0.0.1:0").await.unwrap();
        let dest = receiver.local_addr();

        let messages: &[&[u8]] = &[b"CQ CONTEST DE W1AW", b"5NN", b"TU 73"];

        for msg in messages {
            sender.send_to(msg, dest).await.unwrap();
        }

        // Small delay to let all datagrams arrive
        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut buf = [0u8; 256];
        for expected in messages {
            let n = receiver
                .recv(&mut buf, Duration::from_secs(2))
                .await
                .unwrap();
            assert_eq!(&buf[..n], *expected);
        }
    }

    #[tokio::test]
    async fn large_datagram() {
        let sender = UdpTransport::bind("127.0.0.1:0").await.unwrap();
        let receiver = UdpTransport::bind("127.0.0.1:0").await.unwrap();

        // 1500 bytes is a typical Ethernet MTU. On loopback, much larger
        // datagrams work, but 1500 is the practical limit for VITA-49
        // frames on a real network.
        let data: Vec<u8> = (0..1500).map(|i| (i % 256) as u8).collect();

        sender.send_to(&data, receiver.local_addr()).await.unwrap();

        let mut buf = [0u8; 2048];
        let n = receiver
            .recv(&mut buf, Duration::from_secs(2))
            .await
            .unwrap();

        assert_eq!(n, 1500);
        assert_eq!(&buf[..n], &data[..]);
    }

    #[tokio::test]
    async fn bind_port() {
        let transport = UdpTransport::bind_port(0).await.unwrap();
        assert_ne!(transport.local_addr().port(), 0);
    }

    #[tokio::test]
    async fn connect_filters_source() {
        let socket_a = UdpTransport::bind("127.0.0.1:0").await.unwrap();
        let socket_b = UdpTransport::bind("127.0.0.1:0").await.unwrap();
        let receiver = UdpTransport::bind("127.0.0.1:0").await.unwrap();

        // Connect receiver to socket_a only
        receiver.connect(socket_a.local_addr()).await.unwrap();

        // Send from socket_b first (should be filtered out by the OS)
        socket_b
            .send_to(b"from B", receiver.local_addr())
            .await
            .unwrap();

        // Send from socket_a (should be received)
        socket_a
            .send_to(b"from A", receiver.local_addr())
            .await
            .unwrap();

        // Small delay to let datagrams arrive
        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut buf = [0u8; 256];
        let n = receiver
            .recv(&mut buf, Duration::from_secs(2))
            .await
            .unwrap();

        assert_eq!(&buf[..n], b"from A");
    }
}
