//! Icom model definitions.
//!
//! Each supported Icom rig is described by an [`IcomModel`] struct that
//! captures its CI-V address, baud rate, and capabilities. These are
//! compile-time constants used by the protocol engine to build commands
//! with the correct address and to validate operations against the rig's
//! feature set.
//!
//! Models are defined as factory functions (e.g. [`ic_7610()`]) that
//! return a fully populated [`IcomModel`]. The following models are
//! supported:
//!
//! | Model      | CI-V   | Baud    | Power | Coverage           |
//! |------------|--------|---------|-------|--------------------|
//! | IC-7600    | `0x7A` | 19200   | 100W  | HF + 6m            |
//! | IC-7700    | `0x74` | 19200   | 200W  | HF + 6m            |
//! | IC-7800    | `0x6A` | 19200   | 200W  | HF + 6m            |
//! | IC-7850    | `0x8E` | 115200  | 200W  | HF + 6m            |
//! | IC-7851    | `0x8E` | 115200  | 200W  | HF + 6m            |
//! | IC-7300    | `0x94` | 115200  | 100W  | HF + 6m            |
//! | IC-7610    | `0x98` | 115200  | 100W  | HF + 6m            |
//! | IC-9700    | `0xA2` | 115200  | 100W  | VHF/UHF/23cm       |
//! | IC-705     | `0xA4` | 115200  | 10W   | HF + VHF + UHF     |
//! | IC-7300MK2 | `0xA6` | 115200  | 100W  | HF + 6m            |
//! | IC-7100    | `0x88` | 19200   | 100W  | HF + VHF + UHF     |
//! | IC-9100    | `0x7C` | 19200   | 100W  | HF + VHF + UHF     |
//! | IC-7410    | `0x80` | 19200   | 100W  | HF + 6m            |
//! | IC-905     | `0xAC` | 115200  | 10W   | VHF/UHF/SHF        |

use riglib_core::{BandRange, ConnectionType, Manufacturer, Mode, RigCapabilities, RigDefinition};

/// Static model definition for an Icom transceiver.
///
/// Contains all the information needed to communicate with a specific
/// Icom model over CI-V, including the default bus address, serial
/// parameters, and a full capability description.
#[derive(Debug, Clone)]
pub struct IcomModel {
    /// Human-readable model name (e.g. "IC-7610").
    pub name: &'static str,
    /// Machine-readable model identifier.
    ///
    /// For Icom rigs this is the default CI-V address in hex notation
    /// (e.g. "0x98" for the IC-7610).
    pub model_id: &'static str,
    /// Default CI-V bus address.
    ///
    /// Each Icom model ships with a factory-default CI-V address. Users
    /// can change it in the rig's settings, but this is the common default.
    pub default_civ_address: u8,
    /// Default serial baud rate for USB connections.
    pub default_baud_rate: u32,
    /// Full capability description for this model.
    pub capabilities: RigCapabilities,
}

