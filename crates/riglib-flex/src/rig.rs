//! FlexRadio -- the [`Rig`] trait implementation for FlexRadio SmartSDR transceivers.
//!
//! This module maps the SmartSDR TCP/IP protocol client ([`SmartSdrClient`])
//! to the riglib [`Rig`] trait. Most `get_*` methods read from the cached
//! [`RadioState`](crate::state::RadioState) rather than sending a command, since FlexRadio pushes
//! state changes continuously via status messages after subscription.
//!
//! Meter data (S-meter, SWR, ALC) comes from VITA-49 UDP packets parsed
//! by the UDP receiver task and stored in `RadioState::meter_values`.
//!
//! Frequency conversion between the Rig trait's Hz (`u64`) and SmartSDR's
//! MHz (`f64`) is performed at every trait method boundary using
//! [`codec::hz_to_mhz()`] and [`codec::mhz_to_hz()`].

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{Mutex, broadcast, mpsc};

use riglib_core::audio::{
    AudioBuffer, AudioCapable, AudioReceiver, AudioSender, AudioStreamConfig,
};
use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;
use riglib_core::rig::Rig;
use riglib_core::types::*;

use crate::client::SmartSdrClient;
use crate::codec;
use crate::dax;
use crate::discovery::{self, DiscoveredRadio};
use crate::meters;
use crate::mode;
use crate::models::FlexRadioModel;

/// Default frequency for auto-created slices (14.250 MHz, 20m SSB).
const DEFAULT_SLICE_FREQ_HZ: u64 = 14_250_000;

/// Default mode for auto-created slices.
const DEFAULT_SLICE_MODE: Mode = Mode::USB;

/// Brief delay to allow status messages to update cached state after a
/// slice create command.
const SLICE_CREATE_SETTLE_MS: u64 = 100;

/// A connected FlexRadio transceiver controlled over SmartSDR.
///
/// Constructed via [`FlexRadioBuilder`](crate::builder::FlexRadioBuilder).
/// All rig communication goes through the internal [`SmartSdrClient`].
pub struct FlexRadio {
    pub(crate) client: SmartSdrClient,
    model: FlexRadioModel,
    info: RigInfo,
    auto_create_slices: bool,
    /// Active DAX stream IDs, tracked so stop_audio() can remove them.
    active_dax_streams: Arc<Mutex<Vec<u32>>>,
}

