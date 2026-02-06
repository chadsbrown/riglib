//! FlexRadioBuilder -- fluent builder for constructing [`FlexRadio`] instances.
//!
//! Separates configuration from construction so that callers can set up
//! network parameters, model selection, and behavior flags before
//! establishing the TCP connection to the radio.
//!
//! # Example
//!
//! ```no_run
//! use riglib_flex::builder::FlexRadioBuilder;
//! use riglib_flex::models::flex_6600;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! let rig = FlexRadioBuilder::new()
//!     .model(flex_6600())
//!     .host("192.168.1.100")
//!     .build()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use riglib_core::error::{Error, Result};

use crate::client::{ClientOptions, SmartSdrClient};
use crate::discovery::DiscoveredRadio;
use crate::models::{self, FlexRadioModel};
use crate::rig::FlexRadio;

/// Default SmartSDR TCP command port.
const DEFAULT_TCP_PORT: u16 = 4992;

/// Default SmartSDR VITA-49 UDP port.
const DEFAULT_UDP_PORT: u16 = 4991;

/// Default client program name.
const DEFAULT_CLIENT_NAME: &str = "riglib";

/// Default command timeout.
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_millis(1000);

/// Fluent builder for [`FlexRadio`].
///
/// All configuration has sensible defaults, so the simplest usage is:
///
/// ```ignore
/// let rig = FlexRadioBuilder::new()
///     .host("192.168.1.100")
///     .build()
///     .await?;
/// ```
pub struct FlexRadioBuilder {
    model: Option<FlexRadioModel>,
    host: Option<String>,
    tcp_port: u16,
    udp_port: u16,
    client_name: String,
    auto_create_slices: bool,
    command_timeout: Duration,
}

impl FlexRadioBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        FlexRadioBuilder {
            model: None,
            host: None,
            tcp_port: DEFAULT_TCP_PORT,
            udp_port: DEFAULT_UDP_PORT,
            client_name: DEFAULT_CLIENT_NAME.to_string(),
            auto_create_slices: true,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    /// Set the FlexRadio model.
    ///
    /// If not specified, defaults to FLEX-6600 (a reasonable middle-ground
    /// model for capability reporting).
    pub fn model(mut self, model: FlexRadioModel) -> Self {
        self.model = Some(model);
        self
    }

    /// Set the radio's IP address or hostname.
    pub fn host(mut self, host: &str) -> Self {
        self.host = Some(host.to_string());
        self
    }

    /// Set the SmartSDR TCP command port (default: 4992).
    pub fn tcp_port(mut self, port: u16) -> Self {
        self.tcp_port = port;
        self
    }

    /// Set the VITA-49 UDP port (default: 4991).
    pub fn udp_port(mut self, port: u16) -> Self {
        self.udp_port = port;
        self
    }

    /// Set the client program name sent during registration (default: "riglib").
    pub fn client_name(mut self, name: &str) -> Self {
        self.client_name = name.to_string();
        self
    }

    /// Enable or disable automatic slice creation (default: true).
    ///
    /// When enabled, methods that reference a non-existent slice will
    /// auto-create it with sensible defaults (14.250 MHz USB) before
    /// proceeding with the operation.
    pub fn auto_create_slices(mut self, enable: bool) -> Self {
        self.auto_create_slices = enable;
        self
    }

    /// Set the command response timeout (default: 1000ms).
    pub fn command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Configure the builder from a discovered radio.
    ///
    /// Sets the host and model automatically based on the discovery data.
    /// The model is matched by the `model` field in the discovery packet;
    /// if no match is found, the FLEX-6600 is used as a default.
    pub fn radio(mut self, radio: &DiscoveredRadio) -> Self {
        self.host = Some(radio.ip.to_string());
        self.tcp_port = radio.port;

        // Try to match the model name from discovery to a known model.
        let all_models = models::all_flex_models();
        let matched = all_models.into_iter().find(|m| m.name == radio.model);
        if let Some(m) = matched {
            self.model = Some(m);
        }

        self
    }

    /// Connect to the FlexRadio and build the [`FlexRadio`] instance.
    ///
    /// Requires that [`host()`](Self::host) has been called (either
    /// directly or via [`radio()`](Self::radio)).
    pub async fn build(self) -> Result<FlexRadio> {
        let host = self.host.as_ref().ok_or_else(|| {
            Error::InvalidParameter(
                "host is required: call .host() or .radio() before .build()".into(),
            )
        })?;

        let model = self.model.unwrap_or_else(models::flex_6600);

        let options = ClientOptions {
            client_name: self.client_name.clone(),
            command_timeout: self.command_timeout,
            auto_subscribe: true,
        };

        let client = SmartSdrClient::connect_with_options(host, self.tcp_port, options).await?;

        Ok(FlexRadio::new(client, model, self.auto_create_slices))
    }

    /// Build a [`FlexRadio`] with an already-connected client.
    ///
    /// This is the primary entry point for testing: pass a client that
    /// was connected to a mock SmartSDR server.
    pub fn build_with_client(self, client: SmartSdrClient) -> FlexRadio {
        let model = self.model.unwrap_or_else(models::flex_6600);
        FlexRadio::new(client, model, self.auto_create_slices)
    }
}

