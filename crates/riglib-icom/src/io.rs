//! IO task types and implementation for the unified Icom I/O architecture.
//!
//! This module defines the request/response protocol between rig methods and
//! the single IO task that owns the transport, plus the IO task loop itself.
//!
//! The IO task handles: command/response exchanges, CI-V echo skipping,
//! collision detection and retry, NAK detection, unsolicited transceive
//! frame processing (AI on), and idle frame draining (AI off).

use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;
use riglib_core::transport::Transport;

use crate::civ::{self, CONTROLLER_ADDR, CivFrame, DecodeResult};
use crate::transceive;

// ---------------------------------------------------------------------------
// Types (A.2)
// ---------------------------------------------------------------------------

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
    #[allow(dead_code)]
    Shutdown {
        reply: oneshot::Sender<Box<dyn Transport>>,
    },
}

/// Handle to the IO task. Stored inside `IcomRig`.
pub(crate) struct RigIo {
    /// Real-time command channel — checked first in the IO loop's biased select.
    /// Used for PTT/CW commands that need priority dispatch.
    pub rt_tx: mpsc::Sender<Request>,
    /// Background command channel — checked after RT in the IO loop.
    pub bg_tx: mpsc::Sender<Request>,
    /// Cancellation token for graceful shutdown.
    pub cancel: CancellationToken,
    /// Join handle for the IO task.
    pub task: JoinHandle<()>,
}

impl RigIo {
    /// Send a CI-V command and await the response.
    pub async fn command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<CivFrame> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.bg_tx
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
        self.bg_tx
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

    /// Toggle a serial control line (DTR or RTS) via the background channel.
    #[allow(dead_code)] // Used by tests to exercise the BG path.
    pub async fn set_line(&self, dtr: bool, on: bool) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.bg_tx
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

    /// Send a fire-and-forget CI-V SET command via the real-time channel.
    ///
    /// Identical to [`ack_command`] but routes through `rt_tx`.
    pub async fn rt_ack_command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.rt_tx
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

    /// Toggle a serial control line via the real-time channel.
    ///
    /// Identical to [`set_line`] but routes through `rt_tx`.
    pub async fn rt_set_line(&self, dtr: bool, on: bool) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.rt_tx
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
    #[allow(dead_code)]
    pub async fn shutdown(self) -> Result<Box<dyn Transport>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let _ = self
            .bg_tx
            .send(Request::Shutdown { reply: reply_tx })
            .await;
        let transport = reply_rx.await.map_err(|_| Error::NotConnected)?;
        let _ = self.task.await;
        Ok(transport)
    }
}

// ---------------------------------------------------------------------------
// Spawn (A.3)
// ---------------------------------------------------------------------------

/// Spawn the IO task. Returns the handle for sending commands.
///
/// The IO task owns the transport exclusively and processes all
/// command/response exchanges, echo skipping, collision recovery,
/// and unsolicited transceive frame handling.
pub(crate) fn spawn_io_task(
    transport: Box<dyn Transport>,
    config: IoConfig,
    event_tx: broadcast::Sender<RigEvent>,
) -> RigIo {
    let (rt_tx, rt_rx) = mpsc::channel::<Request>(32);
    let (bg_tx, bg_rx) = mpsc::channel::<Request>(32);
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    let task = tokio::spawn(io_loop(transport, config, event_tx, rt_rx, bg_rx, cancel_clone));

    RigIo {
        rt_tx,
        bg_tx,
        cancel,
        task,
    }
}

// ---------------------------------------------------------------------------
// IO Loop (A.3)
// ---------------------------------------------------------------------------

/// Maximum buffer size before reset to prevent unbounded growth.
/// Applies to both the idle buffer and the per-command response buffer.
/// A normal CI-V frame is 6–15 bytes; 4096 is generous headroom for
/// echo + interleaved transceive + response even on a noisy bus.
const MAX_BUF: usize = 4096;

