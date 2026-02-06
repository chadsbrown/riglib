//! Kenwood model definitions.
//!
//! Each supported Kenwood rig is described by a [`KenwoodModel`] struct that
//! captures its default baud rate, capabilities, and whether the rig has
//! dedicated data mode commands (e.g. the `DA` command on the TS-890S/TS-990S).
//!
//! All modern Kenwood HF transceivers use the same text-based CAT protocol
//! with minor command variations between models. The baud rate is user-
//! configurable in the rig's menu, but the defaults listed here are the
//! factory settings for USB virtual COM port operation.
//!
//! Models are defined as factory functions (e.g. [`ts_590s()`]) that
//! return a fully populated [`KenwoodModel`]. The following models are
//! supported:
//!
//! | Model      | Baud    | Power | Coverage   | Dual RX |
//! |------------|---------|-------|------------|---------|
//! | TS-590S    | 115200  | 100W  | HF + 6m    | No      |
//! | TS-590SG   | 115200  | 100W  | HF + 6m    | No      |
//! | TS-990S    | 115200  | 200W  | HF + 6m    | Yes     |
//! | TS-890S    | 115200  | 100W  | HF + 6m    | Yes     |

use riglib_core::{BandRange, ConnectionType, Manufacturer, Mode, RigCapabilities, RigDefinition};

/// Static model definition for a Kenwood transceiver.
///
/// Contains all the information needed to communicate with a specific
/// Kenwood model over CAT, including the default serial parameters, a
/// full capability description, and whether the rig supports dedicated
/// data mode commands.
#[derive(Debug, Clone)]
pub struct KenwoodModel {
    /// Human-readable model name (e.g. "TS-890S").
    pub name: &'static str,
    /// Machine-readable model identifier.
    ///
    /// For Kenwood rigs this is the model designation used in firmware
    /// and documentation.
    pub model_id: &'static str,
    /// Default serial baud rate for USB virtual COM port connections.
    pub default_baud_rate: u32,
    /// Full capability description for this model.
    pub capabilities: RigCapabilities,
    /// Whether the rig has dedicated data mode commands.
    ///
    /// The TS-890S and TS-990S have a `DA` command for switching between
    /// voice and data sub-modes (e.g. USB vs DATA-USB). Older models
    /// like the TS-590S/SG do not have this distinction in the protocol.
    pub has_data_modes: bool,
}

impl From<&KenwoodModel> for RigDefinition {
    fn from(model: &KenwoodModel) -> Self {
        RigDefinition {
            manufacturer: Manufacturer::Kenwood,
            model_name: model.name,
            connection: ConnectionType::Serial,
            default_baud_rate: Some(model.default_baud_rate),
            capabilities: model.capabilities.clone(),
        }
    }
}

/// Standard set of supported modes for Kenwood HF transceivers.
///
/// All modern Kenwood HF rigs support this mode set. Data modes are
/// included in the generic set for compatibility, even though the
/// `MD` command maps them to the base mode -- the data sub-mode
/// distinction may be handled separately via the `DA` command on
/// models that support it.
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
/// Kenwood HF transceivers typically cover the amateur HF bands from 160m
/// through 6m as a single contiguous transmit range.
fn hf_6m_range() -> Vec<BandRange> {
    vec![BandRange::new(1_800_000, 54_000_000)]
}

/// TS-590S model definition.
///
/// The TS-590S is Kenwood's mid-range HF+6m transceiver introduced in 2010.
/// It features excellent receiver performance with a dual-conversion superhet
/// architecture, 100W output, and a built-in automatic antenna tuner. It was
/// widely praised for its strong close-in dynamic range.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Single receiver
/// - No dedicated data mode commands in CAT protocol
/// - Default CAT baud rate: 115200
pub fn ts_590s() -> KenwoodModel {
    KenwoodModel {
        name: "TS-590S",
        model_id: "TS-590S",
        default_baud_rate: 115_200,
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
        has_data_modes: false,
    }
}

