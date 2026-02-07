//! Amateur radio band identification.
//!
//! Provides a [`Band`] enum covering the standard amateur HF, VHF, UHF,
//! and microwave bands. The primary use case is converting a raw frequency
//! (in hertz) to its band designation for contest logging, band maps, and
//! Cabrillo export.
//!
//! # Example
//!
//! ```
//! use riglib_core::Band;
//!
//! let band = Band::from_freq(14_074_000).unwrap();
//! assert_eq!(band, Band::Band20m);
//! assert_eq!(band.to_string(), "20m");
//! assert!(!band.is_warc());
//! ```

use std::fmt;
use std::str::FromStr;

use crate::BandRange;

/// Standard amateur radio band.
///
/// Covers bands from 160 meters (1.8 MHz) through 6 centimeters (5.65 GHz).
/// Band edges follow ITU Region 2 allocations where they differ between
/// regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Band {
    /// 160 meters (1.8–2.0 MHz).
    Band160m,
    /// 80 meters (3.5–4.0 MHz).
    Band80m,
    /// 60 meters (5.3305–5.4035 MHz).
    Band60m,
    /// 40 meters (7.0–7.3 MHz).
    Band40m,
    /// 30 meters (10.1–10.15 MHz). WARC band -- no contests.
    Band30m,
    /// 20 meters (14.0–14.35 MHz).
    Band20m,
    /// 17 meters (18.068–18.168 MHz). WARC band -- no contests.
    Band17m,
    /// 15 meters (21.0–21.45 MHz).
    Band15m,
    /// 12 meters (24.89–24.99 MHz). WARC band -- no contests.
    Band12m,
    /// 10 meters (28.0–29.7 MHz).
    Band10m,
    /// 6 meters (50.0–54.0 MHz).
    Band6m,
    /// 2 meters (144.0–148.0 MHz).
    Band2m,
    /// 70 centimeters (420.0–450.0 MHz).
    Band70cm,
    /// 23 centimeters (1240–1300 MHz).
    Band23cm,
    /// 13 centimeters (2300–2450 MHz).
    Band13cm,
    /// 6 centimeters (5650–5850 MHz).
    Band6cm,
}

/// All bands in frequency order, lowest first.
const ALL_BANDS: &[Band] = &[
    Band::Band160m,
    Band::Band80m,
    Band::Band60m,
    Band::Band40m,
    Band::Band30m,
    Band::Band20m,
    Band::Band17m,
    Band::Band15m,
    Band::Band12m,
    Band::Band10m,
    Band::Band6m,
    Band::Band2m,
    Band::Band70cm,
    Band::Band23cm,
    Band::Band13cm,
    Band::Band6cm,
];

impl Band {
    /// Returns the band containing the given frequency, or `None` if the
    /// frequency does not fall within any standard amateur band.
    pub fn from_freq(freq_hz: u64) -> Option<Band> {
        ALL_BANDS
            .iter()
            .copied()
            .find(|band| band.freq_range().contains(freq_hz))
    }

    /// Returns the frequency range (lower and upper edges) for this band.
    pub fn freq_range(&self) -> BandRange {
        match self {
            Band::Band160m => BandRange::new(1_800_000, 2_000_000),
            Band::Band80m => BandRange::new(3_500_000, 4_000_000),
            Band::Band60m => BandRange::new(5_330_500, 5_403_500),
            Band::Band40m => BandRange::new(7_000_000, 7_300_000),
            Band::Band30m => BandRange::new(10_100_000, 10_150_000),
            Band::Band20m => BandRange::new(14_000_000, 14_350_000),
            Band::Band17m => BandRange::new(18_068_000, 18_168_000),
            Band::Band15m => BandRange::new(21_000_000, 21_450_000),
            Band::Band12m => BandRange::new(24_890_000, 24_990_000),
            Band::Band10m => BandRange::new(28_000_000, 29_700_000),
            Band::Band6m => BandRange::new(50_000_000, 54_000_000),
            Band::Band2m => BandRange::new(144_000_000, 148_000_000),
            Band::Band70cm => BandRange::new(420_000_000, 450_000_000),
            Band::Band23cm => BandRange::new(1_240_000_000, 1_300_000_000),
            Band::Band13cm => BandRange::new(2_300_000_000, 2_450_000_000),
            Band::Band6cm => BandRange::new(5_650_000_000, 5_850_000_000),
        }
    }