/// The main IO loop. Runs as a spawned Tokio task.
///
/// Uses `tokio::select! { biased; }` to prioritize:
/// 1. Cancellation
/// 2. Real-time (RT) command dispatch
/// 3. Background (BG) command dispatch
/// 4. Idle unsolicited frame reading
async fn io_loop(
    mut transport: Box<dyn Transport>,
    config: IoConfig,
    event_tx: broadcast::Sender<RigEvent>,
    mut rt_rx: mpsc::Receiver<Request>,
    mut bg_rx: mpsc::Receiver<Request>,
    cancel: CancellationToken,
) {
    let mut idle_buf = Vec::new();

    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                debug!("IO task cancelled");
                break;
            }

            req = rt_rx.recv() => {
                match req {
                    Some(Request::Shutdown { reply }) => {
                        debug!("IO task shutdown requested (RT)");
                        let _ = reply.send(transport);
                        return;
                    }
                    Some(req) => handle_request(req, &mut transport, &config, &event_tx).await,
                    None => {
                        debug!("RT channel closed, exiting IO task");
                        break;
                    }
                }
            }

            req = bg_rx.recv() => {
                match req {
                    Some(Request::Shutdown { reply }) => {
                        debug!("IO task shutdown requested (BG)");
                        let _ = reply.send(transport);
                        return;
                    }
                    Some(req) => handle_request(req, &mut transport, &config, &event_tx).await,
                    None => {
                        debug!("BG channel closed, exiting IO task");
                        break;
                    }
                }
            }

            // Idle: read unsolicited data from the bus.
            idle_ok = async {
                let mut buf = [0u8; 256];
                match transport.receive(&mut buf, Duration::from_millis(100)).await {
                    Ok(n) if n > 0 => {
                        idle_buf.extend_from_slice(&buf[..n]);
                        if idle_buf.len() > MAX_BUF {
                            tracing::warn!(
                                len = idle_buf.len(),
                                "idle buffer overflow, resetting"
                            );
                            idle_buf.clear();
                            return true;
                        }
                        if config.ai_enabled {
                            process_idle_frames(
                                &mut idle_buf,
                                config.civ_address,
                                &event_tx,
                            );
                        } else {
                            drain_idle_frames(&mut idle_buf);
                        }
                        true
                    }
                    Ok(_) | Err(Error::Timeout) => {
                        // Timeout or zero bytes — normal idle behavior.
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        true
                    }
                    Err(e) => {
                        tracing::error!("transport error in idle read: {e}");
                        false
                    }
                }
            } => {
                if !idle_ok {
                    let _ = event_tx.send(RigEvent::Disconnected);
                    break;
                }
            }
        }
    }
}

/// Dispatch a single request on the transport.
///
/// Shared by both the RT and BG select arms — the IO loop determines
/// priority; this function handles execution.
///
/// `Shutdown` is handled inline in the IO loop because it requires
/// moving ownership of the transport.
async fn handle_request(
    req: Request,
    transport: &mut Box<dyn Transport>,
    config: &IoConfig,
    event_tx: &broadcast::Sender<RigEvent>,
) {
    match req {
        Request::CivCommand { cmd_bytes, reply } => {
            let result = execute_civ_command(
                &mut **transport,
                &cmd_bytes,
                config,
                if config.ai_enabled { Some(event_tx) } else { None },
            ).await;
            let _ = reply.send(result);
        }
        Request::CivAckCommand { cmd_bytes, reply } => {
            let result = execute_civ_ack_command(
                &mut **transport,
                &cmd_bytes,
                config,
            ).await;
            let _ = reply.send(result);
        }
        Request::SetLine { dtr, on, reply } => {
            let result = if dtr {
                transport.set_dtr(on).await
            } else {
                transport.set_rts(on).await
            };
            let _ = reply.send(result);
        }
        Request::Shutdown { .. } => unreachable!("Shutdown handled in io_loop"),
    }
}

// ---------------------------------------------------------------------------
// Command execution (A.3)
// ---------------------------------------------------------------------------

