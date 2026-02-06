//! Serial port transport for rig communication.
//!
//! This module provides [`SerialTransport`], which implements the [`Transport`]
//! trait for USB virtual COM ports and physical RS-232 serial connections.
//!
//! Most modern transceivers connect via USB and present as virtual serial ports:
//! - Icom: CI-V protocol, typically 19200 or 115200 baud
//! - Elecraft: Extended Kenwood CAT, typically 38400 baud
//! - Kenwood: CAT protocol, typically 9600 or 115200 baud
//! - Yaesu: CAT protocol, typically 4800 or 38400 baud
//!
//! # Example
//!
//! ```no_run
//! use riglib_transport::SerialTransport;
//! use riglib_core::transport::Transport;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! // Open Icom CI-V connection at 19200 baud
//! let mut transport = SerialTransport::open("/dev/ttyUSB0", 19200).await?;
//!
//! // Send a CI-V command
//! transport.send(&[0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD]).await?;
//!
//! // Receive response with 1 second timeout
//! let mut buf = [0u8; 256];
//! let n = transport.receive(&mut buf, Duration::from_secs(1)).await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use riglib_core::error::{Error, Result};
use riglib_core::transport::Transport;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream};

/// Serial port configuration.
///
/// Defaults are appropriate for most modern transceivers:
/// - 8 data bits
/// - 1 stop bit
/// - No parity
/// - No flow control
#[derive(Debug, Clone)]
pub struct SerialConfig {
    /// Baud rate (e.g., 9600, 19200, 38400, 115200)
    pub baud_rate: u32,
    /// Number of data bits (typically 8)
    pub data_bits: DataBits,
    /// Number of stop bits (typically 1)
    pub stop_bits: StopBits,
    /// Parity checking (typically None)
    pub parity: Parity,
    /// Flow control (typically None for CI-V, Sometimes RTS/CTS for CAT)
    pub flow_control: FlowControl,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baud_rate: 9600,
            data_bits: DataBits::Eight,
            stop_bits: StopBits::One,
            parity: Parity::None,
            flow_control: FlowControl::None,
        }
    }
}

/// Number of data bits per character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

impl From<DataBits> for tokio_serial::DataBits {
    fn from(bits: DataBits) -> Self {
        match bits {
            DataBits::Five => tokio_serial::DataBits::Five,
            DataBits::Six => tokio_serial::DataBits::Six,
            DataBits::Seven => tokio_serial::DataBits::Seven,
            DataBits::Eight => tokio_serial::DataBits::Eight,
        }
    }
}

/// Number of stop bits per character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

impl From<StopBits> for tokio_serial::StopBits {
    fn from(bits: StopBits) -> Self {
        match bits {
            StopBits::One => tokio_serial::StopBits::One,
            StopBits::Two => tokio_serial::StopBits::Two,
        }
    }
}

/// Parity checking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
}

impl From<Parity> for tokio_serial::Parity {
    fn from(parity: Parity) -> Self {
        match parity {
            Parity::None => tokio_serial::Parity::None,
            Parity::Odd => tokio_serial::Parity::Odd,
            Parity::Even => tokio_serial::Parity::Even,
        }
    }
}

/// Flow control mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowControl {
    None,
    Software,
    Hardware,
}

impl From<FlowControl> for tokio_serial::FlowControl {
    fn from(flow: FlowControl) -> Self {
        match flow {
            FlowControl::None => tokio_serial::FlowControl::None,
            FlowControl::Software => tokio_serial::FlowControl::Software,
            FlowControl::Hardware => tokio_serial::FlowControl::Hardware,
        }
    }
}

/// Serial port transport for rig communication.
///
/// Implements the [`Transport`] trait for USB virtual COM ports and
/// physical RS-232 connections to transceivers.
pub struct SerialTransport {
    /// The underlying serial port stream
    port: Option<SerialStream>,
    /// Port name for logging/debugging
    port_name: String,
}

impl SerialTransport {
    /// Open a serial port with the given baud rate and default settings.
    ///
    /// Default settings:
    /// - 8 data bits
    /// - 1 stop bit
    /// - No parity
    /// - No flow control
    ///
    /// # Arguments
    ///
    /// * `port` - Serial port path (e.g., "/dev/ttyUSB0" on Linux, "COM3" on Windows)
    /// * `baud_rate` - Baud rate (e.g., 9600, 19200, 38400, 115200)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use riglib_transport::SerialTransport;
    /// # async fn example() -> riglib_core::Result<()> {
    /// let transport = SerialTransport::open("/dev/ttyUSB0", 19200).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open(port: &str, baud_rate: u32) -> Result<Self> {
        let config = SerialConfig {
            baud_rate,
            ..Default::default()
        };
        Self::open_with_config(port, config).await
    }

    /// Open a serial port with full configuration control.
    ///
    /// # Arguments
    ///
    /// * `port` - Serial port path
    /// * `config` - Full serial port configuration
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use riglib_transport::{SerialTransport, SerialConfig, DataBits, StopBits, Parity, FlowControl};
    /// # async fn example() -> riglib_core::Result<()> {
    /// let config = SerialConfig {
    ///     baud_rate: 38400,
    ///     data_bits: DataBits::Eight,
    ///     stop_bits: StopBits::One,
    ///     parity: Parity::None,
    ///     flow_control: FlowControl::Hardware,
    /// };
    /// let transport = SerialTransport::open_with_config("/dev/ttyUSB0", config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open_with_config(port: &str, config: SerialConfig) -> Result<Self> {
        tracing::debug!(
            port = %port,
            baud_rate = config.baud_rate,
            data_bits = ?config.data_bits,
            stop_bits = ?config.stop_bits,
            parity = ?config.parity,
            flow_control = ?config.flow_control,
            "Opening serial port"
        );

        let mut serial_stream = tokio_serial::new(port, config.baud_rate)
            .data_bits(config.data_bits.into())
            .stop_bits(config.stop_bits.into())
            .parity(config.parity.into())
            .flow_control(config.flow_control.into())
            .open_native_async()
            .map_err(|e| {
                tracing::error!(port = %port, error = %e, "Failed to open serial port");
                Error::Transport(format!("Failed to open serial port {}: {}", port, e))
            })?;

        // De-assert DTR and RTS immediately after opening.
        //
        // Many transceivers route DTR/RTS to CW key and/or PTT inputs.
        // If the OS asserts DTR on open (common default), the radio will
        // interpret it as key-down and produce a continuous sidetone.
        // Hamlib, flrig, and wsjt-x all do this same de-assertion.
        if let Err(e) = serial_stream.write_data_terminal_ready(false) {
            tracing::warn!(port = %port, error = %e, "Failed to de-assert DTR");
        }
        if let Err(e) = serial_stream.write_request_to_send(false) {
            tracing::warn!(port = %port, error = %e, "Failed to de-assert RTS");
        }

        tracing::info!(port = %port, baud_rate = config.baud_rate, "Serial port opened successfully");

        Ok(Self {
            port: Some(serial_stream),
            port_name: port.to_string(),
        })
    }

    /// Get the name of the serial port.
    pub fn port_name(&self) -> &str {
        &self.port_name
    }
}