/// TS-590SG model definition.
///
/// The TS-590SG is the 2014 refresh of the TS-590S with improved receiver
/// performance and firmware updates. The CAT protocol is identical to the
/// TS-590S. The "SG" designation stands for "Second Generation".
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Single receiver
/// - No dedicated data mode commands in CAT protocol
/// - Default CAT baud rate: 115200
pub fn ts_590sg() -> KenwoodModel {
    KenwoodModel {
        name: "TS-590SG",
        model_id: "TS-590SG",
        default_baud_rate: 115_200,
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
        has_data_modes: false,
    }
}

/// TS-990S model definition.
///
/// The TS-990S is Kenwood's flagship HF+6m transceiver, introduced in 2013.
/// It features a true dual-receiver architecture with independent main and
/// sub receivers, 200W output, and a large TFT display with dual band scope.
/// It is the top choice for serious Kenwood-platform contesters doing SO2R.
///
/// Key specifications:
/// - 200W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - True dual receiver (main + sub) with independent RF front ends
/// - Dedicated data mode commands (`DA`) in CAT protocol
/// - Default CAT baud rate: 115200
pub fn ts_990s() -> KenwoodModel {
    KenwoodModel {
        name: "TS-990S",
        model_id: "TS-990S",
        default_baud_rate: 115_200,
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
        has_data_modes: true,
    }
}

/// TS-890S model definition.
///
/// The TS-890S is Kenwood's high-end HF+6m transceiver, introduced in 2018.
/// It features a dual-receiver architecture (main + sub), 100W output, and
/// a direct-sampling SDR receiver for the IF stage. It competes directly with
/// the Yaesu FT-DX101D and Icom IC-7610 in the SO2R contest station segment.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Dual receiver (main + sub)
/// - Dedicated data mode commands (`DA`) in CAT protocol
/// - Default CAT baud rate: 115200
pub fn ts_890s() -> KenwoodModel {
    KenwoodModel {
        name: "TS-890S",
        model_id: "TS-890S",
        default_baud_rate: 115_200,
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
        has_data_modes: true,
    }
}

