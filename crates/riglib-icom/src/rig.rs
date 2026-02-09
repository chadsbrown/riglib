//! IcomRig -- the [`Rig`] trait implementation for Icom transceivers.
//!
//! This module ties the CI-V protocol engine ([`civ`], [`commands`]) to a
//! [`Transport`] to produce a working Icom backend. It handles command
//! framing, echo skipping, collision recovery, retry logic, and receiver
//! selection for both single- and dual-receiver rigs.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{Mutex, broadcast};
use tracing::debug;

use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;
use riglib_core::rig::Rig;
use riglib_core::transport::Transport;
use riglib_core::types::*;

#[cfg(feature = "audio")]
use riglib_core::audio::{AudioCapable, AudioReceiver, AudioSender, AudioStreamConfig};

use crate::civ::{self, CONTROLLER_ADDR, CivFrame, DecodeResult};
use crate::commands;
use crate::models::IcomModel;
use crate::transceive::{self, CommandRequest, TransceiveHandle};

// ---------------------------------------------------------------
// BCD helpers for CI-V attenuator byte encoding
// ---------------------------------------------------------------

/// Convert a BCD-encoded attenuator byte to a decimal dB value.
///
/// CI-V encodes attenuator levels in BCD: the byte 0x20 represents 20 dB,
/// 0x06 represents 6 dB, etc.
fn bcd_to_db(bcd: u8) -> u8 {
    let tens = (bcd >> 4) & 0x0F;
    let ones = bcd & 0x0F;
    tens * 10 + ones
}

/// Convert a decimal dB value to a BCD-encoded attenuator byte.
///
/// 20 dB becomes 0x20, 6 dB becomes 0x06, etc.
fn db_to_bcd(db: u8) -> u8 {
    let tens = db / 10;
    let ones = db % 10;
    (tens << 4) | ones
}

/// A connected Icom transceiver controlled over CI-V.
///
/// Constructed via [`IcomBuilder`](crate::builder::IcomBuilder). All rig
/// communication goes through the [`Transport`] provided at build time.
pub struct IcomRig {
    transport: Arc<Mutex<Box<dyn Transport>>>,
    model: IcomModel,
    civ_address: u8,
    event_tx: broadcast::Sender<RigEvent>,
    auto_retry: bool,
    max_retries: u32,
    collision_recovery: bool,
    command_timeout: Duration,
    info: RigInfo,
    ptt_method: PttMethod,
    key_line: KeyLine,
    /// Handle to the background transceive reader task, if active.
    transceive_handle: Mutex<Option<TransceiveHandle>>,
    /// USB audio device name (e.g. "USB Audio CODEC"). When set, this rig
    /// supports audio streaming via the `AudioCapable` trait.
    #[cfg(feature = "audio")]
    audio_device_name: Option<String>,
    /// Active cpal audio backend, created on first `start_rx_audio()` or
    /// `start_tx_audio()` call.
    #[cfg(feature = "audio")]
    audio_backend: Mutex<Option<riglib_transport::CpalAudioBackend>>,
}

