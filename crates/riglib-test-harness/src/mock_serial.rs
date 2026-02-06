//! Mock transport for deterministic testing of protocol engines.
//!
//! [`MockTransport`] implements the [`Transport`] trait with pre-loaded
//! request/response pairs. This lets you test CI-V frame encoding,
//! CAT command generation, and response parsing without real hardware.
//!
//! # Example
//!
//! ```
//! use riglib_test_harness::MockTransport;
//!
//! let mut mock = MockTransport::new();
//! // Pre-load: when the protocol engine sends this request, return this response.
//! mock.expect(&[0xFE, 0xFE, 0x98, 0xE0, 0x03, 0xFD],
//!             &[0xFE, 0xFE, 0xE0, 0x98, 0x03, 0x00, 0x00, 0x60, 0x14, 0x00, 0xFD]);
//! ```

use async_trait::async_trait;
use std::collections::VecDeque;
use std::time::Duration;

use riglib_core::error::{Error, Result};
use riglib_core::transport::Transport;

/// A pre-loaded request/response pair for the mock transport.
#[derive(Debug, Clone)]
struct Expectation {
    /// The exact bytes we expect to be sent.
    request: Vec<u8>,
    /// The bytes to return when the matching request is received.
    response: Vec<u8>,
}

/// A mock [`Transport`] for testing protocol engines without hardware.
///
/// Expectations are consumed in order. When `send()` is called, the sent
/// data is recorded and matched against the next expectation. The
/// corresponding response is then returned by the next `receive()` call.
///
/// If no expectation matches or the queue is exhausted, an error is returned.
#[derive(Debug)]
pub struct MockTransport {
    /// Ordered queue of expected request/response pairs.
    expectations: VecDeque<Expectation>,
    /// The response data pending for the next `receive()` call.
    pending_response: Option<Vec<u8>>,
    /// Cursor into the pending response (how many bytes have been read so far).
    response_cursor: usize,
    /// Whether the transport is "connected".
    connected: bool,
    /// Log of all bytes sent through this transport.
    sent_log: Vec<Vec<u8>>,
}

impl MockTransport {
    /// Create a new mock transport in the connected state.
    pub fn new() -> Self {
        MockTransport {
            expectations: VecDeque::new(),
            pending_response: None,
            response_cursor: 0,
            connected: true,
            sent_log: Vec::new(),
        }
    }

    /// Add an expected request/response pair.
    ///
    /// When `send()` is called with data matching `request`, the subsequent
    /// `receive()` call will return `response`.
    pub fn expect(&mut self, request: &[u8], response: &[u8]) {
        self.expectations.push_back(Expectation {
            request: request.to_vec(),
            response: response.to_vec(),
        });
    }

    /// Return a reference to all data that has been sent through this transport.
    ///
    /// Each element is the byte slice from one `send()` call.
    pub fn sent_data(&self) -> &[Vec<u8>] {
        &self.sent_log
    }

    /// Return the number of expectations that have not yet been consumed.
    pub fn remaining_expectations(&self) -> usize {
        self.expectations.len()
    }

    /// Set the connected state of the mock transport.
    ///
    /// When set to `false`, subsequent `send()` and `receive()` calls will
    /// return [`Error::NotConnected`].
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        if !self.connected {
            return Err(Error::NotConnected);
        }

        // Record what was sent.
        self.sent_log.push(data.to_vec());

