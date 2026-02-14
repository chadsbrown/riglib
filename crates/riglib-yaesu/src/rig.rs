//! YaesuRig -- the [`Rig`] trait implementation for Yaesu transceivers.
//!
//! This module ties the CAT text-protocol engine ([`protocol`], [`commands`])
//! to a [`Transport`] to produce a working Yaesu backend. It handles command
//! framing, semicolon-delimited response parsing, retry logic, and VFO
//! selection for both single- and dual-receiver rigs.
//!
//! The Yaesu CAT protocol is text-based, very similar to Kenwood's protocol.
//! Key differences:
//! - Frequencies are 9-digit zero-padded Hz (Kenwood uses 11)
//! - Mode codes differ and use per-VFO prefixes (MD0/MD1)
//! - AI (Auto Information) mode pushes unsolicited state changes

use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::broadcast;
use tracing::debug;

use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;
use riglib_core::rig::Rig;
use riglib_core::transport::Transport;
use riglib_core::types::*;

#[cfg(feature = "audio")]
use riglib_core::audio::{AudioCapable, AudioReceiver, AudioSender, AudioStreamConfig};

use crate::commands;
use crate::models::YaesuModel;

// ---------------------------------------------------------------------------
// YaesuAiHandler -- processes unsolicited AI frames from Yaesu rigs
// ---------------------------------------------------------------------------

/// AI handler for Yaesu rigs.
///
/// Processes unsolicited Auto Information frames and emits the corresponding
/// [`RigEvent`]s: FA/FB -> FrequencyChanged, MD0/MD1 -> ModeChanged,
/// TX -> PttChanged, RT -> RitChanged, XT -> XitChanged.
struct YaesuAiHandler;

impl riglib_text_io::io::AiHandler for YaesuAiHandler {
    fn process(&self, prefix: &str, data: &str, event_tx: &broadcast::Sender<RigEvent>) {
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
            "RT" => {
                // AI mode sends RT0; (RIT off) or RT1; (RIT on).
                // The offset is not included -- only the on/off state.
                let enabled = data == "1";
                debug!(enabled, "AI RIT update");
                let _ = event_tx.send(RigEvent::RitChanged {
                    enabled,
                    offset_hz: 0,
                });
            }
            "XT" => {
                // AI mode sends XT0; (XIT off) or XT1; (XIT on).
                let enabled = data == "1";
                debug!(enabled, "AI XIT update");
                let _ = event_tx.send(RigEvent::XitChanged {
                    enabled,
                    offset_hz: 0,
                });
            }
            _ => {
                debug!(prefix, data, "unknown AI prefix, ignoring");
            }
        }
    }
}

/// A connected Yaesu transceiver controlled over CAT.
///
/// Constructed via [`YaesuBuilder`](crate::builder::YaesuBuilder). All rig
/// communication goes through the [`Transport`] provided at build time.
pub struct YaesuRig {
    io: riglib_text_io::io::RigIo,
    model: YaesuModel,
    event_tx: broadcast::Sender<RigEvent>,
    command_timeout: Duration,
    info: RigInfo,
    ptt_method: PttMethod,
    key_line: KeyLine,
    set_command_mode: SetCommandMode,
    /// USB audio device name (e.g. "USB Audio CODEC"). When set, this rig
    /// supports audio streaming via the `AudioCapable` trait.
    #[cfg(feature = "audio")]
    audio_device_name: Option<String>,
    /// Active cpal audio backend, created on first `start_rx_audio()` or
    /// `start_tx_audio()` call.
    #[cfg(feature = "audio")]
    audio_backend: tokio::sync::Mutex<Option<riglib_transport::CpalAudioBackend>>,
}

impl YaesuRig {
    /// Create a new `YaesuRig` from its constituent parts.
    ///
    /// This is called by [`YaesuBuilder`](crate::builder::YaesuBuilder);
    /// callers should use the builder API instead.
    ///
    /// Spawns the IO task immediately. The IO task owns the transport
    /// exclusively and processes all command/response exchanges.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        transport: Box<dyn Transport>,
        model: YaesuModel,
        auto_retry: bool,
        max_retries: u32,
        command_timeout: Duration,
        ptt_method: PttMethod,
        key_line: KeyLine,
        set_command_mode: SetCommandMode,
        #[cfg(feature = "audio")] audio_device_name: Option<String>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let info = RigInfo {
            manufacturer: Manufacturer::Yaesu,
            model_name: model.name.to_string(),
            model_id: model.name.to_string(),
        };

        let config = riglib_text_io::io::IoConfig {
            ai_enabled: false,
            command_timeout,
            auto_retry,
            max_retries,
            set_drain_timeout: Duration::from_millis(50),
            digit_suffix_prefixes: &["MD", "RM", "SM", "SH", "NA", "AN", "PA", "RA"],
            ai_prefixes: &["FA", "FB", "MD0", "MD1", "TX", "RT", "XT"],
            shutdown_command: Some(b"AI0;"),
        };

        let io = riglib_text_io::io::spawn_io_task(
            transport,
            config,
            event_tx.clone(),
            Box::new(YaesuAiHandler),
        );

        YaesuRig {
            io,
            model,
            event_tx,
            command_timeout,
            info,
            ptt_method,
            key_line,
            set_command_mode,
            #[cfg(feature = "audio")]
            audio_device_name,
            #[cfg(feature = "audio")]
            audio_backend: tokio::sync::Mutex::new(None),
        }
    }

    /// Send a CAT command and wait for the rig's response.
    ///
    /// Dispatches via the IO task's background channel.
    ///
    /// Returns the decoded prefix and data on success.
    async fn execute_command(&self, cmd: &[u8]) -> Result<(String, String)> {
        self.io.command(cmd.to_vec(), self.command_timeout).await
    }

    /// Send a SET command via the IO task.
    ///
    /// The IO task handles the 50ms drain to catch `?;` errors.
    async fn execute_set_command(&self, cmd: &[u8]) -> Result<()> {
        self.io
            .set_command(cmd.to_vec(), self.command_timeout)
            .await
    }

    /// Convert a raw Yaesu S-meter reading (0-255) to approximate dBm.
    ///
    /// Uses the standard IARU S-meter scale where S9 = -73 dBm and each
    /// S-unit is ~6 dB. The Yaesu meter range 0-255 maps roughly:
    /// 0 = S0 (~-127 dBm), 120 = S9 (-73 dBm), 120-255 = S9 to S9+60 dB.
    fn meter_to_dbm(raw: u16) -> f32 {
        let raw = raw as f32;
        if raw <= 120.0 {
            // S0 to S9: linear mapping over 0..120 => -127..-73 dBm
            -127.0 + (raw / 120.0) * 54.0
        } else {
            // S9 to S9+60: linear mapping over 120..255 => -73..-13 dBm
            -73.0 + ((raw - 120.0) / 135.0) * 60.0
        }
    }
}

