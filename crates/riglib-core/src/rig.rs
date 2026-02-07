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

    /// Subscribe to real-time rig events.
    ///
    /// Returns a broadcast receiver. The channel is bounded; if the consumer
    /// falls behind, older events will be dropped (lagged).
    fn subscribe(&self) -> Result<broadcast::Receiver<RigEvent>>;
}
