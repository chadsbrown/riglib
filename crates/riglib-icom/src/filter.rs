//! IF filter width index ↔ Hz conversion for CI-V command 0x1A 0x03.
//!
//! Icom rigs encode filter width as an index, not Hz. The mapping is
//! non-linear and depends on mode family:
//!
//! - **SSB/CW/RTTY/PSK** — 41-entry table: indices 0–9 → 50–500 Hz
//!   (50 Hz steps), indices 10–40 → 600–3600 Hz (100 Hz steps).
//! - **AM** — linear 200 Hz steps from 200 Hz to 10 kHz.
//! - **FM** — not controllable via 0x1A 0x03 (fixed-bandwidth filter).
//!
//! Table matches hamlib's `filtericom[]` in `rigs/icom/icom.c`.

use riglib_core::types::Mode;

const SSB_FILTER_TABLE: [u32; 41] = [
    50, 100, 150, 200, 250, 300, 350, 400, 450, 500, 600, 700, 800, 900, 1000, 1100, 1200, 1300,
    1400, 1500, 1600, 1700, 1800, 1900, 2000, 2100, 2200, 2300, 2400, 2500, 2600, 2700, 2800, 2900,
    3000, 3100, 3200, 3300, 3400, 3500, 3600,
];

const AM_STEP_HZ: u32 = 200;
const AM_MIN_INDEX: u32 = 0;
const AM_MAX_INDEX: u32 = 49;

/// Mode family determining which filter-width encoding applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterFamily {
    /// SSB/CW/RTTY/PSK and their data variants — uses the 41-entry table.
    Ssb,
    /// AM and data-AM — linear 200 Hz steps.
    Am,
    /// FM — filter width is not settable via 0x1A 0x03.
    Fm,
}

/// Classify a [`Mode`] into the filter-width family used by Icom rigs.
pub fn filter_family(mode: Mode) -> FilterFamily {
    match mode {
        Mode::FM | Mode::DataFM => FilterFamily::Fm,
        Mode::AM | Mode::DataAM => FilterFamily::Am,
        Mode::USB
        | Mode::LSB
        | Mode::CW
        | Mode::CWR
        | Mode::RTTY
        | Mode::RTTYR
        | Mode::DataUSB
        | Mode::DataLSB => FilterFamily::Ssb,
    }
}

/// Convert a raw filter-width index (as returned by 0x1A 0x03) to hertz.
///
/// Returns `None` if the index is out of range for the given mode, or if
/// the mode does not support filter-width readback (FM).
pub fn index_to_hz(mode: Mode, index: u32) -> Option<u32> {
    match filter_family(mode) {
        FilterFamily::Ssb => SSB_FILTER_TABLE.get(index as usize).copied(),
        FilterFamily::Am => {
            if index > AM_MAX_INDEX {
                None
            } else {
                Some((index + 1) * AM_STEP_HZ)
            }
        }
        FilterFamily::Fm => None,
    }
}