#[async_trait]
impl Rig for YaesuRig {
    fn info(&self) -> &RigInfo {
        &self.info
    }

    fn capabilities(&self) -> &RigCapabilities {
        &self.model.capabilities
    }

    async fn receivers(&self) -> Result<Vec<ReceiverId>> {
        let mut rxs = vec![ReceiverId::VFO_A];
        if self.model.capabilities.max_receivers >= 2 {
            rxs.push(ReceiverId::VFO_B);
        }
        Ok(rxs)
    }

    async fn primary_receiver(&self) -> Result<ReceiverId> {
        Ok(ReceiverId::VFO_A)
    }

    async fn secondary_receiver(&self) -> Result<Option<ReceiverId>> {
        if self.model.capabilities.max_receivers >= 2 {
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
        let (_prefix, data) = self.execute_command(&cmd).await?;
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
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_frequency(rx).await?;
            if actual != freq_hz {
                return Err(Error::Protocol(format!(
                    "set_frequency verify failed: expected {freq_hz}, got {actual}"
                )));
            }
            // get_frequency already emitted the event
        } else {
            let _ = self.event_tx.send(RigEvent::FrequencyChanged {
                receiver: rx,
                freq_hz,
            });
        }
        Ok(())
    }

    async fn get_mode(&self, rx: ReceiverId) -> Result<Mode> {
        let cmd = if rx == ReceiverId::VFO_B {
            commands::cmd_read_mode_b()
        } else {
            commands::cmd_read_mode_a()
        };
        debug!(receiver = %rx, "reading mode");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let mode = commands::parse_mode_response(&data)?;
        let _ = self
            .event_tx
            .send(RigEvent::ModeChanged { receiver: rx, mode });
        Ok(mode)
    }

