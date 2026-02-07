//! YaesuRig -- the [`Rig`] trait implementation for Yaesu transceivers.
//!
//! This module ties the CAT text-protocol engine ([`protocol`], [`commands`])
//! to a [`Transport`] to produce a working Yaesu backend. It handles command
//! encoding, response buffering until the `;` terminator is found, error
//! detection (`?;`), retry logic, and VFO A/B selection for both single- and
//! dual-receiver rigs.

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
use crate::models::YaesuModel;
use crate::protocol::{self, DecodeResult};

/// A connected Yaesu transceiver controlled over CAT.
///
/// Constructed via [`YaesuBuilder`](crate::builder::YaesuBuilder). All rig
/// communication goes through the [`Transport`] provided at build time.
pub struct YaesuRig {
    transport: Arc<Mutex<Box<dyn Transport>>>,
    model: YaesuModel,
    event_tx: broadcast::Sender<RigEvent>,
    command_timeout: Duration,
    auto_retry: bool,
    max_retries: u32,
    info: RigInfo,
    ptt_method: PttMethod,
    key_line: KeyLine,
    /// USB audio device name (e.g. "USB Audio CODEC"). When set, this rig
    /// supports audio streaming via the `AudioCapable` trait.
    #[cfg(feature = "audio")]
    audio_device_name: Option<String>,
    /// Active cpal audio backend, created on first `start_rx_audio()` or
    /// `start_tx_audio()` call.
    #[cfg(feature = "audio")]
    audio_backend: Mutex<Option<riglib_transport::CpalAudioBackend>>,
}

impl YaesuRig {
    /// Create a new `YaesuRig` from its constituent parts.
    ///
    /// This is called by [`YaesuBuilder`](crate::builder::YaesuBuilder);
    /// callers should use the builder API instead.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        transport: Box<dyn Transport>,
        model: YaesuModel,
        auto_retry: bool,
        max_retries: u32,
        command_timeout: Duration,
        ptt_method: PttMethod,
        key_line: KeyLine,
        #[cfg(feature = "audio")] audio_device_name: Option<String>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let info = RigInfo {
            manufacturer: Manufacturer::Yaesu,
            model_name: model.name.to_string(),
            model_id: model.name.to_string(),
        };
        YaesuRig {
            transport: Arc::new(Mutex::new(transport)),
            model,
            event_tx,
            command_timeout,
            auto_retry,
            max_retries,
            info,
            ptt_method,
            key_line,
            #[cfg(feature = "audio")]
            audio_device_name,
            #[cfg(feature = "audio")]
            audio_backend: Mutex::new(None),
        }
    }

    /// Send a CAT command and wait for the rig's response.
    ///
    /// Handles:
    /// - Buffered reading until the `;` terminator is found
    /// - Error responses (`?;`) returned as `Error::Protocol`
    /// - Timeout with configurable retry count
    ///
    /// Returns the decoded prefix and data portions of the response.
    async fn execute_command(&self, cmd: &[u8]) -> Result<(String, String)> {
        let retries = if self.auto_retry { self.max_retries } else { 0 };
        let mut transport = self.transport.lock().await;

        for attempt in 0..=retries {
            if attempt > 0 {
                debug!(attempt, "CAT command retry");
                // Brief backoff before retry (increases with each attempt).
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

                        // Try to decode a complete response from the buffer.
                        match protocol::decode_response(&response_buf) {
                            DecodeResult::Response {
                                prefix,
                                data,
                                consumed: _,
                            } => {
                                return Ok((prefix, data));
                            }
                            DecodeResult::Error(_) => {
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
                        // Transport timed out waiting for data. Try one
                        // more decode pass on what we have.
                        if !response_buf.is_empty() {
                            match protocol::decode_response(&response_buf) {
                                DecodeResult::Response {
                                    prefix,
                                    data,
                                    consumed: _,
                                } => {
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
                        // Move to next retry attempt.
                        break;
                    }
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        // tokio::time::timeout expired.
                        if !response_buf.is_empty() {
                            match protocol::decode_response(&response_buf) {
                                DecodeResult::Response {
                                    prefix,
                                    data,
                                    consumed: _,
                                } => {
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
                        break;
                    }
                }
            }
        }

        Err(Error::Timeout)
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
        // Yaesu echoes the command back as confirmation.
        let _response = self.execute_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::FrequencyChanged {
            receiver: rx,
            freq_hz,
        });
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
        let _response = self.execute_command(&cmd).await?;
        let _ = self
            .event_tx
            .send(RigEvent::ModeChanged { receiver: rx, mode });
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
                let _response = self.execute_command(&cmd).await?;
            }
            PttMethod::Dtr => {
                debug!(on, "setting PTT via DTR");
                let mut transport = self.transport.lock().await;
                transport.set_dtr(on).await?;
            }
            PttMethod::Rts => {
                debug!(on, "setting PTT via RTS");
                let mut transport = self.transport.lock().await;
                transport.set_rts(on).await?;
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
        let _response = self.execute_command(&cmd).await?;
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
        let _response = self.execute_command(&cmd).await?;
        let _ = self.event_tx.send(RigEvent::SplitChanged { on });
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
        let _response = self.execute_command(&cmd).await?;
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
                let mut transport = self.transport.lock().await;
                transport.set_dtr(on).await
            }
            KeyLine::Rts => {
                debug!(on, "CW key via RTS");
                let mut transport = self.transport.lock().await;
                transport.set_rts(on).await
            }
        }
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
            #[cfg(feature = "audio")]
            None,
        );

        let freq = rig.get_frequency(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(freq, 14_250_000);
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