impl IcomRig {
    /// Create a new `IcomRig` from its constituent parts.
    ///
    /// This is called by [`IcomBuilder`](crate::builder::IcomBuilder);
    /// callers should use the builder API instead.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        transport: Box<dyn Transport>,
        model: IcomModel,
        civ_address: u8,
        auto_retry: bool,
        max_retries: u32,
        collision_recovery: bool,
        command_timeout: Duration,
        ptt_method: PttMethod,
        key_line: KeyLine,
        #[cfg(feature = "audio")] audio_device_name: Option<String>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let info = RigInfo {
            manufacturer: Manufacturer::Icom,
            model_name: model.name.to_string(),
            model_id: model.model_id.to_string(),
        };
        IcomRig {
            transport: Arc::new(Mutex::new(transport)),
            model,
            civ_address,
            event_tx,
            auto_retry,
            max_retries,
            collision_recovery,
            command_timeout,
            info,
            ptt_method,
            key_line,
            transceive_handle: Mutex::new(None),
            #[cfg(feature = "audio")]
            audio_device_name,
            #[cfg(feature = "audio")]
            audio_backend: Mutex::new(None),
        }
    }

    /// Enable CI-V transceive mode.
    ///
    /// Moves the transport from the `Arc<Mutex<>>` into a background reader
    /// task that listens for unsolicited transceive frames (frequency/mode
    /// changes) and emits them as events. Commands are forwarded to the
    /// reader task via an `mpsc` channel.
    ///
    /// This should be called once after construction, before issuing
    /// commands, when the rig has CI-V Transceive enabled.
    pub async fn start_transceive(&self) {
        let mut handle_guard = self.transceive_handle.lock().await;
        if handle_guard.is_some() {
            debug!("transceive already enabled");
            return;
        }

        // Take the real transport out and replace with a sentinel.
        let real_transport = {
            let mut transport_guard = self.transport.lock().await;
            std::mem::replace(
                &mut *transport_guard,
                Box::new(transceive::DisconnectedTransport) as Box<dyn Transport>,
            )
        };

        let handle = transceive::spawn_reader_task(
            real_transport,
            self.civ_address,
            self.event_tx.clone(),
            self.auto_retry,
            self.max_retries,
            self.collision_recovery,
            self.command_timeout,
        );

        debug!("CI-V transceive mode enabled");
        *handle_guard = Some(handle);
    }

    /// Send a CI-V command and wait for the rig's response frame.
    ///
    /// Dispatches to either the transceive channel path (if enabled) or
    /// the direct transport path.
    async fn execute_command(&self, cmd: &[u8]) -> Result<CivFrame> {
        // Check if transceive mode is active. Lock briefly to clone the sender.
        let maybe_sender = {
            let guard = self.transceive_handle.lock().await;
            guard.as_ref().map(|h| h.cmd_tx.clone())
        };

        if let Some(cmd_tx) = maybe_sender {
            self.execute_command_via_channel(cmd, cmd_tx).await
        } else {
            self.execute_command_direct(cmd).await
        }
    }

    /// Execute a command by sending it through the transceive reader task.
    async fn execute_command_via_channel(
        &self,
        cmd: &[u8],
        cmd_tx: tokio::sync::mpsc::Sender<CommandRequest>,
    ) -> Result<CivFrame> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request = CommandRequest::CivCommand {
            cmd_bytes: cmd.to_vec(),
            response_tx,
        };

        cmd_tx
            .send(request)
            .await
            .map_err(|_| Error::NotConnected)?;

        match tokio::time::timeout(
            self.command_timeout + Duration::from_millis(500),
            response_rx,
        )
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::NotConnected), // oneshot sender dropped
            Err(_) => Err(Error::Timeout),          // overall timeout
        }
    }

    /// Set a serial control line (DTR or RTS) via the transport.
    ///
    /// If transceive mode is active, the request is forwarded through the
    /// command channel to the reader task which owns the transport.
    async fn set_serial_line(&self, dtr: bool, on: bool) -> Result<()> {
        let maybe_sender = {
            let guard = self.transceive_handle.lock().await;
            guard.as_ref().map(|h| h.cmd_tx.clone())
        };

        if let Some(cmd_tx) = maybe_sender {
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            let request = if dtr {
                CommandRequest::SetDtr { on, response_tx }
            } else {
                CommandRequest::SetRts { on, response_tx }
            };
            cmd_tx
                .send(request)
                .await
                .map_err(|_| Error::NotConnected)?;
            match tokio::time::timeout(Duration::from_millis(500), response_rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(Error::NotConnected),
                Err(_) => Err(Error::Timeout),
            }
        } else {
            let mut transport = self.transport.lock().await;
            if dtr {
                transport.set_dtr(on).await
            } else {
                transport.set_rts(on).await
            }
        }
    }

    /// Send a CI-V command directly on the transport (non-transceive mode).
    ///
    /// Handles:
    /// - Echo frames (CI-V bus echoes every transmitted byte back)
    /// - Collision detection and retry (if `collision_recovery` is enabled)
    /// - Timeout with configurable retry count
    async fn execute_command_direct(&self, cmd: &[u8]) -> Result<CivFrame> {
        let retries = if self.auto_retry { self.max_retries } else { 0 };
        let mut transport = self.transport.lock().await;

        for attempt in 0..=retries {
            if attempt > 0 {
                debug!(attempt, "CI-V command retry");
                // Brief backoff before retry (increases with each attempt).
                tokio::time::sleep(Duration::from_millis(20 * attempt as u64)).await;
            }

            transport.send(cmd).await?;

            let mut buf = [0u8; 256];
            let mut response_buf = Vec::new();

            loop {
                match transport.receive(&mut buf, self.command_timeout).await {
                    Ok(n) => {
                        response_buf.extend_from_slice(&buf[..n]);

                        // Attempt to decode frames from the accumulated buffer.
                        loop {
                            match civ::decode_frame(&response_buf) {
                                DecodeResult::Frame(frame, consumed) => {
                                    response_buf.drain(..consumed);

                                    // Skip echo of our own command. The echo
                                    // has dst_addr = rig and src_addr = controller
                                    // (i.e., it is literally our outbound frame
                                    // reflected back by the CI-V bus).
                                    if frame.dst_addr == self.civ_address
                                        && frame.src_addr == CONTROLLER_ADDR
                                    {
                                        debug!("skipping CI-V echo frame");
                                        continue;
                                    }

                                    // This is the actual response from the rig.
                                    if frame.dst_addr == CONTROLLER_ADDR
                                        && frame.src_addr == self.civ_address
                                    {
                                        // Check for NAK.
                                        if frame.is_nak() {
                                            return Err(Error::Protocol("rig returned NAK".into()));
                                        }
                                        return Ok(frame);
                                    }

                                    // Frame from an unexpected address -- skip it
                                    // (possible transceive broadcast on the bus).
                                    debug!(
                                        dst = frame.dst_addr,
                                        src = frame.src_addr,
                                        "skipping CI-V frame from unexpected address"
                                    );
                                }
                                DecodeResult::Incomplete => {
                                    // Need more data from the transport.
                                    break;
                                }
                                DecodeResult::Collision(consumed) => {
                                    response_buf.drain(..consumed);
                                    if self.collision_recovery {
                                        debug!("CI-V collision detected, will retry");
                                        // Break out of the decode loop to retry
                                        // the entire command on the next attempt.
                                        break;
                                    }
                                    return Err(Error::Protocol("CI-V bus collision".into()));
                                }
                            }
                        }

                        // If we broke out of the decode loop due to collision,
                        // move to the next retry attempt.
                        if response_buf.contains(&civ::COLLISION) {
                            break;
                        }
                    }
                    Err(Error::Timeout) => {
                        // Transport timed out waiting for data. If we have
                        // accumulated partial data, try one more decode pass.
                        if !response_buf.is_empty() {
                            if let DecodeResult::Frame(frame, _) = civ::decode_frame(&response_buf)
                            {
                                if frame.dst_addr == CONTROLLER_ADDR
                                    && frame.src_addr == self.civ_address
                                    && !frame.is_nak()
                                {
                                    return Ok(frame);
                                }
                            }
                        }
                        // Move on to next retry attempt.
                        break;
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        Err(Error::Timeout)
    }

    /// Execute a command and expect an ACK frame in return.
    async fn execute_ack_command(&self, cmd: &[u8]) -> Result<()> {
        let frame = self.execute_command(cmd).await?;
        if frame.is_ack() {
            Ok(())
        } else {
            Err(Error::Protocol(format!(
                "expected ACK, got cmd=0x{:02X}",
                frame.cmd
            )))
        }
    }

    /// Select the appropriate receiver/VFO before a frequency or mode command.
    ///
    /// For dual-receiver rigs (IC-7610), uses main/sub selection (0xD0/0xD1).
    /// For single-receiver rigs, uses VFO A/B selection (0x00/0x01).
    async fn select_receiver(&self, rx: ReceiverId) -> Result<()> {
        if self.model.capabilities.has_sub_receiver {
            let cmd = commands::cmd_select_main_sub(self.civ_address, rx);
            debug!(receiver = %rx, "selecting main/sub receiver");
            self.execute_ack_command(&cmd).await
        } else if rx == ReceiverId::VFO_B {
            let cmd = commands::cmd_select_vfo(self.civ_address, rx);
            debug!(receiver = %rx, "selecting VFO");
            self.execute_ack_command(&cmd).await
        } else {
            // VFO A on a single-receiver rig is always active.
            Ok(())
        }
    }

    /// Reassemble the full BCD payload from a frequency response frame.
    ///
    /// The generic CI-V decoder splits the payload after the command byte
    /// into `sub_cmd` (first byte) and `data` (remaining bytes). For
    /// frequency responses (cmd 0x03), all 5 bytes are BCD data -- there
    /// is no real sub-command. We recombine them here.
    fn frame_freq_data(frame: &CivFrame) -> Vec<u8> {
        transceive::reassemble_payload(frame)
    }

    /// Reassemble the mode+filter payload from a mode response frame.
    ///
    /// Same logic as [`frame_freq_data`] -- the generic decoder splits
    /// the first payload byte into `sub_cmd`.
    fn frame_mode_data(frame: &CivFrame) -> Vec<u8> {
        transceive::reassemble_payload(frame)
    }

    /// Reassemble generic response data, combining sub_cmd and data fields.
    fn frame_payload(frame: &CivFrame) -> Vec<u8> {
        transceive::reassemble_payload(frame)
    }

    /// Convert a normalized meter reading (0.0--1.0) to approximate dBm
    /// for S-meter display.
    ///
    /// Uses the standard IARU S-meter scale where S9 = -73 dBm and each
    /// S-unit is 6 dB. The Icom meter range 0--255 maps roughly:
    /// 0 = S0 (-127 dBm), 120 = S9 (-73 dBm), 241 = S9+60 (-13 dBm).
    fn meter_to_dbm(normalized: f32) -> f32 {
        let raw = normalized * 255.0;
        if raw <= 120.0 {
            // S0 to S9: linear mapping over 0..120 => -127..-73 dBm
            -127.0 + (raw / 120.0) * 54.0
        } else {
            // S9 to S9+60: linear mapping over 120..241 => -73..-13 dBm
            -73.0 + ((raw - 120.0) / 121.0) * 60.0
        }
    }

    /// Convert a normalized meter reading to SWR.
    ///
    /// Icom meters report SWR on a nonlinear scale. Approximate mapping:
    /// 0 = 1.0:1, 48 = 1.5:1, 80 = 2.0:1, 120 = 3.0:1, 255 = infinity.
    fn meter_to_swr(normalized: f32) -> f32 {
        let raw = normalized * 255.0;
        if raw < 1.0 {
            1.0
        } else {
            // Rough piecewise approximation.
            1.0 + (raw / 48.0) * 0.5 + (raw / 255.0).powi(2) * 8.0
        }
    }

    /// Convert a normalized meter reading to ALC percentage (0.0--1.0).
    fn meter_to_alc(normalized: f32) -> f32 {
        normalized
    }
}

#[async_trait]
impl Rig for IcomRig {
    fn info(&self) -> &RigInfo {
        &self.info
    }

    fn capabilities(&self) -> &RigCapabilities {
        &self.model.capabilities
    }

    async fn receivers(&self) -> Result<Vec<ReceiverId>> {
        if self.model.capabilities.has_sub_receiver {
            Ok(vec![ReceiverId::VFO_A, ReceiverId::VFO_B])
        } else {
            Ok(vec![ReceiverId::VFO_A])
        }
    }

    async fn primary_receiver(&self) -> Result<ReceiverId> {
        Ok(ReceiverId::VFO_A)
    }

    async fn secondary_receiver(&self) -> Result<Option<ReceiverId>> {
        if self.model.capabilities.has_sub_receiver {
            Ok(Some(ReceiverId::VFO_B))
        } else {
            Ok(None)
        }
    }

    async fn get_frequency(&self, rx: ReceiverId) -> Result<u64> {
        self.select_receiver(rx).await?;
        let cmd = commands::cmd_read_frequency(self.civ_address);
        debug!(receiver = %rx, "reading frequency");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_freq_data(&frame);
        let freq = commands::parse_frequency_response(&data)?;
        let _ = self.event_tx.send(RigEvent::FrequencyChanged {
            receiver: rx,
            freq_hz: freq,
        });
        Ok(freq)
    }

    async fn set_frequency(&self, rx: ReceiverId, freq_hz: u64) -> Result<()> {
        self.select_receiver(rx).await?;
        let cmd = commands::cmd_set_frequency(self.civ_address, freq_hz);
        debug!(receiver = %rx, freq_hz, "setting frequency");
        self.execute_ack_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::FrequencyChanged {
            receiver: rx,
            freq_hz,
        });
        Ok(())
    }

    async fn get_mode(&self, rx: ReceiverId) -> Result<Mode> {
        self.select_receiver(rx).await?;
        let cmd = commands::cmd_read_mode(self.civ_address);
        debug!(receiver = %rx, "reading mode");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_mode_data(&frame);
        let mode = commands::parse_mode_response(&data)?;
        let _ = self
            .event_tx
            .send(RigEvent::ModeChanged { receiver: rx, mode });
        Ok(mode)
    }

    async fn set_mode(&self, rx: ReceiverId, mode: Mode) -> Result<()> {
        self.select_receiver(rx).await?;
        let cmd = commands::cmd_set_mode(self.civ_address, mode);
        debug!(receiver = %rx, %mode, "setting mode");
        self.execute_ack_command(&cmd).await?;
        let _ = self
            .event_tx
            .send(RigEvent::ModeChanged { receiver: rx, mode });
        Ok(())
    }

    async fn get_passband(&self, rx: ReceiverId) -> Result<Passband> {
        self.select_receiver(rx).await?;
        let cmd = commands::cmd_read_if_filter(self.civ_address);
        debug!(receiver = %rx, "reading passband");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_payload(&frame);
        // The IF filter response includes the sub-command byte (0x03) and then
        // filter data. The exact encoding varies by model, but the common
        // pattern is 2 bytes of BCD filter width in units of 50 Hz.
        // We skip the sub-command echo byte and parse the remaining payload.
        if data.len() < 2 {
            return Err(Error::Protocol(format!(
                "IF filter response too short: {} bytes",
                data.len()
            )));
        }
        // Skip sub-cmd echo (0x03) if present, parse remaining as BCD Hz.
        let filter_data = if data[0] == 0x03 { &data[1..] } else { &data };
        if filter_data.len() >= 2 {
            let hi = filter_data[0];
            let lo = filter_data[1];
            let hundreds = ((hi >> 4) & 0x0F) as u32 * 1000
                + (hi & 0x0F) as u32 * 100
                + ((lo >> 4) & 0x0F) as u32 * 10
                + (lo & 0x0F) as u32;
            // Value is in units of 50 Hz on most Icom rigs.
            Ok(Passband::from_hz(hundreds * 50))
        } else {
            Err(Error::Protocol("IF filter response too short".into()))
        }
    }

    async fn set_passband(&self, rx: ReceiverId, pb: Passband) -> Result<()> {
        self.select_receiver(rx).await?;
        // Convert Hz to the rig's 50-Hz unit BCD encoding.
        let units = pb.hz() / 50;
        let hi = ((units / 1000 % 10) << 4 | (units / 100 % 10)) as u8;
        let lo = ((units / 10 % 10) << 4 | (units % 10)) as u8;
        let cmd = civ::encode_frame(
            self.civ_address,
            CONTROLLER_ADDR,
            0x1A,
            Some(0x03),
            &[hi, lo],
        );
        debug!(receiver = %rx, passband_hz = pb.hz(), "setting passband");
        self.execute_ack_command(&cmd).await
    }

    async fn get_ptt(&self) -> Result<bool> {
        let cmd = commands::cmd_read_ptt(self.civ_address);
        debug!("reading PTT state");
        let frame = self.execute_command(&cmd).await?;
        // The PTT response data is in the payload after the sub-command echo.
        // Frame for read PTT: cmd=0x1C, sub_cmd=Some(0x00), data=[0x00|0x01]
        let on = commands::parse_ptt_response(&frame.data)?;
        Ok(on)
    }

    async fn set_ptt(&self, on: bool) -> Result<()> {
        match self.ptt_method {
            PttMethod::Cat => {
                let cmd = commands::cmd_set_ptt(self.civ_address, on);
                debug!(on, "setting PTT via CAT");
                self.execute_ack_command(&cmd).await?;
            }
            PttMethod::Dtr => {
                debug!(on, "setting PTT via DTR");
                self.set_serial_line(true, on).await?;
            }
            PttMethod::Rts => {
                debug!(on, "setting PTT via RTS");
                self.set_serial_line(false, on).await?;
            }
        }
        let _ = self.event_tx.send(RigEvent::PttChanged { on });
        Ok(())
    }

    async fn get_power(&self) -> Result<f32> {
        let cmd = commands::cmd_read_power(self.civ_address);
        debug!("reading power level");
        let frame = self.execute_command(&cmd).await?;
        let normalized = commands::parse_meter_response(&frame.data)?;
        let watts = normalized * self.model.capabilities.max_power_watts;
        Ok(watts)
    }

    async fn set_power(&self, watts: f32) -> Result<()> {
        let max = self.model.capabilities.max_power_watts;
        if watts < 0.0 || watts > max {
            return Err(Error::InvalidParameter(format!(
                "power {watts}W out of range 0-{max}W"
            )));
        }
        let level = ((watts / max) * 255.0).round() as u16;
        let cmd = commands::cmd_set_power(self.civ_address, level);
        debug!(watts, level, "setting power");
        self.execute_ack_command(&cmd).await
    }

    async fn get_s_meter(&self, rx: ReceiverId) -> Result<f32> {
        self.select_receiver(rx).await?;
        let cmd = commands::cmd_read_s_meter(self.civ_address);
        debug!(receiver = %rx, "reading S-meter");
        let frame = self.execute_command(&cmd).await?;
        let normalized = commands::parse_meter_response(&frame.data)?;
        let dbm = Self::meter_to_dbm(normalized);
        let _ = self
            .event_tx
            .send(RigEvent::SmeterReading { receiver: rx, dbm });
        Ok(dbm)
    }

    async fn get_swr(&self) -> Result<f32> {
        let cmd = commands::cmd_read_swr(self.civ_address);
        debug!("reading SWR");
        let frame = self.execute_command(&cmd).await?;
        let normalized = commands::parse_meter_response(&frame.data)?;
        let swr = Self::meter_to_swr(normalized);
        let _ = self.event_tx.send(RigEvent::SwrReading { swr });
        Ok(swr)
    }

    async fn get_alc(&self) -> Result<f32> {
        let cmd = commands::cmd_read_alc(self.civ_address);
        debug!("reading ALC");
        let frame = self.execute_command(&cmd).await?;
        let normalized = commands::parse_meter_response(&frame.data)?;
        Ok(Self::meter_to_alc(normalized))
    }

    async fn get_split(&self) -> Result<bool> {
        let cmd = commands::cmd_read_split(self.civ_address);
        debug!("reading split state");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_payload(&frame);
        let on = commands::parse_split_response(&data)?;
        Ok(on)
    }

    async fn set_split(&self, on: bool) -> Result<()> {
        let cmd = commands::cmd_set_split(self.civ_address, on);
        debug!(on, "setting split");
        self.execute_ack_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::SplitChanged { on });
        Ok(())
    }

    async fn set_tx_receiver(&self, rx: ReceiverId) -> Result<()> {
        if !self.model.capabilities.has_sub_receiver {
            return Err(Error::Unsupported(
                "set_tx_receiver requires dual-receiver rig".into(),
            ));
        }
        // On the IC-7610, selecting a receiver as main/sub implicitly
        // determines which VFO transmits. We use the VFO select command.
        let cmd = commands::cmd_select_vfo(self.civ_address, rx);
        debug!(receiver = %rx, "setting TX receiver");
        self.execute_ack_command(&cmd).await
    }

    async fn set_cw_key(&self, on: bool) -> Result<()> {
        match self.key_line {
            KeyLine::None => Err(Error::Unsupported("no CW key line configured".into())),
            KeyLine::Dtr => {
                debug!(on, "setting CW key via DTR");
                self.set_serial_line(true, on).await
            }
            KeyLine::Rts => {
                debug!(on, "setting CW key via RTS");
                self.set_serial_line(false, on).await
            }
        }
    }

    async fn get_cw_speed(&self) -> Result<u8> {
        let cmd = commands::cmd_read_cw_speed(self.civ_address);
        debug!("reading CW speed");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_payload(&frame);
        if data.len() < 3 {
            return Err(Error::Protocol(format!(
                "CW speed response too short: {} bytes",
                data.len()
            )));
        }
        // Skip the sub-command echo byte (0x0C), parse 2 BCD bytes
        let hi_byte = data[1];
        let lo_byte = data[2];
        let level = ((hi_byte >> 4) & 0x0F) as u32 * 1000
            + (hi_byte & 0x0F) as u32 * 100
            + ((lo_byte >> 4) & 0x0F) as u32 * 10
            + (lo_byte & 0x0F) as u32;
        let wpm = (6 + (level * 42 / 255)) as u8;
        let _ = self.event_tx.send(RigEvent::CwSpeedChanged { wpm });
        Ok(wpm)
    }

    async fn set_cw_speed(&self, wpm: u8) -> Result<()> {
        let level = (((wpm as u32).saturating_sub(6)) * 255) / 42;
        let level = level.min(255) as u16;
        let cmd = commands::cmd_set_cw_speed(self.civ_address, level);
        debug!(wpm, level, "setting CW speed");
        self.execute_ack_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::CwSpeedChanged { wpm });
        Ok(())
    }

    async fn set_vfo_a_eq_b(&self, _receiver: ReceiverId) -> Result<()> {
        let cmd = commands::cmd_vfo_a_eq_b(self.civ_address);
        debug!("setting VFO A=B");
        self.execute_ack_command(&cmd).await
    }

    async fn swap_vfo(&self, _receiver: ReceiverId) -> Result<()> {
        let cmd = commands::cmd_vfo_swap(self.civ_address);
        debug!("swapping VFO A/B");
        self.execute_ack_command(&cmd).await
    }

    async fn get_antenna(&self, _receiver: ReceiverId) -> Result<AntennaPort> {
        let cmd = commands::cmd_read_antenna(self.civ_address);
        debug!("reading antenna port");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_payload(&frame);
        let ant_byte = commands::parse_antenna_response(&data)?;
        let port = match ant_byte {
            0x01 => AntennaPort::Ant1,
            0x02 => AntennaPort::Ant2,
            0x03 => AntennaPort::Ant3,
            0x04 => AntennaPort::Ant4,
            other => {
                return Err(Error::Protocol(format!(
                    "unknown antenna port byte: 0x{other:02X}"
                )));
            }
        };
        Ok(port)
    }

    async fn set_antenna(&self, _receiver: ReceiverId, port: AntennaPort) -> Result<()> {
        let ant_byte = match port {
            AntennaPort::Ant1 => 0x01,
            AntennaPort::Ant2 => 0x02,
            AntennaPort::Ant3 => 0x03,
            AntennaPort::Ant4 => 0x04,
        };
        let cmd = commands::cmd_set_antenna(self.civ_address, ant_byte);
        debug!(%port, "setting antenna port");
        self.execute_ack_command(&cmd).await
    }

    async fn get_agc(&self, rx: ReceiverId) -> Result<AgcMode> {
        self.select_receiver(rx).await?;

        // On SDR-generation rigs, check time constant first for AGC off detection.
        // These rigs don't support mode byte 0x00; instead, time constant 0 = AGC off.
        if self.model.has_agc_time_constant {
            let tc_cmd = commands::cmd_read_agc_time_constant(self.civ_address);
            debug!(receiver = %rx, "reading AGC time constant");
            let tc_frame = self.execute_command(&tc_cmd).await?;
            let tc_data = Self::frame_payload(&tc_frame);
            // Skip sub-command echo byte (0x04) if present
            let tc_payload = if tc_data.len() >= 2 && tc_data[0] == 0x04 {
                &tc_data[1..]
            } else {
                &tc_data
            };
            let tc = commands::parse_agc_time_constant_response(tc_payload)?;
            if tc == 0x00 {
                let mode = AgcMode::Off;
                let _ = self
                    .event_tx
                    .send(RigEvent::AgcChanged { receiver: rx, mode });
                return Ok(mode);
            }
        }

        let cmd = commands::cmd_read_agc_mode(self.civ_address);
        debug!(receiver = %rx, "reading AGC mode");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_payload(&frame);
        // Skip sub-command echo byte (0x12) if present
        let agc_payload = if data.len() >= 2 && data[0] == 0x12 {
            &data[1..]
        } else {
            &data
        };
        let raw = commands::parse_agc_mode_response(agc_payload)?;

        let mode = match raw {
            0x00 => AgcMode::Off,
            0x01 => AgcMode::Fast,
            0x02 => AgcMode::Medium,
            0x03 => AgcMode::Slow,
            other => {
                return Err(Error::Protocol(format!(
                    "unknown AGC mode byte: 0x{other:02X}"
                )));
            }
        };

        let _ = self
            .event_tx
            .send(RigEvent::AgcChanged { receiver: rx, mode });
        Ok(mode)
    }

    async fn set_agc(&self, rx: ReceiverId, mode: AgcMode) -> Result<()> {
        self.select_receiver(rx).await?;

        match mode {
            AgcMode::Off => {
                if self.model.has_agc_time_constant {
                    // SDR-generation: set time constant to 0 to disable AGC
                    let cmd = commands::cmd_set_agc_time_constant(self.civ_address, 0x00);
                    debug!(receiver = %rx, "setting AGC off via time constant");
                    self.execute_ack_command(&cmd).await?;
                } else {
                    // Older rigs: set AGC mode to 0x00 (off)
                    let cmd = commands::cmd_set_agc_mode(self.civ_address, 0x00);
                    debug!(receiver = %rx, "setting AGC off");
                    self.execute_ack_command(&cmd).await?;
                }
            }
            _ => {
                let mode_byte = match mode {
                    AgcMode::Fast => 0x01,
                    AgcMode::Medium => 0x02,
                    AgcMode::Slow => 0x03,
                    _ => unreachable!(),
                };
                let cmd = commands::cmd_set_agc_mode(self.civ_address, mode_byte);
                debug!(receiver = %rx, %mode, "setting AGC mode");
                self.execute_ack_command(&cmd).await?;
            }
        }

        let _ = self
            .event_tx
            .send(RigEvent::AgcChanged { receiver: rx, mode });
        Ok(())
    }

    async fn get_preamp(&self, rx: ReceiverId) -> Result<PreampLevel> {
        self.select_receiver(rx).await?;

        let cmd = commands::cmd_read_preamp(self.civ_address);
        debug!(receiver = %rx, "reading preamp level");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_payload(&frame);

        // Skip the sub-command echo byte (0x02) if present, same pattern as AGC.
        let preamp_payload = if data.len() >= 2 && data[0] == 0x02 {
            &data[1..]
        } else {
            &data
        };
        let raw = commands::parse_preamp_response(preamp_payload)?;

        let level = match raw {
            0x00 => PreampLevel::Off,
            0x01 => PreampLevel::Preamp1,
            0x02 => PreampLevel::Preamp2,
            other => {
                return Err(Error::Protocol(format!(
                    "unknown preamp level byte: 0x{other:02X}"
                )));
            }
        };

        let _ = self.event_tx.send(RigEvent::PreampChanged {
            receiver: rx,
            level,
        });
        Ok(level)
    }

    async fn set_preamp(&self, rx: ReceiverId, level: PreampLevel) -> Result<()> {
        self.select_receiver(rx).await?;

        // Gate Preamp2 on models that only support Preamp1.
        if level == PreampLevel::Preamp2 && !self.model.has_preamp2 {
            return Err(Error::Unsupported(format!(
                "{} does not support Preamp 2",
                self.model.name
            )));
        }

        let raw = match level {
            PreampLevel::Off => 0x00,
            PreampLevel::Preamp1 => 0x01,
            PreampLevel::Preamp2 => 0x02,
        };

        let cmd = commands::cmd_set_preamp(self.civ_address, raw);
        debug!(receiver = %rx, %level, "setting preamp level");
        self.execute_ack_command(&cmd).await?;

        let _ = self.event_tx.send(RigEvent::PreampChanged {
            receiver: rx,
            level,
        });
        Ok(())
    }

    async fn get_attenuator(&self, rx: ReceiverId) -> Result<u8> {
        self.select_receiver(rx).await?;

        let cmd = commands::cmd_read_attenuator(self.civ_address);
        debug!(receiver = %rx, "reading attenuator level");
        let frame = self.execute_command(&cmd).await?;
        let data = Self::frame_payload(&frame);

        let raw = commands::parse_attenuator_response(&data)?;

        // CI-V attenuator byte is BCD-encoded: 0x00 = off, 0x20 = 20 dB.
        // Convert BCD byte to decimal dB value.
        let db = bcd_to_db(raw);

        let _ = self
            .event_tx
            .send(RigEvent::AttenuatorChanged { receiver: rx, db });
        Ok(db)
    }

    async fn set_attenuator(&self, rx: ReceiverId, db: u8) -> Result<()> {
        self.select_receiver(rx).await?;

        // CI-V attenuator byte is BCD-encoded: convert decimal dB to BCD.
        // e.g. 20 dB -> 0x20, 6 dB -> 0x06.
        let raw = db_to_bcd(db);

        let cmd = commands::cmd_set_attenuator(self.civ_address, raw);
        debug!(receiver = %rx, db, "setting attenuator");
        self.execute_ack_command(&cmd).await?;

        let _ = self
            .event_tx
            .send(RigEvent::AttenuatorChanged { receiver: rx, db });
        Ok(())
    }

    async fn get_rit(&self) -> Result<(bool, i32)> {
        // Read RIT on/off state.
        let on_cmd = commands::cmd_read_rit_on(self.civ_address);
        debug!("reading RIT on/off");
        let on_frame = self.execute_command(&on_cmd).await?;
        let on_data = Self::frame_payload(&on_frame);
        // Skip sub-command echo byte (0x01) if present.
        let on_payload = if on_data.len() >= 2 && on_data[0] == 0x01 {
            &on_data[1..]
        } else {
            &on_data
        };
        let enabled = commands::parse_rit_on_response(on_payload)?;

        // Read shared RIT/XIT offset (sub-command 0x00).
        let offset_cmd = commands::cmd_read_rit_offset(self.civ_address);
        debug!("reading RIT offset");
        let offset_frame = self.execute_command(&offset_cmd).await?;
        let offset_data = Self::frame_payload(&offset_frame);
        // Skip sub-command echo byte (0x00) if present.
        let offset_payload = if offset_data.len() >= 4 && offset_data[0] == 0x00 {
            &offset_data[1..]
        } else {
            &offset_data
        };
        let offset_hz = commands::parse_rit_offset_response(offset_payload)?;

        let _ = self
            .event_tx
            .send(RigEvent::RitChanged { enabled, offset_hz });
        Ok((enabled, offset_hz))
    }

    async fn set_rit(&self, enabled: bool, offset_hz: i32) -> Result<()> {
        let on_cmd = commands::cmd_set_rit_on(self.civ_address, enabled);
        debug!(enabled, "setting RIT on/off");
        self.execute_ack_command(&on_cmd).await?;

        let offset_cmd = commands::cmd_set_rit_offset(self.civ_address, offset_hz);
        debug!(offset_hz, "setting RIT offset");
        self.execute_ack_command(&offset_cmd).await?;

        let _ = self
            .event_tx
            .send(RigEvent::RitChanged { enabled, offset_hz });
        Ok(())
    }

    async fn get_xit(&self) -> Result<(bool, i32)> {
        // Read XIT on/off state.
        let on_cmd = commands::cmd_read_xit_on(self.civ_address);
        debug!("reading XIT on/off");
        let on_frame = self.execute_command(&on_cmd).await?;
        let on_data = Self::frame_payload(&on_frame);
        // Skip sub-command echo byte (0x02) if present.
        let on_payload = if on_data.len() >= 2 && on_data[0] == 0x02 {
            &on_data[1..]
        } else {
            &on_data
        };
        let enabled = commands::parse_xit_on_response(on_payload)?;

        // Read shared RIT/XIT offset (sub-command 0x00 — same register as RIT).
        let offset_cmd = commands::cmd_read_xit_offset(self.civ_address);
        debug!("reading XIT offset");
        let offset_frame = self.execute_command(&offset_cmd).await?;
        let offset_data = Self::frame_payload(&offset_frame);
        // Skip sub-command echo byte (0x00) if present.
        let offset_payload = if offset_data.len() >= 4 && offset_data[0] == 0x00 {
            &offset_data[1..]
        } else {
            &offset_data
        };
        let offset_hz = commands::parse_xit_offset_response(offset_payload)?;

        let _ = self
            .event_tx
            .send(RigEvent::XitChanged { enabled, offset_hz });
        Ok((enabled, offset_hz))
    }

    async fn set_xit(&self, enabled: bool, offset_hz: i32) -> Result<()> {
        let on_cmd = commands::cmd_set_xit_on(self.civ_address, enabled);
        debug!(enabled, "setting XIT on/off");
        self.execute_ack_command(&on_cmd).await?;

        let offset_cmd = commands::cmd_set_xit_offset(self.civ_address, offset_hz);
        debug!(offset_hz, "setting XIT offset");
        self.execute_ack_command(&offset_cmd).await?;

        let _ = self
            .event_tx
            .send(RigEvent::XitChanged { enabled, offset_hz });
        Ok(())
    }

    async fn send_cw_message(&self, message: &str) -> Result<()> {
        // Chunk the message into segments of at most 30 characters each.
        // The rig buffers internally and ACKs each frame, so no inter-chunk
        // delay is needed.
        let chunks: Vec<&str> = if message.is_empty() {
            Vec::new()
        } else {
            message
                .as_bytes()
                .chunks(30)
                .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
                .collect()
        };
        for chunk in &chunks {
            let cmd = commands::cmd_send_cw_message(self.civ_address, chunk);
            debug!(chunk, "sending CW message chunk");
            self.execute_ack_command(&cmd).await?;
        }
        Ok(())
    }

    async fn stop_cw_message(&self) -> Result<()> {
        let cmd = commands::cmd_stop_cw_message(self.civ_address);
        debug!("stopping CW message");
        self.execute_ack_command(&cmd).await
    }

    async fn enable_transceive(&self) -> Result<()> {
        self.start_transceive().await;
        Ok(())
    }

    async fn disable_transceive(&self) -> Result<()> {
        let handle = {
            let mut guard = self.transceive_handle.lock().await;
            guard.take()
        };
        let Some(handle) = handle else {
            return Err(Error::Protocol("transceive not currently enabled".into()));
        };

        let transport = handle.shutdown().await?;

        // Restore transport to direct mode.
        let mut transport_guard = self.transport.lock().await;
        *transport_guard = transport;

        debug!("CI-V transceive mode disabled");
        Ok(())
    }

    fn subscribe(&self) -> Result<broadcast::Receiver<RigEvent>> {
        Ok(self.event_tx.subscribe())
    }
}

