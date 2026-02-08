//! YaesuBuilder -- fluent builder for constructing [`YaesuRig`] instances.
//!
//! Separates configuration from construction so that callers can set up
//! serial port parameters, retry policies, and timeout values before
//! establishing the transport connection.
//!
//! # Example
//!
//! ```no_run
//! use riglib_yaesu::builder::YaesuBuilder;
//! use riglib_yaesu::models::ft_dx10;
//! use std::time::Duration;
//!
//! # fn example() {
//! let rig = YaesuBuilder::new(ft_dx10())
//!     .serial_port("/dev/ttyUSB0")
//!     .baud_rate(38_400)
//!     .command_timeout(Duration::from_millis(300))
//!     .build_with_transport(todo!());
//! # }
//! ```

use std::time::Duration;

use riglib_core::error::{Error, Result};
use riglib_core::transport::Transport;
use riglib_core::types::{KeyLine, PttMethod, SetCommandMode};

use crate::models::YaesuModel;
use crate::rig::YaesuRig;

/// Fluent builder for [`YaesuRig`].
///
/// All configuration has sensible defaults derived from the [`YaesuModel`],
/// so the simplest usage is:
///
/// ```ignore
/// let rig = YaesuBuilder::new(ft_dx10())
///     .serial_port("/dev/ttyUSB0")
///     .build()
///     .await?;
/// ```
pub struct YaesuBuilder {
    model: YaesuModel,
    serial_port: Option<String>,
    baud_rate: Option<u32>,
    auto_retry: bool,
    max_retries: u32,
    command_timeout: Duration,
    ptt_method: PttMethod,
    key_line: KeyLine,
    set_command_mode: SetCommandMode,
    /// USB audio device name for audio streaming (e.g. "USB Audio CODEC").
    #[cfg(feature = "audio")]
    audio_device_name: Option<String>,
}

impl YaesuBuilder {
    /// Create a new builder for the given Yaesu model.
    ///
    /// Defaults are derived from the model definition:
    /// - baud_rate: from model (38400 for all current Yaesu models)
    /// - auto_retry: true
    /// - max_retries: 3
    /// - command_timeout: 500ms
    pub fn new(model: YaesuModel) -> Self {
        YaesuBuilder {
            model,
            serial_port: None,
            baud_rate: None,
            auto_retry: true,
            max_retries: 3,
            command_timeout: Duration::from_millis(500),
            ptt_method: PttMethod::Cat,
            key_line: KeyLine::None,
            set_command_mode: SetCommandMode::default(),
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
    pub fn baud_rate(mut self, rate: u32) -> Self {
        self.baud_rate = Some(rate);
        self
    }

    /// Enable or disable automatic retry on timeout.
    pub fn auto_retry(mut self, enable: bool) -> Self {
        self.auto_retry = enable;
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

    /// Build a [`YaesuRig`] using a serial transport.
    ///
    /// Requires that [`serial_port()`](Self::serial_port) has been called.
    /// The baud rate defaults to the model's default if not overridden.
    pub async fn build(self) -> Result<YaesuRig> {
        let port = self
            .serial_port
            .as_ref()
            .ok_or_else(|| Error::InvalidParameter("serial_port is required for build()".into()))?;
        let baud = self.baud_rate.unwrap_or(self.model.default_baud_rate);

        let transport = riglib_transport::SerialTransport::open(port, baud).await?;
        self.build_with_transport(Box::new(transport))
    }

    /// Build a [`YaesuRig`] with a caller-provided transport.
    ///
    /// This is the primary entry point for testing (pass a
    /// `MockTransport` from `riglib-test-harness`) and for
    /// advanced use cases where the caller manages the transport
    /// lifecycle directly.
    pub fn build_with_transport(self, transport: Box<dyn Transport>) -> Result<YaesuRig> {
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

        Ok(YaesuRig::new(
            transport,
            self.model,
            self.auto_retry,
            self.max_retries,
            self.command_timeout,
            self.ptt_method,
            self.key_line,
            self.set_command_mode,
            #[cfg(feature = "audio")]
            self.audio_device_name,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ft_dx10, ft_dx101d};
    use riglib_core::Rig;
    use riglib_test_harness::MockTransport;

    #[tokio::test]
    async fn test_builder_defaults() {
        let mock = MockTransport::new();
        let rig = YaesuBuilder::new(ft_dx10())
            .build_with_transport(Box::new(mock))
            .unwrap();

        assert_eq!(rig.info().manufacturer, riglib_core::Manufacturer::Yaesu);
        assert_eq!(rig.info().model_name, "FT-DX10");
    }

    #[tokio::test]
    async fn test_builder_custom_settings() {
        let mock = MockTransport::new();
        let rig = YaesuBuilder::new(ft_dx101d())
            .serial_port("/dev/ttyUSB0")
            .baud_rate(9_600)
            .auto_retry(false)
            .max_retries(5)
            .command_timeout(Duration::from_millis(200))
            .build_with_transport(Box::new(mock))
            .unwrap();

        assert_eq!(rig.info().model_name, "FT-DX101D");
        let caps = rig.capabilities();
        assert_eq!(caps.max_receivers, 2);
        assert!(caps.has_sub_receiver);
    }

    #[tokio::test]
    async fn test_builder_requires_serial_port() {
        let result = YaesuBuilder::new(ft_dx10()).build().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_builder_fluent_chain() {
        let mock = MockTransport::new();
        let rig = YaesuBuilder::new(ft_dx10())
            .serial_port("/dev/ttyUSB0")
            .baud_rate(38_400)
            .auto_retry(true)
            .max_retries(5)
            .command_timeout(Duration::from_millis(300))
            .build_with_transport(Box::new(mock))
            .unwrap();

        assert_eq!(rig.info().model_name, "FT-DX10");
    }
}