    /// Returns `true` if this is a WARC band (30m, 17m, or 12m).
    ///
    /// The World Administrative Radio Conference bands are excluded from
    /// most amateur radio contests.
    pub fn is_warc(&self) -> bool {
        matches!(self, Band::Band30m | Band::Band17m | Band::Band12m)
    }

    /// Returns the short band name (e.g. "20m", "70cm").
    pub fn name(&self) -> &'static str {
        match self {
            Band::Band160m => "160m",
            Band::Band80m => "80m",
            Band::Band60m => "60m",
            Band::Band40m => "40m",
            Band::Band30m => "30m",
            Band::Band20m => "20m",
            Band::Band17m => "17m",
            Band::Band15m => "15m",
            Band::Band12m => "12m",
            Band::Band10m => "10m",
            Band::Band6m => "6m",
            Band::Band2m => "2m",
            Band::Band70cm => "70cm",
            Band::Band23cm => "23cm",
            Band::Band13cm => "13cm",
            Band::Band6cm => "6cm",
        }
    }

    /// Returns a slice of all bands in frequency order (lowest first).
    pub fn all() -> &'static [Band] {
        ALL_BANDS
    }
}

impl fmt::Display for Band {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Error returned when a string cannot be parsed into a [`Band`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseBandError(String);

impl fmt::Display for ParseBandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown band: '{}'", self.0)
    }
}

impl std::error::Error for ParseBandError {}

