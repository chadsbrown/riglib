//! The `Rig` trait -- unified interface for all transceiver backends.
//!
//! This trait is the primary API surface of riglib. Contest loggers,
//! panadapter displays, and automation tools program against `dyn Rig`
//! without needing to know which manufacturer's protocol is in use.
//!
//! Each manufacturer backend (riglib-icom, riglib-kenwood, etc.) provides
//! a concrete type that implements this trait.

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::error::Result;
use crate::events::RigEvent;
use crate::types::*;

/// Unified asynchronous interface for controlling a transceiver.
///
/// All methods that communicate with the rig are `async` because the
/// underlying transport may involve serial I/O or network round-trips.
/// Methods that return cached state (like [`info()`](Rig::info) and
/// [`capabilities()`](Rig::capabilities)) are synchronous.
///
/// # Event Subscription
///
/// Use [`subscribe()`](Rig::subscribe) to obtain a broadcast receiver for
/// real-time state change notifications. This is more efficient than polling
/// for contest applications that need to track frequency and mode changes.
#[async_trait]
pub trait Rig: Send + Sync {
    /// Return static information about the connected rig (manufacturer, model).
    fn info(&self) -> &RigInfo;

    /// Return the capabilities of the connected rig.
    fn capabilities(&self) -> &RigCapabilities;

    /// List all available receiver IDs.
    ///
    /// Returns `[VFO_A]` for single-receiver rigs, `[VFO_A, VFO_B]` for
    /// dual-receiver rigs like the IC-7610, and slice IDs for FlexRadio.
    async fn receivers(&self) -> Result<Vec<ReceiverId>>;

    /// Return the primary (main) receiver ID.
    async fn primary_receiver(&self) -> Result<ReceiverId>;

    /// Return the secondary (sub) receiver ID, if the rig has one.
    async fn secondary_receiver(&self) -> Result<Option<ReceiverId>>;

    /// Get the current frequency of a receiver in hertz.
    async fn get_frequency(&self, rx: ReceiverId) -> Result<u64>;

    /// Set the frequency of a receiver in hertz.
    async fn set_frequency(&self, rx: ReceiverId, freq_hz: u64) -> Result<()>;

    /// Get the current operating mode of a receiver.
    async fn get_mode(&self, rx: ReceiverId) -> Result<Mode>;

    /// Set the operating mode of a receiver.
    async fn set_mode(&self, rx: ReceiverId, mode: Mode) -> Result<()>;

    /// Get the current passband (filter width) of a receiver.
    async fn get_passband(&self, rx: ReceiverId) -> Result<Passband>;

    /// Set the passband (filter width) of a receiver.
    async fn set_passband(&self, rx: ReceiverId, pb: Passband) -> Result<()>;

    /// Get the current PTT (push-to-talk) state.
    ///
    /// Returns `true` if the rig is transmitting.
    async fn get_ptt(&self) -> Result<bool>;

    /// Set the PTT state.
    ///
    /// Passing `true` keys the transmitter; `false` returns to receive.
    async fn set_ptt(&self, on: bool) -> Result<()>;

    /// Get the current transmit power setting in watts.
    async fn get_power(&self) -> Result<f32>;

    /// Set the transmit power level in watts.
    async fn set_power(&self, watts: f32) -> Result<()>;

    /// Read the S-meter value for a receiver.
    ///
    /// Returns the signal strength in dBm.
    async fn get_s_meter(&self, rx: ReceiverId) -> Result<f32>;

    /// Read the current SWR (standing wave ratio).
    ///
    /// Only meaningful while transmitting.
    async fn get_swr(&self) -> Result<f32>;

    /// Read the current ALC (automatic level control) reading.
    ///
    /// Values vary by manufacturer; consult the specific backend for
    /// interpretation.
    async fn get_alc(&self) -> Result<f32>;

    /// Get the current split operation state.
    ///
    /// Returns `true` if split is enabled (TX and RX on different frequencies).
    async fn get_split(&self) -> Result<bool>;

    /// Enable or disable split operation.
    async fn set_split(&self, on: bool) -> Result<()>;

    /// Set which receiver is used for transmit in split mode.
    async fn set_tx_receiver(&self, rx: ReceiverId) -> Result<()>;

    /// Assert or de-assert the CW key line.
    ///
    /// Requires a hardware key line to be configured (see builder's
    /// `key_line()` method). Returns `Unsupported` if no key line
    /// is configured or the transport doesn't support it.
    async fn set_cw_key(&self, on: bool) -> Result<()>;

    /// Get the current CW keyer speed in words per minute.
    async fn get_cw_speed(&self) -> Result<u8> {
        Err(crate::error::Error::Unsupported(
            "CW speed control not supported".into(),
        ))
    }