        // Match against the next expectation.
        if let Some(expectation) = self.expectations.pop_front() {
            if data != expectation.request.as_slice() {
                return Err(Error::Protocol(format!(
                    "unexpected send data: expected {:02X?}, got {:02X?}",
                    expectation.request, data
                )));
            }
            self.pending_response = Some(expectation.response);
            self.response_cursor = 0;
            Ok(())
        } else {
            Err(Error::Protocol(
                "no more expectations in mock transport".into(),
            ))
        }
    }

    async fn receive(&mut self, buf: &mut [u8], _timeout: Duration) -> Result<usize> {
        if !self.connected {
            return Err(Error::NotConnected);
        }

        if let Some(ref response) = self.pending_response {
            let remaining = &response[self.response_cursor..];
            if remaining.is_empty() {
                self.pending_response = None;
                self.response_cursor = 0;
                return Err(Error::Timeout);
            }
            let n = remaining.len().min(buf.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.response_cursor += n;
            if self.response_cursor >= response.len() {
                // All response bytes consumed; clear for next exchange.
                self.pending_response = None;
                self.response_cursor = 0;
            }
            Ok(n)
        } else {
            Err(Error::Timeout)
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.connected = false;
        self.pending_response = None;
        self.response_cursor = 0;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_core::transport::Transport;

    #[tokio::test]
    async fn mock_transport_basic_send_receive() {
        let mut mock = MockTransport::new();
        let request = &[0xFE, 0xFE, 0x98, 0xE0, 0x03, 0xFD];
        let response = &[0xFE, 0xFE, 0xE0, 0x98, 0x03, 0x00, 0x60, 0x14, 0xFD];

        mock.expect(request, response);

        // Send the expected request.
        mock.send(request).await.unwrap();

        // Receive the pre-loaded response.
        let mut buf = [0u8; 64];
        let n = mock
            .receive(&mut buf, Duration::from_millis(100))
            .await
            .unwrap();

        assert_eq!(n, response.len());
        assert_eq!(&buf[..n], response);
    }

    #[tokio::test]
    async fn mock_transport_tracks_sent_data() {
        let mut mock = MockTransport::new();
        let req1 = &[0x01, 0x02];
        let req2 = &[0x03, 0x04];

        mock.expect(req1, &[0xFF]);
        mock.expect(req2, &[0xFE]);

        mock.send(req1).await.unwrap();
        mock.send(req2).await.unwrap();

        assert_eq!(mock.sent_data().len(), 2);
        assert_eq!(mock.sent_data()[0], req1);
        assert_eq!(mock.sent_data()[1], req2);
    }

    #[tokio::test]
    async fn mock_transport_wrong_data_errors() {
        let mut mock = MockTransport::new();
        mock.expect(&[0x01], &[0xFF]);

        let result = mock.send(&[0x99]).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protocol(_)));
    }

    #[tokio::test]
    async fn mock_transport_no_expectations_errors() {
        let mut mock = MockTransport::new();

        let result = mock.send(&[0x01]).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protocol(_)));
    }

    #[tokio::test]
    async fn mock_transport_receive_without_send_times_out() {
        let mut mock = MockTransport::new();
        let mut buf = [0u8; 64];

        let result = mock.receive(&mut buf, Duration::from_millis(10)).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Timeout));
    }

    #[tokio::test]
    async fn mock_transport_disconnect() {
        let mut mock = MockTransport::new();
        assert!(mock.is_connected());

        mock.close().await.unwrap();
        assert!(!mock.is_connected());

        // Operations after close should fail.
        let result = mock.send(&[0x01]).await;
        assert!(matches!(result.unwrap_err(), Error::NotConnected));
    }

    #[tokio::test]
    async fn mock_transport_set_connected() {
        let mut mock = MockTransport::new();
        mock.set_connected(false);
        assert!(!mock.is_connected());

        let result = mock.send(&[0x01]).await;
        assert!(matches!(result.unwrap_err(), Error::NotConnected));

        let mut buf = [0u8; 8];
        let result = mock.receive(&mut buf, Duration::from_millis(10)).await;
        assert!(matches!(result.unwrap_err(), Error::NotConnected));
    }

    #[tokio::test]
    async fn mock_transport_remaining_expectations() {
        let mut mock = MockTransport::new();
        mock.expect(&[0x01], &[0xFF]);
        mock.expect(&[0x02], &[0xFE]);
        assert_eq!(mock.remaining_expectations(), 2);

        mock.send(&[0x01]).await.unwrap();
        assert_eq!(mock.remaining_expectations(), 1);

        mock.send(&[0x02]).await.unwrap();
        assert_eq!(mock.remaining_expectations(), 0);
    }

    #[tokio::test]
    async fn mock_transport_partial_receive() {
        let mut mock = MockTransport::new();
        let request = &[0x01];
        let response = &[0xAA, 0xBB, 0xCC, 0xDD];
        mock.expect(request, response);

        mock.send(request).await.unwrap();

        // Read with a buffer smaller than the response.
        let mut buf = [0u8; 2];
        let n = mock
            .receive(&mut buf, Duration::from_millis(100))
            .await
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(&buf[..n], &[0xAA, 0xBB]);

        // Read the remaining bytes.
        let n = mock
            .receive(&mut buf, Duration::from_millis(100))
            .await
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(&buf[..n], &[0xCC, 0xDD]);
    }
}
