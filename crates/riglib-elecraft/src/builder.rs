//! ElecraftBuilder -- fluent builder for constructing [`ElecraftRig`] instances.
//!
//! Separates configuration from construction so that callers can set up
//! serial port parameters, retry policies, and timeout values before
//! establishing the transport connection.
//!
//! # Example
//!
//! ```no_run
//! use riglib_elecraft::builder::ElecraftBuilder;
//! use riglib_elecraft::models::k4;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! let rig = ElecraftBuilder::new(k4())
//!     .serial_port("/dev/ttyUSB0")
//!     .baud_rate(38_400)
//!     .command_timeout(Duration::from_millis(300))
//!     .build_with_transport(todo!())
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use riglib_core::error::{Error, Result};
use riglib_core::transport::Transport;
use riglib_core::types::{KeyLine, PttMethod, SetCommandMode};

use crate::commands;
use crate::models::ElecraftModel;
use crate::rig::ElecraftRig;

/// Fluent builder for [`ElecraftRig`].
///
/// All configuration has sensible defaults derived from the [`ElecraftModel`],
/// so the simplest usage is:
///
/// ```ignore
/// let rig = ElecraftBuilder::new(k4())
///     .serial_port("/dev/ttyUSB0")
///     .build()
///     .await?;
/// ```
pub struct ElecraftBuilder {
    model: ElecraftModel,
    serial_port: Option<String>,
    baud_rate: Option<u32>,
    auto_retry: bool,
    max_retries: u32,
    command_timeout: Duration,
    ptt_method: PttMethod,
    key_line: KeyLine,
    set_command_mode: SetCommandMode,
    /// Whether to enable AI (Auto Information / transceive) mode at connect.
    /// When `true`, the builder sends `AI2;` after construction so the rig
    /// pushes per-event broadcasts (FA, FB, MD, IF, etc.).
    ai: bool,
    /// USB audio device name for audio streaming (e.g. "USB Audio CODEC").
    #[cfg(feature = "audio")]
    audio_device_name: Option<String>,
}

impl ElecraftBuilder {
    /// Create a new builder for the given Elecraft model.
    pub fn new(model: ElecraftModel) -> Self {
        ElecraftBuilder {
            model,
            serial_port: None,
            baud_rate: None,
            auto_retry: true,
            max_retries: 3,
            command_timeout: Duration::from_millis(500),
            ptt_method: PttMethod::Cat,
            key_line: KeyLine::None,
            set_command_mode: SetCommandMode::default(),
            ai: false,
            #[cfg(feature = "audio")]
            audio_device_name: None,
        }
    }

    /// Set the serial port path (e.g. `/dev/ttyUSB0` or `COM3`).
    pub fn serial_port(mut self, port: &str) -> Self {
        self.serial_port = Some(port.to_string());
        self
    }

    /// Override the default baud rate for this model.
    pub fn baud_rate(mut self, baud: u32) -> Self {
        self.baud_rate = Some(baud);
        self
    }

    /// Enable or disable automatic retry on timeout.
    pub fn auto_retry(mut self, enabled: bool) -> Self {
        self.auto_retry = enabled;
        self
    }

    /// Set the maximum number of retry attempts (default: 3).
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Set the timeout for waiting for a response to a single CAT
    /// command (default: 500ms).
    pub fn command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Set the PTT method: CAT command (default), DTR line, or RTS line.
    pub fn ptt_method(mut self, method: PttMethod) -> Self {
        self.ptt_method = method;
        self
    }

    /// Set the CW key line: None (default), DTR, or RTS.
    pub fn key_line(mut self, line: KeyLine) -> Self {
        self.key_line = line;
        self
    }

    /// Set how SET commands are handled: [`Verify`](SetCommandMode::Verify)
    /// (default) issues a follow-up GET to confirm, [`NoVerify`](SetCommandMode::NoVerify)
    /// fires and forgets for maximum throughput.
    pub fn set_command_mode(mut self, mode: SetCommandMode) -> Self {
        self.set_command_mode = mode;
        self
    }

