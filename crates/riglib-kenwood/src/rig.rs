//! KenwoodRig -- the [`Rig`] trait implementation for Kenwood transceivers.
//!
//! This module ties the CAT protocol engine ([`protocol`], [`commands`]) to a
//! [`Transport`] to produce a working Kenwood backend. It handles command
//! framing, semicolon-delimited response parsing, retry logic, and VFO
//! selection for both single- and dual-receiver rigs.
//!
//! The Kenwood CAT protocol is text-based, very similar to Yaesu's protocol.
//! Key differences:
//! - Frequencies are 11-digit zero-padded Hz (Yaesu uses 9)
//! - Mode codes differ (Kenwood: 1=LSB, 2=USB, 3=CW, etc.)
//! - Split is managed via independent `FR`/`FT` commands for RX/TX VFO
//! - AI (Auto Information) mode pushes unsolicited state changes

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

use crate::commands;
use crate::models::{AgcCommandStyle, KenwoodModel};
use crate::protocol::{self, DecodeResult};
use crate::transceive::{self, CommandRequest, TransceiveHandle};

/// A connected Kenwood transceiver controlled over CAT.
///
/// Constructed via [`KenwoodBuilder`](crate::builder::KenwoodBuilder). All rig
/// communication goes through the [`Transport`] provided at build time.
pub struct KenwoodRig {
    transport: Arc<Mutex<Box<dyn Transport>>>,
    model: KenwoodModel,
    event_tx: broadcast::Sender<RigEvent>,
    auto_retry: bool,
    max_retries: u32,
    command_timeout: Duration,
    info: RigInfo,
    ptt_method: PttMethod,
    key_line: KeyLine,
    /// Handle to the background AI transceive reader task, if active.
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

impl KenwoodRig {
    /// Create a new `KenwoodRig` from its constituent parts.
    ///
    /// This is called by [`KenwoodBuilder`](crate::builder::KenwoodBuilder);
    /// callers should use the builder API instead.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        transport: Box<dyn Transport>,
        model: KenwoodModel,
        auto_retry: bool,
        max_retries: u32,
        command_timeout: Duration,
        ptt_method: PttMethod,
        key_line: KeyLine,
        #[cfg(feature = "audio")] audio_device_name: Option<String>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let info = RigInfo {
            manufacturer: Manufacturer::Kenwood,
            model_name: model.name.to_string(),
            model_id: model.model_id.to_string(),
        };
        KenwoodRig {
            transport: Arc::new(Mutex::new(transport)),
            model,
            event_tx,
            auto_retry,
            max_retries,
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

    /// Enable Kenwood AI (Auto Information) mode.
    ///
    /// Moves the transport from the `Arc<Mutex<>>` into a background reader
    /// task that listens for unsolicited AI responses (frequency, mode, PTT,
    /// RIT/XIT changes) and emits them as events. Commands are forwarded to
    /// the reader task via an `mpsc` channel.
    ///
    /// This should be called once after construction, before issuing
    /// commands, when AI mode (`AI2;`) is desired.
    pub async fn enable_ai_mode(&self) {
        let mut handle_guard = self.transceive_handle.lock().await;
        if handle_guard.is_some() {
            debug!("AI transceive already enabled");
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
            self.event_tx.clone(),
            self.auto_retry,
            self.max_retries,
            self.command_timeout,
        );

        debug!("Kenwood AI transceive mode enabled");
        *handle_guard = Some(handle);
    }

    /// Send a CAT command and wait for the rig's response.
    ///
    /// Dispatches to either the transceive channel path (if AI mode is
    /// enabled) or the direct transport path.
    ///
    /// Returns the decoded prefix and data on success.
    async fn execute_command(&self, cmd: &[u8]) -> Result<(String, String)> {
        // Check if AI transceive mode is active. Lock briefly to clone the sender.
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

    /// Execute a command by sending it through the AI transceive reader task.
    async fn execute_command_via_channel(
        &self,
        cmd: &[u8],
        cmd_tx: tokio::sync::mpsc::Sender<CommandRequest>,
    ) -> Result<(String, String)> {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request = CommandRequest::CatCommand {
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
    /// If AI transceive mode is active, the request is forwarded through the
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

    /// Send a CAT command directly on the transport (non-AI mode).
    ///
    /// Reads bytes from the transport until a semicolon terminator is found,
    /// then decodes the response. Handles:
    /// - Timeout with configurable retry count
    /// - Error responses (`?;`)
    ///
    /// Returns the decoded prefix and data on success.
    async fn execute_command_direct(&self, cmd: &[u8]) -> Result<(String, String)> {
        let retries = if self.auto_retry { self.max_retries } else { 0 };
        let mut transport = self.transport.lock().await;

        for attempt in 0..=retries {
            if attempt > 0 {
                debug!(attempt, "Kenwood CAT command retry");
                tokio::time::sleep(Duration::from_millis(20 * attempt as u64)).await;
            }

            transport.send(cmd).await?;

            let mut buf = [0u8; 256];
            let mut response_buf = Vec::new();

            loop {
                match tokio::time::timeout(
                    self.command_timeout,
                    transport.receive(&mut buf, self.command_timeout),
                )
                .await
                {
                    Ok(Ok(n)) => {
                        response_buf.extend_from_slice(&buf[..n]);

                        // Attempt to decode a response from accumulated data.
                        match protocol::decode_response(&response_buf) {
                            DecodeResult::Response {
                                prefix,
                                data,
                                consumed,
                            } => {
                                response_buf.drain(..consumed);
                                return Ok((prefix, data));
                            }
                            DecodeResult::Error(consumed) => {
                                response_buf.drain(..consumed);
                                return Err(Error::Protocol(
                                    "rig returned error response (?;)".into(),
                                ));
                            }
                            DecodeResult::Incomplete => {
                                // Need more data, continue reading.
                            }
                        }
                    }
                    Ok(Err(Error::Timeout)) => {
                        // Transport timed out. Try to decode what we have.
                        if !response_buf.is_empty() {
                            match protocol::decode_response(&response_buf) {
                                DecodeResult::Response { prefix, data, .. } => {
                                    return Ok((prefix, data));
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
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        // tokio::time::timeout expired.
                        if !response_buf.is_empty() {
                            match protocol::decode_response(&response_buf) {
                                DecodeResult::Response { prefix, data, .. } => {
                                    return Ok((prefix, data));
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
                }
            }
        }

        Err(Error::Timeout)
    }

    /// Send a CAT command that produces no response data (set commands).
    ///
    /// Kenwood set commands typically echo back the command as confirmation.
    /// We send the command and wait for the echoed response.
    async fn execute_set_command(&self, cmd: &[u8]) -> Result<()> {
        let _ = self.execute_command(cmd).await?;
        Ok(())
    }

    /// Convert a raw Kenwood S-meter value (0-30 typical) to dBm.
    ///
    /// Uses the standard IARU S-meter scale where S9 = -73 dBm and each
    /// S-unit is 6 dB. Kenwood S-meter readings typically range 0-30 where
    /// 0-15 covers S0 to S9, and 15-30 covers S9 to S9+60dB.
    fn meter_to_dbm(raw: u16) -> f32 {
        if raw <= 15 {
            // S0 to S9: linear mapping over 0..15 => -127..-73 dBm
            -127.0 + (raw as f32 / 15.0) * 54.0
        } else {
            // S9 to S9+60: linear mapping over 15..30 => -73..-13 dBm
            -73.0 + ((raw as f32 - 15.0) / 15.0) * 60.0
        }
    }

    /// Convert a raw Kenwood SWR meter value to SWR ratio.
    ///
    /// Kenwood SWR meter values are model-dependent but typically range
    /// 0-30. We approximate: 0 = 1.0:1, 30 = high SWR.
    fn meter_to_swr(raw: u16) -> f32 {
        if raw == 0 {
            1.0
        } else {
            // Rough approximation: each unit adds about 0.1 SWR above 1.0
            1.0 + (raw as f32 * 0.1)
        }
    }

    /// Convert a raw Kenwood ALC meter value to a normalized 0.0-1.0 reading.
    fn meter_to_alc(raw: u16) -> f32 {
        // Kenwood ALC meters typically read 0-30.
        (raw as f32 / 30.0).min(1.0)
    }
}

#[async_trait]
impl Rig for KenwoodRig {
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
        let cmd = if rx == ReceiverId::VFO_B {
            commands::cmd_read_frequency_b()
        } else {
            commands::cmd_read_frequency_a()
        };
        debug!(receiver = %rx, "reading frequency");
        let (_, data) = self.execute_command(&cmd).await?;
        let freq = commands::parse_frequency_response(&data)?;
        let _ = self.event_tx.send(RigEvent::FrequencyChanged {
            receiver: rx,
            freq_hz: freq,
        });
        Ok(freq)
    }

    async fn set_frequency(&self, rx: ReceiverId, freq_hz: u64) -> Result<()> {
        let cmd = if rx == ReceiverId::VFO_B {
            commands::cmd_set_frequency_b(freq_hz)
        } else {
            commands::cmd_set_frequency_a(freq_hz)
        };
        debug!(receiver = %rx, freq_hz, "setting frequency");
        self.execute_set_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::FrequencyChanged {
            receiver: rx,
            freq_hz,
        });
        Ok(())
    }

    async fn get_mode(&self, _rx: ReceiverId) -> Result<Mode> {
        let cmd = commands::cmd_read_mode();
        debug!("reading mode");
        let (_, data) = self.execute_command(&cmd).await?;
        let mode = commands::parse_mode_response(&data)?;
        let _ = self.event_tx.send(RigEvent::ModeChanged {
            receiver: _rx,
            mode,
        });
        Ok(mode)
    }

    async fn set_mode(&self, _rx: ReceiverId, mode: Mode) -> Result<()> {
        let cmd = commands::cmd_set_mode(&mode);
        debug!(%mode, "setting mode");
        self.execute_set_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::ModeChanged {
            receiver: _rx,
            mode,
        });
        Ok(())
    }

    async fn get_passband(&self, _rx: ReceiverId) -> Result<Passband> {
        let cmd = commands::cmd_read_passband();
        debug!("reading passband");
        let (_, data) = self.execute_command(&cmd).await?;
        // Kenwood SH response data is a 2-digit index representing the
        // filter width. The mapping is model-dependent. We provide a
        // rough approximation based on common Kenwood convention.
        let index: u32 = data
            .parse()
            .map_err(|e| Error::Protocol(format!("invalid passband data: {data:?} ({e})")))?;
        // Rough mapping: index 0 = narrowest (200 Hz), index 31 = widest (5000 Hz).
        // Linear interpolation provides a reasonable approximation.
        let hz = 200 + index * 150;
        Ok(Passband::from_hz(hz))
    }

    async fn set_passband(&self, _rx: ReceiverId, pb: Passband) -> Result<()> {
        // Reverse the rough mapping: hz -> index.
        let hz = pb.hz();
        let index = if hz <= 200 {
            0u8
        } else {
            ((hz - 200) / 150).min(31) as u8
        };
        let cmd = commands::cmd_set_passband(index);
        debug!(passband_hz = pb.hz(), index, "setting passband");
        self.execute_set_command(&cmd).await
    }

    async fn get_ptt(&self) -> Result<bool> {
        let cmd = commands::cmd_read_ptt();
        debug!("reading PTT state");
        let (_, data) = self.execute_command(&cmd).await?;
        let on = commands::parse_ptt_response(&data)?;
        Ok(on)
    }

    async fn set_ptt(&self, on: bool) -> Result<()> {
        match self.ptt_method {
            PttMethod::Cat => {
                let cmd = commands::cmd_set_ptt(on);
                debug!(on, "setting PTT via CAT");
                self.execute_set_command(&cmd).await?;
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
        let cmd = commands::cmd_read_power();
        debug!("reading power level");
        let (_, data) = self.execute_command(&cmd).await?;
        let watts = commands::parse_power_response(&data)?;
        Ok(watts as f32)
    }

    async fn set_power(&self, watts: f32) -> Result<()> {
        let max = self.model.capabilities.max_power_watts;
        if watts < 0.0 || watts > max {
            return Err(Error::InvalidParameter(format!(
                "power {watts}W out of range 0-{max}W"
            )));
        }
        let cmd = commands::cmd_set_power(watts.round() as u16);
        debug!(watts, "setting power");
        self.execute_set_command(&cmd).await
    }

    async fn get_s_meter(&self, rx: ReceiverId) -> Result<f32> {
        let cmd = commands::cmd_read_s_meter();
        debug!(receiver = %rx, "reading S-meter");
        let (_, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_meter_response(&data)?;
        let dbm = Self::meter_to_dbm(raw);
        let _ = self
            .event_tx
            .send(RigEvent::SmeterReading { receiver: rx, dbm });
        Ok(dbm)
    }

    async fn get_swr(&self) -> Result<f32> {
        let cmd = commands::cmd_read_swr();
        debug!("reading SWR");
        let (_, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_meter_response(&data)?;
        let swr = Self::meter_to_swr(raw);
        let _ = self.event_tx.send(RigEvent::SwrReading { swr });
        Ok(swr)
    }

    async fn get_alc(&self) -> Result<f32> {
        let cmd = commands::cmd_read_alc();
        debug!("reading ALC");
        let (_, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_meter_response(&data)?;
        Ok(Self::meter_to_alc(raw))
    }

    async fn get_split(&self) -> Result<bool> {
        // Kenwood uses FR/FT to determine split state.
        // Split is on when RX VFO != TX VFO.
        // We read the TX VFO (FT) and check if it is VFO B.
        let cmd = commands::cmd_read_tx_vfo();
        debug!("reading split state via FT");
        let (_, data) = self.execute_command(&cmd).await?;
        let tx_on_b = commands::parse_vfo_response(&data)?;
        Ok(tx_on_b)
    }

    async fn set_split(&self, on: bool) -> Result<()> {
        let (fr_cmd, ft_cmd) = commands::cmd_set_split(on);
        debug!(on, "setting split");
        self.execute_set_command(&fr_cmd).await?;
        self.execute_set_command(&ft_cmd).await?;
        let _ = self.event_tx.send(RigEvent::SplitChanged { on });
        Ok(())
    }

    async fn set_tx_receiver(&self, rx: ReceiverId) -> Result<()> {
        let vfo_b = rx == ReceiverId::VFO_B;
        let cmd = commands::cmd_set_tx_vfo(vfo_b);
        debug!(receiver = %rx, "setting TX VFO");
        self.execute_set_command(&cmd).await
    }

    async fn set_cw_key(&self, on: bool) -> Result<()> {
        match self.key_line {
            KeyLine::None => Err(Error::Unsupported(
                "no CW key line configured (use builder's key_line() method)".into(),
            )),
            KeyLine::Dtr => {
                debug!(on, "CW key via DTR");
                self.set_serial_line(true, on).await
            }
            KeyLine::Rts => {
                debug!(on, "CW key via RTS");
                self.set_serial_line(false, on).await
            }
        }
    }

    async fn get_cw_speed(&self) -> Result<u8> {
        let cmd = commands::cmd_read_cw_speed();
        debug!("reading CW speed");
        let (_, data) = self.execute_command(&cmd).await?;
        let wpm = commands::parse_cw_speed_response(&data)?;
        let _ = self.event_tx.send(RigEvent::CwSpeedChanged { wpm });
        Ok(wpm)
    }

    async fn set_cw_speed(&self, wpm: u8) -> Result<()> {
        let cmd = commands::cmd_set_cw_speed(wpm);
        debug!(wpm, "setting CW speed");
        self.execute_set_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::CwSpeedChanged { wpm });
        Ok(())
    }

    async fn get_agc(&self, rx: ReceiverId) -> Result<AgcMode> {
        debug!(receiver = %rx, "reading AGC mode");
        let mode = match self.model.agc_command_style {
            AgcCommandStyle::GcSimple => {
                let cmd = commands::cmd_read_agc_mode();
                let (_, data) = self.execute_command(&cmd).await?;
                let raw = commands::parse_agc_mode_response(&data)?;
                // TS-890S GC: 0=Off, 1=Slow, 2=Medium, 3=Fast
                match raw {
                    0 => AgcMode::Off,
                    1 => AgcMode::Slow,
                    2 => AgcMode::Medium,
                    3 => AgcMode::Fast,
                    other => {
                        return Err(Error::Protocol(format!(
                            "unknown Kenwood AGC mode value: {other}"
                        )));
                    }
                }
            }
            AgcCommandStyle::GcVfo => {
                let vfo = rx.index();
                let cmd = commands::cmd_read_agc_mode_vfo(vfo);
                let (_, data) = self.execute_command(&cmd).await?;
                let (_vfo, raw) = commands::parse_agc_mode_vfo_response(&data)?;
                // TS-990S GC: same mapping as TS-890S
                match raw {
                    0 => AgcMode::Off,
                    1 => AgcMode::Slow,
                    2 => AgcMode::Medium,
                    3 => AgcMode::Fast,
                    other => {
                        return Err(Error::Protocol(format!(
                            "unknown Kenwood AGC mode value: {other}"
                        )));
                    }
                }
            }
            AgcCommandStyle::GtTimeConstant => {
                let cmd = commands::cmd_read_agc_time_constant();
                let (_, data) = self.execute_command(&cmd).await?;
                let tc = commands::parse_agc_time_constant_response(&data)?;
                // TS-590S/SG GT: 0=Off, 5=Fast, 10=Medium, 20=Slow
                match tc {
                    0 => AgcMode::Off,
                    5 => AgcMode::Fast,
                    10 => AgcMode::Medium,
                    20 => AgcMode::Slow,
                    other => {
                        return Err(Error::Protocol(format!(
                            "unknown Kenwood AGC time constant: {other}"
                        )));
                    }
                }
            }
        };
        let _ = self
            .event_tx
            .send(RigEvent::AgcChanged { receiver: rx, mode });
        Ok(mode)
    }

    async fn set_agc(&self, rx: ReceiverId, mode: AgcMode) -> Result<()> {
        debug!(receiver = %rx, ?mode, "setting AGC mode");
        match self.model.agc_command_style {
            AgcCommandStyle::GcSimple => {
                // TS-890S GC: 0=Off, 1=Slow, 2=Medium, 3=Fast
                let value = match mode {
                    AgcMode::Off => 0,
                    AgcMode::Slow => 1,
                    AgcMode::Medium => 2,
                    AgcMode::Fast => 3,
                };
                let cmd = commands::cmd_set_agc_mode(value);
                self.execute_set_command(&cmd).await?;
            }
            AgcCommandStyle::GcVfo => {
                let vfo = rx.index();
                let value = match mode {
                    AgcMode::Off => 0,
                    AgcMode::Slow => 1,
                    AgcMode::Medium => 2,
                    AgcMode::Fast => 3,
                };
                let cmd = commands::cmd_set_agc_mode_vfo(vfo, value);
                self.execute_set_command(&cmd).await?;
            }
            AgcCommandStyle::GtTimeConstant => {
                // TS-590S/SG GT: 0=Off, 5=Fast, 10=Medium, 20=Slow
                let tc = match mode {
                    AgcMode::Off => 0,
                    AgcMode::Fast => 5,
                    AgcMode::Medium => 10,
                    AgcMode::Slow => 20,
                };
                let cmd = commands::cmd_set_agc_time_constant(tc);
                self.execute_set_command(&cmd).await?;
            }
        }
        let _ = self
            .event_tx
            .send(RigEvent::AgcChanged { receiver: rx, mode });
        Ok(())
    }

    async fn get_preamp(&self, rx: ReceiverId) -> Result<PreampLevel> {
        debug!(receiver = %rx, "reading preamp level");
        let cmd = commands::cmd_read_preamp();
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_preamp_response(&data)?;
        let level = match raw {
            0 => PreampLevel::Off,
            1 => PreampLevel::Preamp1,
            2 => PreampLevel::Preamp2,
            other => {
                return Err(Error::Protocol(format!(
                    "unknown Kenwood preamp level: {other}"
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
        // Gate Preamp2 on models that only support Preamp1.
        if level == PreampLevel::Preamp2 && !self.model.has_preamp2 {
            return Err(Error::Unsupported(format!(
                "{} does not support Preamp 2",
                self.model.name
            )));
        }

        let value = match level {
            PreampLevel::Off => 0,
            PreampLevel::Preamp1 => 1,
            PreampLevel::Preamp2 => 2,
        };
        let cmd = commands::cmd_set_preamp(value);
        debug!(receiver = %rx, ?level, "setting preamp");
        self.execute_set_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::PreampChanged {
            receiver: rx,
            level,
        });
        Ok(())
    }

    async fn get_attenuator(&self, rx: ReceiverId) -> Result<u8> {
        debug!(receiver = %rx, "reading attenuator level");
        let cmd = commands::cmd_read_attenuator();
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let db = commands::parse_attenuator_response(&data)?;
        let _ = self
            .event_tx
            .send(RigEvent::AttenuatorChanged { receiver: rx, db });
        Ok(db)
    }

    async fn set_attenuator(&self, rx: ReceiverId, db: u8) -> Result<()> {
        let cmd = commands::cmd_set_attenuator(db);
        debug!(receiver = %rx, db, "setting attenuator");
        self.execute_set_command(&cmd).await?;
        let _ = self
            .event_tx
            .send(RigEvent::AttenuatorChanged { receiver: rx, db });
        Ok(())
    }

    async fn get_rit(&self) -> Result<(bool, i32)> {
        debug!("reading RIT state");
        let cmd = commands::cmd_read_rit();
        let (_, data) = self.execute_command(&cmd).await?;
        let enabled = commands::parse_rit_response(&data)?;

        let offset_cmd = commands::cmd_read_rit_xit_offset();
        let (_, offset_data) = self.execute_command(&offset_cmd).await?;
        let offset_hz = commands::parse_rit_xit_offset_response(&offset_data)?;

        let _ = self
            .event_tx
            .send(RigEvent::RitChanged { enabled, offset_hz });
        Ok((enabled, offset_hz))
    }

    async fn set_rit(&self, enabled: bool, offset_hz: i32) -> Result<()> {
        debug!(enabled, offset_hz, "setting RIT");
        let cmd = commands::cmd_set_rit_on(enabled);
        self.execute_set_command(&cmd).await?;

        let clear_cmd = commands::cmd_rit_clear();
        self.execute_set_command(&clear_cmd).await?;

        if offset_hz != 0 {
            let steps = offset_hz.unsigned_abs() / self.model.rit_step_hz as u32;
            if offset_hz > 0 {
                let up_cmd = commands::cmd_rit_up();
                for _ in 0..steps {
                    self.execute_set_command(&up_cmd).await?;
                }
            } else {
                let down_cmd = commands::cmd_rit_down();
                for _ in 0..steps {
                    self.execute_set_command(&down_cmd).await?;
                }
            }
        }

        let _ = self
            .event_tx
            .send(RigEvent::RitChanged { enabled, offset_hz });
        Ok(())
    }

    async fn get_xit(&self) -> Result<(bool, i32)> {
        debug!("reading XIT state");
        let cmd = commands::cmd_read_xit();
        let (_, data) = self.execute_command(&cmd).await?;
        let enabled = commands::parse_xit_response(&data)?;

        let offset_cmd = commands::cmd_read_rit_xit_offset();
        let (_, offset_data) = self.execute_command(&offset_cmd).await?;
        let offset_hz = commands::parse_rit_xit_offset_response(&offset_data)?;

        let _ = self
            .event_tx
            .send(RigEvent::XitChanged { enabled, offset_hz });
        Ok((enabled, offset_hz))
    }

    async fn set_xit(&self, enabled: bool, offset_hz: i32) -> Result<()> {
        debug!(enabled, offset_hz, "setting XIT");
        let cmd = commands::cmd_set_xit_on(enabled);
        self.execute_set_command(&cmd).await?;

        let clear_cmd = commands::cmd_rit_clear();
        self.execute_set_command(&clear_cmd).await?;

        if offset_hz != 0 {
            let steps = offset_hz.unsigned_abs() / self.model.rit_step_hz as u32;
            if offset_hz > 0 {
                let up_cmd = commands::cmd_rit_up();
                for _ in 0..steps {
                    self.execute_set_command(&up_cmd).await?;
                }
            } else {
                let down_cmd = commands::cmd_rit_down();
                for _ in 0..steps {
                    self.execute_set_command(&down_cmd).await?;
                }
            }
        }

        let _ = self
            .event_tx
            .send(RigEvent::XitChanged { enabled, offset_hz });
        Ok(())
    }

    async fn set_vfo_a_eq_b(&self, _receiver: ReceiverId) -> Result<()> {
        let cmd = commands::cmd_vfo_a_eq_b();
        debug!("setting VFO A=B");
        self.execute_set_command(&cmd).await
    }

    async fn swap_vfo(&self, _receiver: ReceiverId) -> Result<()> {
        Err(Error::Unsupported(
            "Kenwood does not support VFO swap".into(),
        ))
    }

    async fn get_antenna(&self, _receiver: ReceiverId) -> Result<AntennaPort> {
        let cmd = commands::cmd_read_antenna();
        debug!("reading antenna port");
        let (_, data) = self.execute_command(&cmd).await?;
        let ant = commands::parse_antenna_response(&data)?;
        let port = match ant {
            1 => AntennaPort::Ant1,
            2 => AntennaPort::Ant2,
            3 => AntennaPort::Ant3,
            4 => AntennaPort::Ant4,
            other => {
                return Err(Error::Protocol(format!(
                    "unknown antenna port number: {other}"
                )));
            }
        };
        Ok(port)
    }

    async fn set_antenna(&self, _receiver: ReceiverId, port: AntennaPort) -> Result<()> {
        let ant = match port {
            AntennaPort::Ant1 => 1,
            AntennaPort::Ant2 => 2,
            AntennaPort::Ant3 => 3,
            AntennaPort::Ant4 => 4,
        };
        let cmd = commands::cmd_set_antenna(ant);
        debug!(%port, "setting antenna port");
        self.execute_set_command(&cmd).await
    }

    async fn send_cw_message(&self, message: &str) -> Result<()> {
        if message.is_empty() {
            return Ok(());
        }

        // Split the message into chunks of at most 24 characters.
        let chars: Vec<char> = message.chars().collect();
        let chunks: Vec<String> = chars.chunks(24).map(|c| c.iter().collect()).collect();

        debug!(
            message_len = message.len(),
            num_chunks = chunks.len(),
            "sending CW message"
        );

        for (i, chunk) in chunks.iter().enumerate() {
            // Poll the buffer status before sending each chunk.
            const MAX_BUFFER_RETRIES: u32 = 50;
            for retry in 0..MAX_BUFFER_RETRIES {
                let buf_cmd = commands::cmd_read_cw_buffer();
                let (_prefix, data) = self.execute_command(&buf_cmd).await?;
                let buffer_ready = commands::parse_cw_buffer_response(&data)?;

                if buffer_ready {
                    break;
                }

                if retry == MAX_BUFFER_RETRIES - 1 {
                    return Err(Error::Timeout);
                }

                debug!(retry, "CW buffer full, waiting");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            let send_cmd = commands::cmd_send_cw_message(chunk);
            debug!(chunk_index = i, chunk = %chunk, "sending CW chunk");
            self.execute_set_command(&send_cmd).await?;
        }

        Ok(())
    }

    async fn stop_cw_message(&self) -> Result<()> {
        let cmd = commands::cmd_stop_cw_message();
        debug!("stopping CW message");
        self.execute_set_command(&cmd).await
    }

    async fn enable_transceive(&self) -> Result<()> {
        self.enable_ai_mode().await;
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

        debug!("Kenwood AI transceive mode disabled");
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
impl AudioCapable for KenwoodRig {
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

    /// Helper to build a KenwoodRig with a MockTransport for testing.
    fn make_test_rig(mock: MockTransport) -> KenwoodRig {
        use crate::models::ts_890s;
        KenwoodRig::new(
            Box::new(mock),
            ts_890s(),
            true, // auto_retry
            3,    // max_retries
            Duration::from_millis(500),
            PttMethod::Cat,
            KeyLine::None,
            #[cfg(feature = "audio")]
            None,
        )
    }

    /// Helper to build a single-receiver rig for testing.
    fn make_single_rx_rig(mock: MockTransport) -> KenwoodRig {
        use crate::models::ts_590s;
        KenwoodRig::new(
            Box::new(mock),
            ts_590s(),
            true,
            3,
            Duration::from_millis(500),
            PttMethod::Cat,
            KeyLine::None,
            #[cfg(feature = "audio")]
            None,
        )
    }

    // -----------------------------------------------------------------
    // get_frequency / set_frequency
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_frequency_a() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_frequency_a();
        let response = b"FA00014250000;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let freq = rig.get_frequency(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(freq, 14_250_000);
    }

    #[tokio::test]
    async fn test_get_frequency_b() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_frequency_b();
        let response = b"FB00007000000;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let freq = rig.get_frequency(ReceiverId::VFO_B).await.unwrap();
        assert_eq!(freq, 7_000_000);
    }

    #[tokio::test]
    async fn test_set_frequency_a() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_frequency_a(7_000_000);
        // Kenwood echoes back the set command as confirmation.
        let response = b"FA00007000000;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_frequency(ReceiverId::VFO_A, 7_000_000)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_frequency_b() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_frequency_b(28_500_000);
        let response = b"FB00028500000;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_frequency(ReceiverId::VFO_B, 28_500_000)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_frequency_11_digit_format() {
        // Verify the 11-digit format works correctly for a typical frequency.
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_frequency_a(14_074_000);
        assert_eq!(set_cmd, b"FA00014074000;");
        let response = b"FA00014074000;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_frequency(ReceiverId::VFO_A, 14_074_000)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------
    // get_mode / set_mode
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_mode_usb() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_mode();
        let response = b"MD2;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let mode = rig.get_mode(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, Mode::USB);
    }

    #[tokio::test]
    async fn test_get_mode_cw() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_mode();
        let response = b"MD3;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let mode = rig.get_mode(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, Mode::CW);
    }

    #[tokio::test]
    async fn test_set_mode_cw() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_mode(&Mode::CW);
        let response = b"MD3;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_mode(ReceiverId::VFO_A, Mode::CW).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_mode_rtty() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_mode(&Mode::RTTY);
        let response = b"MD6;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_mode(ReceiverId::VFO_A, Mode::RTTY).await.unwrap();
    }

    // -----------------------------------------------------------------
    // PTT
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_ptt_off() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_ptt();
        let response = b"TX0;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let ptt = rig.get_ptt().await.unwrap();
        assert!(!ptt);
    }

    #[tokio::test]
    async fn test_get_ptt_on() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_ptt();
        let response = b"TX1;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let ptt = rig.get_ptt().await.unwrap();
        assert!(ptt);
    }

    #[tokio::test]
    async fn test_set_ptt_on() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_ptt(true);
        let response = b"TX1;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_ptt(true).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_ptt_off() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_ptt(false);
        let response = b"TX0;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_ptt(false).await.unwrap();
    }

    // -----------------------------------------------------------------
    // Power
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_power() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_power();
        let response = b"PC050;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let watts = rig.get_power().await.unwrap();
        assert!((watts - 50.0).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_set_power() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_power(75);
        let response = b"PC075;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_power(75.0).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_power_out_of_range() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        // TS-890S max is 100W.
        let result = rig.set_power(200.0).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------
    // S-meter
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_s_meter() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_s_meter();
        // SM00015; -> selector 0, value 0015 (approximately S9)
        let response = b"SM00015;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let dbm = rig.get_s_meter(ReceiverId::VFO_A).await.unwrap();
        // S9 should be approximately -73 dBm
        assert!(
            dbm < -70.0 && dbm > -76.0,
            "S9 should be near -73 dBm, got {dbm}"
        );
    }

    // -----------------------------------------------------------------
    // SWR
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_swr() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_swr();
        // RM10000; -> selector 1, value 0000 (1.0:1 SWR)
        let response = b"RM10000;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let swr = rig.get_swr().await.unwrap();
        assert!(
            (swr - 1.0).abs() < 0.1,
            "zero reading should be ~1.0:1 SWR, got {swr}"
        );
    }

    // -----------------------------------------------------------------
    // ALC
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_alc() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_alc();
        // RM20015; -> selector 2, value 0015
        let response = b"RM20015;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let alc = rig.get_alc().await.unwrap();
        let expected = 15.0 / 30.0;
        assert!(
            (alc - expected).abs() < 0.01,
            "expected ~{expected}, got {alc}"
        );
    }

    // -----------------------------------------------------------------
    // Split
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_split_on() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_tx_vfo();
        let response = b"FT1;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let split = rig.get_split().await.unwrap();
        assert!(split);
    }

    #[tokio::test]
    async fn test_get_split_off() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_tx_vfo();
        let response = b"FT0;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let split = rig.get_split().await.unwrap();
        assert!(!split);
    }

    #[tokio::test]
    async fn test_set_split_on() {
        let mut mock = MockTransport::new();

        let (fr_cmd, ft_cmd) = commands::cmd_set_split(true);
        mock.expect(&fr_cmd, b"FR0;".as_ref());
        mock.expect(&ft_cmd, b"FT1;".as_ref());

        let rig = make_test_rig(mock);
        rig.set_split(true).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_split_off() {
        let mut mock = MockTransport::new();

        let (fr_cmd, ft_cmd) = commands::cmd_set_split(false);
        mock.expect(&fr_cmd, b"FR0;".as_ref());
        mock.expect(&ft_cmd, b"FT0;".as_ref());

        let rig = make_test_rig(mock);
        rig.set_split(false).await.unwrap();
    }

    // -----------------------------------------------------------------
    // info / capabilities
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_info() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let info = rig.info();
        assert_eq!(info.manufacturer, Manufacturer::Kenwood);
        assert_eq!(info.model_name, "TS-890S");
        assert_eq!(info.model_id, "TS-890S");
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
    async fn test_receivers_single_rx() {
        let mock = MockTransport::new();
        let rig = make_single_rx_rig(mock);
        let rxs = rig.receivers().await.unwrap();
        assert_eq!(rxs, vec![ReceiverId::VFO_A]);
    }

    #[tokio::test]
    async fn test_primary_receiver() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        assert_eq!(rig.primary_receiver().await.unwrap(), ReceiverId::VFO_A);
    }

    #[tokio::test]
    async fn test_secondary_receiver_dual() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        assert_eq!(
            rig.secondary_receiver().await.unwrap(),
            Some(ReceiverId::VFO_B)
        );
    }

    #[tokio::test]
    async fn test_secondary_receiver_single() {
        let mock = MockTransport::new();
        let rig = make_single_rx_rig(mock);
        assert_eq!(rig.secondary_receiver().await.unwrap(), None);
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
    // Event subscription receives events
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_events_emitted_on_set_frequency() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_frequency_a(14_074_000);
        let response = b"FA00014074000;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_frequency(ReceiverId::VFO_A, 14_074_000)
            .await
            .unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::FrequencyChanged { receiver, freq_hz } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(freq_hz, 14_074_000);
            }
            other => panic!("expected FrequencyChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_events_emitted_on_set_ptt() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_ptt(true);
        let response = b"TX1;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_ptt(true).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::PttChanged { on } => {
                assert!(on);
            }
            other => panic!("expected PttChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_events_emitted_on_set_split() {
        let mut mock = MockTransport::new();

        let (fr_cmd, ft_cmd) = commands::cmd_set_split(true);
        mock.expect(&fr_cmd, b"FR0;".as_ref());
        mock.expect(&ft_cmd, b"FT1;".as_ref());

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_split(true).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::SplitChanged { on } => {
                assert!(on);
            }
            other => panic!("expected SplitChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // CW speed
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_cw_speed() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_cw_speed();
        let response = b"KS025;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let wpm = rig.get_cw_speed().await.unwrap();
        assert_eq!(wpm, 25);
    }

    #[tokio::test]
    async fn test_set_cw_speed() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_cw_speed(25);
        let response = b"KS025;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_cw_speed(25).await.unwrap();
    }

    #[tokio::test]
    async fn test_events_emitted_on_set_cw_speed() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_cw_speed(30);
        let response = b"KS030;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_cw_speed(30).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::CwSpeedChanged { wpm } => {
                assert_eq!(wpm, 30);
            }
            other => panic!("expected CwSpeedChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // AGC — TS-890S (GcSimple)
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_agc_890_fast() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_agc_mode();
        mock.expect(&cmd, b"GC3;");

        let rig = make_test_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Fast);
    }

    #[tokio::test]
    async fn test_get_agc_890_off() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_agc_mode();
        mock.expect(&cmd, b"GC0;");

        let rig = make_test_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Off);
    }

    #[tokio::test]
    async fn test_set_agc_890_slow() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_agc_mode(1);
        mock.expect(&cmd, b"GC1;");

        let rig = make_test_rig(mock);
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Slow).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_agc_890_fast() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_agc_mode(3);
        mock.expect(&cmd, b"GC3;");

        let rig = make_test_rig(mock);
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Fast).await.unwrap();
    }

    // -----------------------------------------------------------------
    // AGC — TS-990S (GcVfo)
    // -----------------------------------------------------------------

    fn make_ts990_rig(mock: MockTransport) -> KenwoodRig {
        use crate::models::ts_990s;
        KenwoodRig::new(
            Box::new(mock),
            ts_990s(),
            true,
            3,
            Duration::from_millis(500),
            PttMethod::Cat,
            KeyLine::None,
            #[cfg(feature = "audio")]
            None,
        )
    }

    #[tokio::test]
    async fn test_get_agc_990_main_fast() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_agc_mode_vfo(0);
        // Response: GC03; → vfo=0, mode=3 (Fast)
        mock.expect(&cmd, b"GC03;");

        let rig = make_ts990_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Fast);
    }

    #[tokio::test]
    async fn test_get_agc_990_sub_slow() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_agc_mode_vfo(1);
        // Response: GC11; → vfo=1, mode=1 (Slow)
        mock.expect(&cmd, b"GC11;");

        let rig = make_ts990_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_B).await.unwrap();
        assert_eq!(mode, AgcMode::Slow);
    }

    #[tokio::test]
    async fn test_set_agc_990_sub_medium() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_agc_mode_vfo(1, 2);
        mock.expect(&cmd, b"GC12;");

        let rig = make_ts990_rig(mock);
        rig.set_agc(ReceiverId::VFO_B, AgcMode::Medium)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------
    // AGC — TS-590S (GtTimeConstant)
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_agc_590_fast() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_agc_time_constant();
        mock.expect(&cmd, b"GT005;");

        let rig = make_single_rx_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Fast);
    }

    #[tokio::test]
    async fn test_get_agc_590_off() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_agc_time_constant();
        mock.expect(&cmd, b"GT000;");

        let rig = make_single_rx_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Off);
    }

    #[tokio::test]
    async fn test_get_agc_590_slow() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_agc_time_constant();
        mock.expect(&cmd, b"GT020;");

        let rig = make_single_rx_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Slow);
    }

    #[tokio::test]
    async fn test_set_agc_590_slow() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_agc_time_constant(20);
        mock.expect(&cmd, b"GT020;");

        let rig = make_single_rx_rig(mock);
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Slow).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_agc_590_off() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_agc_time_constant(0);
        mock.expect(&cmd, b"GT000;");

        let rig = make_single_rx_rig(mock);
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Off).await.unwrap();
    }

    // -----------------------------------------------------------------
    // AGC events
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_agc_emits_event() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_agc_mode(3);
        mock.expect(&cmd, b"GC3;");

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_agc(ReceiverId::VFO_A, AgcMode::Fast).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::AgcChanged { receiver, mode } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(mode, AgcMode::Fast);
            }
            other => panic!("expected AgcChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Preamp — TS-890S (GcSimple, has_preamp2 = false)
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_preamp_off() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_preamp();
        mock.expect(&cmd, b"PA0;");

        let rig = make_test_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Off);
    }

    #[tokio::test]
    async fn test_get_preamp_1() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_preamp();
        mock.expect(&cmd, b"PA1;");

        let rig = make_test_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Preamp1);
    }

    #[tokio::test]
    async fn test_get_preamp_2() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_preamp();
        mock.expect(&cmd, b"PA2;");

        // Use TS-990S which supports Preamp2
        let rig = make_ts990_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Preamp2);
    }

    #[tokio::test]
    async fn test_set_preamp_off() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_preamp(0);
        mock.expect(&cmd, b"PA0;");

        let rig = make_test_rig(mock);
        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Off)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_preamp_1() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_preamp(1);
        mock.expect(&cmd, b"PA1;");

        let rig = make_test_rig(mock);
        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp1)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_preamp_2_on_990() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_preamp(2);
        mock.expect(&cmd, b"PA2;");

        // TS-990S has_preamp2 = true, so this should succeed
        let rig = make_ts990_rig(mock);
        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp2)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_preamp_2_on_890_returns_unsupported() {
        // TS-890S has_preamp2 = false
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let result = rig
            .set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp2)
            .await;
        match result {
            Err(Error::Unsupported(msg)) => {
                assert!(msg.contains("does not support Preamp 2"));
            }
            Err(other) => panic!("expected Unsupported error, got: {other:?}"),
            Ok(()) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn test_set_preamp_2_on_590_returns_unsupported() {
        // TS-590S has_preamp2 = false
        let mock = MockTransport::new();
        let rig = make_single_rx_rig(mock);
        let result = rig
            .set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp2)
            .await;
        match result {
            Err(Error::Unsupported(msg)) => {
                assert!(msg.contains("does not support Preamp 2"));
            }
            Err(other) => panic!("expected Unsupported error, got: {other:?}"),
            Ok(()) => panic!("expected error, got Ok"),
        }
    }

    #[tokio::test]
    async fn test_preamp_emits_event_on_get() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_preamp();
        mock.expect(&cmd, b"PA1;");

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Preamp1);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::PreampChanged { receiver, level } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(level, PreampLevel::Preamp1);
            }
            other => panic!("expected PreampChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_preamp_emits_event_on_set() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_preamp(1);
        mock.expect(&cmd, b"PA1;");

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
        let cmd = commands::cmd_read_attenuator();
        mock.expect(&cmd, b"RA00;");

        let rig = make_test_rig(mock);
        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(db, 0);
    }

    #[tokio::test]
    async fn test_get_attenuator_6db() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_attenuator();
        mock.expect(&cmd, b"RA06;");

        let rig = make_test_rig(mock);
        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(db, 6);
    }

    #[tokio::test]
    async fn test_get_attenuator_12db() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_attenuator();
        mock.expect(&cmd, b"RA12;");

        let rig = make_test_rig(mock);
        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(db, 12);
    }

    #[tokio::test]
    async fn test_get_attenuator_18db() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_attenuator();
        mock.expect(&cmd, b"RA18;");

        let rig = make_test_rig(mock);
        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(db, 18);
    }

    #[tokio::test]
    async fn test_set_attenuator_off() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_attenuator(0);
        mock.expect(&cmd, b"RA00;");

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 0).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_attenuator_6db() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_attenuator(6);
        mock.expect(&cmd, b"RA06;");

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 6).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_attenuator_12db() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_attenuator(12);
        mock.expect(&cmd, b"RA12;");

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 12).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_attenuator_18db() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_attenuator(18);
        mock.expect(&cmd, b"RA18;");

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 18).await.unwrap();
    }

    #[tokio::test]
    async fn test_attenuator_emits_event_on_get() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_read_attenuator();
        mock.expect(&cmd, b"RA12;");

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(db, 12);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::AttenuatorChanged { receiver, db } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(db, 12);
            }
            other => panic!("expected AttenuatorChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_attenuator_emits_event_on_set() {
        let mut mock = MockTransport::new();
        let cmd = commands::cmd_set_attenuator(18);
        mock.expect(&cmd, b"RA18;");

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_attenuator(ReceiverId::VFO_A, 18).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::AttenuatorChanged { receiver, db } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(db, 18);
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

        // First command: read RIT on/off
        let rit_cmd = commands::cmd_read_rit();
        mock.expect(&rit_cmd, b"RT1;");

        // Second command: read RIT/XIT offset
        let offset_cmd = commands::cmd_read_rit_xit_offset();
        mock.expect(&offset_cmd, b"RO+00050;");

        let rig = make_test_rig(mock);
        let (enabled, offset) = rig.get_rit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset, 50);
    }

    #[tokio::test]
    async fn test_get_rit_off() {
        let mut mock = MockTransport::new();

        let rit_cmd = commands::cmd_read_rit();
        mock.expect(&rit_cmd, b"RT0;");

        let offset_cmd = commands::cmd_read_rit_xit_offset();
        mock.expect(&offset_cmd, b"RO+00000;");

        let rig = make_test_rig(mock);
        let (enabled, offset) = rig.get_rit().await.unwrap();
        assert!(!enabled);
        assert_eq!(offset, 0);
    }

    #[tokio::test]
    async fn test_get_rit_negative() {
        let mut mock = MockTransport::new();

        let rit_cmd = commands::cmd_read_rit();
        mock.expect(&rit_cmd, b"RT1;");

        let offset_cmd = commands::cmd_read_rit_xit_offset();
        mock.expect(&offset_cmd, b"RO-00120;");

        let rig = make_test_rig(mock);
        let (enabled, offset) = rig.get_rit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset, -120);
    }

    #[tokio::test]
    async fn test_set_rit() {
        // TS-890S has rit_step_hz=1, so offset +3 means 3 x RU;
        let mut mock = MockTransport::new();

        // 1. Set RIT on
        let rit_on_cmd = commands::cmd_set_rit_on(true);
        mock.expect(&rit_on_cmd, b"RT1;");

        // 2. Clear offset
        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"RC;");

        // 3. Three RU; commands for +3 Hz
        let up_cmd = commands::cmd_rit_up();
        mock.expect(&up_cmd, b"RU;");
        mock.expect(&up_cmd, b"RU;");
        mock.expect(&up_cmd, b"RU;");

        let rig = make_test_rig(mock);
        rig.set_rit(true, 3).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_rit_disable() {
        let mut mock = MockTransport::new();

        // 1. Set RIT off
        let rit_off_cmd = commands::cmd_set_rit_on(false);
        mock.expect(&rit_off_cmd, b"RT0;");

        // 2. Clear offset
        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"RC;");

        // No RU/RD commands since offset is 0
        let rig = make_test_rig(mock);
        rig.set_rit(false, 0).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_rit_negative() {
        // TS-890S has rit_step_hz=1, so offset -3 means 3 x RD;
        let mut mock = MockTransport::new();

        // 1. Set RIT on
        let rit_on_cmd = commands::cmd_set_rit_on(true);
        mock.expect(&rit_on_cmd, b"RT1;");

        // 2. Clear offset
        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"RC;");

        // 3. Three RD; commands for -3 Hz
        let down_cmd = commands::cmd_rit_down();
        mock.expect(&down_cmd, b"RD;");
        mock.expect(&down_cmd, b"RD;");
        mock.expect(&down_cmd, b"RD;");

        let rig = make_test_rig(mock);
        rig.set_rit(true, -3).await.unwrap();
    }

    #[tokio::test]
    async fn test_rit_emits_event() {
        let mut mock = MockTransport::new();

        // set_rit(true, 0) — enable with zero offset
        let rit_on_cmd = commands::cmd_set_rit_on(true);
        mock.expect(&rit_on_cmd, b"RT1;");

        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"RC;");

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        rig.set_rit(true, 0).await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::RitChanged { enabled, offset_hz } => {
                assert!(enabled);
                assert_eq!(offset_hz, 0);
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

        let xit_cmd = commands::cmd_read_xit();
        mock.expect(&xit_cmd, b"XT1;");

        let offset_cmd = commands::cmd_read_rit_xit_offset();
        mock.expect(&offset_cmd, b"RO+00100;");

        let rig = make_test_rig(mock);
        let (enabled, offset) = rig.get_xit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset, 100);
    }

    #[tokio::test]
    async fn test_set_xit() {
        // TS-890S has rit_step_hz=1, so offset +2 means 2 x RU;
        let mut mock = MockTransport::new();

        // 1. Set XIT on
        let xit_on_cmd = commands::cmd_set_xit_on(true);
        mock.expect(&xit_on_cmd, b"XT1;");

        // 2. Clear offset
        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"RC;");

        // 3. Two RU; commands for +2 Hz
        let up_cmd = commands::cmd_rit_up();
        mock.expect(&up_cmd, b"RU;");
        mock.expect(&up_cmd, b"RU;");

        let rig = make_test_rig(mock);
        rig.set_xit(true, 2).await.unwrap();
    }

    // -----------------------------------------------------------------
    // VFO A=B
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_set_vfo_a_eq_b() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_vfo_a_eq_b();
        let response = b"AB;".to_vec();
        mock.expect(&cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_vfo_a_eq_b(ReceiverId::VFO_A).await.unwrap();
    }

    // -----------------------------------------------------------------
    // swap_vfo (unsupported)
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_swap_vfo_unsupported() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let result = rig.swap_vfo(ReceiverId::VFO_A).await;
        match result {
            Err(Error::Unsupported(msg)) => {
                assert!(msg.contains("Kenwood does not support VFO swap"));
            }
            Err(other) => panic!("expected Unsupported error, got: {other:?}"),
            Ok(()) => panic!("expected error, got Ok"),
        }
    }

    // -----------------------------------------------------------------
    // Antenna
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_antenna() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_antenna();
        let response = b"AN1;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let port = rig.get_antenna(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(port, AntennaPort::Ant1);
    }

    #[tokio::test]
    async fn test_get_antenna_2() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_antenna();
        let response = b"AN2;".to_vec();
        mock.expect(&read_cmd, &response);

        let rig = make_test_rig(mock);
        let port = rig.get_antenna(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(port, AntennaPort::Ant2);
    }

    #[tokio::test]
    async fn test_set_antenna() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_antenna(1);
        let response = b"AN1;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_antenna(ReceiverId::VFO_A, AntennaPort::Ant1)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_antenna_2() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_antenna(2);
        let response = b"AN2;".to_vec();
        mock.expect(&set_cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_antenna(ReceiverId::VFO_A, AntennaPort::Ant2)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------
    // CW messages
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_send_cw_message() {
        let mut mock = MockTransport::new();

        // Buffer query: KY0; means buffer ready
        let buf_cmd = commands::cmd_read_cw_buffer();
        mock.expect(&buf_cmd, b"KY0;");

        // Send command: Kenwood echoes back the command
        let send_cmd = commands::cmd_send_cw_message("CQ DE W1AW");
        mock.expect(&send_cmd, &send_cmd);

        let rig = make_test_rig(mock);
        rig.send_cw_message("CQ DE W1AW").await.unwrap();
    }

    #[tokio::test]
    async fn test_send_cw_message_chunked() {
        let mut mock = MockTransport::new();

        // Message longer than 24 chars forces two chunks.
        let message = "CQ CQ CQ DE W1AW W1AW W1AW K";
        let chars: Vec<char> = message.chars().collect();
        let chunk1: String = chars[..24].iter().collect();
        let chunk2: String = chars[24..].iter().collect();

        // Chunk 1: buffer query + send
        let buf_cmd = commands::cmd_read_cw_buffer();
        mock.expect(&buf_cmd, b"KY0;");
        let send_cmd1 = commands::cmd_send_cw_message(&chunk1);
        mock.expect(&send_cmd1, &send_cmd1);

        // Chunk 2: buffer query + send
        let buf_cmd2 = commands::cmd_read_cw_buffer();
        mock.expect(&buf_cmd2, b"KY0;");
        let send_cmd2 = commands::cmd_send_cw_message(&chunk2);
        mock.expect(&send_cmd2, &send_cmd2);

        let rig = make_test_rig(mock);
        rig.send_cw_message(message).await.unwrap();
    }

    #[tokio::test]
    async fn test_stop_cw_message() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_stop_cw_message();
        mock.expect(&cmd, &cmd);

        let rig = make_test_rig(mock);
        rig.stop_cw_message().await.unwrap();
    }

    #[tokio::test]
    async fn test_send_cw_message_empty() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        rig.send_cw_message("").await.unwrap();
    }

    // -----------------------------------------------------------------
    // Error handling
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_error_response() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_read_mode();
        let response = b"?;".to_vec();
        mock.expect(&cmd, &response);

        let rig = make_test_rig(mock);
        let result = rig.get_mode(ReceiverId::VFO_A).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------
    // TX receiver selection
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_set_tx_receiver_vfo_a() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_set_tx_vfo(false); // VFO A
        let response = b"FT0;".to_vec();
        mock.expect(&cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_tx_receiver(ReceiverId::VFO_A).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_tx_receiver_vfo_b() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_set_tx_vfo(true); // VFO B
        let response = b"FT1;".to_vec();
        mock.expect(&cmd, &response);

        let rig = make_test_rig(mock);
        rig.set_tx_receiver(ReceiverId::VFO_B).await.unwrap();
    }

    // -----------------------------------------------------------------
    // AudioCapable (feature = "audio")
    // -----------------------------------------------------------------

    #[cfg(feature = "audio")]
    mod audio_tests {
        use super::*;
        use riglib_core::audio::{AudioCapable, AudioSampleFormat};

        fn make_audio_rig(mock: MockTransport, device_name: Option<&str>) -> KenwoodRig {
            use crate::models::ts_890s;
            KenwoodRig::new(
                Box::new(mock),
                ts_890s(),
                true,
                3,
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
            rig.stop_audio().await.unwrap();
        }
    }
}