/// Returns a list of all supported Kenwood model definitions.
///
/// This is useful for building model selection UIs or iterating over
/// all known models for auto-detection.
pub fn all_kenwood_models() -> Vec<KenwoodModel> {
    vec![ts_590s(), ts_590sg(), ts_990s(), ts_890s()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_core::Mode;

    // ---------------------------------------------------------------
    // TS-590S
    // ---------------------------------------------------------------

    #[test]
    fn ts590s_basic_properties() {
        let model = ts_590s();
        assert_eq!(model.name, "TS-590S");
        assert_eq!(model.model_id, "TS-590S");
        assert_eq!(model.default_baud_rate, 115_200);
        assert!(!model.has_data_modes);
    }

    #[test]
    fn ts590s_single_receiver() {
        let model = ts_590s();
        assert_eq!(model.capabilities.max_receivers, 1);
        assert!(!model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ts590s_has_split() {
        let model = ts_590s();
        assert!(model.capabilities.has_split);
    }

    #[test]
    fn ts590s_max_power() {
        let model = ts_590s();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ts590s_frequency_coverage() {
        let model = ts_590s();
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
    fn ts590s_supported_modes() {
        let model = ts_590s();
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
    // TS-590SG
    // ---------------------------------------------------------------

    #[test]
    fn ts590sg_basic_properties() {
        let model = ts_590sg();
        assert_eq!(model.name, "TS-590SG");
        assert_eq!(model.model_id, "TS-590SG");
        assert_eq!(model.default_baud_rate, 115_200);
        assert!(!model.has_data_modes);
    }

    #[test]
    fn ts590sg_single_receiver() {
        let model = ts_590sg();
        assert_eq!(model.capabilities.max_receivers, 1);
        assert!(!model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ts590sg_max_power() {
        let model = ts_590sg();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // ---------------------------------------------------------------
    // TS-990S
    // ---------------------------------------------------------------

    #[test]
    fn ts990s_basic_properties() {
        let model = ts_990s();
        assert_eq!(model.name, "TS-990S");
        assert_eq!(model.model_id, "TS-990S");
        assert_eq!(model.default_baud_rate, 115_200);
        assert!(model.has_data_modes);
    }

    #[test]
    fn ts990s_dual_receiver() {
        let model = ts_990s();
        assert_eq!(model.capabilities.max_receivers, 2);
        assert!(model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ts990s_has_split() {
        let model = ts_990s();
        assert!(model.capabilities.has_split);
    }

    #[test]
    fn ts990s_max_power() {
        let model = ts_990s();
        assert!((model.capabilities.max_power_watts - 200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ts990s_frequency_coverage() {
        let model = ts_990s();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].contains(14_074_000)); // FT8 on 20m
        assert!(ranges[0].contains(50_313_000)); // 6m CW
        assert!(!ranges[0].contains(144_000_000)); // no 2m
    }

    // ---------------------------------------------------------------
    // TS-890S
    // ---------------------------------------------------------------

    #[test]
    fn ts890s_basic_properties() {
        let model = ts_890s();
        assert_eq!(model.name, "TS-890S");
        assert_eq!(model.model_id, "TS-890S");
        assert_eq!(model.default_baud_rate, 115_200);
        assert!(model.has_data_modes);
    }

    #[test]
    fn ts890s_dual_receiver() {
        let model = ts_890s();
        assert_eq!(model.capabilities.max_receivers, 2);
        assert!(model.capabilities.has_sub_receiver);
    }

    #[test]
    fn ts890s_has_split() {
        let model = ts_890s();
        assert!(model.capabilities.has_split);
    }

    #[test]
    fn ts890s_max_power() {
        let model = ts_890s();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ts890s_frequency_coverage() {
        let model = ts_890s();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].contains(7_074_000)); // FT8 on 40m
        assert!(ranges[0].contains(50_100_000)); // 6m
        assert!(!ranges[0].contains(144_000_000)); // no 2m
    }

    // ---------------------------------------------------------------
    // Cross-model tests
    // ---------------------------------------------------------------

    #[test]
    fn all_models_have_unique_names() {
        let models = all_kenwood_models();
        let mut names: Vec<&str> = models.iter().map(|m| m.name).collect();
        let count_before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count_before, "duplicate model names found");
    }

    #[test]
    fn all_models_have_split() {
        for model in all_kenwood_models() {
            assert!(
                model.capabilities.has_split,
                "{} should support split",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_modes() {
        for model in all_kenwood_models() {
            assert!(
                !model.capabilities.supported_modes.is_empty(),
                "{} should have at least one mode",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_frequency_ranges() {
        for model in all_kenwood_models() {
            assert!(
                !model.capabilities.frequency_ranges.is_empty(),
                "{} should have at least one frequency range",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_positive_power() {
        for model in all_kenwood_models() {
            assert!(
                model.capabilities.max_power_watts > 0.0,
                "{} should have positive max power",
                model.name
            );
        }
    }

    #[test]
    fn all_models_use_115200_baud() {
        for model in all_kenwood_models() {
            assert_eq!(
                model.default_baud_rate, 115_200,
                "{} should default to 115200 baud",
                model.name
            );
        }
    }

    #[test]
    fn all_models_count() {
        let models = all_kenwood_models();
        assert_eq!(models.len(), 4, "expected 4 Kenwood models");
    }

    #[test]
    fn all_models_cover_20m() {
        for model in all_kenwood_models() {
            let covers_20m = model
                .capabilities
                .frequency_ranges
                .iter()
                .any(|r| r.contains(14_250_000));
            assert!(covers_20m, "{} should cover 20m", model.name);
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
        for model in all_kenwood_models() {
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
    fn only_890_990_have_data_modes() {
        assert!(!ts_590s().has_data_modes);
        assert!(!ts_590sg().has_data_modes);
        assert!(ts_990s().has_data_modes);
        assert!(ts_890s().has_data_modes);
    }

    #[test]
    fn only_890_990_have_sub_receiver() {
        assert!(!ts_590s().capabilities.has_sub_receiver);
        assert!(!ts_590sg().capabilities.has_sub_receiver);
        assert!(ts_990s().capabilities.has_sub_receiver);
        assert!(ts_890s().capabilities.has_sub_receiver);
    }
}