impl From<&IcomModel> for RigDefinition {
    fn from(model: &IcomModel) -> Self {
        RigDefinition {
            manufacturer: Manufacturer::Icom,
            model_name: model.name,
            connection: ConnectionType::Serial,
            default_baud_rate: Some(model.default_baud_rate),
            capabilities: model.capabilities.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions for building mode lists
// ---------------------------------------------------------------------------

/// Standard HF mode set used by most Icom HF transceivers.
///
/// Includes all analog voice modes, CW, RTTY, and data sub-modes.
fn hf_modes() -> Vec<Mode> {
    vec![
        Mode::LSB,
        Mode::USB,
        Mode::CW,
        Mode::CWR,
        Mode::RTTY,
        Mode::RTTYR,
        Mode::AM,
        Mode::FM,
        Mode::DataUSB,
        Mode::DataLSB,
        Mode::DataFM,
        Mode::DataAM,
    ]
}

/// Extended mode set for VHF/UHF-capable transceivers.
///
/// Same as the HF set -- all Icom multi-band rigs support the full
/// mode complement including data sub-modes on FM and AM.
fn all_modes() -> Vec<Mode> {
    hf_modes()
}

// ---------------------------------------------------------------------------
// HF + 6m band range (shared by most HF rigs)
// ---------------------------------------------------------------------------

/// Single contiguous range covering 1.8 MHz through 54 MHz (160m - 6m).
fn hf_6m_ranges() -> Vec<BandRange> {
    vec![BandRange::new(1_800_000, 54_000_000)]
}

// ---------------------------------------------------------------------------
// Model definitions
// ---------------------------------------------------------------------------

/// IC-7600 model definition.
///
/// The IC-7600 is Icom's mid-range HF transceiver introduced in 2009 with
/// dual-watch DSP and 100W output. It uses a traditional superhet
/// architecture with a 36 MHz first IF and DSP at the second IF.
///
/// Key specifications:
/// - CI-V address: `0x7A`
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - Dual VFO, no independent sub receiver
/// - 100W maximum output
/// - 19200 baud default (older USB interface)
pub fn ic_7600() -> IcomModel {
    IcomModel {
        name: "IC-7600",
        model_id: "0x7A",
        default_civ_address: 0x7A,
        default_baud_rate: 19_200,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: hf_modes(),
            frequency_ranges: hf_6m_ranges(),
            max_power_watts: 100.0,
        },
    }
}

/// IC-7700 model definition.
///
/// The IC-7700 is Icom's high-end HF transceiver with 200W output, a
/// built-in automatic antenna tuner, and dual-watch capability with two
/// independent receivers. It was positioned between the IC-7600 and the
/// flagship IC-7800.
///
/// Key specifications:
/// - CI-V address: `0x74`
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - Dual watch (2 receivers)
/// - 200W maximum output
/// - Built-in automatic antenna tuner
pub fn ic_7700() -> IcomModel {
    IcomModel {
        name: "IC-7700",
        model_id: "0x74",
        default_civ_address: 0x74,
        default_baud_rate: 19_200,
        capabilities: RigCapabilities {
            max_receivers: 2,
            has_sub_receiver: true,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: hf_modes(),
            frequency_ranges: hf_6m_ranges(),
            max_power_watts: 200.0,
        },
    }
}

/// IC-7800 model definition.
///
/// The IC-7800 was Icom's former flagship HF transceiver, featuring true
/// dual independent receivers with separate RF front-ends and 200W output.
/// It was the gold standard for SO2R contesting before the IC-7610 and
/// IC-7851 replaced it.
///
/// Key specifications:
/// - CI-V address: `0x6A`
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - True dual independent receivers (2 separate RF paths)
/// - 200W maximum output
pub fn ic_7800() -> IcomModel {
    IcomModel {
        name: "IC-7800",
        model_id: "0x6A",
        default_civ_address: 0x6A,
        default_baud_rate: 19_200,
        capabilities: RigCapabilities {
            max_receivers: 2,
            has_sub_receiver: true,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: hf_modes(),
            frequency_ranges: hf_6m_ranges(),
            max_power_watts: 200.0,
        },
    }
}

/// IC-7850 model definition.
///
/// The IC-7850 is Icom's premium flagship HF transceiver with true dual
/// independent receivers, direct-sampling SDR architecture, and 200W
/// output. It shares the same CI-V address (`0x8E`) and protocol as the
/// IC-7851. The IC-7851 is essentially the same radio with minor cosmetic
/// and production refinements.
///
/// Key specifications:
/// - CI-V address: `0x8E`
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - True dual independent receivers
/// - 200W maximum output
/// - Direct-sampling SDR architecture
pub fn ic_7850() -> IcomModel {
    IcomModel {
        name: "IC-7850",
        model_id: "0x8E",
        default_civ_address: 0x8E,
        default_baud_rate: 115_200,
        capabilities: RigCapabilities {
            max_receivers: 2,
            has_sub_receiver: true,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: true,
            supported_modes: hf_modes(),
            frequency_ranges: hf_6m_ranges(),
            max_power_watts: 200.0,
        },
    }
}

/// IC-7851 model definition.
///
/// The IC-7851 is a refined version of the IC-7850 with identical RF
/// performance and CI-V protocol. It shares the same CI-V address
/// (`0x8E`). From a software control perspective, the two models are
/// interchangeable.
///
/// Key specifications:
/// - CI-V address: `0x8E` (same as IC-7850)
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - True dual independent receivers
/// - 200W maximum output
/// - Direct-sampling SDR architecture
pub fn ic_7851() -> IcomModel {
    IcomModel {
        name: "IC-7851",
        model_id: "0x8E",
        default_civ_address: 0x8E,
        default_baud_rate: 115_200,
        capabilities: RigCapabilities {
            max_receivers: 2,
            has_sub_receiver: true,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: true,
            supported_modes: hf_modes(),
            frequency_ranges: hf_6m_ranges(),
            max_power_watts: 200.0,
        },
    }
}

/// IC-7300 model definition.
///
/// The IC-7300 is Icom's hugely popular entry-level SDR transceiver,
/// introduced in 2016. It brought direct-sampling SDR architecture to an
/// affordable price point and quickly became one of the best-selling HF
/// rigs of all time. It features a single receiver, 100W output, and a
/// real-time spectrum scope.
///
/// Key specifications:
/// - CI-V address: `0x94`
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - Single receiver
/// - 100W maximum output
/// - Direct-sampling SDR architecture
/// - USB connection at 115200 baud
pub fn ic_7300() -> IcomModel {
    IcomModel {
        name: "IC-7300",
        model_id: "0x94",
        default_civ_address: 0x94,
        default_baud_rate: 115_200,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: hf_modes(),
            frequency_ranges: hf_6m_ranges(),
            max_power_watts: 100.0,
        },
    }
}

/// IC-7610 model definition.
///
/// The IC-7610 is Icom's flagship HF contest transceiver with dual
/// independent receivers, direct-sampling SDR architecture, and I/Q
/// output over USB. It is the primary target for SO2R (single operator,
/// two radios) contesting setups using a single physical rig.
///
/// Key specifications:
/// - CI-V address: `0x98`
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - Dual independent receivers (main + sub)
/// - 100W maximum output
/// - USB I/Q output (raw SDR data)
/// - All standard amateur modes plus data sub-modes
pub fn ic_7610() -> IcomModel {
    IcomModel {
        name: "IC-7610",
        model_id: "0x98",
        default_civ_address: 0x98,
        default_baud_rate: 115_200,
        capabilities: RigCapabilities {
            max_receivers: 2,
            has_sub_receiver: true,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: true,
            supported_modes: vec![
                Mode::LSB,
                Mode::USB,
                Mode::CW,
                Mode::CWR,
                Mode::RTTY,
                Mode::RTTYR,
                Mode::AM,
                Mode::FM,
                Mode::DataUSB,
                Mode::DataLSB,
            ],
            frequency_ranges: vec![
                // HF bands: 1.8 MHz to 30 MHz (general coverage receive,
                // but we list the ham TX range for capability purposes).
                // The IC-7610 actually has general coverage receive 0.03-60 MHz
                // but transmit is amateur bands only. We list the full
                // transmit-capable range.
                BandRange::new(1_800_000, 54_000_000),
            ],
            max_power_watts: 100.0,
        },
    }
}

/// IC-9700 model definition.
///
/// The IC-9700 is Icom's SDR-based VHF/UHF/1.2 GHz all-mode transceiver,
/// introduced in 2019. It covers the 144 MHz, 430 MHz, and 1296 MHz
/// amateur bands with direct-sampling SDR on VHF/UHF and a traditional
/// architecture on 23cm. It supports dual-watch operation.
///
/// Key specifications:
/// - CI-V address: `0xA2`
/// - Frequency coverage: 144-148 MHz, 420-450 MHz, 1240-1300 MHz
/// - Dual watch (2 receivers)
/// - Power: 100W (VHF), 75W (UHF), 10W (23cm)
/// - USB connection at 115200 baud
pub fn ic_9700() -> IcomModel {
    IcomModel {
        name: "IC-9700",
        model_id: "0xA2",
        default_civ_address: 0xA2,
        default_baud_rate: 115_200,
        capabilities: RigCapabilities {
            max_receivers: 2,
            has_sub_receiver: true,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: true,
            supported_modes: all_modes(),
            frequency_ranges: vec![
                BandRange::new(144_000_000, 148_000_000),
                BandRange::new(420_000_000, 450_000_000),
                BandRange::new(1_240_000_000, 1_300_000_000),
            ],
            // Max power on the highest-power band (VHF).
            max_power_watts: 100.0,
        },
    }
}

/// IC-705 model definition.
///
/// The IC-705 is Icom's portable QRP transceiver covering HF through UHF
/// with 10W output. Introduced in 2020, it features a direct-sampling SDR
/// architecture, built-in GPS, Bluetooth, WiFi, and USB-C connectivity.
/// Despite its low power, it is popular for portable contesting, POTA/SOTA
/// activations, and satellite operation.
///
/// Key specifications:
/// - CI-V address: `0xA4`
/// - Frequency coverage: 1.8-54 MHz (HF+6m), 144-148 MHz, 420-450 MHz
/// - Single receiver
/// - 10W maximum output (5W on internal battery)
/// - Bluetooth, WiFi, USB-C connectivity
pub fn ic_705() -> IcomModel {
    IcomModel {
        name: "IC-705",
        model_id: "0xA4",
        default_civ_address: 0xA4,
        default_baud_rate: 115_200,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: all_modes(),
            frequency_ranges: vec![
                BandRange::new(1_800_000, 54_000_000),
                BandRange::new(144_000_000, 148_000_000),
                BandRange::new(420_000_000, 450_000_000),
            ],
            max_power_watts: 10.0,
        },
    }
}

