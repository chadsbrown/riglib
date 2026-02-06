//! Yaesu model definitions.
//!
//! Each supported Yaesu rig is described by a [`YaesuModel`] struct that
//! captures its default baud rate and capabilities. These are compile-time
//! constants used by the protocol engine to validate operations against the
//! rig's feature set.
//!
//! All modern Yaesu HF transceivers use the same text-based CAT protocol
//! with minor command variations between models. The baud rate is user-
//! configurable in the rig's menu, but the defaults listed here are the
//! factory settings for USB virtual COM port operation.

use riglib_core::{BandRange, ConnectionType, Manufacturer, Mode, RigCapabilities, RigDefinition};

/// Static model definition for a Yaesu transceiver.
///
/// Contains all the information needed to communicate with a specific
/// Yaesu model over CAT, including the default serial parameters and
/// a full capability description.
#[derive(Debug, Clone)]
pub struct YaesuModel {
    /// Human-readable model name (e.g. "FT-DX10").
    pub name: &'static str,
    /// Default serial baud rate for USB virtual COM port connections.
    pub default_baud_rate: u32,
    /// Full capability description for this model.
    pub capabilities: RigCapabilities,
    /// Whether the rig has true dual VFO (A/B) operation.
    ///
    /// All HF Yaesu transceivers have dual VFO, but some higher-end models
    /// (FT-DX101D/MP) have a true main+sub dual receiver architecture
    /// rather than simple VFO A/B switching.
    pub has_dual_vfo: bool,
}

impl From<&YaesuModel> for RigDefinition {
    fn from(model: &YaesuModel) -> Self {
        RigDefinition {
            manufacturer: Manufacturer::Yaesu,
            model_name: model.name,
            connection: ConnectionType::Serial,
            default_baud_rate: Some(model.default_baud_rate),
            capabilities: model.capabilities.clone(),
        }
    }
}

/// Standard set of supported modes for Yaesu HF transceivers.
///
/// All modern Yaesu HF rigs support this mode set. Individual models may
/// add C4FM or other modes, but those are not represented in the generic
/// [`Mode`] enum.
fn standard_hf_modes() -> Vec<Mode> {
    vec![
        Mode::LSB,
        Mode::USB,
        Mode::CW,
        Mode::CWR,
        Mode::AM,
        Mode::FM,
        Mode::RTTY,
        Mode::RTTYR,
        Mode::DataUSB,
        Mode::DataLSB,
        Mode::DataFM,
        Mode::DataAM,
    ]
}

/// Standard HF + 6m frequency range (1.8 MHz to 54 MHz).
///
/// Yaesu HF transceivers typically cover the amateur HF bands from 160m
/// through 6m as a single contiguous transmit range (with band-pass
/// filtering and PA matching per band).
fn hf_6m_range() -> Vec<BandRange> {
    vec![BandRange::new(1_800_000, 54_000_000)]
}

/// FT-DX10 model definition.
///
/// The FT-DX10 is a 100W HF + 6m SDR transceiver positioned as Yaesu's
/// mid-range contest radio. It features a direct-sampling SDR architecture,
/// a 3DSS (3-Dimensional Spectrum Stream) display, and excellent close-in
/// dynamic range.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Dual VFO (A/B)
/// - Direct-sampling SDR receiver
/// - Default CAT baud rate: 38400
pub fn ft_dx10() -> YaesuModel {
    YaesuModel {
        name: "FT-DX10",
        default_baud_rate: 38_400,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: standard_hf_modes(),
            frequency_ranges: hf_6m_range(),
            max_power_watts: 100.0,
        },
        has_dual_vfo: true,
    }
}

/// FT-891 model definition.
///
/// The FT-891 is a compact, rugged 100W HF + 6m transceiver designed for
/// mobile, portable, and field operations (popular for Field Day and POTA).
/// It uses a triple-conversion superheterodyne receiver with Yaesu's
/// narrowband SDR for the IF stage.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Dual VFO (A/B)
/// - Compact form factor (no built-in display waterfall)
/// - Default CAT baud rate: 38400
pub fn ft_891() -> YaesuModel {
    YaesuModel {
        name: "FT-891",
        default_baud_rate: 38_400,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: standard_hf_modes(),
            frequency_ranges: hf_6m_range(),
            max_power_watts: 100.0,
        },
        has_dual_vfo: true,
    }
}

/// FT-991A model definition.
///
/// The FT-991A is Yaesu's all-band, all-mode transceiver covering HF through
/// UHF. For riglib purposes we focus on HF + 6m CAT operation, but the rig
/// also covers 2m and 70cm.
///
/// Key specifications:
/// - 100W output on HF/6m, 50W on 2m, 20W on 70cm
/// - HF + 6m + 2m + 70cm coverage
/// - Dual VFO (A/B)
/// - Built-in automatic antenna tuner
/// - Default CAT baud rate: 38400
pub fn ft_991a() -> YaesuModel {
    YaesuModel {
        name: "FT-991A",
        default_baud_rate: 38_400,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: standard_hf_modes(),
            frequency_ranges: vec![
                // HF + 6m
                BandRange::new(1_800_000, 54_000_000),
                // 2m
                BandRange::new(144_000_000, 148_000_000),
                // 70cm
                BandRange::new(420_000_000, 450_000_000),
            ],
            max_power_watts: 100.0,
        },
        has_dual_vfo: true,
    }
}