#[async_trait]
impl Transport for SerialTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        let port = self.port.as_mut().ok_or(Error::NotConnected)?;

        tracing::trace!(
            port = %self.port_name,
            bytes = data.len(),
            data = ?data,
            "Sending data"
        );

        port.write_all(data).await.map_err(|e| {
            tracing::error!(
                port = %self.port_name,
                error = %e,
                "Failed to send data"
            );
            // Check if this is a broken pipe or connection issue
            if e.kind() == std::io::ErrorKind::BrokenPipe
                || e.kind() == std::io::ErrorKind::NotConnected
            {
                Error::ConnectionLost
            } else {
                Error::Io(e)
            }
        })?;

        // Flush to ensure data is transmitted immediately
        port.flush().await.map_err(|e| {
            tracing::error!(
                port = %self.port_name,
                error = %e,
                "Failed to flush serial port"
            );
            Error::Io(e)
        })?;

        tracing::trace!(port = %self.port_name, "Data sent successfully");

        Ok(())
    }

    async fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        let port = self.port.as_mut().ok_or(Error::NotConnected)?;

        tracing::trace!(
            port = %self.port_name,
            buf_len = buf.len(),
            timeout_ms = timeout.as_millis(),
            "Waiting for data"
        );

        let result = tokio::time::timeout(timeout, port.read(buf)).await;

        match result {
            Ok(Ok(n)) => {
                tracing::trace!(
                    port = %self.port_name,
                    bytes = n,
                    data = ?&buf[..n],
                    "Received data"
                );
                Ok(n)
            }
            Ok(Err(e)) => {
                tracing::error!(
                    port = %self.port_name,
                    error = %e,
                    "Failed to receive data"
                );
                // Check if this is a connection issue
                if e.kind() == std::io::ErrorKind::BrokenPipe
                    || e.kind() == std::io::ErrorKind::NotConnected
                {
                    Err(Error::ConnectionLost)
                } else {
                    Err(Error::Io(e))
                }
            }
            Err(_) => {
                tracing::trace!(
                    port = %self.port_name,
                    timeout_ms = timeout.as_millis(),
                    "Timeout waiting for data"
                );
                Err(Error::Timeout)
            }
        }
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(mut port) = self.port.take() {
            tracing::debug!(port = %self.port_name, "Closing serial port");

            // Flush any pending data before closing
            if let Err(e) = port.flush().await {
                tracing::warn!(
                    port = %self.port_name,
                    error = %e,
                    "Failed to flush before closing (continuing anyway)"
                );
            }

            // The port will be dropped here, which closes it
            tracing::info!(port = %self.port_name, "Serial port closed");
        }

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.port.is_some()
    }
}

// Implement Drop to ensure the port is closed properly
impl Drop for SerialTransport {
    fn drop(&mut self) {
        if self.port.is_some() {
            tracing::debug!(port = %self.port_name, "SerialTransport dropped, closing port");
            // The port will be automatically closed when dropped
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serial_config_default() {
        let config = SerialConfig::default();
        assert_eq!(config.baud_rate, 9600);
        assert_eq!(config.data_bits, DataBits::Eight);
        assert_eq!(config.stop_bits, StopBits::One);
        assert_eq!(config.parity, Parity::None);
        assert_eq!(config.flow_control, FlowControl::None);
    }

    #[test]
    fn test_data_bits_conversion() {
        let _: tokio_serial::DataBits = DataBits::Five.into();
        let _: tokio_serial::DataBits = DataBits::Six.into();
        let _: tokio_serial::DataBits = DataBits::Seven.into();
        let _: tokio_serial::DataBits = DataBits::Eight.into();
    }

    #[test]
    fn test_stop_bits_conversion() {
        let _: tokio_serial::StopBits = StopBits::One.into();
        let _: tokio_serial::StopBits = StopBits::Two.into();
    }

    #[test]
    fn test_parity_conversion() {
        let _: tokio_serial::Parity = Parity::None.into();
        let _: tokio_serial::Parity = Parity::Odd.into();
        let _: tokio_serial::Parity = Parity::Even.into();
    }

    #[test]
    fn test_flow_control_conversion() {
        let _: tokio_serial::FlowControl = FlowControl::None.into();
        let _: tokio_serial::FlowControl = FlowControl::Software.into();
        let _: tokio_serial::FlowControl = FlowControl::Hardware.into();
    }
}
