//! IO task types and implementation for text-protocol rig backends.
//!
//! This module implements the single-IO-task pattern for Kenwood, Elecraft, and
//! Yaesu rigs. One tokio task owns the transport exclusively and processes all
//! command/response exchanges, SET command drain, unsolicited AI frame handling,
//! and graceful shutdown.
//!
//! Compared to the Icom IO task, this is simpler: no CI-V echo frames, no bus
//! collision detection, no BCD encoding. Everything is semicolon-terminated ASCII.

use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;
use riglib_core::transport::Transport;

use crate::protocol::{self, DecodeResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Configuration for the text-protocol IO task.
pub struct IoConfig {
    /// Whether AI (Auto Information / transceive) mode is enabled.
    pub ai_enabled: bool,
    /// Timeout for a single command/response exchange.
    pub command_timeout: Duration,
    /// Whether to automatically retry on timeout.
    pub auto_retry: bool,
    /// Maximum number of retries when `auto_retry` is enabled.
    pub max_retries: u32,
    /// How long to drain after a SET command to catch `?;` errors (default 50ms).
    pub set_drain_timeout: Duration,
    /// Command prefixes that include a trailing digit in the prefix.
    ///
    /// Kenwood/Elecraft: `&[]`. Yaesu: `&["MD", "RM", "SM", "SH", "NA", "AN", "PA", "RA"]`.
    pub digit_suffix_prefixes: &'static [&'static str],
    /// Response prefixes that should be treated as unsolicited AI frames.
    pub ai_prefixes: &'static [&'static str],
    /// Optional command to send on shutdown (e.g. `b"AI0;"` to disable AI mode).
    pub shutdown_command: Option<&'static [u8]>,
}

