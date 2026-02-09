//! IO task types for the unified Icom I/O architecture.
//!
//! These types define the request/response protocol between rig methods and the
//! single IO task that owns the transport. No behavioral changes are introduced
//! here — the IO task loop itself is implemented in sub-phase A.3.

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use riglib_core::error::{Error, Result};
use riglib_core::transport::Transport;

use crate::civ::CivFrame;

/// Configuration for the IO task.
pub(crate) struct IoConfig {
    /// CI-V address of the target rig (e.g. 0x98 for IC-7610).
    pub civ_address: u8,
    /// Whether AI (transceive) mode is enabled — controls unsolicited event emission.
    pub ai_enabled: bool,
    /// Timeout for a single command/response exchange.
    pub command_timeout: Duration,
    /// Whether to automatically retry on timeout or collision.
    pub auto_retry: bool,
    /// Maximum number of retries when `auto_retry` is enabled.
    pub max_retries: u32,
    /// Whether to detect and recover from CI-V bus collisions.
    pub collision_recovery: bool,
}

/// A command request sent from rig methods to the IO task.
pub(crate) enum Request {
    /// A CI-V command with expected response.
    CivCommand {
        cmd_bytes: Vec<u8>,
        reply: oneshot::Sender<Result<CivFrame>>,
    },
    /// Fire-and-forget ACK-only command (SET operations).
    CivAckCommand {
        cmd_bytes: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Toggle a serial control line (DTR/RTS for hardware PTT/CW).
    SetLine {
        dtr: bool,
        on: bool,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Graceful shutdown; returns the transport for test recovery.
    Shutdown {
        reply: oneshot::Sender<Box<dyn Transport>>,
    },
}

/// Handle to the IO task. Stored inside `IcomRig`.
pub(crate) struct RigIo {
    /// Command channel (single for now; Phase B splits into rt_tx/bg_tx).
    pub cmd_tx: mpsc::Sender<Request>,
    /// Cancellation token for graceful shutdown.
    pub cancel: CancellationToken,
    /// Join handle for the IO task.
    pub task: JoinHandle<()>,
}

impl RigIo {
    /// Send a CI-V command and await the response.
    pub async fn command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<CivFrame> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(Request::CivCommand {
                cmd_bytes: cmd,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Error::NotConnected)?;

        // Safety-net timeout: command_timeout + 500ms for channel overhead.
        // The IO task enforces the real transport-level timeout internally.
        match tokio::time::timeout(timeout + Duration::from_millis(500), reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::NotConnected),
            Err(_) => Err(Error::Timeout),
        }
    }

    /// Send a fire-and-forget CI-V SET command and await the ACK.
    pub async fn ack_command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(Request::CivAckCommand {
                cmd_bytes: cmd,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Error::NotConnected)?;

        match tokio::time::timeout(timeout + Duration::from_millis(500), reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::NotConnected),
            Err(_) => Err(Error::Timeout),
        }
    }

    /// Toggle a serial control line (DTR or RTS).
    pub async fn set_line(&self, dtr: bool, on: bool) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(Request::SetLine {
                dtr,
                on,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Error::NotConnected)?;

        match reply_rx.await {
            Ok(result) => result,
            Err(_) => Err(Error::NotConnected),
        }
    }

    /// Shut down the IO task and recover the transport.
    pub async fn shutdown(self) -> Result<Box<dyn Transport>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self
            .cmd_tx
            .send(Request::Shutdown { reply: reply_tx })
            .await;
        let transport = reply_rx.await.map_err(|_| Error::NotConnected)?;
        let _ = self.task.await;
        Ok(transport)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_config_construction() {
        let config = IoConfig {
            civ_address: 0x98,
            ai_enabled: false,
            command_timeout: Duration::from_millis(500),
            auto_retry: true,
            max_retries: 3,
            collision_recovery: true,
        };
        assert_eq!(config.civ_address, 0x98);
        assert!(!config.ai_enabled);
        assert_eq!(config.command_timeout, Duration::from_millis(500));
        assert!(config.auto_retry);
        assert_eq!(config.max_retries, 3);
        assert!(config.collision_recovery);
    }