    /// Enable AI (Auto Information / transceive) mode. Default: `false`.
    ///
    /// When `true`, the builder sends `AI2;` to the radio at connect time.
    /// On Elecraft K3/K3S/K4/KX3/KX2, AI2 is the per-event broadcast mode:
    /// the rig sends an `FA`/`FB`/`MD`/etc. response whenever any
    /// front-panel control changes. The IO task parses these and emits
    /// [`RigEvent`](riglib_core::events::RigEvent)s on the channel returned
    /// by [`subscribe()`](riglib_core::rig::Rig::subscribe). When `false`
    /// (the default), unsolicited frames are silently drained.
    ///
    /// (AI1 mode, which only sends `IF` responses on freq/mode change, is
    /// not used by this builder — AI2 carries strictly more information.)
    ///
    /// On rig drop the IO task sends `AI0;` to leave the radio clean.
    ///
    /// When using AI mode, callers typically should **not** also poll
    /// [`get_frequency`](riglib_core::rig::Rig::get_frequency) and
    /// [`get_mode`](riglib_core::rig::Rig::get_mode) at high rates — the
    /// poll responses and the rig's broadcast frames can collide.
    pub fn ai(mut self, enabled: bool) -> Self {
        self.ai = enabled;
        self
    }

    /// Set the USB audio device name for audio streaming.
    ///
    /// The name should match a device reported by
    /// [`list_audio_devices()`](riglib_transport::audio::list_audio_devices),
    /// typically `"USB Audio CODEC"` for most ham transceivers.
    ///
    /// When set, the rig will support the [`AudioCapable`](riglib_core::AudioCapable)
    /// trait for RX and TX audio streaming over the rig's USB audio CODEC.
    #[cfg(feature = "audio")]
    pub fn audio_device(mut self, name: &str) -> Self {
        self.audio_device_name = Some(name.to_string());
        self
    }

    /// Build an [`ElecraftRig`] with a caller-provided transport.
    ///
    /// This is the primary entry point for testing (pass a
    /// `MockTransport` from `riglib-test-harness`) and for
    /// advanced use cases where the caller manages the transport
    /// lifecycle directly.
    ///
    /// When [`ai(true)`](Self::ai) was called, this also sends `AI2;` to
    /// the radio after the IO task is spawned, so transceive broadcasts
    /// begin flowing immediately.
    pub async fn build_with_transport(self, transport: Box<dyn Transport>) -> Result<ElecraftRig> {
        // Validate that ptt_method and key_line don't use the same serial line.
        if self.ptt_method == PttMethod::Dtr && self.key_line == KeyLine::Dtr {
            return Err(Error::InvalidParameter(
                "ptt_method and key_line cannot both use DTR".into(),
            ));
        }
        if self.ptt_method == PttMethod::Rts && self.key_line == KeyLine::Rts {
            return Err(Error::InvalidParameter(
                "ptt_method and key_line cannot both use RTS".into(),
            ));
        }

        let command_timeout = self.command_timeout;
        let ai = self.ai;
        let rig = ElecraftRig::new(
            transport,
            self.model,
            self.auto_retry,
            self.max_retries,
            self.command_timeout,
            self.ptt_method,
            self.key_line,
            self.set_command_mode,
            ai,
            #[cfg(feature = "audio")]
            self.audio_device_name,
        );

        if ai {
            rig.io
                .set_command(commands::cmd_set_ai(true), command_timeout)
                .await?;
        }

        Ok(rig)
    }

    /// Build an [`ElecraftRig`] using a serial transport.
    ///
    /// Requires that [`serial_port()`](Self::serial_port) has been called.
    /// The baud rate defaults to the model's default if not overridden.
    pub async fn build(self) -> Result<ElecraftRig> {
        let port = self
            .serial_port
            .as_ref()
            .ok_or_else(|| Error::InvalidParameter("serial_port is required for build()".into()))?;
        let baud = self.baud_rate.unwrap_or(self.model.default_baud_rate);

        let transport = riglib_transport::SerialTransport::open(port, baud).await?;
        self.build_with_transport(Box::new(transport)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::k4;
    use riglib_core::Rig;
    use riglib_test_harness::MockTransport;

    #[tokio::test]
    async fn builder_defaults() {
        let mock = MockTransport::new();
        let rig = ElecraftBuilder::new(k4())
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().manufacturer, riglib_core::Manufacturer::Elecraft);
        assert_eq!(rig.info().model_name, "K4");
    }

    #[tokio::test]
    async fn builder_custom_settings() {
        let mock = MockTransport::new();
        let rig = ElecraftBuilder::new(k4())
            .serial_port("/dev/ttyUSB0")
            .baud_rate(9600)
            .auto_retry(false)
            .max_retries(5)
            .command_timeout(Duration::from_millis(200))
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().model_name, "K4");
    }