/// A request sent from rig methods to the IO task.
pub enum Request {
    /// A CAT query command with expected response (GET operations).
    CatCommand {
        cmd_bytes: Vec<u8>,
        reply: oneshot::Sender<Result<(String, String)>>,
    },
    /// A SET command — drain briefly to catch `?;` errors.
    SetCommand {
        cmd_bytes: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Toggle a serial control line (DTR/RTS for hardware PTT/CW).
    SetLine {
        dtr: bool,
        on: bool,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Graceful shutdown; returns the transport for recovery.
    Shutdown {
        reply: oneshot::Sender<Box<dyn Transport>>,
    },
}

/// Callback trait for processing unsolicited AI (Auto Information) frames.
///
/// Each manufacturer backend implements this to parse AI frames into
/// [`RigEvent`]s. The IO task calls [`AiHandler::process`] for every
/// decoded frame whose prefix is in [`IoConfig::ai_prefixes`].
pub trait AiHandler: Send + Sync + 'static {
    fn process(&self, prefix: &str, data: &str, event_tx: &broadcast::Sender<RigEvent>);
}

/// No-op AI handler for AI-off mode and testing.
pub struct NullAiHandler;

impl AiHandler for NullAiHandler {
    fn process(&self, _prefix: &str, _data: &str, _event_tx: &broadcast::Sender<RigEvent>) {}
}

/// Handle to the IO task. Stored inside the rig driver struct.
pub struct RigIo {
    /// Real-time command channel — checked first in the IO loop's biased select.
    pub rt_tx: mpsc::Sender<Request>,
    /// Background command channel — checked after RT in the IO loop.
    pub bg_tx: mpsc::Sender<Request>,
    /// Cancellation token for graceful shutdown.
    pub cancel: CancellationToken,
    /// Join handle for the IO task.
    pub task: JoinHandle<()>,
}

impl RigIo {
    /// Send a CAT query command via the background channel and await the response.
    pub async fn command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<(String, String)> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.bg_tx
            .send(Request::CatCommand {
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

    /// Send a SET command via the background channel and await the drain result.
    pub async fn set_command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.bg_tx
            .send(Request::SetCommand {
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

    /// Send a CAT query command via the real-time channel.
    pub async fn rt_command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<(String, String)> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.rt_tx
            .send(Request::CatCommand {
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

    /// Send a SET command via the real-time channel.
    pub async fn rt_set_command(&self, cmd: Vec<u8>, timeout: Duration) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.rt_tx
            .send(Request::SetCommand {
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

    /// Toggle a serial control line via the real-time channel.
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
// Spawn
// ---------------------------------------------------------------------------

/// Spawn the IO task. Returns the handle for sending commands.
///
/// The IO task owns the transport exclusively and processes all
/// command/response exchanges, SET command drains, and unsolicited
/// AI frame handling.
pub fn spawn_io_task(
    transport: Box<dyn Transport>,
    config: IoConfig,
    event_tx: broadcast::Sender<RigEvent>,
    ai_handler: Box<dyn AiHandler>,
) -> RigIo {
    let (rt_tx, rt_rx) = mpsc::channel::<Request>(32);
    let (bg_tx, bg_rx) = mpsc::channel::<Request>(32);
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    let task = tokio::spawn(io_loop(
        transport, config, event_tx, ai_handler, rt_rx, bg_rx, cancel_clone,
    ));

    RigIo {
        rt_tx,
        bg_tx,
        cancel,
        task,
    }
}

// ---------------------------------------------------------------------------
// IO Loop
// ---------------------------------------------------------------------------

/// Maximum buffer size before reset to prevent unbounded growth.
/// Text-protocol frames are typically 5–20 bytes; 8192 is generous headroom.
const MAX_BUF: usize = 8192;

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
    ai_handler: Box<dyn AiHandler>,
    mut rt_rx: mpsc::Receiver<Request>,
    mut bg_rx: mpsc::Receiver<Request>,
    cancel: CancellationToken,
) {
    let mut idle_buf = Vec::new();

    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                debug!("text IO task cancelled");
                break;
            }

            req = rt_rx.recv() => {
                match req {
                    Some(Request::Shutdown { reply }) => {
                        debug!("IO task shutdown requested (RT)");
                        if let Some(cmd) = config.shutdown_command {
                            let _ = transport.send(cmd).await;
                        }
                        let _ = reply.send(transport);
                        return;
                    }
                    Some(req) => handle_request(
                        req, &mut transport, &config, &*ai_handler, &event_tx,
                    ).await,
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
                        if let Some(cmd) = config.shutdown_command {
                            let _ = transport.send(cmd).await;
                        }
                        let _ = reply.send(transport);
                        return;
                    }
                    Some(req) => handle_request(
                        req, &mut transport, &config, &*ai_handler, &event_tx,
                    ).await,
                    None => {
                        debug!("BG channel closed, exiting IO task");
                        break;
                    }
                }
            }

            // Idle: read unsolicited data from the serial port.
            _ = async {
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
                            return;
                        }
                        if config.ai_enabled {
                            process_idle_frames(
                                &mut idle_buf,
                                &config,
                                &*ai_handler,
                                &event_tx,
                            );
                        } else {
                            drain_idle_frames(&mut idle_buf, &config);
                        }
                    }
                    _ => {
                        // Timeout or error — yield briefly so the loop
                        // can check for commands or cancellation.
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            } => {}
        }
    }
}

/// Dispatch a single request on the transport.
///
/// Shared by both the RT and BG select arms — the IO loop determines
/// priority; this function handles execution.
async fn handle_request(
    req: Request,
    transport: &mut Box<dyn Transport>,
    config: &IoConfig,
    ai_handler: &dyn AiHandler,
    event_tx: &broadcast::Sender<RigEvent>,
) {
    match req {
        Request::CatCommand { cmd_bytes, reply } => {
            let result =
                execute_cat_command(&mut **transport, &cmd_bytes, config, ai_handler, event_tx)
                    .await;
            let _ = reply.send(result);
        }
        Request::SetCommand { cmd_bytes, reply } => {
            let result =
                execute_set_command(&mut **transport, &cmd_bytes, config, ai_handler, event_tx)
                    .await;
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
// Command execution
// ---------------------------------------------------------------------------

/// Execute a CAT query command on the transport.
///
/// Sends the command, reads until a response with a matching prefix is found,
/// handles interleaved AI frames, `?;` errors, and retry with backoff.
async fn execute_cat_command(
    transport: &mut dyn Transport,
    cmd: &[u8],
    config: &IoConfig,
    ai_handler: &dyn AiHandler,
    event_tx: &broadcast::Sender<RigEvent>,
) -> Result<(String, String)> {
    let retries = if config.auto_retry {
        config.max_retries
    } else {
        0
    };
    let expected_prefix =
        protocol::extract_command_prefix(cmd, config.digit_suffix_prefixes);

    for attempt in 0..=retries {
        if attempt > 0 {
            debug!(attempt, "text protocol CAT command retry");
            tokio::time::sleep(Duration::from_millis(20 * attempt as u64)).await;
        }

        transport.send(cmd).await?;

        let mut recv_buf = [0u8; 256];
        let mut response_buf = Vec::new();

        loop {
            match transport.receive(&mut recv_buf, config.command_timeout).await {
                Ok(n) => {
                    response_buf.extend_from_slice(&recv_buf[..n]);

                    // Bounded buffer: prevent unbounded growth from
                    // malformed input or noise on the line.
                    if response_buf.len() > MAX_BUF {
                        tracing::warn!(
                            len = response_buf.len(),
                            "response buffer overflow, clearing and retrying"
                        );
                        response_buf.clear();
                        break;
                    }

                    loop {
                        match protocol::decode_response(
                            &response_buf,
                            config.digit_suffix_prefixes,
                        ) {
                            DecodeResult::Response {
                                prefix,
                                data,
                                consumed,
                            } => {
                                response_buf.drain(..consumed);

                                // Check if this is the response to our command.
                                if prefix == expected_prefix {
                                    return Ok((prefix, data));
                                }

                                // Interleaved AI response — process if enabled.
                                if config.ai_enabled
                                    && config.ai_prefixes.contains(&prefix.as_str())
                                {
                                    ai_handler.process(&prefix, &data, event_tx);
                                    continue;
                                }

                                debug!(
                                    prefix,
                                    data,
                                    expected_prefix,
                                    "skipping unexpected response"
                                );
                            }
                            DecodeResult::Error(consumed) => {
                                response_buf.drain(..consumed);
                                return Err(Error::Protocol(
                                    "rig returned error response (?;)".into(),
                                ));
                            }
                            DecodeResult::Incomplete => break,
                        }
                    }
                }
                Err(Error::Timeout) => {
                    // Transport timed out. Try one more decode pass on
                    // any accumulated partial data.
                    if !response_buf.is_empty() {
                        match protocol::decode_response(
                            &response_buf,
                            config.digit_suffix_prefixes,
                        ) {
                            DecodeResult::Response { prefix, data, .. } => {
                                if prefix == expected_prefix {
                                    return Ok((prefix, data));
                                }
                                if config.ai_enabled
                                    && config.ai_prefixes.contains(&prefix.as_str())
                                {
                                    ai_handler.process(&prefix, &data, event_tx);
                                }
                            }
                            DecodeResult::Error(_) => {
                                return Err(Error::Protocol(
                                    "rig returned error response (?;)".into(),
                                ));
                            }
                            DecodeResult::Incomplete => {}
                        }
                    }
                    break; // Move to next retry attempt.
                }
                Err(e) => return Err(e),
            }
        }
    }

    Err(Error::Timeout)
}

/// Execute a SET command on the transport.
///
/// Sends the command, then drains for `set_drain_timeout` to catch `?;`
/// errors. A timeout with no error response is treated as success.
async fn execute_set_command(
    transport: &mut dyn Transport,
    cmd: &[u8],
    config: &IoConfig,
    ai_handler: &dyn AiHandler,
    event_tx: &broadcast::Sender<RigEvent>,
) -> Result<()> {
    transport.send(cmd).await?;

    let deadline = tokio::time::Instant::now() + config.set_drain_timeout;
    let mut recv_buf = [0u8; 256];
    let mut drain_buf = Vec::new();

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;

        match transport.receive(&mut recv_buf, remaining).await {
            Ok(n) if n > 0 => {
                drain_buf.extend_from_slice(&recv_buf[..n]);

                if drain_buf.len() > MAX_BUF {
                    tracing::warn!("set command drain buffer overflow, clearing");
                    drain_buf.clear();
                    break;
                }

                loop {
                    match protocol::decode_response(
                        &drain_buf,
                        config.digit_suffix_prefixes,
                    ) {
                        DecodeResult::Response {
                            prefix,
                            data,
                            consumed,
                        } => {
                            drain_buf.drain(..consumed);
                            if config.ai_enabled
                                && config.ai_prefixes.contains(&prefix.as_str())
                            {
                                ai_handler.process(&prefix, &data, event_tx);
                            }
                        }
                        DecodeResult::Error(consumed) => {
                            drain_buf.drain(..consumed);
                            return Err(Error::Protocol(
                                "rig returned error response (?;)".into(),
                            ));
                        }
                        DecodeResult::Incomplete => break,
                    }
                }
            }
            _ => break, // Timeout or error — drain is done.
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Idle frame processing
// ---------------------------------------------------------------------------

/// Process complete responses in the idle buffer, emitting AI events.
///
/// Non-AI responses are logged and discarded. Incomplete data is left
/// in the buffer for the next read cycle.
fn process_idle_frames(
    buf: &mut Vec<u8>,
    config: &IoConfig,
    ai_handler: &dyn AiHandler,
    event_tx: &broadcast::Sender<RigEvent>,
) {
    loop {
        match protocol::decode_response(buf, config.digit_suffix_prefixes) {
            DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                buf.drain(..consumed);
                if config.ai_prefixes.contains(&prefix.as_str()) {
                    ai_handler.process(&prefix, &data, event_tx);
                } else {
                    debug!(prefix, data, "ignoring non-AI response in idle read");
                }
            }
            DecodeResult::Error(consumed) => {
                buf.drain(..consumed);
                debug!("error response in idle read, discarding");
            }
            DecodeResult::Incomplete => break,
        }
    }
}

/// Drain complete frames from the idle buffer without emitting events.
///
/// Used when AI mode is disabled to prevent unbounded buffer growth.
fn drain_idle_frames(buf: &mut Vec<u8>, config: &IoConfig) {
    loop {
        match protocol::decode_response(buf, config.digit_suffix_prefixes) {
            DecodeResult::Response { consumed, .. } => {
                buf.drain(..consumed);
            }
            DecodeResult::Error(consumed) => {
                buf.drain(..consumed);
            }
            DecodeResult::Incomplete => break,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_core::types::ReceiverId;
    use riglib_test_harness::MockTransport;

    /// Helper config for Kenwood-style tests (no digit suffixes).
    fn test_config() -> IoConfig {
        IoConfig {
            ai_enabled: false,
            command_timeout: Duration::from_millis(500),
            auto_retry: false,
            max_retries: 0,
            set_drain_timeout: Duration::from_millis(50),
            digit_suffix_prefixes: &[],
            ai_prefixes: &["FA", "FB", "MD", "TX", "RT", "XT"],
            shutdown_command: None,
        }
    }

    /// A test AI handler that emits FrequencyChanged for "FA" prefixes.
    struct TestAiHandler;

    impl AiHandler for TestAiHandler {
        fn process(&self, prefix: &str, data: &str, event_tx: &broadcast::Sender<RigEvent>) {
            if prefix == "FA" {
                if let Ok(freq_hz) = data.parse::<u64>() {
                    let _ = event_tx.send(RigEvent::FrequencyChanged {
                        receiver: ReceiverId::VFO_A,
                        freq_hz,
                    });
                }
            }
        }
    }

    // =======================================================================
    // Type construction tests
    // =======================================================================

    #[test]
    fn io_config_construction() {
        let config = test_config();
        assert!(!config.ai_enabled);
        assert_eq!(config.command_timeout, Duration::from_millis(500));
        assert!(!config.auto_retry);
        assert_eq!(config.max_retries, 0);
        assert_eq!(config.set_drain_timeout, Duration::from_millis(50));
        assert!(config.digit_suffix_prefixes.is_empty());
        assert_eq!(config.ai_prefixes.len(), 6);
        assert!(config.shutdown_command.is_none());
    }

    #[test]
    fn request_cat_command_construction() {
        let (reply_tx, _) = oneshot::channel();
        let cmd = b"FA;".to_vec();
        let request = Request::CatCommand {
            cmd_bytes: cmd.clone(),
            reply: reply_tx,
        };
        match request {
            Request::CatCommand { cmd_bytes, .. } => assert_eq!(cmd_bytes, cmd),
            _ => panic!("expected CatCommand"),
        }
    }

    #[test]
    fn request_set_command_construction() {
        let (reply_tx, _) = oneshot::channel();
        let cmd = b"FA00014074000;".to_vec();
        let request = Request::SetCommand {
            cmd_bytes: cmd.clone(),
            reply: reply_tx,
        };
        match request {
            Request::SetCommand { cmd_bytes, .. } => assert_eq!(cmd_bytes, cmd),
            _ => panic!("expected SetCommand"),
        }
    }

    // =======================================================================
    // RigIo handle tests (channel-level, no IO loop)
    // =======================================================================

    #[tokio::test]
    async fn rig_io_command_not_connected() {
        let (bg_tx, _rx) = mpsc::channel(32);
        drop(_rx);

        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo {
            rt_tx: bg_tx.clone(),
            bg_tx,
            cancel,
            task,
        };
        let result = io.command(b"FA;".to_vec(), Duration::from_millis(100)).await;
        assert!(matches!(result, Err(Error::NotConnected)));
    }

    #[tokio::test]
    async fn rig_io_set_command_not_connected() {
        let (bg_tx, _rx) = mpsc::channel(32);
        drop(_rx);

        let cancel = CancellationToken::new();
        let task = tokio::spawn(async {});

        let io = RigIo {
            rt_tx: bg_tx.clone(),
            bg_tx,
            cancel,
            task,
        };
        let result = io
            .set_command(b"FA00014074000;".to_vec(), Duration::from_millis(100))
            .await;
        assert!(matches!(result, Err(Error::NotConnected)));
    }

    // =======================================================================
    // IO task — CatCommand tests
    // =======================================================================

    #[tokio::test]
    async fn io_task_basic_cat_command() {
        let mut mock = MockTransport::new();
        mock.expect(b"FA;", b"FA00014074000;");

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io.command(b"FA;".to_vec(), Duration::from_millis(500)).await;
        assert!(result.is_ok());
        let (prefix, data) = result.unwrap();
        assert_eq!(prefix, "FA");
        assert_eq!(data, "00014074000");

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_cat_command_error_response() {
        let mut mock = MockTransport::new();
        mock.expect(b"FA;", b"?;");

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io.command(b"FA;".to_vec(), Duration::from_millis(500)).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protocol(_)));

        let _ = io.shutdown().await;
    }

    // =======================================================================
    // IO task — SetCommand tests
    // =======================================================================

    #[tokio::test]
    async fn io_task_set_command_success() {
        let mut mock = MockTransport::new();
        // SET with no error — empty response means timeout during drain = success.
        mock.expect(b"FA00014074000;", b"");

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io
            .set_command(b"FA00014074000;".to_vec(), Duration::from_millis(500))
            .await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_set_command_catches_error() {
        let mut mock = MockTransport::new();
        mock.expect(b"FA99999999999;", b"?;");

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io
            .set_command(b"FA99999999999;".to_vec(), Duration::from_millis(500))
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protocol(_)));

        let _ = io.shutdown().await;
    }

    // =======================================================================
    // IO task — SetLine tests
    // =======================================================================

    #[tokio::test]
    async fn io_task_set_line_dtr() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io.set_line(true, true).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_set_line_rts() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io.set_line(false, true).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    // =======================================================================
    // IO task — RT channel tests
    // =======================================================================

    #[tokio::test]
    async fn io_task_rt_cat_command() {
        let mut mock = MockTransport::new();
        mock.expect(b"FA;", b"FA00014074000;");

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io
            .rt_command(b"FA;".to_vec(), Duration::from_millis(500))
            .await;
        assert!(result.is_ok());
        let (prefix, data) = result.unwrap();
        assert_eq!(prefix, "FA");
        assert_eq!(data, "00014074000");

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_rt_set_command() {
        let mut mock = MockTransport::new();
        mock.expect(b"FA00014074000;", b"");

        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io
            .rt_set_command(b"FA00014074000;".to_vec(), Duration::from_millis(500))
            .await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    #[tokio::test]
    async fn io_task_rt_set_line_dtr() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io.rt_set_line(true, true).await;
        assert!(result.is_ok());

        let _ = io.shutdown().await;
    }

    // =======================================================================
    // IO task — RT priority over BG
    // =======================================================================

    #[tokio::test]
    async fn io_task_rt_priority_over_bg() {
        let mut mock = MockTransport::new();

        // Expectations in RT-first order.
        // RT command: FA query.
        mock.expect(b"FA;", b"FA00014074000;");
        // BG commands: MD query x3.
        mock.expect(b"MD;", b"MD3;");
        mock.expect(b"MD;", b"MD3;");
        mock.expect(b"MD;", b"MD3;");

        let (event_tx, _) = broadcast::channel(16);
        let cancel = CancellationToken::new();
        let (rt_tx, rt_rx) = mpsc::channel::<Request>(32);
        let (bg_tx, bg_rx) = mpsc::channel::<Request>(32);

        // Pre-fill BG channel with 3 commands BEFORE starting the IO loop.
        let mut bg_replies = Vec::new();
        for _ in 0..3 {
            let (reply_tx, reply_rx) = oneshot::channel();
            bg_tx
                .send(Request::CatCommand {
                    cmd_bytes: b"MD;".to_vec(),
                    reply: reply_tx,
                })
                .await
                .unwrap();
            bg_replies.push(reply_rx);
        }

        // Pre-fill RT channel with 1 command.
        let (rt_reply_tx, rt_reply_rx) = oneshot::channel();
        rt_tx
            .send(Request::CatCommand {
                cmd_bytes: b"FA;".to_vec(),
                reply: rt_reply_tx,
            })
            .await
            .unwrap();

        // Spawn the IO loop directly to test biased select behavior.
        let task = tokio::spawn(io_loop(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
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
    // IO task — Interleaved AI frame during CatCommand
    // =======================================================================

    #[tokio::test]
    async fn io_task_interleaved_ai_frame() {
        let mut mock = MockTransport::new();

        // Response: unsolicited AI frequency update + actual MD response.
        mock.expect(b"MD;", b"FA00014074000;MD3;");

        let (event_tx, mut event_rx) = broadcast::channel(16);
        let config = IoConfig {
            ai_enabled: true,
            ..test_config()
        };
        let io = spawn_io_task(
            Box::new(mock),
            config,
            event_tx,
            Box::new(TestAiHandler),
        );

        let result = io.command(b"MD;".to_vec(), Duration::from_millis(500)).await;
        assert!(result.is_ok());
        let (prefix, data) = result.unwrap();
        assert_eq!(prefix, "MD");
        assert_eq!(data, "3");

        // Verify the unsolicited frequency change was emitted.
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
    // IO task — Shutdown
    // =======================================================================

    #[tokio::test]
    async fn io_task_shutdown_recovers_transport() {
        let mock = MockTransport::new();
        let (event_tx, _) = broadcast::channel(16);
        let io = spawn_io_task(
            Box::new(mock),
            test_config(),
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io.shutdown().await;
        assert!(result.is_ok());
        let transport = result.unwrap();
        assert!(transport.is_connected());
    }

    #[tokio::test]
    async fn io_task_shutdown_sends_command() {
        let mut mock = MockTransport::new();
        // The shutdown command "AI0;" will be sent, so mock needs to expect it.
        // After send, the transport is returned — no receive expected.
        mock.expect(b"AI0;", b"");

        let (event_tx, _) = broadcast::channel(16);
        let config = IoConfig {
            shutdown_command: Some(b"AI0;"),
            ..test_config()
        };
        let io = spawn_io_task(
            Box::new(mock),
            config,
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io.shutdown().await;
        assert!(result.is_ok());
    }

    // =======================================================================
    // IO task — Response buffer overflow resyncs
    // =======================================================================

    #[tokio::test]
    async fn io_task_response_buffer_overflow_resyncs() {
        let mut mock = MockTransport::new();

        // First attempt: 9000 bytes of garbage (no terminator) → overflow.
        let garbage = vec![b'A'; 9000];
        mock.expect(b"FA;", &garbage);

        // Second attempt (retry): valid response.
        mock.expect(b"FA;", b"FA00014074000;");

        let (event_tx, _) = broadcast::channel(16);
        let config = IoConfig {
            auto_retry: true,
            max_retries: 1,
            ..test_config()
        };
        let io = spawn_io_task(
            Box::new(mock),
            config,
            event_tx,
            Box::new(NullAiHandler),
        );

        let result = io.command(b"FA;".to_vec(), Duration::from_millis(500)).await;
        assert!(result.is_ok(), "expected success after resync, got {result:?}");
        let (prefix, data) = result.unwrap();
        assert_eq!(prefix, "FA");
        assert_eq!(data, "00014074000");

        let _ = io.shutdown().await;
    }

    // =======================================================================
    // Idle frame processing (direct function tests)
    // =======================================================================

    #[test]
    fn process_idle_frames_emits_ai_events() {
        let config = IoConfig {
            ai_enabled: true,
            ..test_config()
        };
        let handler = TestAiHandler;
        let (event_tx, mut event_rx) = broadcast::channel(16);

        let mut buf = b"FA00014074000;MD3;".to_vec();
        process_idle_frames(&mut buf, &config, &handler, &event_tx);

        // FA is handled by TestAiHandler → FrequencyChanged event.
        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::FrequencyChanged { freq_hz, .. } => {
                assert_eq!(freq_hz, 14_074_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }

        // MD is not handled by TestAiHandler but is an AI prefix → no event.
        // (TestAiHandler only handles FA.)
        assert!(event_rx.try_recv().is_err());

        // Buffer should be fully consumed.
        assert!(buf.is_empty());
    }

    #[test]
    fn process_idle_frames_ignores_non_ai() {
        let config = IoConfig {
            ai_enabled: true,
            ..test_config()
        };
        let handler = NullAiHandler;
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // PC is not in ai_prefixes.
        let mut buf = b"PC050;".to_vec();
        process_idle_frames(&mut buf, &config, &handler, &event_tx);

        assert!(event_rx.try_recv().is_err());
        assert!(buf.is_empty());
    }

    #[test]
    fn drain_idle_frames_consumes_complete() {
        let config = test_config();
        let mut buf = b"FA00014074000;MD3;".to_vec();
        drain_idle_frames(&mut buf, &config);
        assert!(buf.is_empty());
    }

    #[test]
    fn drain_idle_frames_preserves_incomplete() {
        let config = test_config();
        let mut buf = b"FA00014074000;MD".to_vec();
        drain_idle_frames(&mut buf, &config);
        assert_eq!(buf, b"MD");
    }
}