    #[test]
    fn request_civ_command_construction() {
        let (reply_tx, _reply_rx) = oneshot::channel();
        let cmd_bytes = vec![0xFE, 0xFE, 0x98, 0xE0, 0x03, 0xFD];
        let request = Request::CivCommand {
            cmd_bytes: cmd_bytes.clone(),
            reply: reply_tx,
        };
        match request {
            Request::CivCommand { cmd_bytes: bytes, .. } => {
                assert_eq!(bytes, cmd_bytes);
            }
            _ => panic!("expected CivCommand"),
        }
    }

    #[test]
    fn request_civ_ack_command_construction() {
        let (reply_tx, _reply_rx) = oneshot::channel();
        let cmd_bytes = vec![0xFE, 0xFE, 0x98, 0xE0, 0x1C, 0x00, 0x01, 0xFD];
        let request = Request::CivAckCommand {
            cmd_bytes: cmd_bytes.clone(),
            reply: reply_tx,
        };
        match request {
            Request::CivAckCommand { cmd_bytes: bytes, .. } => {
                assert_eq!(bytes, cmd_bytes);
            }
            _ => panic!("expected CivAckCommand"),
        }
    }

    #[test]
    fn request_set_line_construction() {
        let (reply_tx, _reply_rx) = oneshot::channel();
        let request = Request::SetLine {
            dtr: true,
            on: true,
            reply: reply_tx,
        };
        match request {
            Request::SetLine { dtr, on, .. } => {
                assert!(dtr);
                assert!(on);
            }
            _ => panic!("expected SetLine"),
        }
    }

    #[tokio::test]
    async fn rig_io_command_not_connected() {
        // Create a RigIo with an immediately-dropped receiver to simulate disconnection.
        let (cmd_tx, _cmd_rx) = mpsc::channel(32);
        drop(_cmd_rx);

        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo { cmd_tx, cancel, task };
        let result = io.command(vec![0x03], Duration::from_millis(100)).await;
        assert!(matches!(result, Err(Error::NotConnected)));
    }

    #[tokio::test]
    async fn rig_io_ack_command_not_connected() {
        let (cmd_tx, _cmd_rx) = mpsc::channel(32);
        drop(_cmd_rx);

        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo { cmd_tx, cancel, task };
        let result = io.ack_command(vec![0x1C, 0x00, 0x01], Duration::from_millis(100)).await;
        assert!(matches!(result, Err(Error::NotConnected)));
    }

    #[tokio::test]
    async fn rig_io_set_line_not_connected() {
        let (cmd_tx, _cmd_rx) = mpsc::channel(32);
        drop(_cmd_rx);

        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo { cmd_tx, cancel, task };
        let result = io.set_line(true, true).await;
        assert!(matches!(result, Err(Error::NotConnected)));
    }

    #[tokio::test]
    async fn rig_io_command_receives_response() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<Request>(32);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo {
            cmd_tx,
            cancel,
            task,
        };

        // Spawn a handler that responds to the command.
        let handler = tokio::spawn(async move {
            if let Some(Request::CivCommand { reply, .. }) = cmd_rx.recv().await {
                let frame = CivFrame {
                    dst_addr: 0xE0,
                    src_addr: 0x98,
                    cmd: 0x03,
                    sub_cmd: Some(0x00),
                    data: vec![0x00, 0x25, 0x14, 0x00],
                };
                let _ = reply.send(Ok(frame));
            }
        });

        let result = io.command(vec![0x03], Duration::from_millis(500)).await;
        assert!(result.is_ok());
        let frame = result.unwrap();
        assert_eq!(frame.cmd, 0x03);
        assert_eq!(frame.data, vec![0x00, 0x25, 0x14, 0x00]);

        handler.await.unwrap();
    }

    #[tokio::test]
    async fn rig_io_ack_command_receives_ok() {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<Request>(32);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo {
            cmd_tx,
            cancel,
            task,
        };

        let handler = tokio::spawn(async move {
            if let Some(Request::CivAckCommand { reply, .. }) = cmd_rx.recv().await {
                let _ = reply.send(Ok(()));
            }
        });

        let result = io.ack_command(vec![0x1C, 0x00, 0x01], Duration::from_millis(500)).await;
        assert!(result.is_ok());

        handler.await.unwrap();
    }
}