// ---------------------------------------------------------------------------
// AudioCapable implementation (behind "audio" feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "audio")]
#[async_trait]
impl AudioCapable for IcomRig {
    async fn start_rx_audio(
        &self,
        _rx: ReceiverId,
        _config: Option<AudioStreamConfig>,
    ) -> Result<AudioReceiver> {
        let device_name = self
            .audio_device_name
            .as_deref()
            .ok_or_else(|| Error::Unsupported("no audio device configured for this rig".into()))?;

        let mut backend_guard = self.audio_backend.lock().await;
        let backend = backend_guard
            .get_or_insert_with(|| riglib_transport::CpalAudioBackend::new(device_name));

        backend.start_input()
    }

    async fn start_tx_audio(&self, _config: Option<AudioStreamConfig>) -> Result<AudioSender> {
        let device_name = self
            .audio_device_name
            .as_deref()
            .ok_or_else(|| Error::Unsupported("no audio device configured for this rig".into()))?;

        let mut backend_guard = self.audio_backend.lock().await;
        let backend = backend_guard
            .get_or_insert_with(|| riglib_transport::CpalAudioBackend::new(device_name));

        backend.start_output()
    }

    async fn stop_audio(&self) -> Result<()> {
        let mut backend_guard = self.audio_backend.lock().await;
        if let Some(ref mut backend) = *backend_guard {
            backend.stop();
        }
        *backend_guard = None;
        Ok(())
    }

