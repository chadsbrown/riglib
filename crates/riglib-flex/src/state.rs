//! Cached radio state from SmartSDR status messages and VITA-49 meter data.
//!
//! The FlexRadio pushes state changes continuously after subscription, so
//! `get_*` methods can return cached values with zero latency. This module
//! defines the state structures updated from:
//!
//! - **Status messages** (TCP) -- slice state, TX state
//! - **VITA-49 meter data** (UDP) -- meter values
//! - **Meter list response** (TCP) -- meter name-to-ID mapping

use std::collections::HashMap;

use crate::meters::MeterMap;

/// Complete cached state of the radio.
///
/// Updated concurrently by the TCP read loop (status messages) and the
/// UDP read loop (VITA-49 meter data). All access is guarded by an
/// `Arc<Mutex<RadioState>>` in the client.
#[derive(Debug, Clone, Default)]
pub struct RadioState {
    /// Per-slice receiver state, keyed by slice index (0-7).
    pub slices: HashMap<u8, SliceState>,
    /// Bidirectional meter name-to-ID mapping, populated from `meter list`.
    pub meters: MeterMap,
    /// Current meter values, keyed by runtime-assigned meter ID.
    pub meter_values: HashMap<u16, i16>,
    /// Transmit state.
    pub tx_state: TxState,
    /// Whether the client is currently connected.
    pub connected: bool,
}

/// State of a single FlexRadio slice (independent receiver).
#[derive(Debug, Clone, Default)]
pub struct SliceState {
    /// Slice index (0-7).
    pub index: u8,
    /// Slice frequency in Hz (converted from MHz in status messages).
    pub frequency_hz: u64,
    /// FlexRadio operating mode string (e.g. "USB", "CW", "DIGU").
    pub mode: String,
    /// Lower filter edge in Hz.
    pub filter_lo: i32,
    /// Upper filter edge in Hz.
    pub filter_hi: i32,
    /// Whether this slice is designated as the TX slice.
    pub is_tx: bool,
    /// Whether this slice is the active (focused) slice.
    pub active: bool,
    /// AGC mode string (e.g. "off", "slow", "med", "fast").
    pub agc_mode: String,
    /// Whether RIT is enabled.
    pub rit_on: bool,
    /// RIT frequency offset in Hz.
    pub rit_freq_hz: i32,
    /// Whether XIT is enabled.
    pub xit_on: bool,
    /// XIT frequency offset in Hz.
    pub xit_freq_hz: i32,
}

/// Transmit state.
#[derive(Debug, Clone, Default)]
pub struct TxState {
    /// Whether the radio is currently transmitting.
    pub transmitting: bool,
    /// Configured TX power level (0-100).
    pub power: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_state_default() {
        let state = RadioState::default();
        assert!(state.slices.is_empty());
        assert!(state.meter_values.is_empty());
        assert!(!state.connected);
        assert!(!state.tx_state.transmitting);
        assert_eq!(state.tx_state.power, 0);
    }

    #[test]
    fn slice_state_default() {
        let slice = SliceState::default();
        assert_eq!(slice.index, 0);
        assert_eq!(slice.frequency_hz, 0);
        assert_eq!(slice.mode, "");
        assert_eq!(slice.filter_lo, 0);
        assert_eq!(slice.filter_hi, 0);
        assert!(!slice.is_tx);
        assert!(!slice.active);
        assert!(!slice.rit_on);
        assert_eq!(slice.rit_freq_hz, 0);
        assert!(!slice.xit_on);
        assert_eq!(slice.xit_freq_hz, 0);
    }

    #[test]
    fn tx_state_default() {
        let tx = TxState::default();
        assert!(!tx.transmitting);
        assert_eq!(tx.power, 0);
    }

    #[test]
    fn slice_state_update() {
        let mut state = RadioState::default();
        let slice = SliceState {
            index: 0,
            frequency_hz: 14_250_000,
            mode: "USB".to_string(),
            filter_lo: 100,
            filter_hi: 2900,
            is_tx: true,
            active: true,
            agc_mode: String::new(),
            ..SliceState::default()
        };
        state.slices.insert(0, slice);

        let s = state.slices.get(&0).unwrap();
        assert_eq!(s.index, 0);
        assert_eq!(s.frequency_hz, 14_250_000);
        assert_eq!(s.mode, "USB");
        assert_eq!(s.filter_lo, 100);
        assert_eq!(s.filter_hi, 2900);
        assert!(s.is_tx);
        assert!(s.active);
    }

    #[test]
    fn meter_value_update() {
        let mut state = RadioState::default();
        state.meters.insert(5, "s-meter");
        state.meter_values.insert(5, -73);
        state.meter_values.insert(10, 42);

        assert_eq!(state.meter_values.get(&5), Some(&-73));
        assert_eq!(state.meter_values.get(&10), Some(&42));
        assert_eq!(state.meter_values.get(&99), None);

        // Verify meter map lookup
        assert_eq!(state.meters.id_for_name("s-meter"), Some(5));
    }

    #[test]
    fn tx_state_update() {
        let mut state = RadioState::default();
        state.tx_state.transmitting = true;
        state.tx_state.power = 75;

        assert!(state.tx_state.transmitting);
        assert_eq!(state.tx_state.power, 75);
    }

    #[test]
    fn multiple_slices() {
        let mut state = RadioState::default();
        for i in 0..4u8 {
            state.slices.insert(
                i,
                SliceState {
                    index: i,
                    frequency_hz: 14_000_000 + i as u64 * 50_000,
                    mode: "CW".to_string(),
                    is_tx: i == 0,
                    active: i == 0,
                    ..SliceState::default()
                },
            );
        }

        assert_eq!(state.slices.len(), 4);
        assert_eq!(state.slices.get(&0).unwrap().frequency_hz, 14_000_000);
        assert_eq!(state.slices.get(&1).unwrap().frequency_hz, 14_050_000);
        assert_eq!(state.slices.get(&2).unwrap().frequency_hz, 14_100_000);
        assert_eq!(state.slices.get(&3).unwrap().frequency_hz, 14_150_000);
        assert!(state.slices.get(&0).unwrap().is_tx);
        assert!(!state.slices.get(&1).unwrap().is_tx);
    }

    #[test]
    fn connected_state_transition() {
        let mut state = RadioState::default();
        assert!(!state.connected);
        state.connected = true;
        assert!(state.connected);
        state.connected = false;
        assert!(!state.connected);
    }
}
