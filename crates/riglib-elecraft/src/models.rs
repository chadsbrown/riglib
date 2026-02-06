//! Elecraft model definitions.
//!
//! Each supported Elecraft rig is described by an [`ElecraftModel`] struct that
//! captures its default baud rate, capabilities, and whether the rig uses
//! the K4 extended command set (vs the K3 command set).
//!
//! All Elecraft HF transceivers use the same extended Kenwood text-based CAT
//! protocol with minor command variations between K3-family and K4-family.
//! The baud rate is user-configurable in the rig's menu, but the defaults
//! listed here are the factory settings for USB virtual COM port operation.
//!
//! Models are defined as factory functions (e.g. [`k3()`]) that return a
//! fully populated [`ElecraftModel`]. The following models are supported:
//!
//! | Model  | Baud  | Power | Coverage | Dual RX    | K4 cmd set |
//! |--------|-------|-------|----------|------------|------------|
//! | K3     | 38400 | 100W  | HF + 6m  | Yes (KRX3) | No         |
//! | K3S    | 38400 | 100W  | HF + 6m  | Yes (KRX3) | No         |
//! | KX3    | 38400 | 15W   | HF + 6m  | No         | No         |
//! | KX2    | 38400 | 10W   | HF + 6m  | No         | No         |
//! | K4     | 38400 | 100W  | HF + 6m  | Yes (built)| Yes        |

use riglib_core::{BandRange, Mode, RigCapabilities};

/// Static model definition for an Elecraft transceiver.
///
/// Contains all the information needed to communicate with a specific
/// Elecraft model over CAT, including the default serial parameters, a
/// full capability description, and whether the rig uses the K4 extended
/// command set.
#[derive(Debug, Clone)]
pub struct ElecraftModel {
    /// Human-readable model name (e.g. "K3", "K4").
    pub name: &'static str,
    /// Machine-readable model identifier.
    ///
    /// For Elecraft rigs this is the model designation used in the
    /// identification command response (`K3` or `K4`).
    pub model_id: &'static str,
    /// Default serial baud rate for USB virtual COM port connections.
    pub default_baud_rate: u32,
    /// Full capability description for this model.
    pub capabilities: RigCapabilities,
    /// Whether the rig uses the K4 extended command set.
    ///
    /// The K4 uses `BW` for bandwidth control and has additional commands
    /// not available on K3-family rigs. K3/K3S/KX3/KX2 use the `FW` command
    /// for filter bandwidth.
    pub is_k4: bool,
    /// Whether the rig has a sub receiver.
    ///
    /// The K3/K3S can have a sub receiver with the KRX3 option installed.
    /// The K4 has a built-in sub receiver. KX3/KX2 are single-receiver
    /// portables.
    pub has_sub_receiver: bool,
}

/// Standard set of supported modes for Elecraft HF transceivers.
///
/// All Elecraft HF rigs support this mode set. Elecraft uses mode 6 (DATA)
/// for digital modes (DataUSB) and mode 9 (DATA-R) for reverse digital
/// modes (DataLSB), providing proper round-trip mapping for contest
/// logging software.
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
/// Elecraft HF transceivers typically cover the amateur HF bands from 160m
/// through 6m as a single contiguous transmit range.
fn hf_6m_range() -> Vec<BandRange> {
    vec![BandRange::new(1_800_000, 54_000_000)]
}

/// K3 model definition.
///
/// The Elecraft K3 is a 100W HF+6m transceiver first introduced in 2008.
/// It is widely regarded as one of the finest contesting radios ever made,
/// with exceptional close-in dynamic range, a fully synthesized VFO, and
/// a modular architecture supporting the KRX3 sub-receiver option.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Optional sub receiver (KRX3) -- assumed installed for contest ops
/// - Uses K3 command set (FW for bandwidth)
/// - Default CAT baud rate: 38400
pub fn k3() -> ElecraftModel {
    ElecraftModel {
        name: "K3",
        model_id: "K3",
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
        is_k4: false,
        has_sub_receiver: true,
    }
}

/// K3S model definition.
///
/// The Elecraft K3S is the 2015 revision of the K3 with improved synthesizer,
/// tighter filters, and enhanced firmware. The CAT protocol is identical to
/// the K3. Like the K3, it supports the KRX3 sub-receiver option.
///
/// Key specifications:
/// - 100W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Optional sub receiver (KRX3) -- assumed installed for contest ops
/// - Uses K3 command set (FW for bandwidth)
/// - Default CAT baud rate: 38400
pub fn k3s() -> ElecraftModel {
    ElecraftModel {
        name: "K3S",
        model_id: "K3",
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
        is_k4: false,
        has_sub_receiver: true,
    }
}