    fn audio_supported(&self) -> bool {
        self.audio_device_name.is_some()
    }

    fn native_audio_config(&self) -> AudioStreamConfig {
        riglib_core::audio::usb_audio_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_test_harness::MockTransport;

    const IC7610_ADDR: u8 = 0x98;

    /// Helper to build an IcomRig with a MockTransport for testing.
    fn make_test_rig(mock: MockTransport) -> IcomRig {
        use crate::models::ic_7610;
        IcomRig::new(
            Box::new(mock),
            ic_7610(),
            IC7610_ADDR,
            true, // auto_retry
            3,    // max_retries
            true, // collision_recovery
            Duration::from_millis(500),
            PttMethod::Cat,
            KeyLine::None,
            #[cfg(feature = "audio")]
            None,
        )
    }

    /// Build a combined response: echo + actual response from rig.
    fn echo_and_response(cmd_bytes: &[u8], response_bytes: &[u8]) -> Vec<u8> {
        let mut combined = cmd_bytes.to_vec();
        combined.extend_from_slice(response_bytes);
        combined
    }

    /// Build an ACK from the rig to the controller.
    fn ack_frame() -> Vec<u8> {
        civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, civ::ACK, None, &[])
    }

    /// Build a NAK from the rig to the controller.
    fn nak_frame() -> Vec<u8> {
        civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, civ::NAK, None, &[])
    }

    // -----------------------------------------------------------------
    // get_frequency / set_frequency
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_frequency() {
        let mut mock = MockTransport::new();

        // Expect: select main receiver (VFO_A on dual-rx rig)
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        let select_response = echo_and_response(&select_cmd, &ack_frame());
        mock.expect(&select_cmd, &select_response);

        // Expect: read frequency command
        let read_cmd = commands::cmd_read_frequency(IC7610_ADDR);
        // Rig responds with 14.250 MHz in BCD
        let freq_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x03,
            None,
            &[0x00, 0x00, 0x25, 0x14, 0x00],
        );
        let combined = echo_and_response(&read_cmd, &freq_response);
        mock.expect(&read_cmd, &combined);

        let rig = make_test_rig(mock);
        let freq = rig.get_frequency(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(freq, 14_250_000);
    }

    #[tokio::test]
    async fn test_set_frequency() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        let select_response = echo_and_response(&select_cmd, &ack_frame());
        mock.expect(&select_cmd, &select_response);

        // Set frequency
        let set_cmd = commands::cmd_set_frequency(IC7610_ADDR, 7_000_000);
        let set_response = echo_and_response(&set_cmd, &ack_frame());
        mock.expect(&set_cmd, &set_response);

        let rig = make_test_rig(mock);
        rig.set_frequency(ReceiverId::VFO_A, 7_000_000)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------
    // get_mode / set_mode
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_mode() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // Read mode: rig responds with USB (mode=0x01, filter=0x01)
        let read_cmd = commands::cmd_read_mode(IC7610_ADDR);
        let mode_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x04,
            None,
            &[0x01, 0x01], // USB, filter 1
        );
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &mode_response));

        let rig = make_test_rig(mock);
        let mode = rig.get_mode(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, Mode::USB);
    }

    #[tokio::test]
    async fn test_set_mode() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // Set mode to CW
        let set_cmd = commands::cmd_set_mode(IC7610_ADDR, Mode::CW);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_mode(ReceiverId::VFO_A, Mode::CW).await.unwrap();
    }

    // -----------------------------------------------------------------
    // PTT
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_ptt() {
        let mut mock = MockTransport::new();

        // Read PTT: rig responds with PTT off
        let read_cmd = commands::cmd_read_ptt(IC7610_ADDR);
        // Response: cmd=0x1C, sub=0x00, data=0x00 (receive)
        let ptt_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x1C, Some(0x00), &[0x00]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &ptt_response));

        let rig = make_test_rig(mock);
        let ptt = rig.get_ptt().await.unwrap();
        assert!(!ptt);
    }

    #[tokio::test]
    async fn test_set_ptt_on() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_ptt(IC7610_ADDR, true);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_ptt(true).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_ptt_off() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_ptt(IC7610_ADDR, false);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_ptt(false).await.unwrap();
    }

    // -----------------------------------------------------------------
    // S-meter
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_s_meter() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // Read S-meter: value 0120 BCD = [0x01, 0x20] (approximately S9)
        let read_cmd = commands::cmd_read_s_meter(IC7610_ADDR);
        let meter_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x15,
            Some(0x02),
            &[0x01, 0x20],
        );
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &meter_response));

        let rig = make_test_rig(mock);
        let dbm = rig.get_s_meter(ReceiverId::VFO_A).await.unwrap();
        // S9 should be approximately -73 dBm
        assert!(
            dbm < -70.0 && dbm > -80.0,
            "S9 should be near -73 dBm, got {dbm}"
        );
    }

    // -----------------------------------------------------------------
    // Split
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_split() {
        let mut mock = MockTransport::new();

        // Read split state: rig responds with split on (sub_cmd = 0x01)
        let read_cmd = commands::cmd_read_split(IC7610_ADDR);
        let split_response = civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x0F, Some(0x01), &[]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &split_response));

        let rig = make_test_rig(mock);
        let split = rig.get_split().await.unwrap();
        assert!(split);
    }

    #[tokio::test]
    async fn test_set_split() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_split(IC7610_ADDR, true);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_split(true).await.unwrap();
    }

    // -----------------------------------------------------------------
    // CW speed
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_cw_speed() {
        let mut mock = MockTransport::new();

        // Read CW speed: rig responds with level 128 BCD = [0x01, 0x28]
        let read_cmd = commands::cmd_read_cw_speed(IC7610_ADDR);
        let cw_speed_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x14,
            Some(0x0C),
            &[0x01, 0x28],
        );
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &cw_speed_response));

        let rig = make_test_rig(mock);
        let wpm = rig.get_cw_speed().await.unwrap();
        // level=128, wpm = 6 + (128 * 42 / 255) = 6 + 21 = 27
        let expected_wpm = (6 + (128u32 * 42 / 255)) as u8;
        assert_eq!(wpm, expected_wpm, "expected {expected_wpm} WPM, got {wpm}");
    }

    #[tokio::test]
    async fn test_set_cw_speed() {
        let mut mock = MockTransport::new();

        // Set CW speed to 27 WPM => level = ((27-6) * 255) / 42 = 127
        let level = ((27u32 - 6) * 255) / 42;
        let set_cmd = commands::cmd_set_cw_speed(IC7610_ADDR, level as u16);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_cw_speed(27).await.unwrap();
    }

    // -----------------------------------------------------------------
    // VFO A=B / VFO Swap
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_set_vfo_a_eq_b() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_vfo_a_eq_b(IC7610_ADDR);
        mock.expect(&cmd, &echo_and_response(&cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_vfo_a_eq_b(ReceiverId::VFO_A).await.unwrap();
    }

    #[tokio::test]
    async fn test_swap_vfo() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_vfo_swap(IC7610_ADDR);
        mock.expect(&cmd, &echo_and_response(&cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.swap_vfo(ReceiverId::VFO_A).await.unwrap();
    }

    // -----------------------------------------------------------------
    // Antenna
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_antenna_ant1() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_antenna(IC7610_ADDR);
        let antenna_response = civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x12, None, &[0x01]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &antenna_response));

        let rig = make_test_rig(mock);
        let port = rig.get_antenna(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(port, AntennaPort::Ant1);
    }

    #[tokio::test]
    async fn test_get_antenna_ant2() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_antenna(IC7610_ADDR);
        let antenna_response = civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x12, None, &[0x02]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &antenna_response));

        let rig = make_test_rig(mock);
        let port = rig.get_antenna(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(port, AntennaPort::Ant2);
    }

    #[tokio::test]
    async fn test_set_antenna_ant1() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_antenna(IC7610_ADDR, 0x01);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_antenna(ReceiverId::VFO_A, AntennaPort::Ant1)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_antenna_ant2() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_antenna(IC7610_ADDR, 0x02);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_antenna(ReceiverId::VFO_A, AntennaPort::Ant2)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------
    // AGC
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_agc_fast() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // IC-7610 is SDR: read time constant first => non-zero (AGC active)
        let tc_cmd = commands::cmd_read_agc_time_constant(IC7610_ADDR);
        let tc_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x1A,
            Some(0x04),
            &[0x05], // non-zero = AGC is on
        );
        mock.expect(&tc_cmd, &echo_and_response(&tc_cmd, &tc_response));

        // Read AGC mode => fast (0x01)
        let agc_cmd = commands::cmd_read_agc_mode(IC7610_ADDR);
        let agc_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x16, Some(0x12), &[0x01]);
        mock.expect(&agc_cmd, &echo_and_response(&agc_cmd, &agc_response));

        let rig = make_test_rig(mock);
        let agc = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(agc, AgcMode::Fast);
    }

    #[tokio::test]
    async fn test_get_agc_off_via_time_constant() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // IC-7610 is SDR: read time constant => 0x00 (AGC off)
        let tc_cmd = commands::cmd_read_agc_time_constant(IC7610_ADDR);
        let tc_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x1A,
            Some(0x04),
            &[0x00], // zero = AGC off
        );
        mock.expect(&tc_cmd, &echo_and_response(&tc_cmd, &tc_response));

        // Should NOT read AGC mode (returns early)

        let rig = make_test_rig(mock);
        let agc = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(agc, AgcMode::Off);
    }

    #[tokio::test]
    async fn test_get_agc_slow() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // IC-7610: time constant non-zero
        let tc_cmd = commands::cmd_read_agc_time_constant(IC7610_ADDR);
        let tc_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x1A, Some(0x04), &[0x10]);
        mock.expect(&tc_cmd, &echo_and_response(&tc_cmd, &tc_response));

        // Read AGC mode => slow (0x03)
        let agc_cmd = commands::cmd_read_agc_mode(IC7610_ADDR);
        let agc_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x16, Some(0x12), &[0x03]);
        mock.expect(&agc_cmd, &echo_and_response(&agc_cmd, &agc_response));

        let rig = make_test_rig(mock);
        let agc = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(agc, AgcMode::Slow);
    }

    #[tokio::test]
    async fn test_set_agc_fast() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // Set AGC mode to fast (0x01)
        let set_cmd = commands::cmd_set_agc_mode(IC7610_ADDR, 0x01);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Fast).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_agc_off_sdr_rig() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // IC-7610 is SDR: set AGC off via time constant = 0
        let tc_cmd = commands::cmd_set_agc_time_constant(IC7610_ADDR, 0x00);
        mock.expect(&tc_cmd, &echo_and_response(&tc_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Off).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_agc_off_older_rig() {
        // Use IC-7600 (non-SDR, has_agc_time_constant = false)
        const IC7600_ADDR: u8 = 0x7A;
        let mut mock = MockTransport::new();

        // IC-7600 is single-rx, VFO_A doesn't require receiver select

        // Set AGC mode to 0x00 (off via mode byte)
        let set_cmd = commands::cmd_set_agc_mode(IC7600_ADDR, 0x00);
        let ack = civ::encode_frame(CONTROLLER_ADDR, IC7600_ADDR, civ::ACK, None, &[]);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack));

        use crate::models::ic_7600;
        let rig = IcomRig::new(
            Box::new(mock),
            ic_7600(),
            IC7600_ADDR,
            true,
            3,
            true,
            Duration::from_millis(500),
            PttMethod::Cat,
            KeyLine::None,
            #[cfg(feature = "audio")]
            None,
        );
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Off).await.unwrap();
    }

    #[tokio::test]
    async fn test_agc_event_emitted_on_set() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // Set AGC mode to medium
        let set_cmd = commands::cmd_set_agc_mode(IC7610_ADDR, 0x02);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_agc(ReceiverId::VFO_A, AgcMode::Medium)
            .await
            .unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::AgcChanged { receiver, mode } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(mode, AgcMode::Medium);
            }
            other => panic!("expected AgcChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Preamp
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_preamp_off() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // Read preamp: rig responds with preamp off (0x00)
        let read_cmd = commands::cmd_read_preamp(IC7610_ADDR);
        let preamp_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x16, Some(0x02), &[0x00]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &preamp_response));

        let rig = make_test_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Off);
    }

    #[tokio::test]
    async fn test_get_preamp_1() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let read_cmd = commands::cmd_read_preamp(IC7610_ADDR);
        let preamp_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x16, Some(0x02), &[0x01]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &preamp_response));

        let rig = make_test_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Preamp1);
    }

    #[tokio::test]
    async fn test_get_preamp_2() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let read_cmd = commands::cmd_read_preamp(IC7610_ADDR);
        let preamp_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x16, Some(0x02), &[0x02]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &preamp_response));

        let rig = make_test_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Preamp2);
    }

    #[tokio::test]
    async fn test_set_preamp_1() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let set_cmd = commands::cmd_set_preamp(IC7610_ADDR, 0x01);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp1)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_preamp_2_on_7610() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let set_cmd = commands::cmd_set_preamp(IC7610_ADDR, 0x02);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        // IC-7610 has_preamp2 = true, so this should succeed
        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp2)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_preamp_2_on_7300_returns_unsupported() {
        // IC-7300 has_preamp2 = false
        const IC7300_ADDR: u8 = 0x94;
        let mock = MockTransport::new();

        use crate::models::ic_7300;
        let rig = IcomRig::new(
            Box::new(mock),
            ic_7300(),
            IC7300_ADDR,
            true,
            3,
            true,
            Duration::from_millis(500),
            PttMethod::Cat,
            KeyLine::None,
            #[cfg(feature = "audio")]
            None,
        );

        let result = rig
            .set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp2)
            .await;
        assert!(result.is_err());
        match result {
            Err(Error::Unsupported(msg)) => {
                assert!(
                    msg.contains("Preamp 2"),
                    "expected message about Preamp 2, got: {msg}"
                );
            }
            other => panic!("expected Unsupported error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_preamp_event_emitted_on_set() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let set_cmd = commands::cmd_set_preamp(IC7610_ADDR, 0x01);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp1)
            .await
            .unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::PreampChanged { receiver, level } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(level, PreampLevel::Preamp1);
            }
            other => panic!("expected PreampChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Attenuator
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_attenuator_off() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // Read attenuator: rig responds with att off (0x00)
        let read_cmd = commands::cmd_read_attenuator(IC7610_ADDR);
        let att_response = civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x11, None, &[0x00]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &att_response));

        let rig = make_test_rig(mock);
        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(db, 0);
    }

    #[tokio::test]
    async fn test_get_attenuator_on() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let read_cmd = commands::cmd_read_attenuator(IC7610_ADDR);
        let att_response = civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x11, None, &[0x20]);
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &att_response));

        let rig = make_test_rig(mock);
        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        // Icom 0x20 BCD byte = 20 dB decimal
        assert_eq!(db, 20);
    }

    #[tokio::test]
    async fn test_set_attenuator_off() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let set_cmd = commands::cmd_set_attenuator(IC7610_ADDR, 0x00);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 0).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_attenuator_20db() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let set_cmd = commands::cmd_set_attenuator(IC7610_ADDR, 0x20);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 20).await.unwrap();
    }

    #[tokio::test]
    async fn test_attenuator_event_emitted_on_set() {
        let mut mock = MockTransport::new();

        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        let set_cmd = commands::cmd_set_attenuator(IC7610_ADDR, 0x20);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_attenuator(ReceiverId::VFO_A, 20).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::AttenuatorChanged { receiver, db } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(db, 20);
            }
            other => panic!("expected AttenuatorChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // RIT
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_rit() {
        let mut mock = MockTransport::new();

        // Read RIT on/off: rig responds with RIT on (0x01)
        let on_cmd = commands::cmd_read_rit_on(IC7610_ADDR);
        let on_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x21, Some(0x01), &[0x01]);
        mock.expect(&on_cmd, &echo_and_response(&on_cmd, &on_response));

        // Read RIT offset: rig responds with +150 Hz
        // LE-BCD [0x50, 0x01], sign 0x00 (positive)
        let offset_cmd = commands::cmd_read_rit_offset(IC7610_ADDR);
        let offset_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x21,
            Some(0x00),
            &[0x50, 0x01, 0x00],
        );
        mock.expect(
            &offset_cmd,
            &echo_and_response(&offset_cmd, &offset_response),
        );

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        let (enabled, offset_hz) = rig.get_rit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset_hz, 150);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::RitChanged { enabled, offset_hz } => {
                assert!(enabled);
                assert_eq!(offset_hz, 150);
            }
            other => panic!("expected RitChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_set_rit() {
        let mut mock = MockTransport::new();

        // Set RIT on
        let on_cmd = commands::cmd_set_rit_on(IC7610_ADDR, true);
        mock.expect(&on_cmd, &echo_and_response(&on_cmd, &ack_frame()));

        // Set RIT offset to -300 Hz
        let offset_cmd = commands::cmd_set_rit_offset(IC7610_ADDR, -300);
        mock.expect(&offset_cmd, &echo_and_response(&offset_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_rit(true, -300).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::RitChanged { enabled, offset_hz } => {
                assert!(enabled);
                assert_eq!(offset_hz, -300);
            }
            other => panic!("expected RitChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // XIT
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_xit() {
        let mut mock = MockTransport::new();

        // Read XIT on/off: rig responds with XIT off (0x00)
        let on_cmd = commands::cmd_read_xit_on(IC7610_ADDR);
        let on_response =
            civ::encode_frame(CONTROLLER_ADDR, IC7610_ADDR, 0x21, Some(0x02), &[0x00]);
        mock.expect(&on_cmd, &echo_and_response(&on_cmd, &on_response));

        // Read XIT offset: rig responds with +500 Hz
        // LE-BCD [0x00, 0x05], sign 0x00 (positive)
        let offset_cmd = commands::cmd_read_xit_offset(IC7610_ADDR);
        let offset_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x21,
            Some(0x00),
            &[0x00, 0x05, 0x00],
        );
        mock.expect(
            &offset_cmd,
            &echo_and_response(&offset_cmd, &offset_response),
        );

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        let (enabled, offset_hz) = rig.get_xit().await.unwrap();
        assert!(!enabled);
        assert_eq!(offset_hz, 500);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::XitChanged { enabled, offset_hz } => {
                assert!(!enabled);
                assert_eq!(offset_hz, 500);
            }
            other => panic!("expected XitChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_set_xit() {
        let mut mock = MockTransport::new();

        // Set XIT off
        let on_cmd = commands::cmd_set_xit_on(IC7610_ADDR, false);
        mock.expect(&on_cmd, &echo_and_response(&on_cmd, &ack_frame()));

        // Set XIT offset to 0 Hz
        let offset_cmd = commands::cmd_set_xit_offset(IC7610_ADDR, 0);
        mock.expect(&offset_cmd, &echo_and_response(&offset_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_xit(false, 0).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::XitChanged { enabled, offset_hz } => {
                assert!(!enabled);
                assert_eq!(offset_hz, 0);
            }
            other => panic!("expected XitChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // subscribe
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_subscribe_returns_receiver() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let _rx = rig.subscribe().unwrap();
    }

    // -----------------------------------------------------------------
    // info / capabilities
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_info() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let info = rig.info();
        assert_eq!(info.manufacturer, Manufacturer::Icom);
        assert_eq!(info.model_name, "IC-7610");
        assert_eq!(info.model_id, "0x98");
    }

    #[tokio::test]
    async fn test_capabilities() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let caps = rig.capabilities();
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -----------------------------------------------------------------
    // receivers
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_receivers_dual_rx() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let rxs = rig.receivers().await.unwrap();
        assert_eq!(rxs, vec![ReceiverId::VFO_A, ReceiverId::VFO_B]);
    }

    #[tokio::test]
    async fn test_primary_receiver() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        assert_eq!(rig.primary_receiver().await.unwrap(), ReceiverId::VFO_A);
    }

    #[tokio::test]
    async fn test_secondary_receiver() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        assert_eq!(
            rig.secondary_receiver().await.unwrap(),
            Some(ReceiverId::VFO_B)
        );
    }

    // -----------------------------------------------------------------
    // Auto-retry on collision
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_auto_retry_on_collision() {
        let mut mock = MockTransport::new();

        // First attempt: collision response
        let set_cmd = commands::cmd_set_ptt(IC7610_ADDR, true);
        let collision_response = vec![0xFE, 0xFE, 0xFC, 0xFD];
        mock.expect(&set_cmd, &collision_response);

        // Second attempt: success
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_ptt(true).await.unwrap();
    }

    // -----------------------------------------------------------------
    // NAK handling
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_nak_returns_error() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_ptt(IC7610_ADDR, true);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &nak_frame()));

        let rig = make_test_rig(mock);
        let result = rig.set_ptt(true).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------
    // Power commands
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_power() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_power(IC7610_ADDR);
        // Response: power level 0128 BCD = [0x01, 0x28] => 128/255 * 100W
        let power_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x14,
            Some(0x0A),
            &[0x01, 0x28],
        );
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &power_response));

        let rig = make_test_rig(mock);
        let watts = rig.get_power().await.unwrap();
        let expected = (128.0 / 255.0) * 100.0;
        assert!(
            (watts - expected).abs() < 0.5,
            "expected ~{expected}W, got {watts}W"
        );
    }

    #[tokio::test]
    async fn test_set_power() {
        let mut mock = MockTransport::new();

        // 50W on 100W rig => level = (50/100)*255 = 128 (rounded)
        let level = ((50.0f32 / 100.0) * 255.0).round() as u16;
        let set_cmd = commands::cmd_set_power(IC7610_ADDR, level);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.set_power(50.0).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_power_out_of_range() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let result = rig.set_power(200.0).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------
    // SWR / ALC
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_swr() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_swr(IC7610_ADDR);
        // Low SWR reading: 0000 BCD
        let swr_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x15,
            Some(0x12),
            &[0x00, 0x00],
        );
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &swr_response));

        let rig = make_test_rig(mock);
        let swr = rig.get_swr().await.unwrap();
        assert!(
            (swr - 1.0).abs() < 0.1,
            "zero reading should be ~1.0:1 SWR, got {swr}"
        );
    }

    #[tokio::test]
    async fn test_get_alc() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_alc(IC7610_ADDR);
        let alc_response = civ::encode_frame(
            CONTROLLER_ADDR,
            IC7610_ADDR,
            0x15,
            Some(0x13),
            &[0x01, 0x28],
        );
        mock.expect(&read_cmd, &echo_and_response(&read_cmd, &alc_response));

        let rig = make_test_rig(mock);
        let alc = rig.get_alc().await.unwrap();
        let expected = 128.0 / 255.0;
        assert!(
            (alc - expected).abs() < 0.01,
            "expected ~{expected}, got {alc}"
        );
    }

    // -----------------------------------------------------------------
    // Event subscription receives events
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_events_emitted_on_set_frequency() {
        let mut mock = MockTransport::new();

        // Select main receiver
        let select_cmd = commands::cmd_select_main_sub(IC7610_ADDR, ReceiverId::VFO_A);
        mock.expect(&select_cmd, &echo_and_response(&select_cmd, &ack_frame()));

        // Set frequency
        let set_cmd = commands::cmd_set_frequency(IC7610_ADDR, 14_074_000);
        mock.expect(&set_cmd, &echo_and_response(&set_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_frequency(ReceiverId::VFO_A, 14_074_000)
            .await
            .unwrap();

        // We should receive at least a FrequencyChanged event.
        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::FrequencyChanged { receiver, freq_hz } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(freq_hz, 14_074_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // CW message sending
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_send_cw_message() {
        let mut mock = MockTransport::new();

        // Short message (under 30 chars) — single chunk
        let cw_cmd = commands::cmd_send_cw_message(IC7610_ADDR, "CQ CQ DE W1AW");
        mock.expect(&cw_cmd, &echo_and_response(&cw_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.send_cw_message("CQ CQ DE W1AW").await.unwrap();
    }

    #[tokio::test]
    async fn test_send_cw_message_chunked() {
        let mut mock = MockTransport::new();

        // 50-character message — should be split into two chunks (30 + 20)
        let message = "CQ CQ CQ DE W1AW W1AW W1AW K  CQ CQ CQ DE W1AW PSE";
        assert_eq!(message.len(), 50);

        let chunk1 = &message[..30];
        let chunk2 = &message[30..];

        let cmd1 = commands::cmd_send_cw_message(IC7610_ADDR, chunk1);
        mock.expect(&cmd1, &echo_and_response(&cmd1, &ack_frame()));

        let cmd2 = commands::cmd_send_cw_message(IC7610_ADDR, chunk2);
        mock.expect(&cmd2, &echo_and_response(&cmd2, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.send_cw_message(message).await.unwrap();
    }

    #[tokio::test]
    async fn test_stop_cw_message() {
        let mut mock = MockTransport::new();

        let stop_cmd = commands::cmd_stop_cw_message(IC7610_ADDR);
        mock.expect(&stop_cmd, &echo_and_response(&stop_cmd, &ack_frame()));

        let rig = make_test_rig(mock);
        rig.stop_cw_message().await.unwrap();
    }

    #[tokio::test]
    async fn test_send_cw_message_empty() {
        let mock = MockTransport::new();

        // Empty string — no chunks to send, should succeed immediately
        let rig = make_test_rig(mock);
        rig.send_cw_message("").await.unwrap();
    }

    // -----------------------------------------------------------------
    // AudioCapable (feature = "audio")
    // -----------------------------------------------------------------

    #[cfg(feature = "audio")]
    mod audio_tests {
        use super::*;
        use riglib_core::audio::{AudioCapable, AudioSampleFormat};

        fn make_audio_rig(mock: MockTransport, device_name: Option<&str>) -> IcomRig {
            use crate::models::ic_7610;
            IcomRig::new(
                Box::new(mock),
                ic_7610(),
                IC7610_ADDR,
                true,
                3,
                true,
                Duration::from_millis(500),
                PttMethod::Cat,
                KeyLine::None,
                device_name.map(|s| s.to_string()),
            )
        }

        #[test]
        fn test_audio_supported_with_device() {
            let mock = MockTransport::new();
            let rig = make_audio_rig(mock, Some("USB Audio CODEC"));
            assert!(rig.audio_supported());
        }

        #[test]
        fn test_audio_not_supported_without_device() {
            let mock = MockTransport::new();
            let rig = make_audio_rig(mock, None);
            assert!(!rig.audio_supported());
        }

        #[test]
        fn test_native_audio_config() {
            let mock = MockTransport::new();
            let rig = make_audio_rig(mock, Some("USB Audio CODEC"));
            let config = rig.native_audio_config();
            assert_eq!(config.sample_rate, 48000);
            assert_eq!(config.channels, 2);
            assert_eq!(config.sample_format, AudioSampleFormat::I16);
        }

        #[tokio::test]
        async fn test_start_rx_audio_without_device_returns_error() {
            let mock = MockTransport::new();
            let rig = make_audio_rig(mock, None);
            let result = rig.start_rx_audio(ReceiverId::VFO_A, None).await;
            match result {
                Err(riglib_core::error::Error::Unsupported(msg)) => {
                    assert!(msg.contains("no audio device configured"));
                }
                Err(other) => panic!("expected Unsupported error, got: {:?}", other),
                Ok(_) => panic!("expected error, got Ok"),
            }
        }

        #[tokio::test]
        async fn test_start_tx_audio_without_device_returns_error() {
            let mock = MockTransport::new();
            let rig = make_audio_rig(mock, None);
            let result = rig.start_tx_audio(None).await;
            match result {
                Err(riglib_core::error::Error::Unsupported(msg)) => {
                    assert!(msg.contains("no audio device configured"));
                }
                Err(other) => panic!("expected Unsupported error, got: {:?}", other),
                Ok(_) => panic!("expected error, got Ok"),
            }
        }

        #[tokio::test]
        async fn test_stop_audio_without_backend_is_ok() {
            let mock = MockTransport::new();
            let rig = make_audio_rig(mock, None);
            // Calling stop_audio when no backend exists should succeed.
            rig.stop_audio().await.unwrap();
        }
    }
}
