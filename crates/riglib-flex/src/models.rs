//! FlexRadio model definitions.
//!
//! Each supported FlexRadio SDR is described by a [`FlexRadioModel`] struct
//! that captures its hardware capabilities (slice count, SCU count,
//! panadapter count) and a full [`RigCapabilities`] description. These are
//! used by the protocol engine to validate operations against the radio's
//! feature set (e.g. rejecting a third slice on a FLEX-6400).
//!
//! All FlexRadio models use the same SmartSDR TCP/IP API. The only
//! differences between models are hardware limits and whether an
//! integrated front panel (Maestro) is present. The M-suffix variants
//! (6400M, 6600M) are identical to their non-M counterparts from an API
//! perspective.
//!
//! Models are defined as factory functions (e.g. [`flex_6400()`]) that
//! return a fully populated [`FlexRadioModel`]. The following models are
//! supported:
//!
//! | Model       | model_id    | Slices | SCUs | Panadapters | Front Panel |
//! |-------------|-------------|--------|------|-------------|-------------|
//! | FLEX-6400   | `flex6400`  | 2      | 1    | 2           | No          |
//! | FLEX-6400M  | `flex6400m` | 2      | 1    | 2           | Yes         |
//! | FLEX-6600   | `flex6600`  | 4      | 2    | 4           | No          |
//! | FLEX-6600M  | `flex6600m` | 4      | 2    | 4           | Yes         |
//! | FLEX-6700   | `flex6700`  | 8      | 2    | 8           | No          |
//! | FLEX-8400   | `flex8400`  | 4      | 2    | 4           | No          |
//! | FLEX-8600   | `flex8600`  | 8      | 2    | 8           | No          |

use riglib_core::{BandRange, Mode, RigCapabilities};

/// Static model definition for a FlexRadio SDR transceiver.
///
/// Contains all the information needed to identify a FlexRadio model and
/// validate operations against its hardware limits. Each slice in a
/// FlexRadio is an independent receiver with its own frequency, mode,
/// and filter settings.
#[derive(Debug, Clone)]
pub struct FlexRadioModel {
    /// Human-readable model name (e.g. "FLEX-6600").
    pub name: &'static str,
    /// Machine-readable model identifier (e.g. "flex6600").
    ///
    /// This matches the identifier returned by the SmartSDR `info` command.
    pub model_id: &'static str,
    /// Maximum number of simultaneous slices (independent receivers).
    pub max_slices: u8,
    /// Maximum number of simultaneous panadapter displays.
    pub max_panadapters: u8,
    /// Number of Signal Capture Units (independent ADC/antenna inputs).
    ///
    /// All slices sharing an SCU use the same antenna and wideband
    /// digitizer. Slices on different SCUs can use different antennas.
    pub scu_count: u8,
    /// Whether this model has an integrated front panel (Maestro display).
    ///
    /// The M-suffix variants (FLEX-6400M, FLEX-6600M) include an
    /// integrated touchscreen. This has no effect on the SmartSDR API.
    pub has_front_panel: bool,
    /// Full capability description for this model.
    pub capabilities: RigCapabilities,
}

// ---------------------------------------------------------------------------
// Helper functions for building mode lists and frequency ranges
// ---------------------------------------------------------------------------