    /// Set the CW keyer speed in words per minute.
    async fn set_cw_speed(&self, _wpm: u8) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "CW speed control not supported".into(),
        ))
    }

    /// Copy the active VFO to the inactive VFO (VFO A=B).
    async fn set_vfo_a_eq_b(&self, _receiver: ReceiverId) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "VFO A=B not supported".into(),
        ))
    }

    /// Swap VFO A and VFO B frequencies.
    async fn swap_vfo(&self, _receiver: ReceiverId) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "VFO swap not supported".into(),
        ))
    }

    /// Get the currently selected antenna port for a receiver.
    async fn get_antenna(&self, _receiver: ReceiverId) -> Result<AntennaPort> {
        Err(crate::error::Error::Unsupported(
            "antenna port selection not supported".into(),
        ))
    }

    /// Set the antenna port for a receiver.
    async fn set_antenna(&self, _receiver: ReceiverId, _port: AntennaPort) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "antenna port selection not supported".into(),
        ))
    }

    /// Get the current AGC mode for a receiver.
    async fn get_agc(&self, _receiver: ReceiverId) -> Result<AgcMode> {
        Err(crate::error::Error::Unsupported(
            "AGC control not supported".into(),
        ))
    }

    /// Set the AGC mode for a receiver.
    async fn set_agc(&self, _receiver: ReceiverId, _mode: AgcMode) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "AGC control not supported".into(),
        ))
    }

    /// Get the current preamp level for a receiver.
    async fn get_preamp(&self, _receiver: ReceiverId) -> Result<PreampLevel> {
        Err(crate::error::Error::Unsupported(
            "preamp control not supported".into(),
        ))
    }

    /// Set the preamp level for a receiver.
    async fn set_preamp(&self, _receiver: ReceiverId, _level: PreampLevel) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "preamp control not supported".into(),
        ))
    }

    /// Get the current attenuator level for a receiver.
    async fn get_attenuator(&self, _receiver: ReceiverId) -> Result<AttenuatorLevel> {
        Err(crate::error::Error::Unsupported(
            "attenuator control not supported".into(),
        ))
    }

    /// Set the attenuator level for a receiver.
    async fn set_attenuator(
        &self,
        _receiver: ReceiverId,
        _level: AttenuatorLevel,
    ) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "attenuator control not supported".into(),
        ))
    }

    /// Get the current RIT (Receiver Incremental Tuning) state and offset.
    ///
    /// Returns `(enabled, offset_hz)` where `offset_hz` can be negative.
    async fn get_rit(&self) -> Result<(bool, i32)> {
        Err(crate::error::Error::Unsupported(
            "RIT not supported".into(),
        ))
    }

    /// Set the RIT state and offset in hertz.
    async fn set_rit(&self, _enabled: bool, _offset_hz: i32) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "RIT not supported".into(),
        ))
    }

    /// Get the current XIT (Transmitter Incremental Tuning) state and offset.
    ///
    /// Returns `(enabled, offset_hz)` where `offset_hz` can be negative.
    async fn get_xit(&self) -> Result<(bool, i32)> {
        Err(crate::error::Error::Unsupported(
            "XIT not supported".into(),
        ))
    }

    /// Set the XIT state and offset in hertz.
    async fn set_xit(&self, _enabled: bool, _offset_hz: i32) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "XIT not supported".into(),
        ))
    }

    /// Send a CW message as text.
    ///
    /// The rig's built-in keyer will convert the text to Morse code at the
    /// current keyer speed. The maximum message length varies by manufacturer
    /// (typically 24-30 characters). Longer messages will be chunked
    /// automatically where supported.
    async fn send_cw_message(&self, _message: &str) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "CW message sending not supported".into(),
        ))
    }

    /// Stop any in-progress CW message transmission.
    async fn stop_cw_message(&self) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "CW message sending not supported".into(),
        ))
    }

    /// Enable transceive (AI) mode for automatic state updates.
    ///
    /// When enabled, the rig will automatically send status updates when
    /// the user changes settings on the front panel. These are emitted
    /// as events via the [`subscribe`](Rig::subscribe) channel.
    async fn enable_transceive(&self) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "transceive mode not supported".into(),
        ))
    }

    /// Disable transceive (AI) mode.
    async fn disable_transceive(&self) -> Result<()> {
        Err(crate::error::Error::Unsupported(
            "transceive mode not supported".into(),
        ))
    }

    /// Subscribe to real-time rig events.
    ///
    /// Returns a broadcast receiver. The channel is bounded; if the consumer
    /// falls behind, older events will be dropped (lagged).
    fn subscribe(&self) -> Result<broadcast::Receiver<RigEvent>>;
}