/// FT-DX101D model definition.
///
/// The FT-DX101D is Yaesu's flagship 100W HF + 6m transceiver with a true
/// dual-receiver architecture (main + sub). The sub receiver has its own
/// dedicated RF front end, making it a strong contender for SO2R-in-a-box
/// contest operation.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - True dual receiver (main + sub) with independent RF front ends
/// - Direct-sampling SDR on main receiver
/// - Default CAT baud rate: 38400
pub fn ft_dx101d() -> YaesuModel {
    YaesuModel {
        name: "FT-DX101D",
        default_baud_rate: 38_400,
        capabilities: RigCapabilities {
            max_receivers: 2,
            has_sub_receiver: true,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: standard_hf_modes(),
            frequency_ranges: hf_6m_range(),
            max_power_watts: 100.0,
        },
        has_dual_vfo: true,
    }
}

/// FT-DX101MP model definition.
///
/// The FT-DX101MP is the 200W variant of the FT-DX101D. Identical in
/// features except for the higher-power final amplifier stage using
/// push-pull MOS FETs.
///
/// Key specifications:
/// - 200W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - True dual receiver (main + sub)
/// - Direct-sampling SDR on main receiver
/// - Default CAT baud rate: 38400
pub fn ft_dx101mp() -> YaesuModel {
    YaesuModel {
        name: "FT-DX101MP",
        default_baud_rate: 38_400,
        capabilities: RigCapabilities {
            max_receivers: 2,
            has_sub_receiver: true,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: standard_hf_modes(),
            frequency_ranges: hf_6m_range(),
            max_power_watts: 200.0,
        },
        has_dual_vfo: true,
    }
}

/// FT-710 model definition.
///
/// The FT-710 (also sold as FT-710 AESS) is Yaesu's entry-level SDR HF + 6m
/// transceiver. It features a direct-sampling architecture similar to the
/// FT-DX10 but at a lower price point with a simplified front panel.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Dual VFO (A/B)
/// - Direct-sampling SDR receiver
/// - Default CAT baud rate: 38400
pub fn ft_710() -> YaesuModel {
    YaesuModel {
        name: "FT-710",
        default_baud_rate: 38_400,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: standard_hf_modes(),
            frequency_ranges: hf_6m_range(),
            max_power_watts: 100.0,
        },
        has_dual_vfo: true,
    }
}