/// Standard mode set for all FlexRadio SDR transceivers.
///
/// All FlexRadio models support the same set of operating modes via
/// the SmartSDR API. FlexRadio-specific modes (SAM, NFM, FDV) are
/// mapped to the closest riglib [`Mode`] equivalents at the protocol
/// layer; this list represents the riglib-canonical modes.
fn flex_modes() -> Vec<Mode> {
    vec![
        Mode::USB,
        Mode::LSB,
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

/// HF + 6m frequency range (1.8 MHz to 54 MHz).
///
/// All current FlexRadio models cover this single contiguous range.
fn hf_6m_range() -> Vec<BandRange> {
    vec![BandRange::new(1_800_000, 54_000_000)]
}

/// Build a [`RigCapabilities`] for a FlexRadio model.
///
/// All FlexRadio SDRs share the same mode set, frequency coverage,
/// power output, and streaming capabilities. The only variable is
/// `max_receivers`, which equals the model's slice count.
fn flex_capabilities(max_slices: u8) -> RigCapabilities {
    RigCapabilities {
        max_receivers: max_slices,
        has_sub_receiver: max_slices > 1,
        has_split: true,
        has_audio_streaming: true,
        has_iq_output: true,
        supported_modes: flex_modes(),
        frequency_ranges: hf_6m_range(),
        max_power_watts: 100.0,
    }
}

// ---------------------------------------------------------------------------
// Model definitions
// ---------------------------------------------------------------------------

/// FLEX-6400 model definition.
///
/// The FLEX-6400 is FlexRadio's entry-level SDR transceiver introduced
/// in 2018. It features 2 slices, 1 SCU, and 2 panadapters. No
/// integrated front panel -- operated exclusively via SmartSDR client
/// software or a networked Maestro.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - 2 simultaneous slices (receivers)
/// - 1 SCU (single ADC path)
/// - 2 panadapters
/// - Network-only control (SmartSDR API)
pub fn flex_6400() -> FlexRadioModel {
    FlexRadioModel {
        name: "FLEX-6400",
        model_id: "flex6400",
        max_slices: 2,
        max_panadapters: 2,
        scu_count: 1,
        has_front_panel: false,
        capabilities: flex_capabilities(2),
    }
}

/// FLEX-6400M model definition.
///
/// The FLEX-6400M is the Maestro variant of the FLEX-6400, adding an
/// integrated front-panel touchscreen display. Identical to the
/// FLEX-6400 in all other respects (same slice count, SCU count, and
/// RF performance). From an API perspective, the two models are
/// interchangeable.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - 2 simultaneous slices (receivers)
/// - 1 SCU (single ADC path)
/// - 2 panadapters
/// - Integrated Maestro front panel
pub fn flex_6400m() -> FlexRadioModel {
    FlexRadioModel {
        name: "FLEX-6400M",
        model_id: "flex6400m",
        max_slices: 2,
        max_panadapters: 2,
        scu_count: 1,
        has_front_panel: true,
        capabilities: flex_capabilities(2),
    }
}

/// FLEX-6600 model definition.
///
/// The FLEX-6600 is FlexRadio's mid-range SDR transceiver introduced
/// in 2018. It doubles the capability of the 6400 series with 4 slices,
/// 2 SCUs, and 4 panadapters. The dual SCU architecture allows slices
/// on different SCUs to use independent antenna inputs.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - 4 simultaneous slices (receivers)
/// - 2 SCUs (dual ADC paths, independent antennas)
/// - 4 panadapters
/// - Network-only control (SmartSDR API)
pub fn flex_6600() -> FlexRadioModel {
    FlexRadioModel {
        name: "FLEX-6600",
        model_id: "flex6600",
        max_slices: 4,
        max_panadapters: 4,
        scu_count: 2,
        has_front_panel: false,
        capabilities: flex_capabilities(4),
    }
}

/// FLEX-6600M model definition.
///
/// The FLEX-6600M is the Maestro variant of the FLEX-6600, adding an
/// integrated front-panel touchscreen display. Identical to the
/// FLEX-6600 in all other respects.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - 4 simultaneous slices (receivers)
/// - 2 SCUs (dual ADC paths, independent antennas)
/// - 4 panadapters
/// - Integrated Maestro front panel
pub fn flex_6600m() -> FlexRadioModel {
    FlexRadioModel {
        name: "FLEX-6600M",
        model_id: "flex6600m",
        max_slices: 4,
        max_panadapters: 4,
        scu_count: 2,
        has_front_panel: true,
        capabilities: flex_capabilities(4),
    }
}

/// FLEX-6700 model definition.
///
/// The FLEX-6700 was FlexRadio's flagship SDR transceiver introduced
/// in 2013 and now discontinued. It features 8 slices, 2 SCUs, and 8
/// panadapters -- the highest slice count in the 6000 series. Popular
/// in multi-operator contest stations for its ability to run many
/// independent receivers simultaneously.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - 8 simultaneous slices (receivers)
/// - 2 SCUs (dual ADC paths, independent antennas)
/// - 8 panadapters
/// - Network-only control (SmartSDR API)
/// - Discontinued (replaced by FLEX-8600)
pub fn flex_6700() -> FlexRadioModel {
    FlexRadioModel {
        name: "FLEX-6700",
        model_id: "flex6700",
        max_slices: 8,
        max_panadapters: 8,
        scu_count: 2,
        has_front_panel: false,
        capabilities: flex_capabilities(8),
    }
}

/// FLEX-8400 model definition.
///
/// The FLEX-8400 is part of FlexRadio's next-generation 8000 series
/// introduced in 2024. It replaces the FLEX-6600 at the mid-range tier
/// with improved ADC performance and updated FPGA processing, while
/// maintaining the same slice/SCU/panadapter configuration.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - 4 simultaneous slices (receivers)
/// - 2 SCUs (dual ADC paths, independent antennas)
/// - 4 panadapters
/// - Network-only control (SmartSDR API)
pub fn flex_8400() -> FlexRadioModel {
    FlexRadioModel {
        name: "FLEX-8400",
        model_id: "flex8400",
        max_slices: 4,
        max_panadapters: 4,
        scu_count: 2,
        has_front_panel: false,
        capabilities: flex_capabilities(4),
    }
}

/// FLEX-8600 model definition.
///
/// The FLEX-8600 is the flagship of FlexRadio's 8000 series introduced
/// in 2024. It replaces the FLEX-6700 as the top-tier FlexRadio SDR
/// with 8 slices, 2 SCUs, and 8 panadapters. Designed for demanding
/// multi-operator contest stations and high-performance remote
/// operation.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - 8 simultaneous slices (receivers)
/// - 2 SCUs (dual ADC paths, independent antennas)
/// - 8 panadapters
/// - Network-only control (SmartSDR API)
pub fn flex_8600() -> FlexRadioModel {
    FlexRadioModel {
        name: "FLEX-8600",
        model_id: "flex8600",
        max_slices: 8,
        max_panadapters: 8,
        scu_count: 2,
        has_front_panel: false,
        capabilities: flex_capabilities(8),
    }
}

/// Returns a list of all supported FlexRadio model definitions.
///
/// This is useful for building model selection UIs or iterating over
/// all known models for auto-detection via SmartSDR discovery.
pub fn all_flex_models() -> Vec<FlexRadioModel> {
    vec![
        flex_6400(),
        flex_6400m(),
        flex_6600(),
        flex_6600m(),
        flex_6700(),
        flex_8400(),
        flex_8600(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_core::Mode;

    // -------------------------------------------------------------------
    // FLEX-6400
    // -------------------------------------------------------------------

    #[test]
    fn flex_6400_basic_properties() {
        let model = flex_6400();
        assert_eq!(model.name, "FLEX-6400");
        assert_eq!(model.model_id, "flex6400");
        assert_eq!(model.max_slices, 2);
        assert_eq!(model.max_panadapters, 2);
        assert_eq!(model.scu_count, 1);
        assert!(!model.has_front_panel);
    }

    #[test]
    fn flex_6400_capabilities() {
        let model = flex_6400();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_audio_streaming);
        assert!(caps.has_iq_output);
        assert_eq!(caps.supported_modes.len(), 12);
        assert_eq!(caps.frequency_ranges.len(), 1);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------
    // FLEX-6400M
    // -------------------------------------------------------------------

    #[test]
    fn flex_6400m_basic_properties() {
        let model = flex_6400m();
        assert_eq!(model.name, "FLEX-6400M");
        assert_eq!(model.model_id, "flex6400m");
        assert_eq!(model.max_slices, 2);
        assert_eq!(model.max_panadapters, 2);
        assert_eq!(model.scu_count, 1);
        assert!(model.has_front_panel);
    }

    #[test]
    fn flex_6400m_capabilities() {
        let model = flex_6400m();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_audio_streaming);
        assert!(caps.has_iq_output);
        assert_eq!(caps.supported_modes.len(), 12);
        assert_eq!(caps.frequency_ranges.len(), 1);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------
    // FLEX-6600
    // -------------------------------------------------------------------

    #[test]
    fn flex_6600_basic_properties() {
        let model = flex_6600();
        assert_eq!(model.name, "FLEX-6600");
        assert_eq!(model.model_id, "flex6600");
        assert_eq!(model.max_slices, 4);
        assert_eq!(model.max_panadapters, 4);
        assert_eq!(model.scu_count, 2);
        assert!(!model.has_front_panel);
    }

    #[test]
    fn flex_6600_capabilities() {
        let model = flex_6600();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 4);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_audio_streaming);
        assert!(caps.has_iq_output);
        assert_eq!(caps.supported_modes.len(), 12);
        assert_eq!(caps.frequency_ranges.len(), 1);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------
    // FLEX-6600M
    // -------------------------------------------------------------------

    #[test]
    fn flex_6600m_basic_properties() {
        let model = flex_6600m();
        assert_eq!(model.name, "FLEX-6600M");
        assert_eq!(model.model_id, "flex6600m");
        assert_eq!(model.max_slices, 4);
        assert_eq!(model.max_panadapters, 4);
        assert_eq!(model.scu_count, 2);
        assert!(model.has_front_panel);
    }

    #[test]
    fn flex_6600m_capabilities() {
        let model = flex_6600m();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 4);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_audio_streaming);
        assert!(caps.has_iq_output);
        assert_eq!(caps.supported_modes.len(), 12);
        assert_eq!(caps.frequency_ranges.len(), 1);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------
    // FLEX-6700
    // -------------------------------------------------------------------

    #[test]
    fn flex_6700_basic_properties() {
        let model = flex_6700();
        assert_eq!(model.name, "FLEX-6700");
        assert_eq!(model.model_id, "flex6700");
        assert_eq!(model.max_slices, 8);
        assert_eq!(model.max_panadapters, 8);
        assert_eq!(model.scu_count, 2);
        assert!(!model.has_front_panel);
    }

    #[test]
    fn flex_6700_capabilities() {
        let model = flex_6700();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 8);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_audio_streaming);
        assert!(caps.has_iq_output);
        assert_eq!(caps.supported_modes.len(), 12);
        assert_eq!(caps.frequency_ranges.len(), 1);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------
    // FLEX-8400
    // -------------------------------------------------------------------

    #[test]
    fn flex_8400_basic_properties() {
        let model = flex_8400();
        assert_eq!(model.name, "FLEX-8400");
        assert_eq!(model.model_id, "flex8400");
        assert_eq!(model.max_slices, 4);
        assert_eq!(model.max_panadapters, 4);
        assert_eq!(model.scu_count, 2);
        assert!(!model.has_front_panel);
    }

    #[test]
    fn flex_8400_capabilities() {
        let model = flex_8400();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 4);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_audio_streaming);
        assert!(caps.has_iq_output);
        assert_eq!(caps.supported_modes.len(), 12);
        assert_eq!(caps.frequency_ranges.len(), 1);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------
    // FLEX-8600
    // -------------------------------------------------------------------

    #[test]
    fn flex_8600_basic_properties() {
        let model = flex_8600();
        assert_eq!(model.name, "FLEX-8600");
        assert_eq!(model.model_id, "flex8600");
        assert_eq!(model.max_slices, 8);
        assert_eq!(model.max_panadapters, 8);
        assert_eq!(model.scu_count, 2);
        assert!(!model.has_front_panel);
    }

    #[test]
    fn flex_8600_capabilities() {
        let model = flex_8600();
        let caps = &model.capabilities;
        assert_eq!(caps.max_receivers, 8);
        assert!(caps.has_sub_receiver);
        assert!(caps.has_split);
        assert!(caps.has_audio_streaming);
        assert!(caps.has_iq_output);
        assert_eq!(caps.supported_modes.len(), 12);
        assert_eq!(caps.frequency_ranges.len(), 1);
        assert!((caps.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------
    // Cross-model tests
    // -------------------------------------------------------------------

    #[test]
    fn all_models_have_split() {
        for model in all_flex_models() {
            assert!(
                model.capabilities.has_split,
                "{} should support split",
                model.name
            );
        }
    }

    #[test]
    fn all_models_100w() {
        for model in all_flex_models() {
            assert!(
                (model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON,
                "{} should have 100W max power",
                model.name
            );
        }
    }

    #[test]
    fn all_models_cover_20m() {
        for model in all_flex_models() {
            let covers_20m = model
                .capabilities
                .frequency_ranges
                .iter()
                .any(|r| r.contains(14_250_000));
            assert!(covers_20m, "{} should cover 20m", model.name);
        }
    }

    #[test]
    fn all_models_have_12_modes() {
        for model in all_flex_models() {
            assert_eq!(
                model.capabilities.supported_modes.len(),
                12,
                "{} should have 12 modes",
                model.name
            );
        }
    }

    #[test]
    fn all_models_support_core_modes() {
        let core_modes = [
            Mode::USB,
            Mode::LSB,
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
        ];
        for model in all_flex_models() {
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
    fn all_models_have_iq_output() {
        for model in all_flex_models() {
            assert!(
                model.capabilities.has_iq_output,
                "{} should have IQ output (DAX IQ)",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_audio_streaming() {
        for model in all_flex_models() {
            assert!(
                model.capabilities.has_audio_streaming,
                "{} should have audio streaming (DAX audio)",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_unique_names() {
        let models = all_flex_models();
        let mut names: Vec<&str> = models.iter().map(|m| m.name).collect();
        let count_before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count_before, "duplicate model names found");
    }

    #[test]
    fn all_models_have_unique_model_ids() {
        let models = all_flex_models();
        let mut ids: Vec<&str> = models.iter().map(|m| m.model_id).collect();
        let count_before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count_before, "duplicate model_ids found");
    }

    #[test]
    fn all_flex_models_returns_7() {
        let models = all_flex_models();
        assert_eq!(models.len(), 7, "expected 7 FlexRadio models");
    }

    #[test]
    fn flex_6400_and_6400m_differ_only_in_front_panel() {
        let base = flex_6400();
        let maestro = flex_6400m();

        // Front panel differs
        assert!(!base.has_front_panel);
        assert!(maestro.has_front_panel);

        // Everything else is the same
        assert_eq!(base.max_slices, maestro.max_slices);
        assert_eq!(base.max_panadapters, maestro.max_panadapters);
        assert_eq!(base.scu_count, maestro.scu_count);
        assert_eq!(
            base.capabilities.max_receivers,
            maestro.capabilities.max_receivers
        );
        assert_eq!(
            base.capabilities.has_sub_receiver,
            maestro.capabilities.has_sub_receiver
        );
        assert_eq!(base.capabilities.has_split, maestro.capabilities.has_split);
        assert_eq!(
            base.capabilities.has_audio_streaming,
            maestro.capabilities.has_audio_streaming
        );
        assert_eq!(
            base.capabilities.has_iq_output,
            maestro.capabilities.has_iq_output
        );
        assert_eq!(
            base.capabilities.supported_modes,
            maestro.capabilities.supported_modes
        );
        assert_eq!(
            base.capabilities.frequency_ranges,
            maestro.capabilities.frequency_ranges
        );
        assert!(
            (base.capabilities.max_power_watts - maestro.capabilities.max_power_watts).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn flex_6600_and_6600m_differ_only_in_front_panel() {
        let base = flex_6600();
        let maestro = flex_6600m();

        // Front panel differs
        assert!(!base.has_front_panel);
        assert!(maestro.has_front_panel);

        // Everything else is the same
        assert_eq!(base.max_slices, maestro.max_slices);
        assert_eq!(base.max_panadapters, maestro.max_panadapters);
        assert_eq!(base.scu_count, maestro.scu_count);
        assert_eq!(
            base.capabilities.max_receivers,
            maestro.capabilities.max_receivers
        );
        assert_eq!(
            base.capabilities.has_sub_receiver,
            maestro.capabilities.has_sub_receiver
        );
        assert_eq!(base.capabilities.has_split, maestro.capabilities.has_split);
        assert_eq!(
            base.capabilities.has_audio_streaming,
            maestro.capabilities.has_audio_streaming
        );
        assert_eq!(
            base.capabilities.has_iq_output,
            maestro.capabilities.has_iq_output
        );
        assert_eq!(
            base.capabilities.supported_modes,
            maestro.capabilities.supported_modes
        );
        assert_eq!(
            base.capabilities.frequency_ranges,
            maestro.capabilities.frequency_ranges
        );
        assert!(
            (base.capabilities.max_power_watts - maestro.capabilities.max_power_watts).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn all_models_hf_6m_coverage() {
        for model in all_flex_models() {
            let ranges = &model.capabilities.frequency_ranges;
            assert_eq!(
                ranges.len(),
                1,
                "{} should have exactly 1 frequency range",
                model.name
            );

            let range = &ranges[0];
            // 160m band edge
            assert!(
                range.contains(1_800_000),
                "{} should cover 160m",
                model.name
            );
            // 20m contest frequency
            assert!(
                range.contains(14_250_000),
                "{} should cover 20m",
                model.name
            );
            // 6m
            assert!(range.contains(50_100_000), "{} should cover 6m", model.name);
            // Top of 6m
            assert!(
                range.contains(54_000_000),
                "{} should cover top of 6m",
                model.name
            );
            // Below coverage
            assert!(
                !range.contains(1_799_999),
                "{} should not cover below 160m",
                model.name
            );
            // 2m not supported
            assert!(
                !range.contains(144_000_000),
                "{} should not cover 2m",
                model.name
            );
        }
    }

    #[test]
    fn max_receivers_equals_max_slices() {
        for model in all_flex_models() {
            assert_eq!(
                model.capabilities.max_receivers, model.max_slices,
                "{}: max_receivers should equal max_slices",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_sub_receiver() {
        // All FlexRadio models have max_slices > 1, so all should have
        // has_sub_receiver = true.
        for model in all_flex_models() {
            assert!(
                model.capabilities.has_sub_receiver,
                "{} should have sub receiver (max_slices={})",
                model.name, model.max_slices
            );
        }
    }
}