    #[tokio::test]
    async fn builder_serial_port_required_for_build() {
        let result = ElecraftBuilder::new(k4()).build().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn builder_fluent_chain() {
        let mock = MockTransport::new();
        let rig = ElecraftBuilder::new(k4())
            .serial_port("/dev/ttyUSB0")
            .baud_rate(38_400)
            .auto_retry(true)
            .max_retries(2)
            .command_timeout(Duration::from_millis(300))
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().model_name, "K4");
    }

    #[tokio::test]
    async fn builder_with_k3() {
        use crate::models::k3;

        let mock = MockTransport::new();
        let rig = ElecraftBuilder::new(k3())
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().model_name, "K3");
        assert_eq!(rig.capabilities().max_receivers, 2);
        assert!(rig.capabilities().has_sub_receiver);
    }

    #[tokio::test]
    async fn builder_with_kx3() {
        use crate::models::kx3;

        let mock = MockTransport::new();
        let rig = ElecraftBuilder::new(kx3())
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().model_name, "KX3");
        assert_eq!(rig.capabilities().max_receivers, 1);
        assert!(!rig.capabilities().has_sub_receiver);
    }

    #[tokio::test]
    async fn builder_with_kx2() {
        use crate::models::kx2;

        let mock = MockTransport::new();
        let rig = ElecraftBuilder::new(kx2())
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().model_name, "KX2");
        assert_eq!(rig.capabilities().max_receivers, 1);
        assert!(!rig.capabilities().has_sub_receiver);
        assert!((rig.capabilities().max_power_watts - 10.0).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn builder_with_k3s() {
        use crate::models::k3s;

        let mock = MockTransport::new();
        let rig = ElecraftBuilder::new(k3s())
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().model_name, "K3S");
        assert_eq!(rig.capabilities().max_receivers, 2);
        assert!(rig.capabilities().has_sub_receiver);
        assert!((rig.capabilities().max_power_watts - 100.0).abs() < f32::EPSILON);
    }

    // ------------------------------------------------------------------
    // AI / transceive
    // ------------------------------------------------------------------

    /// `.ai(true)` sends `AI2;` at connect (per-event broadcast mode).
    /// If anything else were sent, the `MockTransport` would refuse.
    #[tokio::test]
    async fn ai_true_sends_ai2_at_connect() {
        let mut mock = MockTransport::new();
        mock.expect(b"AI2;", b"");

        let _rig = ElecraftBuilder::new(k4())
            .ai(true)
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();
    }

    /// Default builder (no `.ai()`) sends nothing at connect.
    #[tokio::test]
    async fn ai_default_sends_nothing_at_connect() {
        let mock = MockTransport::new();
        let _rig = ElecraftBuilder::new(k4())
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();
    }

    /// With `.ai(true)`, an interleaved `FB...;` in a `FA;` response is
    /// emitted as an event for VFO B; the trailing `FA` returns cleanly.
    #[tokio::test]
    async fn ai_true_emits_event_and_doesnt_poison_query() {
        use riglib_core::events::RigEvent;
        use riglib_core::types::ReceiverId;

        let mut mock = MockTransport::new();
        mock.expect(b"AI2;", b"");
        // Elecraft uses Kenwood-compatible 11-digit frequencies.
        mock.expect(b"FA;", b"FB00014300000;FA00014250000;");

        let rig = ElecraftBuilder::new(k4())
            .ai(true)
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        let mut events = rig.subscribe().unwrap();
        let freq = rig.get_frequency(ReceiverId::VFO_A).await.unwrap();
        assert_eq!(freq, 14_250_000);

        let event = events.try_recv().expect("expected an FB event");
        match event {
            RigEvent::FrequencyChanged { receiver, freq_hz } => {
                assert_eq!(receiver, ReceiverId::VFO_B);
                assert_eq!(freq_hz, 14_300_000);
            }
            other => panic!("expected FrequencyChanged for VFO_B, got {other:?}"),
        }
    }

    /// `enable_transceive` and `disable_transceive` must not send any
    /// bytes; with no expectations queued, any send would error.
    #[tokio::test]
    async fn enable_disable_transceive_are_noops() {
        let mock = MockTransport::new();
        let rig = ElecraftBuilder::new(k4())
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        rig.enable_transceive().await.unwrap();
        rig.disable_transceive().await.unwrap();
    }
}