/// Execute a CI-V command on the transport, handling echo, collision,
/// retry, and interleaved transceive frames.
///
/// This is the unified implementation that handles all CI-V command
/// exchanges through the IO task.
async fn execute_civ_command(
    transport: &mut dyn Transport,
    cmd: &[u8],
    config: &IoConfig,
    event_tx: Option<&broadcast::Sender<RigEvent>>,
) -> Result<CivFrame> {
    let retries = if config.auto_retry {
        config.max_retries
    } else {
        0
    };
    let civ_address = config.civ_address;

    for attempt in 0..=retries {
        if attempt > 0 {
            debug!(attempt, "CI-V command retry");
            tokio::time::sleep(Duration::from_millis(20 * attempt as u64)).await;
        }

        transport.send(cmd).await?;

        let mut buf = [0u8; 256];
        let mut response_buf = Vec::new();
        let mut collision_detected = false;

        loop {
            match transport.receive(&mut buf, config.command_timeout).await {
                Ok(n) => {
                    response_buf.extend_from_slice(&buf[..n]);

                    // Bounded buffer: prevent unbounded growth from
                    // malformed input or noise on the CI-V bus.
                    if response_buf.len() > MAX_BUF {
                        tracing::warn!(
                            len = response_buf.len(),
                            "response buffer overflow, clearing and retrying"
                        );
                        response_buf.clear();
                        break;
                    }

                    loop {
                        match civ::decode_frame(&response_buf) {
                            DecodeResult::Frame(frame, consumed) => {
                                response_buf.drain(..consumed);

                                // Skip echo of our own command.
                                if frame.dst_addr == civ_address
                                    && frame.src_addr == CONTROLLER_ADDR
                                {
                                    debug!("skipping CI-V echo frame");
                                    continue;
                                }

                                // Actual response from the rig to us.
                                if frame.dst_addr == CONTROLLER_ADDR
                                    && frame.src_addr == civ_address
                                {
                                    if frame.is_nak() {
                                        return Err(Error::Protocol(
                                            "rig returned NAK".into(),
                                        ));
                                    }
                                    return Ok(frame);
                                }

                                // Interleaved transceive broadcast — emit if AI enabled.
                                if let Some(tx) = event_tx {
                                    if transceive::is_transceive_frame(&frame, civ_address) {
                                        transceive::process_single_transceive_frame(
                                            &frame,
                                            civ_address,
                                            tx,
                                        );
                                        continue;
                                    }
                                }

                                debug!(
                                    dst = frame.dst_addr,
                                    src = frame.src_addr,
                                    "skipping CI-V frame from unexpected address"
                                );
                            }
                            DecodeResult::Incomplete => break,
                            DecodeResult::Collision(consumed) => {
                                response_buf.drain(..consumed);
                                if config.collision_recovery {
                                    debug!("CI-V collision detected, will retry");
                                    collision_detected = true;
                                    break;
                                }
                                return Err(Error::Protocol(
                                    "CI-V bus collision".into(),
                                ));
                            }
                        }
                    }

                    // Collision detected — break receive loop immediately
                    // to retry without waiting for a timeout.
                    if collision_detected {
                        break;
                    }
                }
                Err(Error::Timeout) => {
                    // Transport timed out. Try one more decode pass on
                    // any accumulated partial data.
                    if !response_buf.is_empty() {
                        if let DecodeResult::Frame(frame, _) =
                            civ::decode_frame(&response_buf)
                        {
                            if frame.dst_addr == CONTROLLER_ADDR
                                && frame.src_addr == civ_address
                                && !frame.is_nak()
                            {
                                return Ok(frame);
                            }
                        }
                    }
                    break;
                }
                Err(e) => return Err(e),
            }
        }
    }

    Err(Error::Timeout)
}

/// Execute a CI-V SET command and verify the ACK response.
///
/// SET operations don't generate transceive events, so no event_tx is needed.
async fn execute_civ_ack_command(
    transport: &mut dyn Transport,
    cmd: &[u8],
    config: &IoConfig,
) -> Result<()> {
    let frame = execute_civ_command(transport, cmd, config, None).await?;
    if frame.is_ack() {
        Ok(())
    } else {
        Err(Error::Protocol(format!(
            "expected ACK, got cmd=0x{:02X}",
            frame.cmd
        )))
    }
}