impl FromStr for Band {
    type Err = ParseBandError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "160m" | "160" => Ok(Band::Band160m),
            "80m" | "80" => Ok(Band::Band80m),
            "60m" | "60" => Ok(Band::Band60m),
            "40m" | "40" => Ok(Band::Band40m),
            "30m" | "30" => Ok(Band::Band30m),
            "20m" | "20" => Ok(Band::Band20m),
            "17m" | "17" => Ok(Band::Band17m),
            "15m" | "15" => Ok(Band::Band15m),
            "12m" | "12" => Ok(Band::Band12m),
            "10m" | "10" => Ok(Band::Band10m),
            "6m" | "6" => Ok(Band::Band6m),
            "2m" | "2" => Ok(Band::Band2m),
            "70cm" | "70" => Ok(Band::Band70cm),
            "23cm" | "23" => Ok(Band::Band23cm),
            "13cm" | "13" => Ok(Band::Band13cm),
            "6cm" => Ok(Band::Band6cm),
            _ => Err(ParseBandError(s.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_freq_hf_bands() {
        assert_eq!(Band::from_freq(1_840_000), Some(Band::Band160m));
        assert_eq!(Band::from_freq(3_573_000), Some(Band::Band80m));
        assert_eq!(Band::from_freq(5_357_000), Some(Band::Band60m));
        assert_eq!(Band::from_freq(7_074_000), Some(Band::Band40m));
        assert_eq!(Band::from_freq(10_136_000), Some(Band::Band30m));
        assert_eq!(Band::from_freq(14_074_000), Some(Band::Band20m));
        assert_eq!(Band::from_freq(18_100_000), Some(Band::Band17m));
        assert_eq!(Band::from_freq(21_074_000), Some(Band::Band15m));
        assert_eq!(Band::from_freq(24_915_000), Some(Band::Band12m));
        assert_eq!(Band::from_freq(28_074_000), Some(Band::Band10m));
    }

    #[test]
    fn from_freq_vhf_uhf() {
        assert_eq!(Band::from_freq(50_313_000), Some(Band::Band6m));
        assert_eq!(Band::from_freq(144_174_000), Some(Band::Band2m));
        assert_eq!(Band::from_freq(432_100_000), Some(Band::Band70cm));
    }

    #[test]
    fn from_freq_microwave() {
        assert_eq!(Band::from_freq(1_296_000_000), Some(Band::Band23cm));
        assert_eq!(Band::from_freq(2_400_000_000), Some(Band::Band13cm));
        assert_eq!(Band::from_freq(5_760_000_000), Some(Band::Band6cm));
    }

    #[test]
    fn from_freq_band_edges() {
        // Lower edges (inclusive)
        assert_eq!(Band::from_freq(1_800_000), Some(Band::Band160m));
        assert_eq!(Band::from_freq(14_000_000), Some(Band::Band20m));

        // Upper edges (inclusive)
        assert_eq!(Band::from_freq(2_000_000), Some(Band::Band160m));
        assert_eq!(Band::from_freq(14_350_000), Some(Band::Band20m));

        // Just outside
        assert_eq!(Band::from_freq(1_799_999), None);
        assert_eq!(Band::from_freq(14_350_001), None);
    }

    #[test]
    fn from_freq_out_of_band() {
        assert_eq!(Band::from_freq(0), None);
        assert_eq!(Band::from_freq(13_500_000), None); // shortwave broadcast
        assert_eq!(Band::from_freq(100_000_000), None); // FM broadcast
    }

    #[test]
    fn display() {
        assert_eq!(Band::Band20m.to_string(), "20m");
        assert_eq!(Band::Band70cm.to_string(), "70cm");
        assert_eq!(Band::Band160m.to_string(), "160m");
    }

    #[test]
    fn from_str_with_suffix() {
        assert_eq!("20m".parse::<Band>().unwrap(), Band::Band20m);
        assert_eq!("70cm".parse::<Band>().unwrap(), Band::Band70cm);
        assert_eq!("160M".parse::<Band>().unwrap(), Band::Band160m);
    }

    #[test]
    fn from_str_without_suffix() {
        assert_eq!("20".parse::<Band>().unwrap(), Band::Band20m);
        assert_eq!("70".parse::<Band>().unwrap(), Band::Band70cm);
        assert_eq!("160".parse::<Band>().unwrap(), Band::Band160m);
    }

    #[test]
    fn from_str_invalid() {
        assert!("99m".parse::<Band>().is_err());
        assert!("abc".parse::<Band>().is_err());
    }

    #[test]
    fn display_round_trip() {
        for &band in Band::all() {
            let s = band.to_string();
            let parsed: Band = s.parse().expect("should round-trip");
            assert_eq!(band, parsed);
        }
    }

    #[test]
    fn warc_bands() {
        assert!(Band::Band30m.is_warc());
        assert!(Band::Band17m.is_warc());
        assert!(Band::Band12m.is_warc());

        assert!(!Band::Band20m.is_warc());
        assert!(!Band::Band40m.is_warc());
        assert!(!Band::Band15m.is_warc());
        assert!(!Band::Band10m.is_warc());
    }

    #[test]
    fn all_returns_16_bands() {
        assert_eq!(Band::all().len(), 16);
    }

    #[test]
    fn all_in_frequency_order() {
        let bands = Band::all();
        for i in 1..bands.len() {
            assert!(
                bands[i].freq_range().low_hz > bands[i - 1].freq_range().high_hz,
                "{} should be higher than {}",
                bands[i],
                bands[i - 1]
            );
        }
    }

    #[test]
    fn freq_range_consistent() {
        for &band in Band::all() {
            let range = band.freq_range();
            assert!(range.low_hz < range.high_hz, "{band} has invalid range");
            // from_freq should return this band for the midpoint
            let mid = (range.low_hz + range.high_hz) / 2;
            assert_eq!(
                Band::from_freq(mid),
                Some(band),
                "midpoint of {band} should map back"
            );
        }
    }

    #[test]
    fn name_matches_display() {
        for &band in Band::all() {
            assert_eq!(band.name(), band.to_string());
        }
    }

    #[test]
    fn from_str_6m_vs_6cm() {
        // "6m" and "6cm" are both valid bands -- ensure no ambiguity.
        assert_eq!("6m".parse::<Band>().unwrap(), Band::Band6m);
        assert_eq!("6cm".parse::<Band>().unwrap(), Band::Band6cm);
        // "6" without suffix means 6 meters (the more common band).
        assert_eq!("6".parse::<Band>().unwrap(), Band::Band6m);
    }
}