/// IC-7300MK2 model definition.
///
/// The IC-7300MK2 is the 2025 refresh of the popular IC-7300, featuring
/// an updated direct-sampling SDR architecture, USB-C connectivity, LAN
/// port, and HDMI output for an external display. It retains the single-
/// receiver, 100W, HF+6m formula of the original.
///
/// Key specifications:
/// - CI-V address: `0xA6`
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - Single receiver
/// - 100W maximum output
/// - USB-C, LAN, HDMI connectivity
pub fn ic_7300mk2() -> IcomModel {
    IcomModel {
        name: "IC-7300MK2",
        model_id: "0xA6",
        default_civ_address: 0xA6,
        default_baud_rate: 115_200,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: hf_modes(),
            frequency_ranges: hf_6m_ranges(),
            max_power_watts: 100.0,
        },
    }
}

/// IC-7100 model definition.
///
/// The IC-7100 is Icom's all-band, all-mode transceiver covering HF
/// through UHF, introduced in 2012. It features a detachable control
/// head, D-STAR digital voice capability, and a touch screen. It is
/// popular as a mobile/base station for operators wanting HF through
/// UHF in a single package.
///
/// Key specifications:
/// - CI-V address: `0x88`
/// - Frequency coverage: 1.8-54 MHz, 144-148 MHz, 420-450 MHz
/// - Single receiver
/// - Power: 100W (HF/6m), 50W (VHF), 35W (UHF)
/// - 19200 baud default
pub fn ic_7100() -> IcomModel {
    IcomModel {
        name: "IC-7100",
        model_id: "0x88",
        default_civ_address: 0x88,
        default_baud_rate: 19_200,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: all_modes(),
            frequency_ranges: vec![
                BandRange::new(1_800_000, 54_000_000),
                BandRange::new(144_000_000, 148_000_000),
                BandRange::new(420_000_000, 450_000_000),
            ],
            // Max power on HF/6m band.
            max_power_watts: 100.0,
        },
    }
}

