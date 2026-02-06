//! Error types for riglib.
//!
//! All fallible operations across the library return [`Result<T>`], which
//! uses [`Error`] as the error type. Transport-layer, protocol-layer, and
//! application-layer errors are all captured here.

/// The error type for all riglib operations.
///
/// Variants cover the full range of failure modes encountered when
/// communicating with transceivers: physical transport failures,
/// protocol decode errors, timeouts, and unsupported operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A transport-level error (serial port, TCP socket, USB).
    #[error("transport error: {0}")]
    Transport(String),

    /// A protocol-level error (malformed CI-V frame, unexpected CAT response).
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Timed out waiting for a response from the rig.
    ///
    /// This typically indicates the rig is powered off, the baud rate is
    /// wrong, or the CI-V address is incorrect.
    #[error("timeout waiting for response")]
    Timeout,

    /// The requested operation is not supported by this rig model.
    #[error("unsupported operation: {0}")]
    Unsupported(String),

    /// An invalid parameter was passed to a rig command.
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    /// No connection to the rig has been established.
    #[error("not connected")]
    NotConnected,

    /// The connection to the rig was lost unexpectedly.
    #[error("connection lost")]
    ConnectionLost,

    /// An audio or data stream was closed unexpectedly.
    ///
    /// This occurs when the receiver side of an audio channel is dropped
    /// (for TX) or the sender side is dropped (for RX).
    #[error("stream closed")]
    StreamClosed,

    /// An underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// A convenience `Result` alias using [`Error`] as the error type.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_transport() {
        let e = Error::Transport("port busy".into());
        assert_eq!(e.to_string(), "transport error: port busy");
    }

    #[test]
    fn error_display_protocol() {
        let e = Error::Protocol("bad CI-V frame".into());
        assert_eq!(e.to_string(), "protocol error: bad CI-V frame");
    }

    #[test]
    fn error_display_timeout() {
        let e = Error::Timeout;
        assert_eq!(e.to_string(), "timeout waiting for response");
    }

    #[test]
    fn error_display_unsupported() {
        let e = Error::Unsupported("IQ streaming".into());
        assert_eq!(e.to_string(), "unsupported operation: IQ streaming");
    }

    #[test]
    fn error_display_invalid_parameter() {
        let e = Error::InvalidParameter("frequency out of range".into());
        assert_eq!(e.to_string(), "invalid parameter: frequency out of range");
    }

    #[test]
    fn error_display_not_connected() {
        let e = Error::NotConnected;
        assert_eq!(e.to_string(), "not connected");
    }

    #[test]
    fn error_display_connection_lost() {
        let e = Error::ConnectionLost;
        assert_eq!(e.to_string(), "connection lost");
    }

    #[test]
    fn error_display_stream_closed() {
        let e = Error::StreamClosed;
        assert_eq!(e.to_string(), "stream closed");
    }

    #[test]
    fn error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
        let e: Error = io_err.into();
        assert!(matches!(e, Error::Io(_)));
        assert!(e.to_string().contains("pipe broken"));
    }

    #[test]
    fn error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        // io::Error is Send + Sync, so our Error should be too.
        assert_send::<Error>();
        assert_sync::<Error>();
    }

    #[test]
    fn error_implements_std_error() {
        fn assert_std_error<T: std::error::Error>() {}
        assert_std_error::<Error>();
    }

    #[test]
    fn result_alias_works() {
        let ok: Result<u32> = Ok(42);
        match ok {
            Ok(val) => assert_eq!(val, 42),
            Err(_) => panic!("expected Ok"),
        }

        let err: Result<u32> = Err(Error::Timeout);
        assert!(err.is_err());
    }
}