/// KX3 model definition.
///
/// The Elecraft KX3 is a compact, portable 15W HF+6m transceiver introduced
/// in 2012. Despite its small size, it offers excellent receiver performance
/// and is popular for SOTA, POTA, and portable contesting. Power can be
/// increased to 100W with the optional KXPA100 amplifier, but the base
/// radio is rated at 15W.
///
/// Key specifications:
/// - 15W output power (base; 100W with KXPA100)
/// - HF + 6m coverage (1.8-54 MHz)
/// - Single receiver (no sub-receiver option)
/// - Uses K3 command set (FW for bandwidth)
/// - Default CAT baud rate: 38400
pub fn kx3() -> ElecraftModel {
    ElecraftModel {
        name: "KX3",
        model_id: "K3",
        default_baud_rate: 38_400,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: standard_hf_modes(),
            frequency_ranges: hf_6m_range(),
            max_power_watts: 15.0,
        },
        is_k4: false,
        has_sub_receiver: false,
    }
}

/// KX2 model definition.
///
/// The Elecraft KX2 is an ultra-compact portable 10W HF+6m transceiver
/// introduced in 2016. It shares the KX3's receiver architecture in a
/// smaller form factor, optimized for backpacking and field operations.
///
/// Key specifications:
/// - 10W output power
/// - HF + 6m coverage (1.8-54 MHz)
/// - Single receiver (no sub-receiver option)
/// - Uses K3 command set (FW for bandwidth)
/// - Default CAT baud rate: 38400
pub fn kx2() -> ElecraftModel {
    ElecraftModel {
        name: "KX2",
        model_id: "K3",
        default_baud_rate: 38_400,
        capabilities: RigCapabilities {
            max_receivers: 1,
            has_sub_receiver: false,
            has_split: true,
            has_audio_streaming: false,
            has_iq_output: false,
            supported_modes: standard_hf_modes(),
            frequency_ranges: hf_6m_range(),
            max_power_watts: 10.0,
        },
        is_k4: false,
        has_sub_receiver: false,
    }
}

/// K4 model definition.
///
/// The Elecraft K4 is Elecraft's flagship direct-sampling SDR transceiver,
/// introduced in 2021. It features a built-in dual receiver, 100W output
/// (K4 standard and K4D) or 400W (K4HD), and an extended command set
/// that is a superset of the K3 protocol. The K4 can also connect via
/// Ethernet/TCP for remote operation.
///
/// Key specifications:
/// - 100W output power (K4/K4D; 400W for K4HD, but we use 100W as base)
/// - HF + 6m coverage (1.8-54 MHz)
/// - Built-in dual receiver (no external option needed)
/// - Uses K4 extended command set (BW for bandwidth)
/// - Default CAT baud rate: 38400
pub fn k4() -> ElecraftModel {
    ElecraftModel {
        name: "K4",
        model_id: "K4",
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
        is_k4: true,
        has_sub_receiver: true,
    }
}