/// IC-9100 model definition.
///
/// The IC-9100 is Icom's HF/VHF/UHF base station transceiver introduced
/// in 2011. It covers HF through UHF with an optional 23cm (1.2 GHz)
/// module. It is popular for satellite operation and VHF/UHF contesting
/// combined with HF capability.
///
/// Key specifications:
/// - CI-V address: `0x7C`
/// - Frequency coverage: 1.8-54 MHz, 144-148 MHz, 420-450 MHz
/// - Single receiver (dual watch on some band combinations)
/// - Power: 100W (HF/6m), 50W (VHF), 75W (UHF)
/// - 19200 baud default
pub fn ic_9100() -> IcomModel {
    IcomModel {
        name: "IC-9100",
        model_id: "0x7C",
        default_civ_address: 0x7C,
        default_baud_rate: 19_200,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: all_modes(),
            frequency_ranges: vec![
                BandRange::new(1_800_000, 54_000_000),
                BandRange::new(144_000_000, 148_000_000),
                BandRange::new(420_000_000, 450_000_000),
            ],
            max_power_watts: 100.0,
        },
    }
}

/// IC-7410 model definition.
///
/// The IC-7410 is Icom's mid-range HF+6m transceiver introduced in 2012.
/// It features a double-conversion superheterodyne architecture with DSP,
/// 100W output, and a large display. It was positioned as an affordable
/// HF contesting rig.
///
/// Key specifications:
/// - CI-V address: `0x80`
/// - Frequency coverage: 1.8 MHz - 54 MHz (HF + 6m)
/// - Single receiver
/// - 100W maximum output
/// - 19200 baud default
pub fn ic_7410() -> IcomModel {
    IcomModel {
        name: "IC-7410",
        model_id: "0x80",
        default_civ_address: 0x80,
        default_baud_rate: 19_200,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: hf_modes(),
            frequency_ranges: hf_6m_ranges(),
            max_power_watts: 100.0,
        },
    }
}