/// Returns a list of all supported Yaesu model definitions.
///
/// This is useful for building model selection UIs or iterating over
/// all known models for auto-detection.
pub fn all_yaesu_models() -> Vec<YaesuModel> {
    vec![
        ft_dx10(),
        ft_891(),
        ft_991a(),
        ft_dx101d(),
        ft_dx101mp(),
        ft_710(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_core::Mode;

    // ---------------------------------------------------------------
    // FT-DX10
    // ---------------------------------------------------------------

    #[test]
    fn ft_dx10_basic_properties() {
        let model = ft_dx10();
        assert_eq!(model.name, "FT-DX10");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(model.has_dual_vfo);
    }

    #[test]
    fn ft_dx10_single_receiver() {
        let model = ft_dx10();
        assert_eq!(model.capabilities.max_receivers, 1);
        assert!(!model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ft_dx10_has_split() {
        let model = ft_dx10();
        assert!(model.capabilities.has_split);
    }

    #[test]
    fn ft_dx10_max_power() {
        let model = ft_dx10();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ft_dx10_frequency_coverage() {
        let model = ft_dx10();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 1);

        let range = &ranges[0];
        assert!(range.contains(1_800_000)); // 160m
        assert!(range.contains(14_250_000)); // 20m
        assert!(range.contains(50_100_000)); // 6m
        assert!(range.contains(54_000_000)); // top of 6m
        assert!(!range.contains(1_799_999)); // below coverage
        assert!(!range.contains(144_000_000)); // 2m not supported
    }

    #[test]
    fn ft_dx10_supported_modes() {
        let model = ft_dx10();
        let modes = &model.capabilities.supported_modes;
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
        assert!(modes.contains(&Mode::DataFM));
        assert!(modes.contains(&Mode::DataAM));
    }

    // ---------------------------------------------------------------
    // FT-891
    // ---------------------------------------------------------------

    #[test]
    fn ft_891_basic_properties() {
        let model = ft_891();
        assert_eq!(model.name, "FT-891");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(model.has_dual_vfo);
    }

    #[test]
    fn ft_891_single_receiver() {
        let model = ft_891();
        assert_eq!(model.capabilities.max_receivers, 1);
        assert!(!model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ft_891_max_power() {
        let model = ft_891();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // ---------------------------------------------------------------
    // FT-991A
    // ---------------------------------------------------------------

    #[test]
    fn ft_991a_basic_properties() {
        let model = ft_991a();
        assert_eq!(model.name, "FT-991A");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(model.has_dual_vfo);
    }

    #[test]
    fn ft_991a_multiband_coverage() {
        let model = ft_991a();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 3);

        // HF + 6m
        assert!(ranges[0].contains(14_250_000));
        assert!(ranges[0].contains(50_100_000));

        // 2m
        assert!(ranges[1].contains(144_000_000));
        assert!(ranges[1].contains(146_520_000));

        // 70cm
        assert!(ranges[2].contains(432_100_000));
        assert!(ranges[2].contains(446_000_000));
    }

    #[test]
    fn ft_991a_single_receiver() {
        let model = ft_991a();
        assert_eq!(model.capabilities.max_receivers, 1);
        assert!(!model.capabilities.has_sub_receiver);
    }

    // ---------------------------------------------------------------
    // FT-DX101D
    // ---------------------------------------------------------------

    #[test]
    fn ft_dx101d_basic_properties() {
        let model = ft_dx101d();
        assert_eq!(model.name, "FT-DX101D");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(model.has_dual_vfo);
    }

    #[test]
    fn ft_dx101d_dual_receiver() {
        let model = ft_dx101d();
        assert_eq!(model.capabilities.max_receivers, 2);
        assert!(model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ft_dx101d_max_power() {
        let model = ft_dx101d();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // ---------------------------------------------------------------
    // FT-DX101MP
    // ---------------------------------------------------------------

    #[test]
    fn ft_dx101mp_basic_properties() {
        let model = ft_dx101mp();
        assert_eq!(model.name, "FT-DX101MP");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(model.has_dual_vfo);
    }

    #[test]
    fn ft_dx101mp_dual_receiver() {
        let model = ft_dx101mp();
        assert_eq!(model.capabilities.max_receivers, 2);
        assert!(model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ft_dx101mp_max_power() {
        let model = ft_dx101mp();
        assert!((model.capabilities.max_power_watts - 200.0).abs() < f32::EPSILON);
    }

    // ---------------------------------------------------------------
    // FT-710
    // ---------------------------------------------------------------

    #[test]
    fn ft_710_basic_properties() {
        let model = ft_710();
        assert_eq!(model.name, "FT-710");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(model.has_dual_vfo);
    }

    #[test]
    fn ft_710_single_receiver() {
        let model = ft_710();
        assert_eq!(model.capabilities.max_receivers, 1);
        assert!(!model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ft_710_max_power() {
        let model = ft_710();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ft_710_no_iq_output() {
        let model = ft_710();
        assert!(!model.capabilities.has_iq_output);
        assert!(!model.capabilities.has_audio_streaming);
    }

    // ---------------------------------------------------------------
    // Cross-model consistency
    // ---------------------------------------------------------------

    #[test]
    fn all_models_have_split() {
        let models = [
            ft_dx10(),
            ft_891(),
            ft_991a(),
            ft_dx101d(),
            ft_dx101mp(),
            ft_710(),
        ];
        for model in &models {
            assert!(
                model.capabilities.has_split,
                "{} should support split",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_dual_vfo() {
        let models = [
            ft_dx10(),
            ft_891(),
            ft_991a(),
            ft_dx101d(),
            ft_dx101mp(),
            ft_710(),
        ];
        for model in &models {
            assert!(model.has_dual_vfo, "{} should have dual VFO", model.name);
        }
    }

    #[test]
    fn all_models_support_core_modes() {
        let core_modes = [
            Mode::LSB,
            Mode::USB,
            Mode::CW,
            Mode::CWR,
            Mode::AM,
            Mode::FM,
        ];
        let models = [
            ft_dx10(),
            ft_891(),
            ft_991a(),
            ft_dx101d(),
            ft_dx101mp(),
            ft_710(),
        ];
        for model in &models {
            for mode in &core_modes {
                assert!(
                    model.capabilities.supported_modes.contains(mode),
                    "{} should support {mode}",
                    model.name
                );
            }
        }
    }

    #[test]
    fn all_models_cover_20m() {
        let models = [
            ft_dx10(),
            ft_891(),
            ft_991a(),
            ft_dx101d(),
            ft_dx101mp(),
            ft_710(),
        ];
        for model in &models {
            let covers_20m = model
                .capabilities
                .frequency_ranges
                .iter()
                .any(|r| r.contains(14_250_000));
            assert!(covers_20m, "{} should cover 20m", model.name);
        }
    }

    #[test]
    fn only_101_models_have_sub_receiver() {
        assert!(!ft_dx10().capabilities.has_sub_receiver);
        assert!(!ft_891().capabilities.has_sub_receiver);
        assert!(!ft_991a().capabilities.has_sub_receiver);
        assert!(ft_dx101d().capabilities.has_sub_receiver);
        assert!(ft_dx101mp().capabilities.has_sub_receiver);
        assert!(!ft_710().capabilities.has_sub_receiver);
    }
}