    async fn set_mode(&self, rx: ReceiverId, mode: Mode) -> Result<()> {
        let cmd = if rx == ReceiverId::VFO_B {
            commands::cmd_set_mode_b(&mode)
        } else {
            commands::cmd_set_mode_a(&mode)
        };
        debug!(receiver = %rx, %mode, "setting mode");
        self.execute_set_command(&cmd).await?;
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_mode(rx).await?;
            if actual != mode {
                return Err(Error::Protocol(format!(
                    "set_mode verify failed: expected {mode}, got {actual}"
                )));
            }
        } else {
            let _ = self
                .event_tx
                .send(RigEvent::ModeChanged { receiver: rx, mode });
        }
        Ok(())
    }

    async fn get_passband(&self, _rx: ReceiverId) -> Result<Passband> {
        Err(Error::Unsupported(
            "Yaesu passband read not yet implemented (model-dependent)".into(),
        ))
    }

    async fn set_passband(&self, _rx: ReceiverId, _pb: Passband) -> Result<()> {
        Err(Error::Unsupported(
            "Yaesu passband set not yet implemented (model-dependent)".into(),
        ))
    }

    async fn get_ptt(&self) -> Result<bool> {
        let cmd = commands::cmd_read_ptt();
        debug!("reading PTT state");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let on = commands::parse_ptt_response(&data)?;
        Ok(on)
    }

    async fn set_ptt(&self, on: bool) -> Result<()> {
        match self.ptt_method {
            PttMethod::Cat => {
                let cmd = commands::cmd_set_ptt(on);
                debug!(on, "setting PTT via CAT");
                self.io
                    .rt_set_command(cmd.to_vec(), self.command_timeout)
                    .await?;
            }
            PttMethod::Dtr => {
                debug!(on, "setting PTT via DTR");
                self.io.rt_set_line(true, on).await?;
            }
            PttMethod::Rts => {
                debug!(on, "setting PTT via RTS");
                self.io.rt_set_line(false, on).await?;
            }
        }
        let _ = self.event_tx.send(RigEvent::PttChanged { on });
        Ok(())
    }

    async fn get_power(&self) -> Result<f32> {
        let cmd = commands::cmd_read_power();
        debug!("reading power level");
        let (_prefix, data) = self.execute_command(&cmd).await?;
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
        let watts_int = watts.round() as u16;
        let cmd = commands::cmd_set_power(watts_int);
        debug!(watts, watts_int, "setting power");
        self.execute_set_command(&cmd).await?;
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_power().await?;
            let expected = watts_int as f32;
            if (actual - expected).abs() > 1.0 {
                return Err(Error::Protocol(format!(
                    "set_power verify failed: expected {expected}W, got {actual}W"
                )));
            }
        }
        Ok(())
    }

    async fn get_s_meter(&self, rx: ReceiverId) -> Result<f32> {
        // Yaesu SM0 always reads the main receiver's S-meter.
        // For VFO B on dual-receiver rigs, the protocol would use SM1,
        // but for simplicity we use SM0 for all cases currently.
        let cmd = commands::cmd_read_s_meter();
        debug!(receiver = %rx, "reading S-meter");
        let (_prefix, data) = self.execute_command(&cmd).await?;
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
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_meter_response(&data)?;
        Ok(raw as f32)
    }

    async fn get_alc(&self) -> Result<f32> {
        let cmd = commands::cmd_read_alc();
        debug!("reading ALC");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_meter_response(&data)?;
        Ok(raw as f32)
    }

    async fn get_split(&self) -> Result<bool> {
        let cmd = commands::cmd_read_split();
        debug!("reading split state");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let on = commands::parse_split_response(&data)?;
        Ok(on)
    }

    async fn set_split(&self, on: bool) -> Result<()> {
        let cmd = commands::cmd_set_split(on);
        debug!(on, "setting split");
        self.execute_set_command(&cmd).await?;
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_split().await?;
            if actual != on {
                return Err(Error::Protocol(format!(
                    "set_split verify failed: expected {on}, got {actual}"
                )));
            }
            // get_split doesn't emit SplitChanged, so emit it here.
            let _ = self.event_tx.send(RigEvent::SplitChanged { on });
        } else {
            let _ = self.event_tx.send(RigEvent::SplitChanged { on });
        }
        Ok(())
    }

    async fn set_tx_receiver(&self, rx: ReceiverId) -> Result<()> {
        // For Yaesu, setting the TX receiver is done via split mode.
        // If the requested TX receiver is not the primary (VFO A),
        // enable split so VFO B is used for TX. If it is the primary,
        // disable split.
        let on = rx != ReceiverId::VFO_A;
        let cmd = commands::cmd_set_split(on);
        debug!(receiver = %rx, split = on, "setting TX receiver via split");
        self.execute_set_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::SplitChanged { on });
        Ok(())
    }

    async fn set_cw_key(&self, on: bool) -> Result<()> {
        match self.key_line {
            KeyLine::None => Err(Error::Unsupported(
                "no CW key line configured (use builder's key_line() method)".into(),
            )),
            KeyLine::Dtr => {
                debug!(on, "CW key via DTR");
                self.io.rt_set_line(true, on).await
            }
            KeyLine::Rts => {
                debug!(on, "CW key via RTS");
                self.io.rt_set_line(false, on).await
            }
        }
    }

    async fn get_cw_speed(&self) -> Result<u8> {
        let cmd = commands::cmd_read_cw_speed();
        debug!("reading CW speed");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let wpm = commands::parse_cw_speed_response(&data)?;
        let _ = self.event_tx.send(RigEvent::CwSpeedChanged { wpm });
        Ok(wpm)
    }

    async fn set_cw_speed(&self, wpm: u8) -> Result<()> {
        let cmd = commands::cmd_set_cw_speed(wpm);
        debug!(wpm, "setting CW speed");
        self.execute_set_command(&cmd).await?;
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_cw_speed().await?;
            if actual != wpm {
                return Err(Error::Protocol(format!(
                    "set_cw_speed verify failed: expected {wpm}, got {actual}"
                )));
            }
        } else {
            let _ = self.event_tx.send(RigEvent::CwSpeedChanged { wpm });
        }
        Ok(())
    }

    async fn get_agc(&self, _rx: ReceiverId) -> Result<AgcMode> {
        let cmd = commands::cmd_read_agc();
        debug!("reading AGC mode");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_agc_response(&data)?;
        let mode = match raw {
            0 => AgcMode::Off,
            1 => AgcMode::Fast,
            2 => AgcMode::Medium,
            3 => AgcMode::Slow,
            other => {
                return Err(Error::Protocol(format!(
                    "unknown Yaesu AGC mode value: {other}"
                )));
            }
        };
        let _ = self.event_tx.send(RigEvent::AgcChanged {
            receiver: _rx,
            mode,
        });
        Ok(mode)
    }

    async fn set_agc(&self, _rx: ReceiverId, mode: AgcMode) -> Result<()> {
        let value = match mode {
            AgcMode::Off => 0,
            AgcMode::Fast => 1,
            AgcMode::Medium => 2,
            AgcMode::Slow => 3,
        };
        let cmd = commands::cmd_set_agc(value);
        debug!(?mode, "setting AGC mode");
        self.execute_set_command(&cmd).await?;
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_agc(_rx).await?;
            if actual != mode {
                return Err(Error::Protocol(format!(
                    "set_agc verify failed: expected {mode}, got {actual}"
                )));
            }
        } else {
            let _ = self.event_tx.send(RigEvent::AgcChanged {
                receiver: _rx,
                mode,
            });
        }
        Ok(())
    }

    async fn get_preamp(&self, rx: ReceiverId) -> Result<PreampLevel> {
        let cmd = commands::cmd_read_preamp();
        debug!(receiver = %rx, "reading preamp level");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_preamp_response(&data)?;

        let level = match raw {
            0 => PreampLevel::Off,
            1 => PreampLevel::Preamp1,
            2 => PreampLevel::Preamp2,
            other => {
                return Err(Error::Protocol(format!(
                    "unknown Yaesu preamp level value: {other}"
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
            return Err(Error::Unsupported(
                "Preamp2 not supported on this model".into(),
            ));
        }

        let value = match level {
            PreampLevel::Off => 0,
            PreampLevel::Preamp1 => 1,
            PreampLevel::Preamp2 => 2,
        };

        let cmd = commands::cmd_set_preamp(value);
        debug!(receiver = %rx, %level, "setting preamp level");
        self.execute_set_command(&cmd).await?;
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_preamp(rx).await?;
            if actual != level {
                return Err(Error::Protocol(format!(
                    "set_preamp verify failed: expected {level}, got {actual}"
                )));
            }
        } else {
            let _ = self.event_tx.send(RigEvent::PreampChanged {
                receiver: rx,
                level,
            });
        }
        Ok(())
    }

    async fn get_attenuator(&self, rx: ReceiverId) -> Result<u8> {
        let cmd = commands::cmd_read_attenuator();
        debug!(receiver = %rx, "reading attenuator level");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let raw = commands::parse_attenuator_response(&data)?;

        // Yaesu CAT attenuator: 0 = off, 1 = on.
        // Map to dB using the rig's capabilities. For rigs with
        // attenuator_levels [0, 6, 12, 18] a simple on/off maps to
        // the first non-zero step (6 dB). If capabilities are empty,
        // fall back to 0 dB.
        let db = if raw == 0 {
            0
        } else {
            // Use the first non-zero dB step from capabilities, or default to 6.
            self.model
                .capabilities
                .attenuator_levels
                .iter()
                .copied()
                .find(|&v| v > 0)
                .unwrap_or(6)
        };

        let _ = self
            .event_tx
            .send(RigEvent::AttenuatorChanged { receiver: rx, db });
        Ok(db)
    }

    async fn set_attenuator(&self, rx: ReceiverId, db: u8) -> Result<()> {
        // Yaesu CAT attenuator is binary on/off. 0 dB = off, any other = on.
        let value: u8 = if db == 0 { 0 } else { 1 };

        let cmd = commands::cmd_set_attenuator(value);
        debug!(receiver = %rx, db, "setting attenuator");
        self.execute_set_command(&cmd).await?;
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_attenuator(rx).await?;
            // Yaesu is binary: we just check on/off matches.
            let expected_on = db > 0;
            let actual_on = actual > 0;
            if expected_on != actual_on {
                return Err(Error::Protocol(format!(
                    "set_attenuator verify failed: expected {}dB, got {}dB",
                    db, actual
                )));
            }
        } else {
            let _ = self
                .event_tx
                .send(RigEvent::AttenuatorChanged { receiver: rx, db });
        }
        Ok(())
    }

    async fn get_rit(&self) -> Result<(bool, i32)> {
        let cmd = commands::cmd_read_information();
        debug!("reading RIT state via IF command");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let (enabled, offset_hz) = commands::parse_rit_from_info(&data)?;
        let _ = self
            .event_tx
            .send(RigEvent::RitChanged { enabled, offset_hz });
        Ok((enabled, offset_hz))
    }

    async fn set_rit(&self, enabled: bool, offset_hz: i32) -> Result<()> {
        // Set the on/off state
        let cmd = commands::cmd_set_rit_on(enabled);
        debug!(enabled, offset_hz, "setting RIT");
        self.execute_set_command(&cmd).await?;

        // Clear and set the shared offset register
        let clear_cmd = commands::cmd_rit_clear();
        self.execute_set_command(&clear_cmd).await?;

        if offset_hz != 0 {
            let abs_offset = offset_hz.unsigned_abs();
            let offset_cmd = if offset_hz > 0 {
                commands::cmd_rit_up(abs_offset)
            } else {
                commands::cmd_rit_down(abs_offset)
            };
            self.execute_set_command(&offset_cmd).await?;
        }

        let _ = self
            .event_tx
            .send(RigEvent::RitChanged { enabled, offset_hz });
        Ok(())
    }

    async fn get_xit(&self) -> Result<(bool, i32)> {
        let cmd = commands::cmd_read_information();
        debug!("reading XIT state via IF command");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let (enabled, offset_hz) = commands::parse_xit_from_info(&data)?;
        let _ = self
            .event_tx
            .send(RigEvent::XitChanged { enabled, offset_hz });
        Ok((enabled, offset_hz))
    }

    async fn set_xit(&self, enabled: bool, offset_hz: i32) -> Result<()> {
        let cmd = commands::cmd_set_xit_on(enabled);
        debug!(enabled, offset_hz, "setting XIT");
        self.execute_set_command(&cmd).await?;

        // Clear and set the shared offset register (same commands as RIT)
        let clear_cmd = commands::cmd_rit_clear();
        self.execute_set_command(&clear_cmd).await?;

        if offset_hz != 0 {
            let abs_offset = offset_hz.unsigned_abs();
            let offset_cmd = if offset_hz > 0 {
                commands::cmd_rit_up(abs_offset)
            } else {
                commands::cmd_rit_down(abs_offset)
            };
            self.execute_set_command(&offset_cmd).await?;
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
        let cmd = commands::cmd_vfo_swap();
        debug!("swapping VFOs");
        self.execute_set_command(&cmd).await
    }

    async fn get_antenna(&self, _receiver: ReceiverId) -> Result<AntennaPort> {
        let cmd = commands::cmd_read_antenna();
        debug!("reading antenna port");
        let (_prefix, data) = self.execute_command(&cmd).await?;
        let ant = commands::parse_antenna_response(&data)?;
        match ant {
            1 => Ok(AntennaPort::Ant1),
            2 => Ok(AntennaPort::Ant2),
            3 => Ok(AntennaPort::Ant3),
            _ => Err(Error::Protocol(format!(
                "unexpected antenna port number: {ant}"
            ))),
        }
    }

    async fn set_antenna(&self, _receiver: ReceiverId, port: AntennaPort) -> Result<()> {
        let ant = match port {
            AntennaPort::Ant1 => 1,
            AntennaPort::Ant2 => 2,
            AntennaPort::Ant3 => 3,
            AntennaPort::Ant4 => 4,
        };
        let cmd = commands::cmd_set_antenna(ant);
        debug!(?port, "setting antenna port");
        self.execute_set_command(&cmd).await?;
        if self.set_command_mode == SetCommandMode::Verify {
            let actual = self.get_antenna(_receiver).await?;
            if actual != port {
                return Err(Error::Protocol(format!(
                    "set_antenna verify failed: expected {port}, got {actual}"
                )));
            }
        }
        Ok(())
    }

    async fn send_cw_message(&self, message: &str) -> Result<()> {
        let _ = message;
        Err(Error::Unsupported(
            "Yaesu CAT does not support free-text CW message send in this backend".into(),
        ))
    }

    async fn stop_cw_message(&self) -> Result<()> {
        Err(Error::Unsupported(
            "Yaesu CAT does not support free-text CW message send in this backend".into(),
        ))
    }

    async fn enable_transceive(&self) -> Result<()> {
        self.io
            .set_command(b"AI2;".to_vec(), self.command_timeout)
            .await
    }

    async fn disable_transceive(&self) -> Result<()> {
        self.io
            .set_command(b"AI0;".to_vec(), self.command_timeout)
            .await
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
impl AudioCapable for YaesuRig {
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

    /// Helper to build a YaesuRig with a MockTransport and the FT-DX10 model.
    /// Uses NoVerify mode so existing tests don't need extra GET expectations.
    fn make_test_rig(mock: MockTransport) -> YaesuRig {
        use crate::models::ft_dx10;
        YaesuRig::new(
            Box::new(mock),
            ft_dx10(),
            true, // auto_retry
            3,    // max_retries
            Duration::from_millis(500),
            PttMethod::Cat,
            KeyLine::None,
            SetCommandMode::NoVerify,
            #[cfg(feature = "audio")]
            None,
        )
    }

    /// Helper to build a YaesuRig with a MockTransport and the FT-DX101D
    /// (dual-receiver) model.
    fn make_dual_rx_rig(mock: MockTransport) -> YaesuRig {
        use crate::models::ft_dx101d;
        YaesuRig::new(
            Box::new(mock),
            ft_dx101d(),
            true,
            3,
            Duration::from_millis(500),
            PttMethod::Cat,
            KeyLine::None,
            SetCommandMode::NoVerify,
            #[cfg(feature = "audio")]
            None,
        )
    }

    // -----------------------------------------------------------------
    // get_frequency / set_frequency
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_frequency() {
        let mut mock = MockTransport::new();

        // Expect: read VFO-A frequency command "FA;"
        let read_cmd = commands::cmd_read_frequency_a();
        mock.expect(&read_cmd, b"FA014250000;");

        let rig = make_test_rig(mock);
        let freq = rig.get_frequency(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(freq, 14_250_000);
    }

    #[tokio::test]
    async fn test_get_frequency_vfo_b() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_frequency_b();
        mock.expect(&read_cmd, b"FB007000000;");

        let rig = make_test_rig(mock);
        let freq = rig.get_frequency(ReceiverId::VFO_B).await.unwrap();
        assert_eq!(freq, 7_000_000);
    }

    #[tokio::test]
    async fn test_set_frequency() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_frequency_a(14_250_000);
        // Yaesu echoes the command as the response.
        mock.expect(&set_cmd, b"FA014250000;");

        let rig = make_test_rig(mock);
        rig.set_frequency(ReceiverId::VFO_A, 14_250_000)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_frequency_vfo_b() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_frequency_b(7_000_000);
        mock.expect(&set_cmd, b"FB007000000;");

        let rig = make_test_rig(mock);
        rig.set_frequency(ReceiverId::VFO_B, 7_000_000)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------
    // get_mode / set_mode
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_mode() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_mode_a();
        // MD02; = USB
        mock.expect(&read_cmd, b"MD02;");

        let rig = make_test_rig(mock);
        let mode = rig.get_mode(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, Mode::USB);
    }

    #[tokio::test]
    async fn test_get_mode_vfo_b() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_mode_b();
        // MD13; = CW
        mock.expect(&read_cmd, b"MD13;");

        let rig = make_test_rig(mock);
        let mode = rig.get_mode(ReceiverId::VFO_B).await.unwrap();
        assert_eq!(mode, Mode::CW);
    }

    #[tokio::test]
    async fn test_set_mode() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_mode_a(&Mode::CW);
        mock.expect(&set_cmd, b"MD03;");

        let rig = make_test_rig(mock);
        rig.set_mode(ReceiverId::VFO_A, Mode::CW).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_mode_vfo_b() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_mode_b(&Mode::USB);
        mock.expect(&set_cmd, b"MD12;");

        let rig = make_test_rig(mock);
        rig.set_mode(ReceiverId::VFO_B, Mode::USB).await.unwrap();
    }

    // -----------------------------------------------------------------
    // PTT
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_ptt() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_ptt();
        // TX0; = receive (PTT off)
        mock.expect(&read_cmd, b"TX0;");

        let rig = make_test_rig(mock);
        let ptt = rig.get_ptt().await.unwrap();
        assert!(!ptt);
    }

    #[tokio::test]
    async fn test_get_ptt_on() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_ptt();
        // TX1; = transmitting
        mock.expect(&read_cmd, b"TX1;");

        let rig = make_test_rig(mock);
        let ptt = rig.get_ptt().await.unwrap();
        assert!(ptt);
    }

    #[tokio::test]
    async fn test_set_ptt() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_ptt(true);
        mock.expect(&set_cmd, b"TX1;");

        let rig = make_test_rig(mock);
        rig.set_ptt(true).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_ptt_off() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_ptt(false);
        mock.expect(&set_cmd, b"TX0;");

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
        // PC050; = 50 watts
        mock.expect(&read_cmd, b"PC050;");

        let rig = make_test_rig(mock);
        let watts = rig.get_power().await.unwrap();
        assert!(
            (watts - 50.0).abs() < f32::EPSILON,
            "expected 50W, got {watts}W"
        );
    }

    #[tokio::test]
    async fn test_set_power() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_power(50);
        mock.expect(&set_cmd, b"PC050;");

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
    // S-meter
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_s_meter() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_s_meter();
        // SM0120; = S9 reading (value 120)
        mock.expect(&read_cmd, b"SM0120;");

        let rig = make_test_rig(mock);
        let dbm = rig.get_s_meter(ReceiverId::VFO_A).await.unwrap();
        // S9 should be approximately -73 dBm
        assert!(
            dbm < -70.0 && dbm > -76.0,
            "S9 should be near -73 dBm, got {dbm}"
        );
    }

    #[tokio::test]
    async fn test_s_meter_zero() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_s_meter();
        mock.expect(&read_cmd, b"SM0000;");

        let rig = make_test_rig(mock);
        let dbm = rig.get_s_meter(ReceiverId::VFO_A).await.unwrap();
        // S0 should be approximately -127 dBm
        assert!(
            (dbm - (-127.0)).abs() < 1.0,
            "S0 should be near -127 dBm, got {dbm}"
        );
    }

    // -----------------------------------------------------------------
    // SWR / ALC
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_swr() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_swr();
        mock.expect(&read_cmd, b"RM1045;");

        let rig = make_test_rig(mock);
        let swr = rig.get_swr().await.unwrap();
        assert!(
            (swr - 45.0).abs() < f32::EPSILON,
            "expected raw 45, got {swr}"
        );
    }

    #[tokio::test]
    async fn test_alc() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_alc();
        mock.expect(&read_cmd, b"RM5100;");

        let rig = make_test_rig(mock);
        let alc = rig.get_alc().await.unwrap();
        assert!(
            (alc - 100.0).abs() < f32::EPSILON,
            "expected raw 100, got {alc}"
        );
    }

    // -----------------------------------------------------------------
    // Split
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_split() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_split();
        mock.expect(&read_cmd, b"FT1;");

        let rig = make_test_rig(mock);
        let split = rig.get_split().await.unwrap();
        assert!(split);
    }

    #[tokio::test]
    async fn test_get_split_off() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_split();
        mock.expect(&read_cmd, b"FT0;");

        let rig = make_test_rig(mock);
        let split = rig.get_split().await.unwrap();
        assert!(!split);
    }

    #[tokio::test]
    async fn test_set_split() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_split(true);
        mock.expect(&set_cmd, b"FT1;");

        let rig = make_test_rig(mock);
        rig.set_split(true).await.unwrap();
    }

    // -----------------------------------------------------------------
    // info / capabilities
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_info_and_capabilities() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);

        let info = rig.info();
        assert_eq!(info.manufacturer, Manufacturer::Yaesu);
        assert_eq!(info.model_name, "FT-DX10");

        let caps = rig.capabilities();
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -----------------------------------------------------------------
    // receivers
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_receivers_single_rx() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);

        let rxs = rig.receivers().await.unwrap();
        assert_eq!(rxs, vec![ReceiverId::VFO_A]);
    }

    #[tokio::test]
    async fn test_receivers_dual_rx() {
        let mock = MockTransport::new();
        let rig = make_dual_rx_rig(mock);

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
    async fn test_secondary_receiver_single() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        assert_eq!(rig.secondary_receiver().await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_secondary_receiver_dual() {
        let mock = MockTransport::new();
        let rig = make_dual_rx_rig(mock);
        assert_eq!(
            rig.secondary_receiver().await.unwrap(),
            Some(ReceiverId::VFO_B)
        );
    }

    // -----------------------------------------------------------------
    // subscribe / events
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_subscribe_events() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_frequency_a(14_074_000);
        mock.expect(&set_cmd, b"FA014074000;");

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

    // -----------------------------------------------------------------
    // Error response
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_error_response() {
        let mut mock = MockTransport::new();

        // Rig returns ?; for an unsupported command.
        let cmd = commands::cmd_read_frequency_a();
        mock.expect(&cmd, b"?;");

        let rig = make_test_rig(mock);
        let result = rig.get_frequency(ReceiverId::VFO_A).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Protocol(msg) => {
                assert!(msg.contains("error response"), "unexpected message: {msg}");
            }
            other => panic!("expected Protocol error, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // Retry on timeout
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_retry_on_timeout() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_read_frequency_a();
        // First attempt: empty response (will timeout).
        mock.expect(&cmd, b"");
        // Second attempt: valid response.
        mock.expect(&cmd, b"FA014250000;");

        let rig = YaesuRig::new(
            Box::new(mock),
            crate::models::ft_dx10(),
            true,                       // auto_retry enabled
            3,                          // max_retries
            Duration::from_millis(100), // short timeout for test
            PttMethod::Cat,
            KeyLine::None,
            SetCommandMode::NoVerify,
            #[cfg(feature = "audio")]
            None,
        );

        let freq = rig.get_frequency(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(freq, 14_250_000);
    }

    // -----------------------------------------------------------------
    // CW speed
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_cw_speed() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_cw_speed();
        mock.expect(&read_cmd, b"KS025;");

        let rig = make_test_rig(mock);
        let wpm = rig.get_cw_speed().await.unwrap();
        assert_eq!(wpm, 25);
    }

    #[tokio::test]
    async fn test_set_cw_speed() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_cw_speed(25);
        mock.expect(&set_cmd, b"KS025;");

        let rig = make_test_rig(mock);
        rig.set_cw_speed(25).await.unwrap();
    }

    // -----------------------------------------------------------------
    // AGC
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_agc_off() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_agc();
        mock.expect(&read_cmd, b"GT00;");

        let rig = make_test_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Off);
    }

    #[tokio::test]
    async fn test_get_agc_fast() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_agc();
        mock.expect(&read_cmd, b"GT01;");

        let rig = make_test_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Fast);
    }

    #[tokio::test]
    async fn test_get_agc_medium() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_agc();
        mock.expect(&read_cmd, b"GT02;");

        let rig = make_test_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Medium);
    }

    #[tokio::test]
    async fn test_get_agc_slow() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_agc();
        mock.expect(&read_cmd, b"GT03;");

        let rig = make_test_rig(mock);
        let mode = rig.get_agc(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(mode, AgcMode::Slow);
    }

    #[tokio::test]
    async fn test_set_agc_off() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_agc(0);
        mock.expect(&set_cmd, b"GT00;");

        let rig = make_test_rig(mock);
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Off).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_agc_slow() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_agc(3);
        mock.expect(&set_cmd, b"GT03;");

        let rig = make_test_rig(mock);
        rig.set_agc(ReceiverId::VFO_A, AgcMode::Slow).await.unwrap();
    }

    #[tokio::test]
    async fn test_agc_emits_event() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_agc(1);
        mock.expect(&set_cmd, b"GT01;");

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
    // Preamp
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_preamp_off() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_preamp();
        mock.expect(&read_cmd, b"PA00;");

        let rig = make_test_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Off);
    }

    #[tokio::test]
    async fn test_get_preamp_1() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_preamp();
        mock.expect(&read_cmd, b"PA01;");

        let rig = make_test_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Preamp1);
    }

    #[tokio::test]
    async fn test_get_preamp_2() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_preamp();
        mock.expect(&read_cmd, b"PA02;");

        let rig = make_test_rig(mock);
        let level = rig.get_preamp(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(level, PreampLevel::Preamp2);
    }

    #[tokio::test]
    async fn test_set_preamp_off() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_preamp(0);
        mock.expect(&set_cmd, b"PA00;");

        let rig = make_test_rig(mock);
        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Off)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_preamp_1() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_preamp(1);
        mock.expect(&set_cmd, b"PA01;");

        let rig = make_test_rig(mock);
        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp1)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_preamp_2_on_dx101d() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_preamp(2);
        mock.expect(&set_cmd, b"PA02;");

        // FT-DX101D has_preamp2 = true, so this should succeed
        let rig = make_dual_rx_rig(mock);
        rig.set_preamp(ReceiverId::VFO_A, PreampLevel::Preamp2)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_set_preamp_2_on_891_returns_unsupported() {
        use crate::models::ft_891;
        // FT-891 has_preamp2 = false
        let mock = MockTransport::new();
        let rig = YaesuRig::new(
            Box::new(mock),
            ft_891(),
            false,
            0,
            Duration::from_millis(100),
            PttMethod::Cat,
            KeyLine::None,
            SetCommandMode::NoVerify,
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
                    msg.contains("Preamp2"),
                    "expected message about Preamp2, got: {msg}"
                );
            }
            other => panic!("expected Unsupported error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_preamp_get_emits_event() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_preamp();
        mock.expect(&read_cmd, b"PA01;");

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
    async fn test_preamp_set_emits_event() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_preamp(1);
        mock.expect(&set_cmd, b"PA01;");

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
        let read_cmd = commands::cmd_read_attenuator();
        mock.expect(&read_cmd, b"RA00;");

        let rig = make_test_rig(mock);
        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(db, 0);
    }

    #[tokio::test]
    async fn test_get_attenuator_on() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_attenuator();
        mock.expect(&read_cmd, b"RA01;");

        let rig = make_test_rig(mock);
        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        // Yaesu on (01) maps to first non-zero dB step from capabilities (6)
        assert_eq!(db, 6);
    }

    #[tokio::test]
    async fn test_set_attenuator_off() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_attenuator(0);
        mock.expect(&set_cmd, b"RA00;");

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 0).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_attenuator_6db() {
        let mut mock = MockTransport::new();
        // Any non-zero dB maps to value 1 on Yaesu (binary attenuator)
        let set_cmd = commands::cmd_set_attenuator(1);
        mock.expect(&set_cmd, b"RA01;");

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 6).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_attenuator_18db() {
        let mut mock = MockTransport::new();
        // Any non-zero dB maps to 1 on Yaesu
        let set_cmd = commands::cmd_set_attenuator(1);
        mock.expect(&set_cmd, b"RA01;");

        let rig = make_test_rig(mock);
        rig.set_attenuator(ReceiverId::VFO_A, 18).await.unwrap();
    }

    #[tokio::test]
    async fn test_attenuator_get_emits_event() {
        let mut mock = MockTransport::new();
        let read_cmd = commands::cmd_read_attenuator();
        mock.expect(&read_cmd, b"RA01;");

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        let db = rig.get_attenuator(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(db, 6);

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::AttenuatorChanged { receiver, db } => {
                assert_eq!(receiver, ReceiverId::VFO_A);
                assert_eq!(db, 6);
            }
            other => panic!("expected AttenuatorChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_attenuator_set_emits_event() {
        let mut mock = MockTransport::new();
        let set_cmd = commands::cmd_set_attenuator(1);
        mock.expect(&set_cmd, b"RA01;");

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
    // RIT / XIT
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_rit() {
        let mut mock = MockTransport::new();
        let if_cmd = commands::cmd_read_information();
        // IF response: mem=001, freq=014250000, sign=+, offset=0050, rit=1, xit=0, mode=2, ...
        mock.expect(&if_cmd, b"IF001014250000+00501020001;");

        let rig = make_test_rig(mock);
        let (enabled, offset) = rig.get_rit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset, 50);
    }

    #[tokio::test]
    async fn test_get_rit_negative() {
        let mut mock = MockTransport::new();
        let if_cmd = commands::cmd_read_information();
        mock.expect(&if_cmd, b"IF001014250000-01001020001;");

        let rig = make_test_rig(mock);
        let (enabled, offset) = rig.get_rit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset, -100);
    }

    #[tokio::test]
    async fn test_get_rit_off() {
        let mut mock = MockTransport::new();
        let if_cmd = commands::cmd_read_information();
        mock.expect(&if_cmd, b"IF001014250000+00000020001;");

        let rig = make_test_rig(mock);
        let (enabled, offset) = rig.get_rit().await.unwrap();
        assert!(!enabled);
        assert_eq!(offset, 0);
    }

    #[tokio::test]
    async fn test_set_rit() {
        let mut mock = MockTransport::new();
        // All set commands are fire-and-forget (no response from radio).
        // Empty response causes mock to return Timeout, which is success
        // for SET commands in NoVerify mode.
        let rit_on_cmd = commands::cmd_set_rit_on(true);
        mock.expect(&rit_on_cmd, b"");
        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"");
        let up_cmd = commands::cmd_rit_up(50);
        mock.expect(&up_cmd, b"");

        let rig = make_test_rig(mock);
        rig.set_rit(true, 50).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_rit_negative() {
        let mut mock = MockTransport::new();
        let rit_on_cmd = commands::cmd_set_rit_on(true);
        mock.expect(&rit_on_cmd, b"");
        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"");
        let down_cmd = commands::cmd_rit_down(100);
        mock.expect(&down_cmd, b"");

        let rig = make_test_rig(mock);
        rig.set_rit(true, -100).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_rit_zero_offset() {
        let mut mock = MockTransport::new();
        let rit_on_cmd = commands::cmd_set_rit_on(true);
        mock.expect(&rit_on_cmd, b"");
        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"");

        let rig = make_test_rig(mock);
        rig.set_rit(true, 0).await.unwrap();
    }

    #[tokio::test]
    async fn test_get_xit() {
        let mut mock = MockTransport::new();
        let if_cmd = commands::cmd_read_information();
        // XIT on at offset +75: rit=0, xit=1
        mock.expect(&if_cmd, b"IF001014250000+00750120001;");

        let rig = make_test_rig(mock);
        let (enabled, offset) = rig.get_xit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset, 75);
    }

    #[tokio::test]
    async fn test_set_xit() {
        let mut mock = MockTransport::new();
        let xit_on_cmd = commands::cmd_set_xit_on(true);
        mock.expect(&xit_on_cmd, b"");
        let clear_cmd = commands::cmd_rit_clear();
        mock.expect(&clear_cmd, b"");
        let up_cmd = commands::cmd_rit_up(75);
        mock.expect(&up_cmd, b"");

        let rig = make_test_rig(mock);
        rig.set_xit(true, 75).await.unwrap();
    }

    #[tokio::test]
    async fn test_rit_emits_event() {
        let mut mock = MockTransport::new();
        let if_cmd = commands::cmd_read_information();
        mock.expect(&if_cmd, b"IF001014250000+00501020001;");

        let rig = make_test_rig(mock);
        let mut event_rx = rig.subscribe().unwrap();

        let _ = rig.get_rit().await.unwrap();

        let event = event_rx.try_recv().unwrap();
        match event {
            RigEvent::RitChanged { enabled, offset_hz } => {
                assert!(enabled);
                assert_eq!(offset_hz, 50);
            }
            other => panic!("expected RitChanged, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // VFO A=B / VFO swap
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_vfo_a_eq_b() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_vfo_a_eq_b();
        mock.expect(&cmd, b"AB;");

        let rig = make_test_rig(mock);
        rig.set_vfo_a_eq_b(ReceiverId::VFO_A).await.unwrap();
    }

    #[tokio::test]
    async fn test_swap_vfo() {
        let mut mock = MockTransport::new();

        let cmd = commands::cmd_vfo_swap();
        mock.expect(&cmd, b"SV;");

        let rig = make_test_rig(mock);
        rig.swap_vfo(ReceiverId::VFO_A).await.unwrap();
    }

    // -----------------------------------------------------------------
    // Antenna
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_antenna() {
        let mut mock = MockTransport::new();

        let read_cmd = commands::cmd_read_antenna();
        mock.expect(&read_cmd, b"AN01;");

        let rig = make_test_rig(mock);
        let ant = rig.get_antenna(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(ant, AntennaPort::Ant1);
    }

    #[tokio::test]
    async fn test_set_antenna() {
        let mut mock = MockTransport::new();

        let set_cmd = commands::cmd_set_antenna(1);
        mock.expect(&set_cmd, b"AN01;");

        let rig = make_test_rig(mock);
        rig.set_antenna(ReceiverId::VFO_A, AntennaPort::Ant1)
            .await
            .unwrap();
    }

    // -----------------------------------------------------------------
    // Passband unsupported
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_passband_unsupported() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);

        let result = rig.get_passband(ReceiverId::VFO_A).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Unsupported(_)));

        let result = rig
            .set_passband(ReceiverId::VFO_A, Passband::from_hz(2700))
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Unsupported(_)));
    }

    // -----------------------------------------------------------------
    // set_tx_receiver
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_set_tx_receiver_enables_split() {
        let mut mock = MockTransport::new();

        // Setting TX to VFO B should enable split.
        let split_cmd = commands::cmd_set_split(true);
        mock.expect(&split_cmd, b"FT1;");

        let rig = make_test_rig(mock);
        rig.set_tx_receiver(ReceiverId::VFO_B).await.unwrap();
    }

    #[tokio::test]
    async fn test_set_tx_receiver_disables_split() {
        let mut mock = MockTransport::new();

        // Setting TX to VFO A should disable split.
        let split_cmd = commands::cmd_set_split(false);
        mock.expect(&split_cmd, b"FT0;");

        let rig = make_test_rig(mock);
        rig.set_tx_receiver(ReceiverId::VFO_A).await.unwrap();
    }

    // -----------------------------------------------------------------
    // CW messages
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_send_cw_message() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let err = rig.send_cw_message("TEST").await.unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    #[tokio::test]
    async fn test_send_cw_message_chunked() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let message = "ABCDEFGHIJKLMNOPQRSTUVWX0123456789";
        let err = rig.send_cw_message(message).await.unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    #[tokio::test]
    async fn test_stop_cw_message() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let err = rig.stop_cw_message().await.unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    #[tokio::test]
    async fn test_send_cw_message_empty() {
        let mock = MockTransport::new();
        let rig = make_test_rig(mock);
        let err = rig.send_cw_message("").await.unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }

    // -----------------------------------------------------------------
    // AudioCapable (feature = "audio")
    // -----------------------------------------------------------------

    #[cfg(feature = "audio")]
    mod audio_tests {
        use super::*;
        use riglib_core::audio::{AudioCapable, AudioSampleFormat};

        fn make_audio_rig(mock: MockTransport, device_name: Option<&str>) -> YaesuRig {
            use crate::models::ft_dx10;
            YaesuRig::new(
                Box::new(mock),
                ft_dx10(),
                true,
                3,
                Duration::from_millis(500),
                PttMethod::Cat,
                KeyLine::None,
                SetCommandMode::NoVerify,
                device_name.map(|s| s.to_string()),
            )
        }

        #[tokio::test]
        async fn test_audio_supported_with_device() {
            let mock = MockTransport::new();
            let rig = make_audio_rig(mock, Some("USB Audio CODEC"));
            assert!(rig.audio_supported());
        }

        #[tokio::test]
        async fn test_audio_not_supported_without_device() {
            let mock = MockTransport::new();
            let rig = make_audio_rig(mock, None);
            assert!(!rig.audio_supported());
        }

        #[tokio::test]
        async fn test_native_audio_config() {
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
