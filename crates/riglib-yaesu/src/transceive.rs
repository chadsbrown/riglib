//! Yaesu AI (Auto Information) mode transceive listener.
//!
//! When AI mode is enabled on a Yaesu rig (`AI2;`), the rig pushes unsolicited
//! state-change messages like `FA014074000;`, `MD02;`, `TX1;`, `RT01+0050;` etc.
//! These are standard semicolon-terminated CAT responses, identical in format to
//! polled responses.
//!
//! This module provides a background reader task that captures those unsolicited
//! messages and emits them as [`RigEvent`]s. The background task also multiplexes
//! command/response traffic: commands are sent via an `mpsc` channel and responses
//! returned via `oneshot`.
//!
//! Compared to the Icom CI-V transceive module, this is significantly simpler:
//! - No echo frames (point-to-point serial, not a bus)
//! - No collision detection (no shared bus)
//! - No BCD encoding (everything is ASCII text)
//! - Response format is `prefix + data + ;`, decoded by `protocol::decode_response()`
//!
//! Key differences from the Kenwood transceive module:
//! - Yaesu uses `MD0`/`MD1` prefixes (not bare `MD`) for per-VFO mode
//! - Yaesu uses `RT0`/`XT0` prefixes (not bare `RT`/`XT`) for RIT/XIT
//! - Yaesu RIT/XIT responses include both on/off state and signed offset
//! - Yaesu frequencies are 9 digits (not 11)
//! - The `extract_command_prefix` must mirror the protocol decoder's
//!   `DIGIT_SUFFIX_PREFIXES` logic

use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::debug;

use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;
use riglib_core::transport::Transport;
use riglib_core::types::*;

use crate::commands;
use crate::protocol;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A request sent from the rig to the reader task.
pub(crate) enum CommandRequest {
    /// A CAT command to be forwarded to the transport.
    CatCommand {
        cmd_bytes: Vec<u8>,
        response_tx: oneshot::Sender<Result<(String, String)>>,
    },
    /// Set the DTR serial line state.
    SetDtr {
        on: bool,
        response_tx: oneshot::Sender<Result<()>>,
    },
    /// Set the RTS serial line state.
    SetRts {
        on: bool,
        response_tx: oneshot::Sender<Result<()>>,
    },
}

/// Handle to the background transceive reader task.
pub(crate) struct TransceiveHandle {
    pub cmd_tx: mpsc::Sender<CommandRequest>,
    /// Kept so the task can be aborted when the rig is dropped.
    #[allow(dead_code)]
    pub task_handle: JoinHandle<()>,
}

/// Configuration for the reader task's retry/timeout behavior.
struct ReaderConfig {
    auto_retry: bool,
    max_retries: u32,
    command_timeout: Duration,
}

// ---------------------------------------------------------------------------
// DisconnectedTransport sentinel
// ---------------------------------------------------------------------------

/// Sentinel transport placed into the `Arc<Mutex<>>` after the real
/// transport has been moved to the background reader task.
pub(crate) struct DisconnectedTransport;

#[async_trait::async_trait]
impl Transport for DisconnectedTransport {
    async fn send(&mut self, _data: &[u8]) -> Result<()> {
        Err(Error::NotConnected)
    }