// ---------------------------------------------------------------------------
// Idle frame processing (A.3)
// ---------------------------------------------------------------------------

/// Process complete transceive frames in the idle buffer, emitting events.
///
/// Delegates to the existing transceive frame processing logic.
fn process_idle_frames(
    buf: &mut Vec<u8>,
    civ_address: u8,
    event_tx: &broadcast::Sender<RigEvent>,
) {
    transceive::process_transceive_frames(buf, civ_address, event_tx);
}

/// Drain complete frames from the idle buffer without emitting events.
///
/// Used when AI mode is disabled to prevent unbounded buffer growth
/// from stray bytes on the CI-V bus.
fn drain_idle_frames(buf: &mut Vec<u8>) {
    loop {
        match civ::decode_frame(buf) {
            DecodeResult::Frame(_, consumed) => {
                buf.drain(..consumed);
            }
            DecodeResult::Incomplete => break,
            DecodeResult::Collision(consumed) => {
                buf.drain(..consumed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civ::encode_frame;
    use riglib_test_harness::MockTransport;

    const IC7610_ADDR: u8 = 0x98;

    /// Helper to create a standard IoConfig for tests.
    fn test_config() -> IoConfig {
        IoConfig {
            civ_address: IC7610_ADDR,
            ai_enabled: false,
            command_timeout: Duration::from_millis(500),
            auto_retry: false,
            max_retries: 0,
            collision_recovery: false,
        }
    }

    // =======================================================================
    // A.2 — Type construction tests
    // =======================================================================

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
        let (bg_tx, _rx) = mpsc::channel(32);
        drop(_rx);

        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo { rt_tx: bg_tx.clone(), bg_tx, cancel, task };
        let result = io.command(vec![0x03], Duration::from_millis(100)).await;
        assert!(matches!(result, Err(Error::NotConnected)));
    }

    #[tokio::test]
    async fn rig_io_ack_command_not_connected() {
        let (bg_tx, _rx) = mpsc::channel(32);
        drop(_rx);

        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo { rt_tx: bg_tx.clone(), bg_tx, cancel, task };
        let result = io.ack_command(vec![0x1C, 0x00, 0x01], Duration::from_millis(100)).await;
        assert!(matches!(result, Err(Error::NotConnected)));
    }

    #[tokio::test]
    async fn rig_io_set_line_not_connected() {
        let (bg_tx, _rx) = mpsc::channel(32);
        drop(_rx);

        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo { rt_tx: bg_tx.clone(), bg_tx, cancel, task };
        let result = io.set_line(true, true).await;
        assert!(matches!(result, Err(Error::NotConnected)));
    }

    #[tokio::test]
    async fn rig_io_command_receives_response() {
        let (bg_tx, mut cmd_rx) = mpsc::channel::<Request>(32);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo {
            rt_tx: bg_tx.clone(),
            bg_tx,
            cancel,
            task,
        };

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
        let (bg_tx, mut cmd_rx) = mpsc::channel::<Request>(32);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo {
            rt_tx: bg_tx.clone(),
            bg_tx,
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

    // =======================================================================
    // A.3 — IO task loop tests (white-box, using spawn_io_task + MockTransport)
    // =======================================================================

    #[tokio::test]
    async fn io_task_basic_command_response() {
        // AI off, no echo — IC-7300 USB with echo disabled.
        let mut mock = MockTransport::new();

        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x03, None, &[]);
        let response = encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x03,
            Some(0x00),
            &[0x00, 0x60, 0x14, 0x00],
        );
        mock.expect(&cmd, &response);

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_ok());
        let frame = result.unwrap();
        assert_eq!(frame.cmd, 0x03);
        assert_eq!(frame.sub_cmd, Some(0x00));
        assert_eq!(frame.data, vec![0x00, 0x60, 0x14, 0x00]);

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_echo_skip() {
        // AI off, with echo — IC-7300 USB with echo enabled.
        let mut mock = MockTransport::new();

        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x03, None, &[]);
        let response = encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x03,
            Some(0x00),
            &[0x00, 0x60, 0x14, 0x00],
        );

        // MockTransport returns echo + response concatenated.
        let mut full_response = cmd.clone();
        full_response.extend_from_slice(&response);
        mock.expect(&cmd, &full_response);

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_ok());
        let frame = result.unwrap();
        assert_eq!(frame.cmd, 0x03);

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_ack_command() {
        let mut mock = MockTransport::new();

        // PTT ON command.
        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x1C, Some(0x00), &[0x01]);
        let ack = encode_frame(CONTROLLER_ADDR, IC7610_ADDR, civ::ACK, None, &[]);
        mock.expect(&cmd, &ack);

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.ack_command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_nak_response() {
        let mut mock = MockTransport::new();

        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x03, None, &[]);
        let nak = encode_frame(CONTROLLER_ADDR, IC7610_ADDR, civ::NAK, None, &[]);
        mock.expect(&cmd, &nak);

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protocol(_)));

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_collision_with_recovery() {
        let mut mock = MockTransport::new();

        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x03, None, &[]);

        // First attempt: collision (0xFC in frame body).
        let collision = vec![
            0xFE, 0xFE, IC7610_ADDR, CONTROLLER_ADDR, civ::COLLISION, 0xFD,
        ];
        mock.expect(&cmd, &collision);

        // Second attempt (retry): clean response.
        let response = encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x03,
            Some(0x00),
            &[0x00, 0x60, 0x14, 0x00],
        );
        mock.expect(&cmd, &response);

        let (event_tx, _) = broadcast::channel(16);
        let config = IoConfig {
            auto_retry: true,
            max_retries: 3,
            collision_recovery: true,
            ..test_config()
        };
        let io = spawn_io_task(Box::new(mock), config, event_tx);

        let result = io.command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_ok());
        let frame = result.unwrap();
        assert_eq!(frame.cmd, 0x03);

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_collision_no_recovery() {
        let mut mock = MockTransport::new();

        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x03, None, &[]);
        let collision = vec![
            0xFE, 0xFE, IC7610_ADDR, CONTROLLER_ADDR, civ::COLLISION, 0xFD,
        ];
        mock.expect(&cmd, &collision);

        let (event_tx, _) = broadcast::channel(16);
        // collision_recovery is false (default from test_config).
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protocol(_)));

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_set_line_dtr() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.set_line(true, true).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_set_line_rts() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.set_line(false, true).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_shutdown_recovers_transport() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.shutdown().await;
        assert!(result.is_ok());
        let transport = result.unwrap();
        assert!(transport.is_connected());
    }

    #[tokio::test]
    async fn io_task_interleaved_transceive_frame() {
        // AI on: echo + unsolicited freq change + response.
        let mut mock = MockTransport::new();

        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x03, None, &[]);

        let echo = cmd.clone();
        let unsolicited = encode_frame(
            0x00,
            IC7610_ADDR,
            0x00,
            None,
            &civ::freq_to_bcd(14_074_000),
        );
        let response = encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x03,
            Some(0x00),
            &[0x00, 0x60, 0x14, 0x00],
        );

        let mut full = echo;
        full.extend_from_slice(&unsolicited);
        full.extend_from_slice(&response);
        mock.expect(&cmd, &full);

        let (event_tx, mut event_rx) = broadcast::channel(16);
        let config = IoConfig {
            ai_enabled: true,
            ..test_config()
        };
        let io = spawn_io_task(Box::new(mock), config, event_tx);

        let result = io.command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_ok());
        let frame = result.unwrap();
        assert_eq!(frame.cmd, 0x03);

        // Verify the unsolicited freq change was emitted as an event.
        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::FrequencyChanged { freq_hz, .. } => {
                assert_eq!(freq_hz, 14_074_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }

        let _ = io.shutdown().await;
    }

    // =======================================================================
    // B.2 — RT channel tests
    // =======================================================================

    #[tokio::test]
    async fn io_task_rt_ack_command() {
        let mut mock = MockTransport::new();

        // PTT ON via RT channel.
        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x1C, Some(0x00), &[0x01]);
        let ack = encode_frame(CONTROLLER_ADDR, IC7610_ADDR, civ::ACK, None, &[]);
        mock.expect(&cmd, &ack);

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.rt_ack_command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_rt_set_line_dtr() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.rt_set_line(true, true).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_rt_set_line_rts() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(mock), test_config(), event_tx);

        let result = io.rt_set_line(false, true).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_rt_priority_over_bg() {
        // Verify that biased select processes RT commands before BG,
        // even when BG commands were enqueued first.
        //
        // The mock's FIFO expectation queue is loaded in RT-first order.
        // If the IO loop processed BG before RT, the mock would see
        // mismatched bytes and return a protocol error.
        let mut mock = MockTransport::new();

        let rt_cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x1C, Some(0x00), &[0x01]);
        let rt_ack = encode_frame(CONTROLLER_ADDR, IC7610_ADDR, civ::ACK, None, &[]);

        let bg_cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x03, None, &[]);
        let bg_resp = encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x03,
            Some(0x00),
            &[0x00, 0x60, 0x14, 0x00],
        );

        // Expectations in RT-first order.
        mock.expect(&rt_cmd, &rt_ack);
        mock.expect(&bg_cmd, &bg_resp);
        mock.expect(&bg_cmd, &bg_resp);
        mock.expect(&bg_cmd, &bg_resp);

        let (event_tx, _) = broadcast::channel(16);
        let cancel = CancellationToken::new();
        let (rt_tx, rt_rx) = mpsc::channel::<Request>(32);
        let (bg_tx, bg_rx) = mpsc::channel::<Request>(32);

        // Pre-fill BG channel with 3 commands BEFORE starting the IO loop.
        let mut bg_replies = Vec::new();
        for _ in 0..3 {
            let (reply_tx, reply_rx) = oneshot::channel();
            bg_tx
                .send(Request::CivCommand {
                    cmd_bytes: bg_cmd.clone(),
                    reply: reply_tx,
                })
                .await
                .unwrap();
            bg_replies.push(reply_rx);
        }

        // Pre-fill RT channel with 1 command.
        let (rt_reply_tx, rt_reply_rx) = oneshot::channel();
        rt_tx
            .send(Request::CivAckCommand {
                cmd_bytes: rt_cmd.clone(),
                reply: rt_reply_tx,
            })
            .await
            .unwrap();

        // Spawn the IO loop directly to test biased select behavior.
        let task = tokio::spawn(io_loop(
            Box::new(mock),
            test_config(),
            event_tx,
            rt_rx,
            bg_rx,
            cancel.clone(),
        ));

        // All commands should succeed. If RT wasn't processed first,
        // the mock would return a protocol error due to byte mismatch.
        let rt_result = rt_reply_rx.await.unwrap();
        assert!(rt_result.is_ok(), "RT command failed: {rt_result:?}");

        for (i, reply_rx) in bg_replies.into_iter().enumerate() {
            let bg_result = reply_rx.await.unwrap();
            assert!(bg_result.is_ok(), "BG command {i} failed: {bg_result:?}");
        }

        cancel.cancel();
        let _ = task.await;
    }

    // =======================================================================
    // B.3 — Bounded buffer + resync tests
    // =======================================================================

    #[tokio::test]
    async fn io_task_response_buffer_overflow_resyncs() {
        // Feed >4096 bytes of garbage on the first attempt. With auto_retry,
        // the overflow triggers a buffer clear and retry. The second attempt
        // gets a valid response.
        let mut mock = MockTransport::new();

        let cmd = encode_frame(IC7610_ADDR, CONTROLLER_ADDR, 0x03, None, &[]);
        let response = encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x03,
            Some(0x00),
            &[0x00, 0x60, 0x14, 0x00],
        );

        // First attempt: 5000 bytes of non-CI-V garbage (no 0xFD terminator).
        let garbage = vec![0xAA; 5000];
        mock.expect(&cmd, &garbage);

        // Second attempt (retry): valid response.
        mock.expect(&cmd, &response);

        let (event_tx, _) = broadcast::channel(16);
        let config = IoConfig {
            auto_retry: true,
            max_retries: 1,
            ..test_config()
        };
        let io = spawn_io_task(Box::new(mock), config, event_tx);

        let result = io.command(cmd, Duration::from_millis(500)).await;
        assert!(result.is_ok(), "expected success after resync, got {result:?}");
        let frame = result.unwrap();
        assert_eq!(frame.cmd, 0x03);

        let _ = io.shutdown().await;
    }

    // =======================================================================
    // Idle frame processing tests
    // =======================================================================

    // =======================================================================
    // Disconnect detection tests
    // =======================================================================

    /// A transport that returns NotConnected after a specified number of
    /// receive() calls, simulating a USB disconnect.
    struct DisconnectingTransport {
        receive_count: std::sync::atomic::AtomicU32,
        disconnect_after: u32,
    }

    impl DisconnectingTransport {
        fn new(disconnect_after: u32) -> Self {
            Self {
                receive_count: std::sync::atomic::AtomicU32::new(0),
                disconnect_after,
            }
        }
    }

    #[async_trait::async_trait]
    impl Transport for DisconnectingTransport {
        async fn send(&mut self, _data: &[u8]) -> Result<()> {
            Ok(())
        }

        async fn receive(&mut self, _buf: &mut [u8], _timeout: Duration) -> Result<usize> {
            let count = self.receive_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if count >= self.disconnect_after {
                Err(Error::NotConnected)
            } else {
                Err(Error::Timeout)
            }
        }

        async fn close(&mut self) -> Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            true
        }

        async fn set_dtr(&mut self, _on: bool) -> Result<()> {
            Ok(())
        }

        async fn set_rts(&mut self, _on: bool) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn io_task_exits_on_transport_disconnect() {
        // Simulate a USB disconnect: after 2 idle reads (returning Timeout),
        // the transport returns NotConnected. The IO task should exit and
        // emit a Disconnected event.
        let transport = DisconnectingTransport::new(2);

        let (event_tx, mut event_rx) = broadcast::channel(16);
        let io = spawn_io_task(Box::new(transport), test_config(), event_tx);

        // The IO task should exit on its own when it hits NotConnected.
        let result = tokio::time::timeout(Duration::from_secs(2), io.task).await;
        assert!(result.is_ok(), "IO task did not exit after disconnect");
        assert!(result.unwrap().is_ok(), "IO task panicked");

        // Verify that a Disconnected event was emitted.
        let event = event_rx.try_recv().unwrap();
        assert!(
            matches!(event, RigEvent::Disconnected),
            "expected Disconnected event, got {event:?}"
        );
    }

    // =======================================================================
    // Idle frame processing tests
    // =======================================================================

    #[test]
    fn drain_idle_frames_consumes_complete_frames() {
        let frame1 = encode_frame(
            0x00,
            IC7610_ADDR,
            0x00,
            None,
            &civ::freq_to_bcd(7_000_000),
        );
        let frame2 = encode_frame(0x00, IC7610_ADDR, 0x01, None, &[0x03, 0x01]);

        let mut buf = Vec::new();
        buf.extend_from_slice(&frame1);
        buf.extend_from_slice(&frame2);

        drain_idle_frames(&mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn drain_idle_frames_preserves_incomplete() {
        let frame = encode_frame(
            0x00,
            IC7610_ADDR,
            0x00,
            None,
            &civ::freq_to_bcd(7_000_000),
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&frame);
        // Append incomplete frame (no terminator).
        buf.extend_from_slice(&[0xFE, 0xFE, 0x00, IC7610_ADDR]);

        drain_idle_frames(&mut buf);
        // Complete frame consumed, incomplete preserved.
        assert_eq!(buf, vec![0xFE, 0xFE, 0x00, IC7610_ADDR]);
    }
}