/// Convert a desired filter width in hertz to the nearest valid index for
/// the given mode. Rounds to the nearest available slot and clamps to the
/// supported range.
///
/// Returns `None` for modes that do not support filter-width control (FM).
pub fn hz_to_index(mode: Mode, hz: u32) -> Option<u32> {
    match filter_family(mode) {
        FilterFamily::Ssb => {
            let (idx, _) = SSB_FILTER_TABLE
                .iter()
                .enumerate()
                .min_by_key(|&(_, &w)| w.abs_diff(hz))
                .expect("SSB_FILTER_TABLE is non-empty");
            Some(idx as u32)
        }
        FilterFamily::Am => {
            let rounded_steps = (hz + AM_STEP_HZ / 2) / AM_STEP_HZ;
            let idx = rounded_steps.saturating_sub(1);
            Some(idx.clamp(AM_MIN_INDEX, AM_MAX_INDEX))
        }
        FilterFamily::Fm => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssb_table_boundaries() {
        assert_eq!(index_to_hz(Mode::USB, 0), Some(50));
        assert_eq!(index_to_hz(Mode::USB, 9), Some(500));
        assert_eq!(index_to_hz(Mode::USB, 10), Some(600));
        assert_eq!(index_to_hz(Mode::USB, 31), Some(2700));
        assert_eq!(index_to_hz(Mode::USB, 40), Some(3600));
        assert_eq!(index_to_hz(Mode::USB, 41), None);
    }

    #[test]
    fn ssb_all_modes_use_same_table() {
        for mode in [
            Mode::USB,
            Mode::LSB,
            Mode::CW,
            Mode::CWR,
            Mode::RTTY,
            Mode::RTTYR,
            Mode::DataUSB,
            Mode::DataLSB,
        ] {
            assert_eq!(index_to_hz(mode, 31), Some(2700), "mode {mode:?}");
        }
    }

    #[test]
    fn ssb_hz_to_index_exact_values() {
        assert_eq!(hz_to_index(Mode::USB, 50), Some(0));
        assert_eq!(hz_to_index(Mode::USB, 500), Some(9));
        assert_eq!(hz_to_index(Mode::USB, 600), Some(10));
        assert_eq!(hz_to_index(Mode::USB, 2700), Some(31));
        assert_eq!(hz_to_index(Mode::USB, 3600), Some(40));
    }

    #[test]
    fn ssb_hz_to_index_rounds_to_nearest() {
        // 2750 is equidistant from 2700 (idx 31) and 2800 (idx 32);
        // min_by_key returns the first minimum, so 2700.
        assert_eq!(hz_to_index(Mode::USB, 2750), Some(31));
        // 2749 is closer to 2700.
        assert_eq!(hz_to_index(Mode::USB, 2749), Some(31));
        // 2751 is closer to 2800.
        assert_eq!(hz_to_index(Mode::USB, 2751), Some(32));
        // 550 is equidistant between 500 (idx 9) and 600 (idx 10); first wins.
        assert_eq!(hz_to_index(Mode::USB, 550), Some(9));
    }

    #[test]
    fn ssb_hz_to_index_clamps_out_of_range() {
        assert_eq!(hz_to_index(Mode::USB, 0), Some(0));
        assert_eq!(hz_to_index(Mode::USB, 20), Some(0));
        assert_eq!(hz_to_index(Mode::USB, 10_000), Some(40));
        assert_eq!(hz_to_index(Mode::USB, 1_000_000), Some(40));
    }

    #[test]
    fn ssb_round_trip_all_indices() {
        for idx in 0..=40u32 {
            let hz = index_to_hz(Mode::USB, idx).unwrap();
            assert_eq!(hz_to_index(Mode::USB, hz), Some(idx), "idx {idx}");
        }
    }

    #[test]
    fn am_table_boundaries() {
        assert_eq!(index_to_hz(Mode::AM, 0), Some(200));
        assert_eq!(index_to_hz(Mode::AM, 1), Some(400));
        assert_eq!(index_to_hz(Mode::AM, 29), Some(6000));
        assert_eq!(index_to_hz(Mode::AM, 49), Some(10_000));
        assert_eq!(index_to_hz(Mode::AM, 50), None);
    }

    #[test]
    fn am_hz_to_index_rounds_and_clamps() {
        assert_eq!(hz_to_index(Mode::AM, 200), Some(0));
        assert_eq!(hz_to_index(Mode::AM, 300), Some(1));
        assert_eq!(hz_to_index(Mode::AM, 299), Some(0));
        assert_eq!(hz_to_index(Mode::AM, 6000), Some(29));
        assert_eq!(hz_to_index(Mode::AM, 0), Some(0));
        assert_eq!(hz_to_index(Mode::AM, 100_000), Some(49));
    }

    #[test]
    fn am_round_trip_all_indices() {
        for idx in 0..=49u32 {
            let hz = index_to_hz(Mode::AM, idx).unwrap();
            assert_eq!(hz_to_index(Mode::AM, hz), Some(idx), "idx {idx}");
        }
    }

    #[test]
    fn data_am_uses_am_family() {
        assert_eq!(filter_family(Mode::DataAM), FilterFamily::Am);
        assert_eq!(index_to_hz(Mode::DataAM, 29), Some(6000));
    }

    #[test]
    fn fm_is_unsupported() {
        assert_eq!(filter_family(Mode::FM), FilterFamily::Fm);
        assert_eq!(filter_family(Mode::DataFM), FilterFamily::Fm);
        assert_eq!(index_to_hz(Mode::FM, 0), None);
        assert_eq!(hz_to_index(Mode::FM, 2700), None);
    }
}