    async fn receive(&mut self, _buf: &mut [u8], _timeout: Duration) -> Result<usize> {
        Err(Error::NotConnected)
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// AI prefix classification
// ---------------------------------------------------------------------------

/// Known AI (Auto Information) prefixes that Yaesu pushes unsolicited.
///
/// Note: Yaesu uses `MD0`/`MD1` (not bare `MD`) and `RT0`/`XT0` (not bare
/// `RT`/`XT`) because the protocol decoder's `DIGIT_SUFFIX_PREFIXES` absorbs
/// the trailing digit into the prefix.
const AI_PREFIXES: &[&str] = &["FA", "FB", "MD0", "MD1", "TX", "RT0", "XT0"];

/// Returns `true` if the given response prefix is a known AI auto-information
/// prefix that should be processed as an unsolicited state change.
fn is_ai_prefix(prefix: &str) -> bool {
    AI_PREFIXES.contains(&prefix)
}

/// Process a single AI response and emit the appropriate [`RigEvent`].
fn process_ai_response(prefix: &str, data: &str, event_tx: &broadcast::Sender<RigEvent>) {
    match prefix {
        "FA" => match commands::parse_frequency_response(data) {
            Ok(freq_hz) => {
                debug!(freq_hz, "AI frequency update (VFO A)");
                let _ = event_tx.send(RigEvent::FrequencyChanged {
                    receiver: ReceiverId::VFO_A,
                    freq_hz,
                });
            }
            Err(e) => {
                debug!(?e, "failed to parse AI frequency response (FA)");
            }
        },
        "FB" => match commands::parse_frequency_response(data) {
            Ok(freq_hz) => {
                debug!(freq_hz, "AI frequency update (VFO B)");
                let _ = event_tx.send(RigEvent::FrequencyChanged {
                    receiver: ReceiverId::VFO_B,
                    freq_hz,
                });
            }
            Err(e) => {
                debug!(?e, "failed to parse AI frequency response (FB)");
            }
        },
        "MD0" => match commands::parse_mode_response(data) {
            Ok(mode) => {
                debug!(%mode, "AI mode update (VFO A)");
                let _ = event_tx.send(RigEvent::ModeChanged {
                    receiver: ReceiverId::VFO_A,
                    mode,
                });
            }
            Err(e) => {
                debug!(?e, "failed to parse AI mode response (MD0)");
            }
        },
        "MD1" => match commands::parse_mode_response(data) {
            Ok(mode) => {
                debug!(%mode, "AI mode update (VFO B)");
                let _ = event_tx.send(RigEvent::ModeChanged {
                    receiver: ReceiverId::VFO_B,
                    mode,
                });
            }
            Err(e) => {
                debug!(?e, "failed to parse AI mode response (MD1)");
            }
        },
        "TX" => match commands::parse_ptt_response(data) {
            Ok(on) => {
                debug!(on, "AI PTT update");
                let _ = event_tx.send(RigEvent::PttChanged { on });
            }
            Err(e) => {
                debug!(?e, "failed to parse AI PTT response");
            }
        },
        "RT0" => match commands::parse_rit_response(data) {
            Ok((enabled, offset_hz)) => {
                debug!(enabled, offset_hz, "AI RIT update");
                let _ = event_tx.send(RigEvent::RitChanged {
                    enabled,
                    offset_hz,
                });
            }
            Err(e) => {
                debug!(?e, "failed to parse AI RIT response");
            }
        },
        "XT0" => match commands::parse_xit_response(data) {
            Ok((enabled, offset_hz)) => {
                debug!(enabled, offset_hz, "AI XIT update");
                let _ = event_tx.send(RigEvent::XitChanged {
                    enabled,
                    offset_hz,
                });
            }
            Err(e) => {
                debug!(?e, "failed to parse AI XIT response");
            }
        },
        _ => {
            debug!(prefix, data, "unknown AI prefix, ignoring");
        }
    }
}

/// Drain all complete semicolon-terminated responses from a buffer,
/// processing any that match known AI prefixes.
///
/// Incomplete data is left in the buffer for the next read cycle.
fn process_ai_responses(buf: &mut Vec<u8>, event_tx: &broadcast::Sender<RigEvent>) {
    loop {
        match protocol::decode_response(buf) {
            protocol::DecodeResult::Response {
                prefix,
                data,
                consumed,
            } => {
                buf.drain(..consumed);
                if is_ai_prefix(&prefix) {
                    process_ai_response(&prefix, &data, event_tx);
                } else {
                    debug!(prefix, data, "ignoring non-AI response in idle read");
                }
            }
            protocol::DecodeResult::Error(consumed) => {
                buf.drain(..consumed);
                debug!("error response in idle read, discarding");
            }
            protocol::DecodeResult::Incomplete => break,
        }
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn the background reader task.
///
/// The task owns the transport exclusively. Commands are sent via the
/// returned `TransceiveHandle.cmd_tx` channel; unsolicited AI responses
/// are parsed and emitted to `event_tx`.
pub(crate) fn spawn_reader_task(
    transport: Box<dyn Transport>,
    event_tx: broadcast::Sender<RigEvent>,
    auto_retry: bool,
    max_retries: u32,
    command_timeout: Duration,
) -> TransceiveHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<CommandRequest>(16);

    let config = ReaderConfig {
        auto_retry,
        max_retries,
        command_timeout,
    };

    let task_handle = tokio::spawn(reader_loop(transport, config, event_tx, cmd_rx));

    TransceiveHandle {
        cmd_tx,
        task_handle,
    }
}

// ---------------------------------------------------------------------------
// Reader loop
// ---------------------------------------------------------------------------

/// The main loop of the background reader task.
///
/// Uses `tokio::select! { biased; }` to prioritize command handling over
/// idle AI response reading.
async fn reader_loop(
    mut transport: Box<dyn Transport>,
    config: ReaderConfig,
    event_tx: broadcast::Sender<RigEvent>,
    mut cmd_rx: mpsc::Receiver<CommandRequest>,
) {
    let mut idle_buf = Vec::new();

    loop {
        tokio::select! {
            biased;

            // Priority: handle outgoing commands.
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(CommandRequest::CatCommand { cmd_bytes, response_tx }) => {
                        let result = execute_command_on_transport(
                            &mut *transport,
                            &cmd_bytes,
                            &config,
                            &event_tx,
                        )
                        .await;
                        let _ = response_tx.send(result);
                    }
                    Some(CommandRequest::SetDtr { on, response_tx }) => {
                        let result = transport.set_dtr(on).await;
                        let _ = response_tx.send(result);
                    }
                    Some(CommandRequest::SetRts { on, response_tx }) => {
                        let result = transport.set_rts(on).await;
                        let _ = response_tx.send(result);
                    }
                    None => {
                        // All senders dropped -- YaesuRig was dropped.
                        debug!("AI transceive command channel closed, exiting reader loop");
                        break;
                    }
                }
            }

            // Idle: read unsolicited AI responses from the rig.
            _ = async {
                let mut buf = [0u8; 256];
                match transport.receive(&mut buf, Duration::from_millis(100)).await {
                    Ok(n) if n > 0 => {
                        idle_buf.extend_from_slice(&buf[..n]);
                        process_ai_responses(&mut idle_buf, &event_tx);
                    }
                    _ => {
                        // Timeout or error -- just loop back.
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Command execution (inside the reader task)
// ---------------------------------------------------------------------------

/// Extract the expected command prefix from a command byte sequence.
///
/// The prefix is the leading alphabetic characters of the command, plus any
/// trailing digit that is part of the command name for known
/// `DIGIT_SUFFIX_PREFIXES`. This mirrors the prefix splitting logic in
/// `protocol::decode_response()`. For example:
/// - `FA014074000;` -> `"FA"`
/// - `MD02;` -> `"MD0"`
/// - `TX;` -> `"TX"`
/// - `RT01+0050;` -> `"RT0"`
/// - `SM0;` -> `"SM0"`
fn extract_command_prefix(cmd: &[u8]) -> String {
    let s = std::str::from_utf8(cmd).unwrap_or("");
    let alpha_end = s
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    let alpha_prefix = &s[..alpha_end];

    const DIGIT_SUFFIX_PREFIXES: &[&str] =
        &["MD", "RM", "SM", "SH", "NA", "AN", "PA", "RA", "RT", "XT"];

    if DIGIT_SUFFIX_PREFIXES.contains(&alpha_prefix)
        && alpha_end < s.len()
        && s.as_bytes()[alpha_end].is_ascii_digit()
    {
        s[..alpha_end + 1].to_string()
    } else {
        alpha_prefix.to_string()
    }
}

/// Execute a CAT command on the transport, handling retry and interleaved
/// AI responses.
///
/// This is the transceive-aware equivalent of `YaesuRig::execute_command`.
/// Much simpler than the Icom version: no echo frames, no collision detection,
/// no BCD encoding.
async fn execute_command_on_transport(
    transport: &mut dyn Transport,
    cmd: &[u8],
    config: &ReaderConfig,
    event_tx: &broadcast::Sender<RigEvent>,
) -> Result<(String, String)> {
    let retries = if config.auto_retry {
        config.max_retries
    } else {
        0
    };
    let expected_prefix = extract_command_prefix(cmd);

    for attempt in 0..=retries {
        if attempt > 0 {
            debug!(attempt, "Yaesu CAT command retry (AI mode)");
            tokio::time::sleep(Duration::from_millis(20 * attempt as u64)).await;
        }

        transport.send(cmd).await?;

        let mut buf = [0u8; 256];
        let mut response_buf = Vec::new();

        loop {
            match tokio::time::timeout(
                config.command_timeout,
                transport.receive(&mut buf, config.command_timeout),
            )
            .await
            {
                Ok(Ok(n)) => {
                    response_buf.extend_from_slice(&buf[..n]);

                    // Try to decode responses from accumulated data.
                    loop {
                        match protocol::decode_response(&response_buf) {
                            protocol::DecodeResult::Response {
                                prefix,
                                data,
                                consumed,
                            } => {
                                response_buf.drain(..consumed);

                                // Check if this is the response to our command.
                                if prefix == expected_prefix {
                                    return Ok((prefix, data));
                                }

                                // It is an interleaved AI response -- process it
                                // and continue waiting for our response.
                                if is_ai_prefix(&prefix) {
                                    process_ai_response(&prefix, &data, event_tx);
                                } else {
                                    debug!(
                                        prefix,
                                        data,
                                        "skipping unexpected response while waiting for {expected_prefix}"
                                    );
                                }
                            }
                            protocol::DecodeResult::Error(consumed) => {
                                response_buf.drain(..consumed);
                                return Err(Error::Protocol(
                                    "rig returned error response (?;)".into(),
                                ));
                            }
                            protocol::DecodeResult::Incomplete => {
                                // Need more data, continue reading.
                                break;
                            }
                        }
                    }
                }
                Ok(Err(Error::Timeout)) => {
                    // Transport timed out. Try to decode what we have.
                    if !response_buf.is_empty() {
                        match protocol::decode_response(&response_buf) {
                            protocol::DecodeResult::Response { prefix, data, .. } => {
                                if prefix == expected_prefix {
                                    return Ok((prefix, data));
                                }
                                // Interleaved AI -- process and fall through to retry.
                                if is_ai_prefix(&prefix) {
                                    process_ai_response(&prefix, &data, event_tx);
                                }
                            }
                            protocol::DecodeResult::Error(_) => {
                                return Err(Error::Protocol(
                                    "rig returned error response (?;)".into(),
                                ));
                            }
                            protocol::DecodeResult::Incomplete => {}
                        }
                    }
                    break; // Move to next retry attempt.
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    // tokio::time::timeout expired.
                    if !response_buf.is_empty() {
                        match protocol::decode_response(&response_buf) {
                            protocol::DecodeResult::Response { prefix, data, .. } => {
                                if prefix == expected_prefix {
                                    return Ok((prefix, data));
                                }
                                if is_ai_prefix(&prefix) {
                                    process_ai_response(&prefix, &data, event_tx);
                                }
                            }
                            protocol::DecodeResult::Error(_) => {
                                return Err(Error::Protocol(
                                    "rig returned error response (?;)".into(),
                                ));
                            }
                            protocol::DecodeResult::Incomplete => {}
                        }
                    }
                    break; // Move to next retry attempt.
                }
            }
        }
    }

    Err(Error::Timeout)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // is_ai_prefix
    // -------------------------------------------------------------------

    #[test]
    fn test_is_ai_prefix_known() {
        assert!(is_ai_prefix("FA"));
        assert!(is_ai_prefix("FB"));
        assert!(is_ai_prefix("MD0"));
        assert!(is_ai_prefix("MD1"));
        assert!(is_ai_prefix("TX"));
        assert!(is_ai_prefix("RT0"));
        assert!(is_ai_prefix("XT0"));
    }

    #[test]
    fn test_is_ai_prefix_unknown() {
        assert!(!is_ai_prefix("PC"));
        assert!(!is_ai_prefix("SM0"));
        assert!(!is_ai_prefix("AI"));
        assert!(!is_ai_prefix("FR"));
        assert!(!is_ai_prefix("FT"));
        assert!(!is_ai_prefix("KS"));
    }

    // -------------------------------------------------------------------
    // process_ai_response -- frequency
    // -------------------------------------------------------------------

    #[test]
    fn test_process_ai_frequency_a() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        process_ai_response("FA", "014074000", &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::FrequencyChanged { receiver, freq_hz } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(freq_hz, 14_074_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }
    }

    #[test]
    fn test_process_ai_frequency_b() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        process_ai_response("FB", "007000000", &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::FrequencyChanged { receiver, freq_hz } => {
                assert_eq!(receiver, ReceiverId::VFO_B);
                assert_eq!(freq_hz, 7_000_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // process_ai_response -- mode
    // -------------------------------------------------------------------

    #[test]
    fn test_process_ai_mode() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        process_ai_response("MD0", "3", &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::ModeChanged { receiver, mode } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(mode, Mode::CW);
            }
            other => panic!("expected ModeChanged, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // process_ai_response -- PTT
    // -------------------------------------------------------------------

    #[test]
    fn test_process_ai_ptt_on() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        process_ai_response("TX", "1", &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::PttChanged { on } => {
                assert!(on);
            }
            other => panic!("expected PttChanged, got {other:?}"),
        }
    }

    #[test]
    fn test_process_ai_ptt_off() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        process_ai_response("TX", "0", &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::PttChanged { on } => {
                assert!(!on);
            }
            other => panic!("expected PttChanged, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // process_ai_response -- RIT / XIT
    // -------------------------------------------------------------------

    #[test]
    fn test_process_ai_rit() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        process_ai_response("RT0", "1+0050", &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::RitChanged {
                enabled,
                offset_hz,
            } => {
                assert!(enabled);
                assert_eq!(offset_hz, 50);
            }
            other => panic!("expected RitChanged, got {other:?}"),
        }
    }

    #[test]
    fn test_process_ai_xit() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        process_ai_response("XT0", "0+0000", &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::XitChanged {
                enabled,
                offset_hz,
            } => {
                assert!(!enabled);
                assert_eq!(offset_hz, 0);
            }
            other => panic!("expected XitChanged, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // process_ai_responses -- buffer draining
    // -------------------------------------------------------------------

    #[test]
    fn test_process_ai_multiple() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Two AI responses in one buffer.
        let mut buf = b"FA014074000;MD02;".to_vec();
        process_ai_responses(&mut buf, &event_tx);

        // First event: frequency.
        match event_rx.try_recv().unwrap() {
            RigEvent::FrequencyChanged { freq_hz, .. } => {
                assert_eq!(freq_hz, 14_074_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }

        // Second event: mode (MD0 prefix, data "2" = USB).
        match event_rx.try_recv().unwrap() {
            RigEvent::ModeChanged { mode, .. } => {
                assert_eq!(mode, Mode::USB);
            }
            other => panic!("expected ModeChanged, got {other:?}"),
        }

        assert!(buf.is_empty());
    }

    #[test]
    fn test_process_ai_incomplete() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Partial response -- no terminator yet.
        let mut buf = b"FA0140740".to_vec();
        let original_len = buf.len();
        process_ai_responses(&mut buf, &event_tx);

        // No event should be emitted.
        assert!(event_rx.try_recv().is_err());
        // Buffer should be preserved for next read.
        assert_eq!(buf.len(), original_len);
    }

    #[test]
    fn test_process_ai_ignores_non_ai() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // PC050; is a power command response, not an AI prefix.
        let mut buf = b"PC050;".to_vec();
        process_ai_responses(&mut buf, &event_tx);

        // No event should be emitted.
        assert!(event_rx.try_recv().is_err());
        // Buffer should be consumed (response was decoded, just ignored).
        assert!(buf.is_empty());
    }

    // -------------------------------------------------------------------
    // extract_command_prefix
    // -------------------------------------------------------------------

    #[test]
    fn test_extract_command_prefix_fa() {
        assert_eq!(extract_command_prefix(b"FA014074000;"), "FA");
    }

    #[test]
    fn test_extract_command_prefix_md0() {
        assert_eq!(extract_command_prefix(b"MD02;"), "MD0");
    }

    #[test]
    fn test_extract_command_prefix_tx() {
        assert_eq!(extract_command_prefix(b"TX;"), "TX");
    }

    #[test]
    fn test_extract_command_prefix_rt0() {
        assert_eq!(extract_command_prefix(b"RT0;"), "RT0");
    }

    #[test]
    fn test_extract_command_prefix_sm0() {
        assert_eq!(extract_command_prefix(b"SM0;"), "SM0");
    }
}