/// Returns a list of all supported Elecraft model definitions.
///
/// This is useful for building model selection UIs or iterating over
/// all known models for auto-detection.
pub fn all_elecraft_models() -> Vec<ElecraftModel> {
    vec![k3(), k3s(), kx3(), kx2(), k4()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use riglib_core::Mode;

    // ---------------------------------------------------------------
    // K3
    // ---------------------------------------------------------------

    #[test]
    fn k3_basic_properties() {
        let model = k3();
        assert_eq!(model.name, "K3");
        assert_eq!(model.model_id, "K3");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(!model.is_k4);
        assert!(model.has_sub_receiver);
    }

    #[test]
    fn k3_dual_receiver() {
        let model = k3();
        assert_eq!(model.capabilities.max_receivers, 2);
        assert!(model.capabilities.has_sub_receiver);
    }

    #[test]
    fn k3_has_split() {
        let model = k3();
        assert!(model.capabilities.has_split);
    }

    #[test]
    fn k3_max_power() {
        let model = k3();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn k3_frequency_coverage() {
        let model = k3();
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
    fn k3_supported_modes() {
        let model = k3();
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
    // K3S
    // ---------------------------------------------------------------

    #[test]
    fn k3s_basic_properties() {
        let model = k3s();
        assert_eq!(model.name, "K3S");
        assert_eq!(model.model_id, "K3");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(!model.is_k4);
        assert!(model.has_sub_receiver);
    }

    #[test]
    fn k3s_dual_receiver() {
        let model = k3s();
        assert_eq!(model.capabilities.max_receivers, 2);
        assert!(model.capabilities.has_sub_receiver);
    }

    #[test]
    fn k3s_max_power() {
        let model = k3s();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // ---------------------------------------------------------------
    // KX3
    // ---------------------------------------------------------------

    #[test]
    fn kx3_basic_properties() {
        let model = kx3();
        assert_eq!(model.name, "KX3");
        assert_eq!(model.model_id, "K3");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(!model.is_k4);
        assert!(!model.has_sub_receiver);
    }

    #[test]
    fn kx3_single_receiver() {
        let model = kx3();
        assert_eq!(model.capabilities.max_receivers, 1);
        assert!(!model.capabilities.has_sub_receiver);
    }

    #[test]
    fn kx3_max_power() {
        let model = kx3();
        assert!((model.capabilities.max_power_watts - 15.0).abs() < f32::EPSILON);
    }

    // ---------------------------------------------------------------
    // KX2
    // ---------------------------------------------------------------

    #[test]
    fn kx2_basic_properties() {
        let model = kx2();
        assert_eq!(model.name, "KX2");
        assert_eq!(model.model_id, "K3");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(!model.is_k4);
        assert!(!model.has_sub_receiver);
    }

    #[test]
    fn kx2_single_receiver() {
        let model = kx2();
        assert_eq!(model.capabilities.max_receivers, 1);
        assert!(!model.capabilities.has_sub_receiver);
    }

    #[test]
    fn kx2_max_power() {
        let model = kx2();
        assert!((model.capabilities.max_power_watts - 10.0).abs() < f32::EPSILON);
    }

    // ---------------------------------------------------------------
    // K4
    // ---------------------------------------------------------------

    #[test]
    fn k4_basic_properties() {
        let model = k4();
        assert_eq!(model.name, "K4");
        assert_eq!(model.model_id, "K4");
        assert_eq!(model.default_baud_rate, 38_400);
        assert!(model.is_k4);
        assert!(model.has_sub_receiver);
    }

    #[test]
    fn k4_dual_receiver() {
        let model = k4();
        assert_eq!(model.capabilities.max_receivers, 2);
        assert!(model.capabilities.has_sub_receiver);
    }

    #[test]
    fn k4_has_split() {
        let model = k4();
        assert!(model.capabilities.has_split);
    }

    #[test]
    fn k4_max_power() {
        let model = k4();
        assert!((model.capabilities.max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn k4_frequency_coverage() {
        let model = k4();
        let ranges = &model.capabilities.frequency_ranges;
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].contains(14_074_000)); // FT8 on 20m
        assert!(ranges[0].contains(50_313_000)); // 6m CW
        assert!(!ranges[0].contains(144_000_000)); // no 2m
    }

    // ---------------------------------------------------------------
    // Cross-model tests
    // ---------------------------------------------------------------

    #[test]
    fn all_models_have_unique_names() {
        let models = all_elecraft_models();
        let mut names: Vec<&str> = models.iter().map(|m| m.name).collect();
        let count_before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count_before, "duplicate model names found");
    }

    #[test]
    fn all_models_have_split() {
        for model in all_elecraft_models() {
            assert!(
                model.capabilities.has_split,
                "{} should support split",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_modes() {
        for model in all_elecraft_models() {
            assert!(
                !model.capabilities.supported_modes.is_empty(),
                "{} should have at least one mode",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_frequency_ranges() {
        for model in all_elecraft_models() {
            assert!(
                !model.capabilities.frequency_ranges.is_empty(),
                "{} should have at least one frequency range",
                model.name
            );
        }
    }

    #[test]
    fn all_models_have_positive_power() {
        for model in all_elecraft_models() {
            assert!(
                model.capabilities.max_power_watts > 0.0,
                "{} should have positive max power",
                model.name
            );
        }
    }

    #[test]
    fn all_models_use_38400_baud() {
        for model in all_elecraft_models() {
            assert_eq!(
                model.default_baud_rate, 38_400,
                "{} should default to 38400 baud",
                model.name
            );
        }
    }

    #[test]
    fn all_models_count() {
        let models = all_elecraft_models();
        assert_eq!(models.len(), 5, "expected 5 Elecraft models");
    }

    #[test]
    fn all_models_cover_20m() {
        for model in all_elecraft_models() {
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
        for model in all_elecraft_models() {
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
    fn only_k4_is_k4() {
        assert!(!k3().is_k4);
        assert!(!k3s().is_k4);
        assert!(!kx3().is_k4);
        assert!(!kx2().is_k4);
        assert!(k4().is_k4);
    }

    #[test]
    fn k3_k3s_k4_have_sub_receiver() {
        assert!(k3().has_sub_receiver);
        assert!(k3s().has_sub_receiver);
        assert!(!kx3().has_sub_receiver);
        assert!(!kx2().has_sub_receiver);
        assert!(k4().has_sub_receiver);
    }

    #[test]
    fn k3_family_uses_k3_model_id() {
        assert_eq!(k3().model_id, "K3");
        assert_eq!(k3s().model_id, "K3");
        assert_eq!(kx3().model_id, "K3");
        assert_eq!(kx2().model_id, "K3");
    }

    #[test]
    fn k4_uses_k4_model_id() {
        assert_eq!(k4().model_id, "K4");
    }
}
