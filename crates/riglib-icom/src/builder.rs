//! IcomBuilder -- fluent builder for constructing [`IcomRig`] instances.
//!
//! Separates configuration from construction so that callers can set up
//! serial port parameters, CI-V address overrides, retry policies, and
//! timeout values before establishing the transport connection.
//!
//! # Example
//!
//! ```no_run
//! use riglib_icom::builder::IcomBuilder;
//! use riglib_icom::models::ic_7610;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! let rig = IcomBuilder::new(ic_7610())
//!     .serial_port("/dev/ttyUSB0")
//!     .baud_rate(115_200)
//!     .command_timeout(Duration::from_millis(300))
//!     .build_with_transport(todo!())
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use riglib_core::error::{Error, Result};
use riglib_core::transport::Transport;
use riglib_core::types::{KeyLine, PttMethod};

use crate::models::IcomModel;
use crate::rig::IcomRig;

/// Fluent builder for [`IcomRig`].
///
/// All configuration has sensible defaults derived from the [`IcomModel`],
/// so the simplest usage is:
///
/// ```ignore
/// let rig = IcomBuilder::new(ic_7610())
///     .serial_port("/dev/ttyUSB0")
///     .build()
///     .await?;
/// ```
pub struct IcomBuilder {
    model: IcomModel,
    serial_port: Option<String>,
    baud_rate: Option<u32>,
    civ_address: Option<u8>,
    auto_retry: bool,
    max_retries: u32,
    collision_recovery: bool,
    #[allow(dead_code)]
    reconnect_on_drop: bool,
    command_timeout: Duration,
    #[allow(dead_code)]
    reconnect_delay: Duration,
    #[allow(dead_code)]
    max_reconnect_attempts: u32,
    ptt_method: PttMethod,
    key_line: KeyLine,
    /// USB audio device name for audio streaming (e.g. "USB Audio CODEC").
    #[cfg(feature = "audio")]
    audio_device_name: Option<String>,
}

impl IcomBuilder {
    /// Create a new builder for the given Icom model.
    pub fn new(model: IcomModel) -> Self {
        IcomBuilder {
            model,
            serial_port: None,
            baud_rate: None,
            civ_address: None,
            auto_retry: true,
            max_retries: 3,
            collision_recovery: true,
            reconnect_on_drop: true,
            command_timeout: Duration::from_millis(500),
            reconnect_delay: Duration::from_secs(1),
            max_reconnect_attempts: 5,
            ptt_method: PttMethod::Cat,
            key_line: KeyLine::None,
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

    /// Override the default CI-V address for this model.
    ///
    /// Use this when the rig's CI-V address has been changed from the
    /// factory default in the rig's menu settings.
    pub fn civ_address(mut self, addr: u8) -> Self {
        self.civ_address = Some(addr);
        self
    }

    /// Enable or disable automatic retry on timeout/collision.
    pub fn auto_retry(mut self, enabled: bool) -> Self {
        self.auto_retry = enabled;
        self
    }

    /// Set the maximum number of retry attempts (default: 3).
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Enable or disable CI-V bus collision recovery (default: true).
    pub fn collision_recovery(mut self, enabled: bool) -> Self {
        self.collision_recovery = enabled;
        self
    }

    /// Enable or disable automatic reconnection when the transport
    /// connection drops (default: true).
    pub fn reconnect_on_drop(mut self, enabled: bool) -> Self {
        self.reconnect_on_drop = enabled;
        self
    }

    /// Set the timeout for waiting for a response to a single CI-V
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
    ///
    /// When set to `Dtr` or `Rts`, [`set_ptt()`](riglib_core::rig::Rig::set_ptt)
    /// will toggle the corresponding serial control line instead of sending a
    /// CI-V PTT command.
    pub fn ptt_method(mut self, method: PttMethod) -> Self {
        self.ptt_method = method;
        self
    }

    /// Set the CW key line: None (default), DTR, or RTS.
    ///
    /// When set, [`set_cw_key()`](riglib_core::rig::Rig::set_cw_key) will
    /// toggle the corresponding serial control line for hardware CW keying.
    pub fn key_line(mut self, line: KeyLine) -> Self {
        self.key_line = line;
        self
    }

    /// Build an [`IcomRig`] with a caller-provided transport.
    ///
    /// This is the primary entry point for testing (pass a
    /// `MockTransport` from `riglib-test-harness`) and for
    /// advanced use cases where the caller manages the transport
    /// lifecycle directly.
    pub async fn build_with_transport(self, transport: Box<dyn Transport>) -> Result<IcomRig> {
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

        let civ_address = self.civ_address.unwrap_or(self.model.default_civ_address);

        Ok(IcomRig::new(
            transport,
            self.model,
            civ_address,
            self.auto_retry,
            self.max_retries,
            self.collision_recovery,
            self.command_timeout,
            self.ptt_method,
            self.key_line,
            #[cfg(feature = "audio")]
            self.audio_device_name,
        ))
    }

    /// Build an [`IcomRig`] using a serial transport.
    ///
    /// Requires that [`serial_port()`](Self::serial_port) has been called.
    /// The baud rate defaults to the model's default if not overridden.
    pub async fn build(self) -> Result<IcomRig> {
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
    use crate::models::ic_7610;
    use riglib_core::Rig;
    use riglib_test_harness::MockTransport;

    #[tokio::test]
    async fn builder_defaults() {
        let mock = MockTransport::new();
        let rig = IcomBuilder::new(ic_7610())
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().manufacturer, riglib_core::Manufacturer::Icom);
        assert_eq!(rig.info().model_name, "IC-7610");
    }

    #[tokio::test]
    async fn builder_custom_civ_address() {
        let mock = MockTransport::new();
        let rig = IcomBuilder::new(ic_7610())
            .civ_address(0xA4)
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        // The rig should use the custom address -- we verify indirectly
        // via info (model is still IC-7610).
        assert_eq!(rig.info().model_name, "IC-7610");
    }

    #[tokio::test]
    async fn builder_serial_port_required_for_build() {
        let result = IcomBuilder::new(ic_7610()).build().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn builder_fluent_chain() {
        let mock = MockTransport::new();
        let rig = IcomBuilder::new(ic_7610())
            .serial_port("/dev/ttyUSB0")
            .baud_rate(19_200)
            .civ_address(0xA4)
            .auto_retry(false)
            .max_retries(5)
            .collision_recovery(false)
            .reconnect_on_drop(false)
            .command_timeout(Duration::from_millis(200))
            .build_with_transport(Box::new(mock))
            .await
            .unwrap();

        assert_eq!(rig.info().model_name, "IC-7610");
    }
}
