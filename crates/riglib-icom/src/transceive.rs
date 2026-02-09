//! CI-V transceive listener for unsolicited rig broadcasts.
//!
//! When CI-V Transceive is enabled on an Icom radio, the rig broadcasts
//! frequency and mode changes to all CI-V bus participants without being
//! asked. This module provides a background reader task that captures
//! those broadcasts and emits them as [`RigEvent`]s.
//!
//! The background task also multiplexes command/response traffic: commands
//! are sent via an `mpsc` channel and responses returned via `oneshot`.

use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::debug;

use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;
use riglib_core::transport::Transport;
use riglib_core::types::*;

use crate::civ::{self, CONTROLLER_ADDR, CivFrame, DecodeResult};
use crate::commands;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// CI-V broadcast destination address (all devices on the bus).
const BROADCAST_ADDR: u8 = 0x00;

/// CI-V transceive command for frequency data (cmd 0x00).
const CMD_TRANSCEIVE_FREQ: u8 = 0x00;

/// CI-V transceive command for mode data (cmd 0x01).
const CMD_TRANSCEIVE_MODE: u8 = 0x01;

/// CI-V RIT/XIT control (cmd 0x21) -- also broadcast in transceive mode.
const CMD_TRANSCEIVE_RIT_XIT: u8 = 0x21;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A request sent from the rig to the reader task.
pub(crate) enum CommandRequest {
    /// A CI-V command to be forwarded to the transport.
    CivCommand {
        cmd_bytes: Vec<u8>,
        response_tx: oneshot::Sender<Result<CivFrame>>,
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
    /// Shut down the reader loop and return transport ownership.
    Shutdown {
        transport_tx: oneshot::Sender<Box<dyn Transport>>,
    },
}

/// Handle to the background transceive reader task.
pub(crate) struct TransceiveHandle {
    pub cmd_tx: mpsc::Sender<CommandRequest>,
    /// Kept so the task can be joined on shutdown or aborted when the rig is dropped.
    pub task_handle: JoinHandle<()>,
}

impl TransceiveHandle {
    /// Shut down the background reader task and recover the transport.
    ///
    /// Sends a `Shutdown` request to the reader loop, waits for the transport
    /// to be returned via a oneshot channel, then joins the task.
    pub(crate) async fn shutdown(self) -> Result<Box<dyn Transport>> {
        let (transport_tx, transport_rx) = oneshot::channel();
        // Don't care if send fails -- reader might have already exited.
        let _ = self
            .cmd_tx
            .send(CommandRequest::Shutdown { transport_tx })
            .await;
        let transport = transport_rx.await.map_err(|_| Error::NotConnected)?;
        // Wait for the task to finish.
        let _ = self.task_handle.await;
        Ok(transport)
    }
}

/// Configuration for the reader task's retry/timeout behavior.
struct ReaderConfig {
    civ_address: u8,
    auto_retry: bool,
    max_retries: u32,
    collision_recovery: bool,
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
// Shared helpers
// ---------------------------------------------------------------------------

/// Reassemble the full payload from a decoded [`CivFrame`].
///
/// The generic CI-V decoder splits the first payload byte after the command
/// into `sub_cmd` and puts the rest in `data`. For many response types
/// (frequency, mode) we need the complete payload as a contiguous slice.
pub(crate) fn reassemble_payload(frame: &CivFrame) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + frame.data.len());
    if let Some(sub) = frame.sub_cmd {
        payload.push(sub);
    }
    payload.extend_from_slice(&frame.data);
    payload
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn the background reader task.
///
/// The task owns the transport exclusively. Commands are sent via the
/// returned `TransceiveHandle.cmd_tx` channel; unsolicited transceive
/// frames are parsed and emitted to `event_tx`.
pub(crate) fn spawn_reader_task(
    transport: Box<dyn Transport>,
    civ_address: u8,
    event_tx: broadcast::Sender<RigEvent>,
    auto_retry: bool,
    max_retries: u32,
    collision_recovery: bool,
    command_timeout: Duration,
) -> TransceiveHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<CommandRequest>(16);

