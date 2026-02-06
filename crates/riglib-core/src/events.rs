//! Asynchronous rig event types.
//!
//! Events are emitted by rig drivers through a [`tokio::sync::broadcast`] channel
//! when the radio's state changes. Contest loggers and panadapter displays subscribe
//! to these events for real-time UI updates without polling.

use crate::types::{Mode, ReceiverId};

/// An event emitted by a rig driver when radio state changes.
///
/// Subscribe to events via [`crate::rig::Rig::subscribe()`]. Events are delivered
/// on a best-effort basis through a bounded broadcast channel; slow consumers
/// may miss events under heavy load (e.g. rapid VFO knob movement).
#[derive(Debug, Clone)]
pub enum RigEvent {
    /// The frequency of a receiver changed.
    FrequencyChanged {
        /// Which receiver changed frequency.
        receiver: ReceiverId,
        /// New frequency in hertz.
        freq_hz: u64,
    },

    /// The operating mode of a receiver changed.
    ModeChanged {
        /// Which receiver changed mode.
        receiver: ReceiverId,
        /// New operating mode.
        mode: Mode,
    },

    /// Push-to-talk state changed (TX/RX transition).
    PttChanged {
        /// `true` if transmitting, `false` if receiving.
        on: bool,
    },

    /// Split operation state changed.
    SplitChanged {
        /// `true` if split is enabled.
        on: bool,
    },

    /// S-meter reading from a receiver.
    SmeterReading {
        /// Which receiver the reading is from.
        receiver: ReceiverId,
        /// Signal strength in dBm.
        dbm: f32,
    },

    /// Forward power reading during transmit.
    PowerReading {
        /// Power output in watts.
        watts: f32,
    },

    /// SWR reading during transmit.
    SwrReading {
        /// Standing wave ratio (e.g. 1.5 means 1.5:1).
        swr: f32,
    },

    /// Successfully connected to the rig.
    Connected,

    /// Connection to the rig was lost.
    Disconnected,

    /// Attempting to reconnect after a connection loss.
    Reconnecting {
        /// The reconnection attempt number (1-based).
        attempt: u32,
    },
}