/// IC-905 model definition.
///
/// The IC-905 is Icom's microwave/SHF transceiver introduced in 2022,
/// covering VHF, UHF, 23cm, and microwave bands (2.4 GHz, 5.6 GHz,
/// 10 GHz with optional transverters). The main unit is typically
/// mounted near the antenna with remote control via LAN or USB.
///
/// Key specifications:
/// - CI-V address: `0xAC`
/// - Frequency coverage: 144-148 MHz, 420-450 MHz, 1240-1300 MHz,
///   2400-2450 MHz, 5650-5850 MHz
/// - Single receiver
/// - Power: 10W (VHF/UHF/23cm), 2W (2.4 GHz), 1W (5.6 GHz)
/// - LAN primary interface, USB secondary
pub fn ic_905() -> IcomModel {
    IcomModel {
        name: "IC-905",
        model_id: "0xAC",
        default_civ_address: 0xAC,
        default_baud_rate: 115_200,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: all_modes(),
            frequency_ranges: vec![
                BandRange::new(144_000_000, 148_000_000),
                BandRange::new(420_000_000, 450_000_000),
                BandRange::new(1_240_000_000, 1_300_000_000),
                BandRange::new(2_400_000_000, 2_450_000_000),
                BandRange::new(5_650_000_000, 5_850_000_000),
            ],
            max_power_watts: 10.0,
        },
    }
}

