//! CI-V transceive frame processing for unsolicited rig broadcasts.
//!
//! When CI-V Transceive is enabled on an Icom radio, the rig broadcasts
//! frequency and mode changes to all CI-V bus participants without being
//! asked. This module provides helpers that parse those broadcasts and
//! emit them as [`RigEvent`]s.
//!
//! The IO task ([`crate::io`]) calls into these helpers for both idle-read
//! processing and interleaved transceive frame handling during commands.

use tokio::sync::broadcast;
use tracing::debug;

use riglib_core::events::RigEvent;
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