impl Default for FlexRadioBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ClientOptions;
    use crate::models::{flex_6400, flex_6600, flex_8600};
    use riglib_core::rig::Rig;
    use riglib_core::types::Manufacturer;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    /// Helper: create a mock SmartSDR server on a random port.
    async fn mock_smartsdr_server() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        (listener, addr)
    }

    /// Helper: accept a connection and send the standard handshake.
    async fn accept_and_handshake(listener: &TcpListener) -> tokio::net::TcpStream {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"V1.4.0.0\n").await.unwrap();
        stream.write_all(b"H12345678\n").await.unwrap();
        stream.flush().await.unwrap();
        stream
    }

    // -----------------------------------------------------------------------
    // Builder Defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_builder_defaults() {
        let builder = FlexRadioBuilder::new();
        assert_eq!(builder.tcp_port, 4992);
        assert_eq!(builder.udp_port, 4991);
        assert_eq!(builder.client_name, "riglib");
        assert!(builder.auto_create_slices);
        assert_eq!(builder.command_timeout, Duration::from_millis(1000));
        assert!(builder.host.is_none());
        assert!(builder.model.is_none());
    }

    // -----------------------------------------------------------------------
    // Builder Custom Settings
    // -----------------------------------------------------------------------

    #[test]
    fn test_builder_custom_settings() {
        let builder = FlexRadioBuilder::new()
            .model(flex_8600())
            .host("10.0.0.42")
            .tcp_port(5000)
            .udp_port(5001)
            .client_name("contest-logger")
            .auto_create_slices(false)
            .command_timeout(Duration::from_millis(2000));

        assert_eq!(builder.tcp_port, 5000);
        assert_eq!(builder.udp_port, 5001);
        assert_eq!(builder.client_name, "contest-logger");
        assert!(!builder.auto_create_slices);
        assert_eq!(builder.command_timeout, Duration::from_millis(2000));
        assert_eq!(builder.host.as_deref(), Some("10.0.0.42"));
        assert_eq!(builder.model.as_ref().unwrap().name, "FLEX-8600");
    }

    // -----------------------------------------------------------------------
    // Builder Requires Host
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_builder_requires_host() {
        let result = FlexRadioBuilder::new().build().await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        match err {
            Error::InvalidParameter(msg) => {
                assert!(msg.contains("host"), "expected 'host' in error: {msg}");
            }
            other => panic!("expected InvalidParameter, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Build with Client
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_build_with_client() {
        let (listener, addr) = mock_smartsdr_server().await;

        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let options = ClientOptions {
            auto_subscribe: false,
            command_timeout: Duration::from_millis(500),
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        let rig = FlexRadioBuilder::new()
            .model(flex_6400())
            .auto_create_slices(false)
            .build_with_client(client);

        let info = rig.info();
        assert_eq!(info.manufacturer, Manufacturer::FlexRadio);
        assert_eq!(info.model_name, "FLEX-6400");
        assert_eq!(info.model_id, "flex6400");

        let caps = rig.capabilities();
        assert_eq!(caps.max_receivers, 2);

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Build via build() (full TCP connection)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_build_connects() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0].to_string();
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let rig = FlexRadioBuilder::new()
            .model(flex_6600())
            .host(&host)
            .tcp_port(port)
            .build()
            .await
            .unwrap();

        assert_eq!(rig.info().model_name, "FLEX-6600");
        assert!(rig.is_connected());

        rig.disconnect().await.unwrap();
        server.abort();
    }

    // -----------------------------------------------------------------------
    // Builder radio() method
    // -----------------------------------------------------------------------

    #[test]
    fn test_builder_radio_method() {
        use std::net::IpAddr;
        let radio = DiscoveredRadio {
            model: "FLEX-6600".to_string(),
            serial: "12345".to_string(),
            nickname: "MyStation".to_string(),
            ip: "192.168.1.100".parse::<IpAddr>().unwrap(),
            port: 4992,
            firmware_version: "3.5.1.0".to_string(),
        };

        let builder = FlexRadioBuilder::new().radio(&radio);
        assert_eq!(builder.host.as_deref(), Some("192.168.1.100"));
        assert_eq!(builder.tcp_port, 4992);
        assert_eq!(builder.model.as_ref().unwrap().name, "FLEX-6600");
    }

    #[test]
    fn test_builder_radio_unknown_model() {
        use std::net::IpAddr;
        let radio = DiscoveredRadio {
            model: "FLEX-9999".to_string(), // unknown model
            serial: "99999".to_string(),
            nickname: "".to_string(),
            ip: "10.0.0.1".parse::<IpAddr>().unwrap(),
            port: 4992,
            firmware_version: "9.0.0.0".to_string(),
        };

        let builder = FlexRadioBuilder::new().radio(&radio);
        assert_eq!(builder.host.as_deref(), Some("10.0.0.1"));
        // Model should remain None for an unknown model string.
        assert!(builder.model.is_none());
    }

    // -----------------------------------------------------------------------
    // Default trait
    // -----------------------------------------------------------------------

    #[test]
    fn test_default_trait() {
        let builder = FlexRadioBuilder::default();
        assert_eq!(builder.tcp_port, 4992);
        assert_eq!(builder.client_name, "riglib");
    }
}