/// Returns a list of all supported Icom model definitions.
///
/// This is useful for building model selection UIs or iterating over
/// all known models for auto-detection.
pub fn all_icom_models() -> Vec<IcomModel> {
    vec![
        ic_7600(),
        ic_7700(),
        ic_7800(),
        ic_7850(),
        ic_7851(),
        ic_7300(),
        ic_7610(),
        ic_9700(),
        ic_705(),
        ic_7300mk2(),
        ic_7100(),
        ic_9100(),
        ic_7410(),
        ic_905(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_core::Mode;

    // -----------------------------------------------------------------------
    // IC-7610 (existing tests, preserved)
    // -----------------------------------------------------------------------

    #[test]
    fn ic7610_basic_properties() {
        let model = ic_7610();
        assert_eq!(model.name, "IC-7610");
        assert_eq!(model.model_id, "0x98");
        assert_eq!(model.default_civ_address, 0x98);
        assert_eq!(model.default_baud_rate, 115_200);
    }

    #[test]
    fn ic7610_dual_receiver() {
        let model = ic_7610();
        assert_eq!(model.capabilities.max_receivers, 2);
        assert!(model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ic7610_has_split() {
        let model = ic_7610();
        assert!(model.capabilities.has_split);
    }

    #[test]
    fn ic7610_has_iq_output() {
        let model = ic_7610();
        assert!(model.capabilities.has_iq_output);
        assert!(!model.capabilities.has_audio_streaming);
    }

    #[test]
    fn ic7610_max_power() {
        let model = ic_7610();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic7610_frequency_coverage() {
        let model = ic_7610();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 1);

        let range = &ranges[0];
        // 160m band edge
        assert!(range.contains(1_800_000));
        // 20m contest frequency
        assert!(range.contains(14_250_000));
        // 6m
        assert!(range.contains(50_100_000));
        // Just inside upper bound
        assert!(range.contains(54_000_000));
        // Below coverage
        assert!(!range.contains(1_799_999));
        // Above coverage (2m not supported)
        assert!(!range.contains(144_000_000));
    }

    #[test]
    fn ic7610_supported_modes() {
        let model = ic_7610();
        let modes = &model.capabilities.supported_modes;

        // All expected modes present
        assert!(modes.contains(&Mode::LSB));
        assert!(modes.contains(&Mode::USB));
        assert!(modes.contains(&Mode::CW));
        assert!(modes.contains(&Mode::CWR));
        assert!(modes.contains(&Mode::RTTY));
        assert!(modes.contains(&Mode::RTTYR));
        assert!(modes.contains(&Mode::AM));
        assert!(modes.contains(&Mode::FM));
        assert!(modes.contains(&Mode::DataUSB));
        assert!(modes.contains(&Mode::DataLSB));

        // DataFM and DataAM are not standard on the IC-7610
        assert!(!modes.contains(&Mode::DataFM));
        assert!(!modes.contains(&Mode::DataAM));
    }

    // -----------------------------------------------------------------------
    // IC-7600
    // -----------------------------------------------------------------------

    #[test]
    fn ic7600_basic_properties() {
        let model = ic_7600();
        assert_eq!(model.name, "IC-7600");
        assert_eq!(model.model_id, "0x7A");
        assert_eq!(model.default_civ_address, 0x7A);
        assert_eq!(model.default_baud_rate, 19_200);
    }

    #[test]
    fn ic7600_capabilities() {
        let model = ic_7600();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.has_iq_output);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic7600_hf_coverage() {
        let model = ic_7600();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].contains(14_074_000)); // FT8 on 20m
        assert!(ranges[0].contains(50_313_000)); // 6m CW
        assert!(!ranges[0].contains(144_000_000)); // no 2m
    }

    // -----------------------------------------------------------------------
    // IC-7700
    // -----------------------------------------------------------------------

    #[test]
    fn ic7700_basic_properties() {
        let model = ic_7700();
        assert_eq!(model.name, "IC-7700");
        assert_eq!(model.model_id, "0x74");
        assert_eq!(model.default_civ_address, 0x74);
        assert_eq!(model.default_baud_rate, 19_200);
    }

    #[test]
    fn ic7700_capabilities() {
        let model = ic_7700();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 200.0).abs() < f32::EPSILON);
    }

    // -----------------------------------------------------------------------
    // IC-7800
    // -----------------------------------------------------------------------

    #[test]
    fn ic7800_basic_properties() {
        let model = ic_7800();
        assert_eq!(model.name, "IC-7800");
        assert_eq!(model.model_id, "0x6A");
        assert_eq!(model.default_civ_address, 0x6A);
        assert_eq!(model.default_baud_rate, 19_200);
    }

    #[test]
    fn ic7800_capabilities() {
        let model = ic_7800();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 200.0).abs() < f32::EPSILON);
    }

    // -----------------------------------------------------------------------
    // IC-7850
    // -----------------------------------------------------------------------

    #[test]
    fn ic7850_basic_properties() {
        let model = ic_7850();
        assert_eq!(model.name, "IC-7850");
        assert_eq!(model.model_id, "0x8E");
        assert_eq!(model.default_civ_address, 0x8E);
        assert_eq!(model.default_baud_rate, 115_200);
    }

    #[test]
    fn ic7850_capabilities() {
        let model = ic_7850();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_iq_output);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 200.0).abs() < f32::EPSILON);
    }

    // -----------------------------------------------------------------------
    // IC-7851
    // -----------------------------------------------------------------------

    #[test]
    fn ic7851_basic_properties() {
        let model = ic_7851();
        assert_eq!(model.name, "IC-7851");
        assert_eq!(model.model_id, "0x8E");
        assert_eq!(model.default_civ_address, 0x8E);
        assert_eq!(model.default_baud_rate, 115_200);
    }

    #[test]
    fn ic7851_capabilities() {
        let model = ic_7851();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_iq_output);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic7850_and_ic7851_share_civ_address() {
        let m7850 = ic_7850();
        let m7851 = ic_7851();
        assert_eq!(m7850.default_civ_address, m7851.default_civ_address);
        assert_eq!(m7850.model_id, m7851.model_id);
    }

    // -----------------------------------------------------------------------
    // IC-7300
    // -----------------------------------------------------------------------

    #[test]
    fn ic7300_basic_properties() {
        let model = ic_7300();
        assert_eq!(model.name, "IC-7300");
        assert_eq!(model.model_id, "0x94");
        assert_eq!(model.default_civ_address, 0x94);
        assert_eq!(model.default_baud_rate, 115_200);
    }

    #[test]
    fn ic7300_capabilities() {
        let model = ic_7300();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.has_iq_output);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic7300_hf_6m_only() {
        let model = ic_7300();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].contains(3_573_000)); // FT8 on 80m
        assert!(ranges[0].contains(50_100_000)); // 6m
        assert!(!ranges[0].contains(144_000_000)); // no 2m
    }

    // -----------------------------------------------------------------------
    // IC-7300MK2
    // -----------------------------------------------------------------------

    #[test]
    fn ic7300mk2_basic_properties() {
        let model = ic_7300mk2();
        assert_eq!(model.name, "IC-7300MK2");
        assert_eq!(model.model_id, "0xA6");
        assert_eq!(model.default_civ_address, 0xA6);
        assert_eq!(model.default_baud_rate, 115_200);
    }

    #[test]
    fn ic7300mk2_capabilities() {
        let model = ic_7300mk2();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -----------------------------------------------------------------------
    // IC-9700
    // -----------------------------------------------------------------------

    #[test]
    fn ic9700_basic_properties() {
        let model = ic_9700();
        assert_eq!(model.name, "IC-9700");
        assert_eq!(model.model_id, "0xA2");
        assert_eq!(model.default_civ_address, 0xA2);
        assert_eq!(model.default_baud_rate, 115_200);
    }

    #[test]
    fn ic9700_capabilities() {
        let model = ic_9700();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_iq_output);
        assert!(!caps.supported_modes.is_empty());
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic9700_frequency_coverage() {
        let model = ic_9700();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 3);

        // VHF
        assert!(ranges[0].contains(144_000_000));
        assert!(ranges[0].contains(146_000_000));
        // UHF
        assert!(ranges[1].contains(432_000_000));
        // 23cm
        assert!(ranges[2].contains(1_296_000_000));

        // No HF coverage
        let has_hf = ranges.iter().any(|r| r.contains(14_074_000));
        assert!(!has_hf);
    }

    // -----------------------------------------------------------------------
    // IC-705
    // -----------------------------------------------------------------------

    #[test]
    fn ic705_basic_properties() {
        let model = ic_705();
        assert_eq!(model.name, "IC-705");
        assert_eq!(model.model_id, "0xA4");
        assert_eq!(model.default_civ_address, 0xA4);
        assert_eq!(model.default_baud_rate, 115_200);
    }

    #[test]
    fn ic705_capabilities() {
        let model = ic_705();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.supported_modes.is_empty());
        assert!((caps.max_power_watts - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic705_frequency_coverage() {
        let model = ic_705();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 3);

        // HF + 6m
        assert!(ranges[0].contains(7_074_000)); // FT8 on 40m
        assert!(ranges[0].contains(50_100_000)); // 6m
        // VHF
        assert!(ranges[1].contains(145_000_000));
        // UHF
        assert!(ranges[2].contains(432_100_000));
    }

    // -----------------------------------------------------------------------
    // IC-7100
    // -----------------------------------------------------------------------

    #[test]
    fn ic7100_basic_properties() {
        let model = ic_7100();
        assert_eq!(model.name, "IC-7100");
        assert_eq!(model.model_id, "0x88");
        assert_eq!(model.default_civ_address, 0x88);
        assert_eq!(model.default_baud_rate, 19_200);
    }

    #[test]
    fn ic7100_capabilities() {
        let model = ic_7100();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic7100_frequency_coverage() {
        let model = ic_7100();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 3);

        // HF + 6m
        assert!(ranges[0].contains(14_250_000));
        assert!(ranges[0].contains(50_100_000));
        // VHF
        assert!(ranges[1].contains(144_300_000));
        // UHF
        assert!(ranges[2].contains(432_100_000));
    }

    // -----------------------------------------------------------------------
    // IC-9100
    // -----------------------------------------------------------------------

    #[test]
    fn ic9100_basic_properties() {
        let model = ic_9100();
        assert_eq!(model.name, "IC-9100");
        assert_eq!(model.model_id, "0x7C");
        assert_eq!(model.default_civ_address, 0x7C);
        assert_eq!(model.default_baud_rate, 19_200);
    }

    #[test]
    fn ic9100_capabilities() {
        let model = ic_9100();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic9100_frequency_coverage() {
        let model = ic_9100();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 3);

        // HF + 6m
        assert!(ranges[0].contains(7_000_000));
        assert!(ranges[0].contains(50_100_000));
        // VHF
        assert!(ranges[1].contains(144_000_000));
        // UHF
        assert!(ranges[2].contains(432_000_000));
    }

    // -----------------------------------------------------------------------
    // IC-7410
    // -----------------------------------------------------------------------

    #[test]
    fn ic7410_basic_properties() {
        let model = ic_7410();
        assert_eq!(model.name, "IC-7410");
        assert_eq!(model.model_id, "0x80");
        assert_eq!(model.default_civ_address, 0x80);
        assert_eq!(model.default_baud_rate, 19_200);
    }

    #[test]
    fn ic7410_capabilities() {
        let model = ic_7410();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic7410_hf_6m_only() {
        let model = ic_7410();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].contains(21_074_000)); // 15m
        assert!(!ranges[0].contains(144_000_000)); // no VHF
    }

    // -----------------------------------------------------------------------
    // IC-905
    // -----------------------------------------------------------------------

    #[test]
    fn ic905_basic_properties() {
        let model = ic_905();
        assert_eq!(model.name, "IC-905");
        assert_eq!(model.model_id, "0xAC");
        assert_eq!(model.default_civ_address, 0xAC);
        assert_eq!(model.default_baud_rate, 115_200);
    }

    #[test]
    fn ic905_capabilities() {
        let model = ic_905();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 1);
        assert!(!caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(!caps.supported_modes.is_empty());
        assert!(!caps.frequency_ranges.is_empty());
        assert!((caps.max_power_watts - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ic905_frequency_coverage() {
        let model = ic_905();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 5);

        // VHF
        assert!(ranges[0].contains(144_000_000));
        // UHF
        assert!(ranges[1].contains(432_000_000));
        // 23cm
        assert!(ranges[2].contains(1_296_000_000));
        // 13cm
        assert!(ranges[3].contains(2_400_000_000));
        // 6cm
        assert!(ranges[4].contains(5_760_000_000));

        // No HF
        let has_hf = ranges.iter().any(|r| r.contains(14_074_000));
        assert!(!has_hf);
    }

    // -----------------------------------------------------------------------
    // Cross-model tests
    // -----------------------------------------------------------------------

    #[test]
    fn all_models_have_unique_names() {
        let models = all_icom_models();
        let mut names: Vec<&str> = models.iter().map(|m| m.name).collect();
        let count_before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count_before, "duplicate model names found");
    }

    #[test]
    fn all_models_have_split() {
        // All Icom HF rigs support split operation.
        for model in all_icom_models() {
            assert!(
                model.capabilities.has_split,
                "{} should support split",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_modes() {
        for model in all_icom_models() {
            assert!(
                !model.capabilities.supported_modes.is_empty(),
                "{} should have at least one mode",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_frequency_ranges() {
        for model in all_icom_models() {
            assert!(
                !model.capabilities.frequency_ranges.is_empty(),
                "{} should have at least one frequency range",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_positive_power() {
        for model in all_icom_models() {
            assert!(
                model.capabilities.max_power_watts > 0.0,
                "{} should have positive max power",
                model.name
            );
        }
    }

    #[test]
    fn all_models_count() {
        let models = all_icom_models();
        assert_eq!(models.len(), 14, "expected 14 Icom models");
    }
}