    let config = ReaderConfig {
        civ_address,
        auto_retry,
        max_retries,
        collision_recovery,
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
/// idle transceive frame reading.
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
                    Some(CommandRequest::CivCommand { cmd_bytes, response_tx }) => {
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
                    Some(CommandRequest::Shutdown { transport_tx }) => {
                        // CI-V transceive is a rig menu setting, no off command to send.
                        debug!("shutdown requested, returning transport");
                        let _ = transport_tx.send(transport);
                        break;
                    }
                    None => {
                        // All senders dropped -- IcomRig was dropped.
                        debug!("transceive command channel closed, exiting reader loop");
                        break;
                    }
                }
            }

            // Idle: read transceive frames from the bus.
            _ = async {
                let mut buf = [0u8; 256];
                match transport.receive(&mut buf, Duration::from_millis(100)).await {
                    Ok(n) if n > 0 => {
                        idle_buf.extend_from_slice(&buf[..n]);
                        process_transceive_frames(&mut idle_buf, config.civ_address, &event_tx);
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

/// Execute a CI-V command on the transport, handling echo, collision,
/// retry, and interleaved transceive frames.
///
/// This is the transceive-aware equivalent of `IcomRig::execute_command`.
async fn execute_command_on_transport(
    transport: &mut dyn Transport,
    cmd: &[u8],
    config: &ReaderConfig,
    event_tx: &broadcast::Sender<RigEvent>,
) -> Result<CivFrame> {
    let retries = if config.auto_retry {
        config.max_retries
    } else {
        0
    };
    let civ_address = config.civ_address;

    for attempt in 0..=retries {
        if attempt > 0 {
            debug!(attempt, "CI-V command retry (transceive mode)");
            tokio::time::sleep(Duration::from_millis(20 * attempt as u64)).await;
        }

        transport.send(cmd).await?;

        let mut buf = [0u8; 256];
        let mut response_buf = Vec::new();

        loop {
            match transport.receive(&mut buf, config.command_timeout).await {
                Ok(n) => {
                    response_buf.extend_from_slice(&buf[..n]);

                    loop {
                        match civ::decode_frame(&response_buf) {
                            DecodeResult::Frame(frame, consumed) => {
                                response_buf.drain(..consumed);

                                // Skip echo of our own command.
                                if frame.dst_addr == civ_address
                                    && frame.src_addr == CONTROLLER_ADDR
                                {
                                    debug!("skipping CI-V echo frame (transceive mode)");
                                    continue;
                                }

                                // Actual response from the rig to us.
                                if frame.dst_addr == CONTROLLER_ADDR
                                    && frame.src_addr == civ_address
                                {
                                    if frame.is_nak() {
                                        return Err(Error::Protocol("rig returned NAK".into()));
                                    }
                                    return Ok(frame);
                                }

                                // Interleaved transceive broadcast -- emit as event.
                                if is_transceive_frame(&frame, civ_address) {
                                    process_single_transceive_frame(&frame, civ_address, event_tx);
                                    continue;
                                }

                                debug!(
                                    dst = frame.dst_addr,
                                    src = frame.src_addr,
                                    "skipping CI-V frame from unexpected address (transceive mode)"
                                );
                            }
                            DecodeResult::Incomplete => break,
                            DecodeResult::Collision(consumed) => {
                                response_buf.drain(..consumed);
                                if config.collision_recovery {
                                    debug!("CI-V collision detected (transceive mode), will retry");
                                    break;
                                }
                                return Err(Error::Protocol("CI-V bus collision".into()));
                            }
                        }
                    }

                    if response_buf.contains(&civ::COLLISION) {
                        break;
                    }
                }
                Err(Error::Timeout) => {
                    if !response_buf.is_empty() {
                        if let DecodeResult::Frame(frame, _) = civ::decode_frame(&response_buf) {
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

// ---------------------------------------------------------------------------
// Transceive frame processing
// ---------------------------------------------------------------------------

/// Returns true if the frame looks like a transceive broadcast from the rig.
pub(crate) fn is_transceive_frame(frame: &CivFrame, civ_address: u8) -> bool {
    // Transceive frames are sent from the rig to either the broadcast
    // address (0x00) or to the controller address.
    frame.src_addr == civ_address
        && (frame.dst_addr == BROADCAST_ADDR || frame.dst_addr == CONTROLLER_ADDR)
        && (frame.cmd == CMD_TRANSCEIVE_FREQ
            || frame.cmd == CMD_TRANSCEIVE_MODE
            || frame.cmd == CMD_TRANSCEIVE_RIT_XIT)
}

/// Process all complete transceive frames in a buffer, emitting events
/// for each. Incomplete data is left in the buffer for next time.
pub(crate) fn process_transceive_frames(
    buf: &mut Vec<u8>,
    civ_address: u8,
    event_tx: &broadcast::Sender<RigEvent>,
) {
    loop {
        match civ::decode_frame(buf) {
            DecodeResult::Frame(frame, consumed) => {
                buf.drain(..consumed);
                if is_transceive_frame(&frame, civ_address) {
                    process_single_transceive_frame(&frame, civ_address, event_tx);
                } else {
                    debug!(
                        cmd = frame.cmd,
                        src = frame.src_addr,
                        dst = frame.dst_addr,
                        "ignoring non-transceive frame in idle read"
                    );
                }
            }
            DecodeResult::Incomplete => break,
            DecodeResult::Collision(consumed) => {
                buf.drain(..consumed);
                debug!("collision in idle transceive read, discarding");
            }
        }
    }
}

/// Process a single transceive frame and emit the appropriate event.
pub(crate) fn process_single_transceive_frame(
    frame: &CivFrame,
    _civ_address: u8,
    event_tx: &broadcast::Sender<RigEvent>,
) {
    let payload = reassemble_payload(frame);

    match frame.cmd {
        CMD_TRANSCEIVE_FREQ => match commands::parse_frequency_response(&payload) {
            Ok(freq_hz) => {
                debug!(freq_hz, "transceive frequency update");
                let _ = event_tx.send(RigEvent::FrequencyChanged {
                    receiver: ReceiverId::VFO_A,
                    freq_hz,
                });
            }
            Err(e) => {
                debug!(?e, "failed to parse transceive frequency frame");
            }
        },
        CMD_TRANSCEIVE_MODE => match commands::parse_mode_response(&payload) {
            Ok(mode) => {
                debug!(%mode, "transceive mode update");
                let _ = event_tx.send(RigEvent::ModeChanged {
                    receiver: ReceiverId::VFO_A,
                    mode,
                });
            }
            Err(e) => {
                debug!(?e, "failed to parse transceive mode frame");
            }
        },
        CMD_TRANSCEIVE_RIT_XIT => {
            // payload[0] = sub-command: 0x00=shared RIT/XIT offset,
            //                           0x01=RIT on/off, 0x02=XIT on/off
            if payload.is_empty() {
                debug!("empty RIT/XIT transceive frame, ignoring");
                return;
            }
            let sub_cmd = payload[0];
            let data = &payload[1..];
            match sub_cmd {
                0x00 => {
                    // Shared RIT/XIT offset (emit as RitChanged; same register)
                    match commands::parse_rit_offset_response(data) {
                        Ok(offset_hz) => {
                            debug!(offset_hz, "transceive RIT/XIT offset update");
                            let _ = event_tx.send(RigEvent::RitChanged {
                                enabled: true,
                                offset_hz,
                            });
                        }
                        Err(e) => debug!(?e, "failed to parse transceive RIT/XIT offset"),
                    }
                }
                0x01 => {
                    // RIT on/off
                    match commands::parse_rit_on_response(data) {
                        Ok(enabled) => {
                            debug!(enabled, "transceive RIT on/off update");
                            let _ = event_tx.send(RigEvent::RitChanged {
                                enabled,
                                offset_hz: 0,
                            });
                        }
                        Err(e) => debug!(?e, "failed to parse transceive RIT on/off"),
                    }
                }
                0x02 => {
                    // XIT on/off
                    match commands::parse_xit_on_response(data) {
                        Ok(enabled) => {
                            debug!(enabled, "transceive XIT on/off update");
                            let _ = event_tx.send(RigEvent::XitChanged {
                                enabled,
                                offset_hz: 0,
                            });
                        }
                        Err(e) => debug!(?e, "failed to parse transceive XIT on/off"),
                    }
                }
                _ => {
                    debug!(sub_cmd, "unknown RIT/XIT sub-command in transceive frame");
                }
            }
        }
        other => {
            debug!(cmd = other, "unknown transceive command, ignoring");
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

    const IC7610_ADDR: u8 = 0x98;

    #[test]
    fn test_reassemble_payload() {
        let frame = CivFrame {
            dst_addr: CONTROLLER_ADDR,
            src_addr: IC7610_ADDR,
            cmd: 0x03,
            sub_cmd: Some(0x00),
            data: vec![0x00, 0x25, 0x14, 0x00],
        };
        let payload = reassemble_payload(&frame);
        assert_eq!(payload, vec![0x00, 0x00, 0x25, 0x14, 0x00]);
    }

    #[test]
    fn test_reassemble_payload_no_sub_cmd() {
        let frame = CivFrame {
            dst_addr: CONTROLLER_ADDR,
            src_addr: IC7610_ADDR,
            cmd: 0x03,
            sub_cmd: None,
            data: vec![],
        };
        let payload = reassemble_payload(&frame);
        assert!(payload.is_empty());
    }

    #[test]
    fn test_process_freq_frame() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Build a transceive frequency frame: 14.074 MHz
        // BCD for 14,074,000: freq_to_bcd(14_074_000)
        let bcd = civ::freq_to_bcd(14_074_000);
        let frame_bytes =
            encode_frame(BROADCAST_ADDR, IC7610_ADDR, CMD_TRANSCEIVE_FREQ, None, &bcd);

        let mut buf = frame_bytes;
        process_transceive_frames(&mut buf, IC7610_ADDR, &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::FrequencyChanged { receiver, freq_hz } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(freq_hz, 14_074_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn test_process_mode_frame() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Build a transceive mode frame: USB (mode=0x01, filter=0x01)
        let frame_bytes = encode_frame(
            BROADCAST_ADDR,
            IC7610_ADDR,
            CMD_TRANSCEIVE_MODE,
            None,
            &[0x01, 0x01],
        );

        let mut buf = frame_bytes;
        process_transceive_frames(&mut buf, IC7610_ADDR, &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::ModeChanged { receiver, mode } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(mode, Mode::USB);
            }
            other => panic!("expected ModeChanged, got {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn test_process_multiple_frames() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Two transceive frames back-to-back.
        let bcd1 = civ::freq_to_bcd(7_000_000);
        let frame1 = encode_frame(
            BROADCAST_ADDR,
            IC7610_ADDR,
            CMD_TRANSCEIVE_FREQ,
            None,
            &bcd1,
        );
        let frame2 = encode_frame(
            BROADCAST_ADDR,
            IC7610_ADDR,
            CMD_TRANSCEIVE_MODE,
            None,
            &[0x03, 0x01], // CW, filter 1
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(&frame1);
        buf.extend_from_slice(&frame2);

        process_transceive_frames(&mut buf, IC7610_ADDR, &event_tx);

        // First event: frequency.
        match event_rx.try_recv().unwrap() {
            RigEvent::FrequencyChanged { freq_hz, .. } => {
                assert_eq!(freq_hz, 7_000_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }

        // Second event: mode.
        match event_rx.try_recv().unwrap() {
            RigEvent::ModeChanged { mode, .. } => {
                assert_eq!(mode, Mode::CW);
            }
            other => panic!("expected ModeChanged, got {other:?}"),
        }

        assert!(buf.is_empty());
    }

    #[test]
    fn test_process_incomplete_frame() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Partial frame -- only preamble and address, no terminator.
        let mut buf = vec![0xFE, 0xFE, BROADCAST_ADDR, IC7610_ADDR, 0x00];
        let original_len = buf.len();

        process_transceive_frames(&mut buf, IC7610_ADDR, &event_tx);

        // No event should be emitted.
        assert!(event_rx.try_recv().is_err());
        // Buffer should be preserved for next read.
        assert_eq!(buf.len(), original_len);
    }

    #[test]
    fn test_process_ignores_wrong_src() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Frame from a different rig address (0x94 instead of 0x98).
        let bcd = civ::freq_to_bcd(14_074_000);
        let frame_bytes = encode_frame(BROADCAST_ADDR, 0x94, CMD_TRANSCEIVE_FREQ, None, &bcd);

        let mut buf = frame_bytes;
        process_transceive_frames(&mut buf, IC7610_ADDR, &event_tx);

        // No event should be emitted (wrong source).
        assert!(event_rx.try_recv().is_err());
        assert!(buf.is_empty()); // Frame was consumed but ignored.
    }

    #[test]
    fn test_is_transceive_frame_broadcast() {
        let frame = CivFrame {
            dst_addr: BROADCAST_ADDR,
            src_addr: IC7610_ADDR,
            cmd: CMD_TRANSCEIVE_FREQ,
            sub_cmd: None,
            data: vec![],
        };
        assert!(is_transceive_frame(&frame, IC7610_ADDR));
    }

    #[test]
    fn test_is_transceive_frame_to_controller() {
        let frame = CivFrame {
            dst_addr: CONTROLLER_ADDR,
            src_addr: IC7610_ADDR,
            cmd: CMD_TRANSCEIVE_MODE,
            sub_cmd: None,
            data: vec![],
        };
        assert!(is_transceive_frame(&frame, IC7610_ADDR));
    }

    #[test]
    fn test_is_not_transceive_frame_wrong_cmd() {
        let frame = CivFrame {
            dst_addr: BROADCAST_ADDR,
            src_addr: IC7610_ADDR,
            cmd: 0x03, // read freq response, not transceive
            sub_cmd: None,
            data: vec![],
        };
        assert!(!is_transceive_frame(&frame, IC7610_ADDR));
    }

    #[test]
    fn test_is_not_transceive_frame_wrong_src() {
        let frame = CivFrame {
            dst_addr: BROADCAST_ADDR,
            src_addr: 0x94, // Different rig
            cmd: CMD_TRANSCEIVE_FREQ,
            sub_cmd: None,
            data: vec![],
        };
        assert!(!is_transceive_frame(&frame, IC7610_ADDR));
    }

    #[test]
    fn test_is_transceive_frame_rit_xit() {
        let frame = CivFrame {
            dst_addr: BROADCAST_ADDR,
            src_addr: IC7610_ADDR,
            cmd: CMD_TRANSCEIVE_RIT_XIT,
            sub_cmd: Some(0x01),
            data: vec![0x01],
        };
        assert!(is_transceive_frame(&frame, IC7610_ADDR));
    }

    #[test]
    fn test_process_rit_on_frame() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Build a transceive RIT on/off frame: cmd 0x21, sub_cmd 0x01, data [0x01] (RIT on)
        let frame_bytes = encode_frame(
            BROADCAST_ADDR,
            IC7610_ADDR,
            CMD_TRANSCEIVE_RIT_XIT,
            Some(0x01),
            &[0x01],
        );

        let mut buf = frame_bytes;
        process_transceive_frames(&mut buf, IC7610_ADDR, &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::RitChanged { enabled, offset_hz } => {
                assert!(enabled);
                assert_eq!(offset_hz, 0);
            }
            other => panic!("expected RitChanged, got {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn test_process_rit_offset_frame() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Build a transceive RIT/XIT offset frame: cmd 0x21, sub_cmd 0x00,
        // LE-BCD [0x50, 0x01], sign 0x00 (+150 Hz)
        let frame_bytes = encode_frame(
            BROADCAST_ADDR,
            IC7610_ADDR,
            CMD_TRANSCEIVE_RIT_XIT,
            Some(0x00),
            &[0x50, 0x01, 0x00],
        );

        let mut buf = frame_bytes;
        process_transceive_frames(&mut buf, IC7610_ADDR, &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::RitChanged { enabled, offset_hz } => {
                assert!(enabled);
                assert_eq!(offset_hz, 150);
            }
            other => panic!("expected RitChanged, got {other:?}"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn test_process_xit_on_frame() {
        let (event_tx, mut event_rx) = broadcast::channel(16);

        // Build a transceive XIT on/off frame: cmd 0x21, sub_cmd 0x02, data [0x01] (XIT on)
        let frame_bytes = encode_frame(
            BROADCAST_ADDR,
            IC7610_ADDR,
            CMD_TRANSCEIVE_RIT_XIT,
            Some(0x02),
            &[0x01],
        );

        let mut buf = frame_bytes;
        process_transceive_frames(&mut buf, IC7610_ADDR, &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::XitChanged { enabled, offset_hz } => {
                assert!(enabled);
                assert_eq!(offset_hz, 0);
            }
            other => panic!("expected XitChanged, got {other:?}"),
        }
        assert!(buf.is_empty());
    }
}