impl FlexRadio {
    /// Create a new `FlexRadio` from its constituent parts.
    ///
    /// This is called by [`FlexRadioBuilder`](crate::builder::FlexRadioBuilder);
    /// callers should use the builder API instead.
    pub(crate) fn new(
        client: SmartSdrClient,
        model: FlexRadioModel,
        auto_create_slices: bool,
    ) -> Self {
        let info = RigInfo {
            manufacturer: Manufacturer::FlexRadio,
            model_name: model.name.to_string(),
            model_id: model.model_id.to_string(),
        };
        FlexRadio {
            client,
            model,
            info,
            auto_create_slices,
            active_dax_streams: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Ensure a slice exists for the given receiver. If the slice does not
    /// exist and `auto_create_slices` is enabled, create it with sensible
    /// defaults (14.250 MHz USB). Returns an error if the slice does not
    /// exist and auto-creation is disabled.
    async fn ensure_slice(&self, rx: ReceiverId) -> Result<()> {
        let state = self.client.state().await;
        let index = rx.index();
        if state.slices.contains_key(&index) {
            return Ok(());
        }

        if !self.auto_create_slices {
            return Err(Error::InvalidParameter(format!(
                "slice {} does not exist and auto_create_slices is disabled",
                index
            )));
        }

        // Create the slice with default frequency and mode.
        let flex_mode = mode::mode_to_flex(&DEFAULT_SLICE_MODE);
        let cmd = codec::cmd_slice_create(DEFAULT_SLICE_FREQ_HZ, flex_mode);
        self.client.send_command(&cmd).await?;

        // Brief delay to let the status message update the cached state.
        tokio::time::sleep(Duration::from_millis(SLICE_CREATE_SETTLE_MS)).await;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // FlexRadio-specific extensions (not part of the Rig trait)
    // -----------------------------------------------------------------------

    /// Create a new slice at the given frequency and mode.
    ///
    /// Returns the `ReceiverId` for the new slice. The slice index is
    /// returned by the radio in the command response.
    pub async fn create_slice(&self, freq_hz: u64, mode: Mode) -> Result<ReceiverId> {
        let flex_mode = mode::mode_to_flex(&mode);
        let cmd = codec::cmd_slice_create(freq_hz, flex_mode);
        let response = self.client.send_command(&cmd).await?;

        // The response contains the new slice index as a decimal string.
        let index = response.trim().parse::<u8>().map_err(|_| {
            Error::Protocol(format!(
                "expected slice index in create response, got: {}",
                response
            ))
        })?;

        // Wait for status messages to populate the slice state.
        tokio::time::sleep(Duration::from_millis(SLICE_CREATE_SETTLE_MS)).await;

        Ok(ReceiverId::from_index(index))
    }

    /// Destroy a slice.
    pub async fn destroy_slice(&self, rx: ReceiverId) -> Result<()> {
        let cmd = codec::cmd_slice_remove(rx.index());
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    /// Assign a DAX audio channel to a slice.
    ///
    /// DAX channels are numbered 1-8; passing 0 removes the DAX assignment.
    pub async fn set_dax_channel(&self, rx: ReceiverId, channel: u8) -> Result<()> {
        let cmd = format!("slice set {} dax={}", rx.index(), channel);
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    /// Set which slice transmits.
    pub async fn set_tx_slice(&self, rx: ReceiverId) -> Result<()> {
        let cmd = codec::cmd_slice_set_tx(rx.index());
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    /// Discover FlexRadio radios on the LAN.
    ///
    /// Listens for VITA-49 discovery broadcasts for the specified duration
    /// and returns all unique radios found, deduplicated by serial number.
    pub async fn discover(timeout: Duration) -> Result<Vec<DiscoveredRadio>> {
        discovery::discover(timeout).await
    }

    /// Disconnect from the radio and clean up background tasks.
    pub async fn disconnect(&self) -> Result<()> {
        self.client.disconnect().await
    }

    /// Whether the client is currently connected.
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }
}

#[async_trait]
impl Rig for FlexRadio {
    fn info(&self) -> &RigInfo {
        &self.info
    }

    fn capabilities(&self) -> &RigCapabilities {
        &self.model.capabilities
    }

    async fn receivers(&self) -> Result<Vec<ReceiverId>> {
        let state = self.client.state().await;
        if state.slices.is_empty() {
            if self.auto_create_slices {
                return Ok(vec![ReceiverId::from_index(0)]);
            }
            return Ok(Vec::new());
        }

        let mut indices: Vec<u8> = state.slices.keys().copied().collect();
        indices.sort();
        Ok(indices.into_iter().map(ReceiverId::from_index).collect())
    }

    async fn primary_receiver(&self) -> Result<ReceiverId> {
        let state = self.client.state().await;

        // The primary receiver is the TX slice.
        for (&index, slice) in &state.slices {
            if slice.is_tx {
                return Ok(ReceiverId::from_index(index));
            }
        }

        // Fallback: slice 0 or the first available slice.
        if state.slices.contains_key(&0) {
            return Ok(ReceiverId::from_index(0));
        }

        if let Some(&index) = state.slices.keys().min() {
            return Ok(ReceiverId::from_index(index));
        }

        // No slices at all.
        Ok(ReceiverId::from_index(0))
    }

    async fn secondary_receiver(&self) -> Result<Option<ReceiverId>> {
        let state = self.client.state().await;
        let primary = self.primary_receiver().await?;

        let mut indices: Vec<u8> = state.slices.keys().copied().collect();
        indices.sort();

        for index in indices {
            if index != primary.index() {
                return Ok(Some(ReceiverId::from_index(index)));
            }
        }

        Ok(None)
    }

    async fn get_frequency(&self, rx: ReceiverId) -> Result<u64> {
        self.ensure_slice(rx).await?;
        let state = self.client.state().await;
        let index = rx.index();
        let slice = state
            .slices
            .get(&index)
            .ok_or_else(|| Error::InvalidParameter(format!("slice {} does not exist", index)))?;
        Ok(slice.frequency_hz)
    }

    async fn set_frequency(&self, rx: ReceiverId, freq_hz: u64) -> Result<()> {
        self.ensure_slice(rx).await?;
        let cmd = codec::cmd_slice_tune(rx.index(), freq_hz);
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn get_mode(&self, rx: ReceiverId) -> Result<Mode> {
        self.ensure_slice(rx).await?;
        let state = self.client.state().await;
        let index = rx.index();
        let slice = state
            .slices
            .get(&index)
            .ok_or_else(|| Error::InvalidParameter(format!("slice {} does not exist", index)))?;

        if slice.mode.is_empty() {
            return Err(Error::Protocol("slice mode not yet known".into()));
        }

        mode::flex_to_mode(&slice.mode)
    }

    async fn set_mode(&self, rx: ReceiverId, m: Mode) -> Result<()> {
        self.ensure_slice(rx).await?;
        let flex_mode = mode::mode_to_flex(&m);
        let cmd = codec::cmd_slice_set_mode(rx.index(), flex_mode);
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn get_passband(&self, rx: ReceiverId) -> Result<Passband> {
        self.ensure_slice(rx).await?;
        let state = self.client.state().await;
        let index = rx.index();
        let slice = state
            .slices
            .get(&index)
            .ok_or_else(|| Error::InvalidParameter(format!("slice {} does not exist", index)))?;

        // Passband width = filter_hi - filter_lo.
        let width = (slice.filter_hi - slice.filter_lo).unsigned_abs();
        Ok(Passband::from_hz(width))
    }

    async fn set_passband(&self, rx: ReceiverId, pb: Passband) -> Result<()> {
        self.ensure_slice(rx).await?;
        let state = self.client.state().await;
        let index = rx.index();
        let slice = state
            .slices
            .get(&index)
            .ok_or_else(|| Error::InvalidParameter(format!("slice {} does not exist", index)))?;

        // Calculate new filter edges centered on the current filter center.
        let current_center = (slice.filter_hi + slice.filter_lo) / 2;
        let half_width = pb.hz() as i32 / 2;
        let new_lo = current_center - half_width;
        let new_hi = current_center + half_width;

        let cmd = codec::cmd_slice_set_filter(rx.index(), new_lo, new_hi);
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn get_ptt(&self) -> Result<bool> {
        let state = self.client.state().await;
        Ok(state.tx_state.transmitting)
    }

    async fn set_ptt(&self, on: bool) -> Result<()> {
        let cmd = codec::cmd_xmit(on);
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn get_power(&self) -> Result<f32> {
        let state = self.client.state().await;
        // Normalize the 0-100 watt setting to 0.0-1.0, then scale to watts.
        let watts = state.tx_state.power as f32;
        Ok(watts)
    }

    async fn set_power(&self, watts: f32) -> Result<()> {
        let max = self.model.capabilities.max_power_watts;
        if watts < 0.0 || watts > max {
            return Err(Error::InvalidParameter(format!(
                "power {watts}W out of range 0-{max}W"
            )));
        }
        let power_int = watts.round() as u16;
        let cmd = codec::cmd_set_power(power_int);
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn get_s_meter(&self, _rx: ReceiverId) -> Result<f32> {
        // FlexRadio meter values for S-meter are reported as dBm * 128
        // (signed 16-bit). The raw value needs to be divided by 128 to
        // get approximate dBm. Some firmware versions use different
        // scaling; this is a reasonable default.
        match self.client.meter_value(meters::METER_S_LEVEL).await {
            Some(raw) => {
                // FlexRadio reports S-meter as a value where the raw i16
                // maps to roughly dBm when divided by a scale factor.
                // Common interpretation: raw value / 128.0 = dBm (approx).
                let dbm = raw as f32 / 128.0;
                Ok(dbm)
            }
            None => Err(Error::Unsupported(
                "S-meter data not available (no UDP meter receiver or meter not registered)".into(),
            )),
        }
    }

    async fn get_swr(&self) -> Result<f32> {
        match self.client.meter_value(meters::METER_SWR).await {
            Some(raw) => {
                // FlexRadio SWR meter values are scaled: raw / 128.0 gives
                // the SWR ratio. A raw value of 128 = 1.0:1.
                let swr = raw as f32 / 128.0;
                Ok(swr.max(1.0))
            }
            None => Err(Error::Unsupported(
                "SWR meter data not available (no UDP meter receiver or meter not registered)"
                    .into(),
            )),
        }
    }

    async fn get_alc(&self) -> Result<f32> {
        match self.client.meter_value(meters::METER_ALC).await {
            Some(raw) => {
                // FlexRadio ALC meter values: raw / 128.0.
                let alc = raw as f32 / 128.0;
                Ok(alc)
            }
            None => Err(Error::Unsupported(
                "ALC meter data not available (no UDP meter receiver or meter not registered)"
                    .into(),
            )),
        }
    }

    async fn get_split(&self) -> Result<bool> {
        let state = self.client.state().await;

        // Split is active when the TX slice is different from the primary
        // RX slice. In FlexRadio terms: find the TX slice and see if it
        // differs from the "active" (focused) slice.
        let mut tx_index: Option<u8> = None;
        let mut active_index: Option<u8> = None;

        for (&index, slice) in &state.slices {
            if slice.is_tx {
                tx_index = Some(index);
            }
            if slice.active {
                active_index = Some(index);
            }
        }

        match (tx_index, active_index) {
            (Some(tx), Some(active)) => Ok(tx != active),
            _ => Ok(false),
        }
    }

    async fn set_split(&self, on: bool) -> Result<()> {
        let state = self.client.state().await;
        let mut indices: Vec<u8> = state.slices.keys().copied().collect();
        indices.sort();

        if on {
            // Ensure at least 2 slices exist.
            if indices.len() < 2 {
                if self.auto_create_slices {
                    let flex_mode = mode::mode_to_flex(&DEFAULT_SLICE_MODE);
                    let cmd = codec::cmd_slice_create(DEFAULT_SLICE_FREQ_HZ, flex_mode);
                    self.client.send_command(&cmd).await?;
                    tokio::time::sleep(Duration::from_millis(SLICE_CREATE_SETTLE_MS)).await;

                    // Re-read state.
                    let state = self.client.state().await;
                    indices = state.slices.keys().copied().collect();
                    indices.sort();
                } else {
                    return Err(Error::InvalidParameter(
                        "split requires at least 2 slices".into(),
                    ));
                }
            }

            // Set TX on slice 1 (the second slice).
            if indices.len() >= 2 {
                let cmd = codec::cmd_slice_set_tx(indices[1]);
                self.client.send_command(&cmd).await?;
            }
        } else {
            // Set TX on slice 0 (the first slice).
            if !indices.is_empty() {
                let cmd = codec::cmd_slice_set_tx(indices[0]);
                self.client.send_command(&cmd).await?;
            }
        }

        Ok(())
    }

    async fn set_tx_receiver(&self, rx: ReceiverId) -> Result<()> {
        let cmd = codec::cmd_slice_set_tx(rx.index());
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn get_agc(&self, rx: ReceiverId) -> Result<AgcMode> {
        self.ensure_slice(rx).await?;
        let state = self.client.state().await;
        let index = rx.index();
        let slice = state
            .slices
            .get(&index)
            .ok_or_else(|| Error::InvalidParameter(format!("slice {} does not exist", index)))?;

        if slice.agc_mode.is_empty() {
            return Err(Error::Protocol("slice AGC mode not yet known".into()));
        }

        slice
            .agc_mode
            .parse::<AgcMode>()
            .map_err(|_| Error::Protocol(format!("unknown AGC mode: {:?}", slice.agc_mode)))
    }

    async fn set_agc(&self, rx: ReceiverId, mode: AgcMode) -> Result<()> {
        self.ensure_slice(rx).await?;
        let mode_str = match mode {
            AgcMode::Off => "off",
            AgcMode::Fast => "fast",
            AgcMode::Medium => "med",
            AgcMode::Slow => "slow",
        };
        let cmd = codec::cmd_slice_set_agc_mode(rx.index(), mode_str);
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn get_rit(&self) -> Result<(bool, i32)> {
        let rx = self.primary_receiver().await?;
        self.ensure_slice(rx).await?;
        let state = self.client.state().await;
        let index = rx.index();
        let slice = state
            .slices
            .get(&index)
            .ok_or_else(|| Error::InvalidParameter(format!("slice {} does not exist", index)))?;
        Ok((slice.rit_on, slice.rit_freq_hz))
    }

    async fn set_rit(&self, enabled: bool, offset_hz: i32) -> Result<()> {
        let rx = self.primary_receiver().await?;
        self.ensure_slice(rx).await?;
        let index = rx.index();
        let cmd_on = codec::cmd_slice_set_rit_on(index, enabled);
        self.client.send_command(&cmd_on).await?;
        let cmd_freq = codec::cmd_slice_set_rit_freq(index, offset_hz);
        self.client.send_command(&cmd_freq).await?;
        Ok(())
    }

    async fn get_xit(&self) -> Result<(bool, i32)> {
        let rx = self.primary_receiver().await?;
        self.ensure_slice(rx).await?;
        let state = self.client.state().await;
        let index = rx.index();
        let slice = state
            .slices
            .get(&index)
            .ok_or_else(|| Error::InvalidParameter(format!("slice {} does not exist", index)))?;
        Ok((slice.xit_on, slice.xit_freq_hz))
    }

    async fn set_xit(&self, enabled: bool, offset_hz: i32) -> Result<()> {
        let rx = self.primary_receiver().await?;
        self.ensure_slice(rx).await?;
        let index = rx.index();
        let cmd_on = codec::cmd_slice_set_xit_on(index, enabled);
        self.client.send_command(&cmd_on).await?;
        let cmd_freq = codec::cmd_slice_set_xit_freq(index, offset_hz);
        self.client.send_command(&cmd_freq).await?;
        Ok(())
    }

    async fn set_cw_key(&self, _on: bool) -> Result<()> {
        Err(Error::Unsupported(
            "CW key line control not supported on FlexRadio (network-only transport)".into(),
        ))
    }

    async fn send_cw_message(&self, message: &str) -> Result<()> {
        if message.is_empty() {
            return Ok(());
        }
        let cmd = codec::cmd_cwx_send(message);
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn stop_cw_message(&self) -> Result<()> {
        let cmd = codec::cmd_cwx_clear();
        self.client.send_command(&cmd).await?;
        Ok(())
    }

    async fn enable_transceive(&self) -> Result<()> {
        // FlexRadio SmartSDR pushes state changes continuously over TCP;
        // transceive is always on. Nothing to do here.
        Ok(())
    }

    async fn disable_transceive(&self) -> Result<()> {
        Err(Error::Unsupported(
            "disable_transceive not yet implemented for FlexRadio".into(),
        ))
    }

    fn subscribe(&self) -> Result<broadcast::Receiver<RigEvent>> {
        Ok(self.client.subscribe())
    }
}

// ---------------------------------------------------------------------------
// AudioCapable implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl AudioCapable for FlexRadio {
    async fn start_rx_audio(
        &self,
        rx: ReceiverId,
        _config: Option<AudioStreamConfig>,
    ) -> Result<AudioReceiver> {
        // Pick a DAX channel based on the receiver index (1-based).
        let dax_channel = rx.index() + 1;

        // Send: stream create type=dax_rx dax_channel=<ch>
        let response = self
            .client
            .send_command(&codec::cmd_stream_create_dax_rx(dax_channel))
            .await?;

        // Parse stream handle from response (hex number, possibly with 0x prefix).
        let handle_str = response.trim().trim_start_matches("0x");
        let stream_id = u32::from_str_radix(handle_str, 16)
            .map_err(|_| Error::Protocol(format!("invalid stream handle: {}", response)))?;

        // Associate the DAX channel with the slice.
        self.client
            .send_command(&codec::cmd_slice_set_dax(rx.index(), dax_channel))
            .await?;

        // Create the mpsc channel and register with the client for routing.
        let (tx, rx_chan) = mpsc::channel::<AudioBuffer>(64);
        self.client.register_dax_stream(stream_id, tx).await;

        // Track the active stream for stop_audio().
        {
            let mut streams = self.active_dax_streams.lock().await;
            streams.push(stream_id);
        }

        Ok(AudioReceiver::new(rx_chan, dax::dax_audio_config()))
    }

    async fn start_tx_audio(&self, _config: Option<AudioStreamConfig>) -> Result<AudioSender> {
        // TX audio is deferred to WI-4.5.
        Err(Error::Unsupported(
            "TX audio not yet implemented for FlexRadio".into(),
        ))
    }

    async fn stop_audio(&self) -> Result<()> {
        let stream_ids = {
            let mut streams = self.active_dax_streams.lock().await;
            streams.drain(..).collect::<Vec<_>>()
        };

        for stream_id in stream_ids {
            // Unregister from the UDP routing table (drops the sender,
            // which closes the AudioReceiver).
            self.client.unregister_dax_stream(stream_id).await;

            // Tell the radio to remove the stream.
            let cmd = codec::cmd_stream_remove(stream_id);
            // Best-effort: if the radio has already disconnected, ignore
            // the error.
            let _ = self.client.send_command(&cmd).await;
        }

        Ok(())
    }

    fn audio_supported(&self) -> bool {
        true // FlexRadio always supports DAX audio.
    }

    fn native_audio_config(&self) -> AudioStreamConfig {
        dax::dax_audio_config()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientOptions, SmartSdrClient};
    use crate::models::flex_6600;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    /// Helper: create a mock SmartSDR server on a random port.
    async fn mock_smartsdr_server() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        (listener, addr)
    }

    /// Helper: accept a connection and send the standard handshake.
    async fn accept_and_handshake(listener: &TcpListener) -> tokio::net::TcpStream {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"V1.4.0.0\n").await.unwrap();
        stream.write_all(b"H12345678\n").await.unwrap();
        stream.flush().await.unwrap();
        stream
    }

    /// Helper: connect a SmartSdrClient to the mock server.
    async fn connect_client(addr: &str) -> SmartSdrClient {
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();
        let options = ClientOptions {
            auto_subscribe: false,
            command_timeout: Duration::from_millis(500),
            ..ClientOptions::default()
        };
        SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap()
    }

    /// Helper: build a FlexRadio with the given client and default model.
    fn make_flex_radio(client: SmartSdrClient) -> FlexRadio {
        FlexRadio::new(client, flex_6600(), true)
    }

    /// Helper: read a command line from the mock server stream, verify it
    /// contains the expected substring, and respond with success.
    async fn expect_command_and_respond(
        stream: &mut tokio::net::TcpStream,
        expected_substr: &str,
        response_data: &str,
    ) {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let trimmed = line.trim();
        assert!(
            trimmed.contains(expected_substr),
            "expected command containing '{}', got: {}",
            expected_substr,
            trimmed
        );
        let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
        let resp = format!("R{}|00000000|{}\n", seq_str, response_data);
        let inner = reader.into_inner();
        inner.write_all(resp.as_bytes()).await.unwrap();
        inner.flush().await.unwrap();
    }

    // -----------------------------------------------------------------------
    // Info and Capabilities
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_info_and_capabilities() {
        let (listener, addr) = mock_smartsdr_server().await;
        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        let info = rig.info();
        assert_eq!(info.manufacturer, Manufacturer::FlexRadio);
        assert_eq!(info.model_name, "FLEX-6600");
        assert_eq!(info.model_id, "flex6600");

        let caps = rig.capabilities();
        assert_eq!(caps.max_receivers, 4);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_iq_output);
        assert!(caps.has_audio_streaming);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Frequency
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_set_frequency() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            // Give the client time to start the read loop.
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 status with initial frequency.
            stream
                .write_all(
                    b"S12345678|slice 0 RF_frequency=14.250000 mode=USB filter_lo=100 filter_hi=2900 tx=1 active=1\n",
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Read the set_frequency command (slice tune).
            expect_command_and_respond(&mut stream, "slice tune 0 7.074000", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        // Wait for status to be processed.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // get_frequency should read from cached state.
        let freq = rig.get_frequency(ReceiverId::from_index(0)).await.unwrap();
        assert_eq!(freq, 14_250_000);

        // set_frequency should send a slice tune command.
        rig.set_frequency(ReceiverId::from_index(0), 7_074_000)
            .await
            .unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Mode
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_set_mode() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 status with USB mode.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Read the set_mode command.
            expect_command_and_respond(&mut stream, "slice set 0 mode=CW", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let m = rig.get_mode(ReceiverId::from_index(0)).await.unwrap();
        assert_eq!(m, Mode::USB);

        rig.set_mode(ReceiverId::from_index(0), Mode::CW)
            .await
            .unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // PTT
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_set_ptt() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send TX state.
            stream.write_all(b"S12345678|tx state=0\n").await.unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Read the xmit command.
            expect_command_and_respond(&mut stream, "xmit 1", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let ptt = rig.get_ptt().await.unwrap();
        assert!(!ptt);

        rig.set_ptt(true).await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Power
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_set_power() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send TX power state.
            stream.write_all(b"S12345678|tx power=75\n").await.unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Read the set_power command.
            expect_command_and_respond(&mut stream, "transmit set power=50", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let power = rig.get_power().await.unwrap();
        assert!(
            (power - 75.0).abs() < f32::EPSILON,
            "expected 75W, got {power}W"
        );

        rig.set_power(50.0).await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Receivers
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_receivers() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 and slice 1 status.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB tx=1 active=1\n")
                .await
                .unwrap();
            stream
                .write_all(b"S12345678|slice 1 RF_frequency=7.074000 mode=CW tx=0 active=0\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let rxs = rig.receivers().await.unwrap();
        assert_eq!(rxs.len(), 2);
        assert_eq!(rxs[0], ReceiverId::from_index(0));
        assert_eq!(rxs[1], ReceiverId::from_index(1));

        let primary = rig.primary_receiver().await.unwrap();
        assert_eq!(primary, ReceiverId::from_index(0)); // TX slice

        let secondary = rig.secondary_receiver().await.unwrap();
        assert_eq!(secondary, Some(ReceiverId::from_index(1)));

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Split
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_split() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Slice 0: TX=1, active=0; Slice 1: TX=0, active=1
            // This means TX is on slice 0 but active is slice 1 => split ON.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB tx=1 active=0\n")
                .await
                .unwrap();
            stream
                .write_all(b"S12345678|slice 1 RF_frequency=7.074000 mode=CW tx=0 active=1\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Read set_split(false) -- sets TX to slice 0
            expect_command_and_respond(&mut stream, "slice set 0 tx=1", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let split = rig.get_split().await.unwrap();
        assert!(split, "split should be on when TX and active differ");

        // Turn off split.
        rig.set_split(false).await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Subscribe Events
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_subscribe_events() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send a frequency change.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=7.074000\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        let mut rx = rig.subscribe().unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;

        let mut found_freq_change = false;
        while let Ok(event) = rx.try_recv() {
            if let RigEvent::FrequencyChanged { receiver, freq_hz } = event {
                assert_eq!(receiver, ReceiverId::from_index(0));
                assert_eq!(freq_hz, 7_074_000);
                found_freq_change = true;
            }
        }
        assert!(found_freq_change, "expected FrequencyChanged event");

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // AGC
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_agc() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 status with agc_mode.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB agc_mode=fast\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let agc = rig.get_agc(ReceiverId::from_index(0)).await.unwrap();
        assert_eq!(agc, AgcMode::Fast);

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_get_agc_med() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB agc_mode=med\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let agc = rig.get_agc(ReceiverId::from_index(0)).await.unwrap();
        assert_eq!(agc, AgcMode::Medium);

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_get_agc_not_yet_known() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Slice exists but no agc_mode field sent.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let result = rig.get_agc(ReceiverId::from_index(0)).await;
        assert!(result.is_err());

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_set_agc() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 status so the slice exists.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB agc_mode=off\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Read the set_agc command.
            expect_command_and_respond(&mut stream, "slice set 0 agc_mode=slow", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        rig.set_agc(ReceiverId::from_index(0), AgcMode::Slow)
            .await
            .unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_set_agc_medium_sends_med() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB agc_mode=off\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Medium should map to "med" in SmartSDR.
            expect_command_and_respond(&mut stream, "slice set 0 agc_mode=med", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        rig.set_agc(ReceiverId::from_index(0), AgcMode::Medium)
            .await
            .unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // RIT
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_rit() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 status with RIT enabled and offset.
            stream
                .write_all(
                    b"S12345678|slice 0 RF_frequency=14.250000 mode=USB tx=1 active=1 rit_on=1 rit_freq=150\n",
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let (enabled, offset) = rig.get_rit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset, 150);

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_get_rit_disabled() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            stream
                .write_all(
                    b"S12345678|slice 0 RF_frequency=14.250000 mode=USB tx=1 active=1 rit_on=0 rit_freq=0\n",
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let (enabled, offset) = rig.get_rit().await.unwrap();
        assert!(!enabled);
        assert_eq!(offset, 0);

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_set_rit() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 status so the slice exists.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB tx=1 active=1\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Expect the two RIT commands.
            expect_command_and_respond(&mut stream, "slice set 0 rit_on=1", "").await;
            expect_command_and_respond(&mut stream, "slice set 0 rit_freq=250", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        rig.set_rit(true, 250).await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_set_rit_disable() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB tx=1 active=1\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            expect_command_and_respond(&mut stream, "slice set 0 rit_on=0", "").await;
            expect_command_and_respond(&mut stream, "slice set 0 rit_freq=0", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        rig.set_rit(false, 0).await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // XIT
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_get_xit() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            stream
                .write_all(
                    b"S12345678|slice 0 RF_frequency=14.250000 mode=USB tx=1 active=1 xit_on=1 xit_freq=500\n",
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let (enabled, offset) = rig.get_xit().await.unwrap();
        assert!(enabled);
        assert_eq!(offset, 500);

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_set_xit() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB tx=1 active=1\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            expect_command_and_respond(&mut stream, "slice set 0 xit_on=1", "").await;
            expect_command_and_respond(&mut stream, "slice set 0 xit_freq=500", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        rig.set_xit(true, 500).await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Create / Destroy Slice
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_create_slice() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Read the create command and respond with slice index 2.
            expect_command_and_respond(&mut stream, "slice create", "2").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        let rx = rig.create_slice(21_074_000, Mode::USB).await.unwrap();
        assert_eq!(rx, ReceiverId::from_index(2));

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_destroy_slice() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Read the remove command.
            expect_command_and_respond(&mut stream, "slice remove 1", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        rig.destroy_slice(ReceiverId::from_index(1)).await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Passband
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_passband() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 status with filter settings.
            stream
                .write_all(
                    b"S12345678|slice 0 RF_frequency=14.250000 mode=USB filter_lo=100 filter_hi=2900\n",
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Read the set_passband command.
            expect_command_and_respond(&mut stream, "slice set 0 filter_lo", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        tokio::time::sleep(Duration::from_millis(200)).await;

        let pb = rig.get_passband(ReceiverId::from_index(0)).await.unwrap();
        assert_eq!(pb.hz(), 2800); // 2900 - 100

        // Set a narrower passband (500 Hz for CW).
        rig.set_passband(ReceiverId::from_index(0), Passband::from_hz(500))
            .await
            .unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // AudioCapable: basic trait methods
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_audio_supported() {
        let (listener, addr) = mock_smartsdr_server().await;
        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        assert!(rig.audio_supported());

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_native_audio_config() {
        let (listener, addr) = mock_smartsdr_server().await;
        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        let config = rig.native_audio_config();
        assert_eq!(config.sample_rate, 24_000);
        assert_eq!(config.channels, 2);
        assert_eq!(
            config.sample_format,
            riglib_core::audio::AudioSampleFormat::F32
        );

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_tx_audio_unsupported() {
        let (listener, addr) = mock_smartsdr_server().await;
        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        let result = rig.start_tx_audio(None).await;
        assert!(result.is_err());
        match result {
            Err(Error::Unsupported(msg)) => {
                assert!(
                    msg.contains("TX audio not yet implemented"),
                    "unexpected error message: {}",
                    msg
                );
            }
            Err(other) => panic!("expected Unsupported error, got: {:?}", other),
            Ok(_) => panic!("expected error, got Ok"),
        }

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // AudioCapable: start_rx_audio + DAX audio routing via UDP
    // -----------------------------------------------------------------------

    /// Build a VITA-49 DaxAudio packet with the given stream_id and audio
    /// samples (as left/right f32 pairs).
    ///
    /// This is a minimal packet builder for test purposes. It produces a
    /// valid VITA-49 Extension Data packet with FlexRadio OUI and class
    /// code 0x03E3 (DaxAudio).
    fn build_dax_audio_vita49_packet(stream_id: u32, samples: &[(f32, f32)]) -> Vec<u8> {
        use crate::vita49::{FLEXRADIO_OUI, HEADER_SIZE};

        let payload_len = samples.len() * 8;
        let total_bytes = HEADER_SIZE + payload_len;
        assert!(total_bytes % 4 == 0, "total packet must be word-aligned");
        let size_words = (total_bytes / 4) as u16;

        let mut buf = vec![0u8; total_bytes];

        // Header word (offset 0-3, big-endian)
        let packet_type: u32 = 0x3; // Extension Data with Stream ID
        let class_id_present: u32 = 1;
        let mut hw: u32 = 0;
        hw |= (packet_type & 0x0F) << 28;
        hw |= class_id_present << 27;
        // TSI=01 (UTC), TSF=01 (sample count)
        hw |= 0x01 << 20;
        hw |= 0x01 << 18;
        hw |= size_words as u32 & 0x0FFF;
        buf[0..4].copy_from_slice(&hw.to_be_bytes());

        // Stream ID (offset 4-7)
        buf[4..8].copy_from_slice(&stream_id.to_be_bytes());

        // Class OUI (offset 8-11): FlexRadio OUI << 8
        let class_upper: u32 = FLEXRADIO_OUI << 8;
        buf[8..12].copy_from_slice(&class_upper.to_be_bytes());

        // Info class code (0x534C) + packet class code (0x03E3 = DaxAudio)
        let class_lower: u32 = (0x534C_u32 << 16) | 0x03E3_u32;
        buf[12..16].copy_from_slice(&class_lower.to_be_bytes());

        // Integer timestamp (offset 16-19) -- zero for test
        // Fractional timestamp (offset 20-27) -- zero for test
        // (already zeroed)

        // Payload: interleaved left/right f32 little-endian
        let mut offset = HEADER_SIZE;
        for &(left, right) in samples {
            buf[offset..offset + 4].copy_from_slice(&left.to_le_bytes());
            buf[offset + 4..offset + 8].copy_from_slice(&right.to_le_bytes());
            offset += 8;
        }

        buf
    }

    #[tokio::test]
    async fn test_start_rx_audio() {
        let (listener, addr) = mock_smartsdr_server().await;

        let stream_handle: u32 = 0x2000_0001;

        let server = tokio::spawn({
            async move {
                let mut stream = accept_and_handshake(&listener).await;

                // Read slice 0 status so the slice exists for ensure_slice.
                tokio::time::sleep(Duration::from_millis(50)).await;
                stream
                    .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB\n")
                    .await
                    .unwrap();
                stream.flush().await.unwrap();

                tokio::time::sleep(Duration::from_millis(100)).await;

                // Expect: stream create type=dax_rx dax_channel=1
                {
                    let mut reader = BufReader::new(&mut stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).await.unwrap();
                    let trimmed = line.trim();
                    assert!(
                        trimmed.contains("stream create type=dax_rx dax_channel=1"),
                        "expected dax_rx create, got: {}",
                        trimmed
                    );
                    let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
                    let resp = format!("R{}|00000000|{:08X}\n", seq_str, stream_handle);
                    let inner = reader.into_inner();
                    inner.write_all(resp.as_bytes()).await.unwrap();
                    inner.flush().await.unwrap();
                }

                // Expect: slice set 0 dax=1
                {
                    let mut reader = BufReader::new(&mut stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).await.unwrap();
                    let trimmed = line.trim();
                    assert!(
                        trimmed.contains("slice set 0 dax=1"),
                        "expected slice set dax, got: {}",
                        trimmed
                    );
                    let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
                    let resp = format!("R{}|00000000|\n", seq_str);
                    let inner = reader.into_inner();
                    inner.write_all(resp.as_bytes()).await.unwrap();
                    inner.flush().await.unwrap();
                }

                // Keep connection alive.
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });

        let client = connect_client(&addr).await;

        // Start a mock UDP receiver via channel instead of a real UdpSocket.
        let (udp_tx, udp_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        client.start_mock_udp_receiver(udp_rx).await;

        let rig = make_flex_radio(client);

        // Wait for slice status to be processed.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Start RX audio on slice 0 (VFO-A).
        let mut audio_rx = rig
            .start_rx_audio(ReceiverId::from_index(0), None)
            .await
            .unwrap();

        // Verify the config.
        assert_eq!(audio_rx.config().sample_rate, 24_000);
        assert_eq!(audio_rx.config().channels, 2);

        // Inject a synthetic DAX audio VITA-49 packet via the mock channel.
        let known_samples: Vec<(f32, f32)> =
            vec![(0.25, -0.25), (0.5, -0.5), (0.75, -0.75), (1.0, -1.0)];
        let packet = build_dax_audio_vita49_packet(stream_handle, &known_samples);
        udp_tx.send(packet).await.unwrap();

        // Receive the audio buffer (with timeout to avoid hanging).
        let buf = tokio::time::timeout(Duration::from_secs(2), audio_rx.recv())
            .await
            .expect("timed out waiting for audio buffer")
            .expect("audio stream closed unexpectedly");

        // Verify the audio data matches what we sent.
        assert_eq!(buf.sample_rate, 24_000);
        assert_eq!(buf.channels, 2);
        assert_eq!(buf.samples.len(), 8); // 4 stereo frames = 8 samples
        assert_eq!(buf.frame_count(), 4);

        // Check individual sample values.
        let expected: Vec<f32> = vec![0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1.0, -1.0];
        for (i, (&got, &exp)) in buf.samples.iter().zip(expected.iter()).enumerate() {
            assert!(
                (got - exp).abs() < f32::EPSILON,
                "sample {} mismatch: got {}, expected {}",
                i,
                got,
                exp
            );
        }

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_stop_audio() {
        let (listener, addr) = mock_smartsdr_server().await;

        let stream_handle: u32 = 0x2000_0002;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send slice 0 status.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=14.250000 mode=USB\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Expect: stream create type=dax_rx dax_channel=1
            {
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let trimmed = line.trim();
                assert!(
                    trimmed.contains("stream create type=dax_rx"),
                    "expected stream create, got: {}",
                    trimmed
                );
                let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
                let resp = format!("R{}|00000000|{:08X}\n", seq_str, stream_handle);
                let inner = reader.into_inner();
                inner.write_all(resp.as_bytes()).await.unwrap();
                inner.flush().await.unwrap();
            }

            // Expect: slice set 0 dax=1
            {
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let trimmed = line.trim();
                assert!(
                    trimmed.contains("slice set 0 dax=1"),
                    "expected slice set dax, got: {}",
                    trimmed
                );
                let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
                let resp = format!("R{}|00000000|\n", seq_str);
                let inner = reader.into_inner();
                inner.write_all(resp.as_bytes()).await.unwrap();
                inner.flush().await.unwrap();
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            // Expect: stream remove 0x20000002
            {
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let trimmed = line.trim();
                assert!(
                    trimmed.contains("stream remove 0x20000002"),
                    "expected stream remove, got: {}",
                    trimmed
                );
                let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
                let resp = format!("R{}|00000000|\n", seq_str);
                let inner = reader.into_inner();
                inner.write_all(resp.as_bytes()).await.unwrap();
                inner.flush().await.unwrap();
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        // Wait for slice status.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Start audio.
        let mut audio_rx = rig
            .start_rx_audio(ReceiverId::from_index(0), None)
            .await
            .unwrap();

        // Stop audio -- should send stream remove command.
        rig.stop_audio().await.unwrap();

        // The AudioReceiver should now be closed (sender was dropped).
        let result = tokio::time::timeout(Duration::from_millis(200), audio_rx.recv()).await;
        match result {
            Ok(None) => {} // Expected: stream closed.
            Ok(Some(_)) => panic!("expected stream to be closed after stop_audio"),
            Err(_) => {
                // Timeout is also acceptable if the channel hasn't been
                // polled yet. The sender was dropped, so the next recv
                // will return None.
            }
        }

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // CW Messages (CWX)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_send_cw_message() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Read the cwx send command.
            expect_command_and_respond(&mut stream, "cwx send", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        rig.send_cw_message("CQ TEST").await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_send_cw_message_empty() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        // Empty message should return Ok without sending a command.
        rig.send_cw_message("").await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_stop_cw_message() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Read the cwx clear command.
            expect_command_and_respond(&mut stream, "cwx clear", "").await;

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let client = connect_client(&addr).await;
        let rig = make_flex_radio(client);

        rig.stop_cw_message().await.unwrap();

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Meter injection via mock UDP channel
    // -----------------------------------------------------------------------

    /// Build a VITA-49 MeterData packet with the given meter readings.
    ///
    /// Each reading is a (meter_id, value) pair encoded as big-endian
    /// u16 + i16 in the payload. Uses class code 0x8002.
    fn build_meter_vita49_packet(readings: &[(u16, i16)]) -> Vec<u8> {
        use crate::vita49::{FLEXRADIO_OUI, HEADER_SIZE};

        let payload_len = readings.len() * 4;
        let total_bytes = HEADER_SIZE + payload_len;
        assert!(total_bytes % 4 == 0, "total packet must be word-aligned");
        let size_words = (total_bytes / 4) as u16;

        let mut buf = vec![0u8; total_bytes];

        // Header word (offset 0-3, big-endian)
        let packet_type: u32 = 0x3; // Extension Data with Stream ID
        let class_id_present: u32 = 1;
        let mut hw: u32 = 0;
        hw |= (packet_type & 0x0F) << 28;
        hw |= class_id_present << 27;
        // TSI=01 (UTC), TSF=01 (sample count)
        hw |= 0x01 << 20;
        hw |= 0x01 << 18;
        hw |= size_words as u32 & 0x0FFF;
        buf[0..4].copy_from_slice(&hw.to_be_bytes());

        // Stream ID (offset 4-7) -- arbitrary for meter data
        buf[4..8].copy_from_slice(&0x0000_0001u32.to_be_bytes());

        // Class OUI (offset 8-11): FlexRadio OUI << 8
        let class_upper: u32 = FLEXRADIO_OUI << 8;
        buf[8..12].copy_from_slice(&class_upper.to_be_bytes());

        // Info class code (0x534C) + packet class code (0x8002 = MeterData)
        let class_lower: u32 = (0x534C_u32 << 16) | 0x8002_u32;
        buf[12..16].copy_from_slice(&class_lower.to_be_bytes());

        // Integer timestamp (offset 16-19) -- zero for test
        // Fractional timestamp (offset 20-27) -- zero for test
        // (already zeroed)

        // Payload: pairs of (u16 meter_id, i16 value), big-endian
        let mut offset = HEADER_SIZE;
        for &(meter_id, value) in readings {
            buf[offset..offset + 2].copy_from_slice(&meter_id.to_be_bytes());
            buf[offset + 2..offset + 4].copy_from_slice(&value.to_be_bytes());
            offset += 4;
        }

        buf
    }

    #[tokio::test]
    async fn test_meter_injection_via_channel() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Register meter ID 1 as "SLC0-S" via a TCP meter status message.
            stream
                .write_all(b"S12345678|meter 1 num=1 nam=SLC0-S\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            // Keep connection alive.
            tokio::time::sleep(Duration::from_secs(2)).await;
        });

        let client = connect_client(&addr).await;

        // Start mock UDP receiver via channel.
        let (udp_tx, udp_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
        client.start_mock_udp_receiver(udp_rx).await;

        // Wait for the meter status message to be processed by the TCP read loop.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Verify the meter is registered but has no value yet.
        assert!(
            client.meter_value("SLC0-S").await.is_none(),
            "meter should have no value before injection"
        );

        // Build and inject a VITA-49 meter packet with meter_id=1, value=-73.
        let packet = build_meter_vita49_packet(&[(1, -73)]);
        udp_tx.send(packet).await.unwrap();

        // Give the background task time to process the packet.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify the meter value is now accessible.
        let value = client
            .meter_value("SLC0-S")
            .await
            .expect("meter value should be set after injection");
        assert_eq!(value, -73, "expected meter value -73, got {}", value);

        // Inject a second packet to verify updates work.
        let packet2 = build_meter_vita49_packet(&[(1, 42)]);
        udp_tx.send(packet2).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let value2 = client
            .meter_value("SLC0-S")
            .await
            .expect("meter value should be updated");
        assert_eq!(value2, 42, "expected updated meter value 42, got {}", value2);

        client.disconnect().await.unwrap();
        server.abort();
    }
}
