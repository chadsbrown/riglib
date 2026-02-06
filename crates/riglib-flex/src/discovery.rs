//! FlexRadio LAN discovery via VITA-49 UDP broadcasts.
//!
//! FlexRadio transceivers announce their presence on the local network by
//! sending VITA-49 packets with class code `0xFFFF` (Discovery) to UDP
//! port 4992. This module listens for these broadcasts and parses the
//! discovery payload to identify available radios.
//!
//! # Usage
//!
//! ```no_run
//! use riglib_flex::discovery;
//! use std::time::Duration;
//!
//! # async fn example() -> riglib_core::Result<()> {
//! let radios = discovery::discover(Duration::from_secs(3)).await?;
//! for radio in &radios {
//!     println!("{} ({}) at {}:{}", radio.model, radio.serial, radio.ip, radio.port);
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use riglib_core::error::{Error, Result};

use crate::vita49;

/// Default FlexRadio discovery broadcast port.
pub const DISCOVERY_PORT: u16 = 4992;

/// A FlexRadio discovered on the local network.
#[derive(Debug, Clone)]
pub struct DiscoveredRadio {
    /// Radio model name (e.g. "FLEX-6600").
    pub model: String,
    /// Radio serial number.
    pub serial: String,
    /// User-assigned nickname.
    pub nickname: String,
    /// IP address of the radio.
    pub ip: IpAddr,
    /// TCP command port (typically 4992).
    pub port: u16,
    /// Firmware version string.
    pub firmware_version: String,
}

/// Listen for FlexRadio discovery broadcasts on the default port (4992).
///
/// Returns all unique radios discovered within the timeout period.
/// Radios are deduplicated by serial number.
pub async fn discover(timeout: Duration) -> Result<Vec<DiscoveredRadio>> {
    discover_on_port(DISCOVERY_PORT, timeout).await
}

/// Listen for FlexRadio discovery broadcasts on a specific port.
///
/// This variant allows tests to use a non-privileged port for mock
/// discovery packets sent via loopback.
pub async fn discover_on_port(port: u16, timeout: Duration) -> Result<Vec<DiscoveredRadio>> {
    let bind_addr = format!("0.0.0.0:{}", port);
    let socket = tokio::net::UdpSocket::bind(&bind_addr).await.map_err(|e| {
        Error::Transport(format!(
            "failed to bind discovery socket on {}: {}",
            bind_addr, e
        ))
    })?;

    tracing::debug!(port = port, "Listening for FlexRadio discovery broadcasts");

    let mut radios: HashMap<String, DiscoveredRadio> = HashMap::new();
    let mut buf = [0u8; 4096];

    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, src_addr))) => {
                match parse_discovery_packet(&buf[..n], src_addr.ip()) {
                    Ok(radio) => {
                        tracing::debug!(
                            model = %radio.model,
                            serial = %radio.serial,
                            ip = %radio.ip,
                            "Discovered FlexRadio"
                        );
                        radios.entry(radio.serial.clone()).or_insert(radio);
                    }
                    Err(_) => {
                        // Not a discovery packet or failed to parse -- ignore.
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::trace!(error = %e, "Discovery recv error");
            }
            Err(_) => {
                // Timeout reached.
                break;
            }
        }
    }

    let result: Vec<DiscoveredRadio> = radios.into_values().collect();
    tracing::debug!(count = result.len(), "Discovery complete");
    Ok(result)
}

/// Parse a single UDP datagram as a FlexRadio discovery packet.
///
/// The datagram must be a VITA-49 packet with class code 0xFFFF
/// (Discovery). The payload contains key-value text data about the radio.
fn parse_discovery_packet(data: &[u8], src_ip: IpAddr) -> Result<DiscoveredRadio> {
    let packet = vita49::parse_packet(data)?;

    if packet.header.stream_type != vita49::StreamType::Discovery {
        return Err(Error::Protocol("not a discovery packet".into()));
    }

    // The discovery payload is ASCII key=value pairs separated by spaces.
    let payload_str = std::str::from_utf8(packet.payload)
        .map_err(|_| Error::Protocol("discovery payload is not valid UTF-8".into()))?;

    let mut kv: HashMap<String, String> = HashMap::new();
    for token in payload_str.split_whitespace() {
        if let Some(eq_pos) = token.find('=') {
            let key = token[..eq_pos].to_string();
            let value = token[eq_pos + 1..].to_string();
            kv.insert(key, value);
        }
    }

    let model = kv.get("model").cloned().unwrap_or_default();
    let serial = kv.get("serial").cloned().unwrap_or_default();
    let nickname = kv
        .get("nickname")
        .or_else(|| kv.get("callsign"))
        .cloned()
        .unwrap_or_default();
    let port = kv
        .get("port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(4992u16);
    let firmware_version = kv.get("version").cloned().unwrap_or_default();

    // Prefer the IP from the packet's key-value data if available,
    // otherwise use the source address of the UDP datagram.
    let ip = kv.get("ip").and_then(|s| s.parse().ok()).unwrap_or(src_ip);

    Ok(DiscoveredRadio {
        model,
        serial,
        nickname,
        ip,
        port,
        firmware_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover_timeout_empty() {
        // Listen very briefly on a random port. No broadcasts should arrive.
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        // Drop the socket so discover_on_port can bind to the same port.
        drop(socket);

        let radios = discover_on_port(port, Duration::from_millis(50))
            .await
            .unwrap();
        assert!(radios.is_empty());
    }

    #[tokio::test]
    async fn test_parse_discovery_packet() {
        // Build a mock VITA-49 discovery packet.
        let payload = b"model=FLEX-6600 serial=12345 nickname=MyStation version=3.5.1.0 ip=192.168.1.100 port=4992";

        // Ensure payload is 4-byte aligned for VITA-49.
        let mut padded_payload = payload.to_vec();
        while padded_payload.len() % 4 != 0 {
            padded_payload.push(b' ');
        }

        let total_bytes = vita49::HEADER_SIZE + padded_payload.len();
        let size_words = (total_bytes / 4) as u16;

        let mut pkt = Vec::with_capacity(total_bytes);

        // Build header manually for discovery class code 0xFFFF.
        // Header word: type=0x3, class_id=1, no trailer
        let mut hw: u32 = 0;
        hw |= 0x3 << 28; // packet type
        hw |= 1 << 27; // class_id present
        hw |= 0x01 << 20; // TSI = UTC
        hw |= 0x01 << 18; // TSF
        hw |= size_words as u32 & 0x0FFF;
        pkt.extend_from_slice(&hw.to_be_bytes());

        // Stream ID
        pkt.extend_from_slice(&0u32.to_be_bytes());

        // Class OUI
        let class_upper: u32 = vita49::FLEXRADIO_OUI << 8;
        pkt.extend_from_slice(&class_upper.to_be_bytes());

        // Info class + packet class code (0xFFFF = Discovery)
        let class_lower: u32 = (0x534Cu32 << 16) | 0xFFFF;
        pkt.extend_from_slice(&class_lower.to_be_bytes());

        // Timestamps
        pkt.extend_from_slice(&0u32.to_be_bytes());
        pkt.extend_from_slice(&0u64.to_be_bytes());

        // Payload
        pkt.extend_from_slice(&padded_payload);

        let src_ip: IpAddr = "10.0.0.1".parse().unwrap();
        let radio = parse_discovery_packet(&pkt, src_ip).unwrap();

        assert_eq!(radio.model, "FLEX-6600");
        assert_eq!(radio.serial, "12345");
        assert_eq!(radio.nickname, "MyStation");
        assert_eq!(radio.firmware_version, "3.5.1.0");
        assert_eq!(radio.ip, "192.168.1.100".parse::<IpAddr>().unwrap());
        assert_eq!(radio.port, 4992);
    }

    #[tokio::test]
    async fn test_parse_discovery_fallback_ip() {
        // When the payload does not include an "ip" key, use the source IP.
        let payload = b"model=FLEX-8600 serial=99999 version=4.0.0.0    ";
        // Already 4-byte aligned at 48 bytes.

        let total_bytes = vita49::HEADER_SIZE + payload.len();
        let size_words = (total_bytes / 4) as u16;

        let mut pkt = Vec::with_capacity(total_bytes);

        let mut hw: u32 = 0;
        hw |= 0x3 << 28;
        hw |= 1 << 27;
        hw |= 0x01 << 20;
        hw |= 0x01 << 18;
        hw |= size_words as u32 & 0x0FFF;
        pkt.extend_from_slice(&hw.to_be_bytes());

        pkt.extend_from_slice(&0u32.to_be_bytes());

        let class_upper: u32 = vita49::FLEXRADIO_OUI << 8;
        pkt.extend_from_slice(&class_upper.to_be_bytes());

        let class_lower: u32 = (0x534Cu32 << 16) | 0xFFFF;
        pkt.extend_from_slice(&class_lower.to_be_bytes());

        pkt.extend_from_slice(&0u32.to_be_bytes());
        pkt.extend_from_slice(&0u64.to_be_bytes());

        pkt.extend_from_slice(payload);

        let src_ip: IpAddr = "192.168.1.50".parse().unwrap();
        let radio = parse_discovery_packet(&pkt, src_ip).unwrap();

        assert_eq!(radio.model, "FLEX-8600");
        assert_eq!(radio.serial, "99999");
        // IP should fall back to source address.
        assert_eq!(radio.ip, src_ip);
        assert_eq!(radio.port, 4992); // default
    }

    #[tokio::test]
    async fn test_reject_non_discovery_packet() {
        // Build a meter data packet (class code 0x8002) -- should be rejected.
        let payload = [0u8; 8]; // two meter readings
        let total_bytes = vita49::HEADER_SIZE + payload.len();
        let size_words = (total_bytes / 4) as u16;

        let mut pkt = Vec::with_capacity(total_bytes);

        let mut hw: u32 = 0;
        hw |= 0x3 << 28;
        hw |= 1 << 27;
        hw |= 0x01 << 20;
        hw |= 0x01 << 18;
        hw |= size_words as u32 & 0x0FFF;
        pkt.extend_from_slice(&hw.to_be_bytes());

        pkt.extend_from_slice(&0u32.to_be_bytes());

        let class_upper: u32 = vita49::FLEXRADIO_OUI << 8;
        pkt.extend_from_slice(&class_upper.to_be_bytes());

        let class_lower: u32 = (0x534Cu32 << 16) | 0x8002; // MeterData
        pkt.extend_from_slice(&class_lower.to_be_bytes());

        pkt.extend_from_slice(&0u32.to_be_bytes());
        pkt.extend_from_slice(&0u64.to_be_bytes());

        pkt.extend_from_slice(&payload);

        let src_ip: IpAddr = "10.0.0.1".parse().unwrap();
        let result = parse_discovery_packet(&pkt, src_ip);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_discover_with_mock_broadcast() {
        // Bind a receiver on a random port.
        let recv_socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let recv_port = recv_socket.local_addr().unwrap().port();
        // Drop so discover_on_port can bind.
        drop(recv_socket);

        // Build a discovery packet.
        let payload = b"model=FLEX-6400 serial=ABCDE nickname=TestRadio version=3.4.0.0 ip=127.0.0.1 port=4992";
        let mut padded_payload = payload.to_vec();
        while padded_payload.len() % 4 != 0 {
            padded_payload.push(b' ');
        }

        let total_bytes = vita49::HEADER_SIZE + padded_payload.len();
        let size_words = (total_bytes / 4) as u16;

        let mut pkt = Vec::with_capacity(total_bytes);
        let mut hw: u32 = 0;
        hw |= 0x3 << 28;
        hw |= 1 << 27;
        hw |= 0x01 << 20;
        hw |= 0x01 << 18;
        hw |= size_words as u32 & 0x0FFF;
        pkt.extend_from_slice(&hw.to_be_bytes());
        pkt.extend_from_slice(&0u32.to_be_bytes());
        let class_upper: u32 = vita49::FLEXRADIO_OUI << 8;
        pkt.extend_from_slice(&class_upper.to_be_bytes());
        let class_lower: u32 = (0x534Cu32 << 16) | 0xFFFF;
        pkt.extend_from_slice(&class_lower.to_be_bytes());
        pkt.extend_from_slice(&0u32.to_be_bytes());
        pkt.extend_from_slice(&0u64.to_be_bytes());
        pkt.extend_from_slice(&padded_payload);

        // Spawn a sender that will send the discovery packet shortly.
        let pkt_clone = pkt.clone();
        let sender = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let send_socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let dest: std::net::SocketAddr = format!("127.0.0.1:{}", recv_port).parse().unwrap();
            send_socket.send_to(&pkt_clone, dest).await.unwrap();
        });

        let radios = discover_on_port(recv_port, Duration::from_millis(300))
            .await
            .unwrap();

        sender.await.unwrap();

        assert_eq!(radios.len(), 1);
        assert_eq!(radios[0].model, "FLEX-6400");
        assert_eq!(radios[0].serial, "ABCDE");
        assert_eq!(radios[0].nickname, "TestRadio");
    }

    #[tokio::test]
    async fn test_discovery_deduplication() {
        // Send two packets with the same serial number -- should deduplicate.
        let recv_socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let recv_port = recv_socket.local_addr().unwrap().port();
        drop(recv_socket);

        let build_discovery_pkt = |serial: &str| -> Vec<u8> {
            let payload_str = format!(
                "model=FLEX-6600 serial={} nickname=Station version=3.5.0.0",
                serial
            );
            let mut padded = payload_str.into_bytes();
            while padded.len() % 4 != 0 {
                padded.push(b' ');
            }

            let total = vita49::HEADER_SIZE + padded.len();
            let words = (total / 4) as u16;

            let mut pkt = Vec::with_capacity(total);
            let mut hw: u32 = 0;
            hw |= 0x3 << 28;
            hw |= 1 << 27;
            hw |= 0x01 << 20;
            hw |= 0x01 << 18;
            hw |= words as u32 & 0x0FFF;
            pkt.extend_from_slice(&hw.to_be_bytes());
            pkt.extend_from_slice(&0u32.to_be_bytes());
            let class_upper: u32 = vita49::FLEXRADIO_OUI << 8;
            pkt.extend_from_slice(&class_upper.to_be_bytes());
            let class_lower: u32 = (0x534Cu32 << 16) | 0xFFFF;
            pkt.extend_from_slice(&class_lower.to_be_bytes());
            pkt.extend_from_slice(&0u32.to_be_bytes());
            pkt.extend_from_slice(&0u64.to_be_bytes());
            pkt.extend_from_slice(&padded);
            pkt
        };

        let pkt1 = build_discovery_pkt("SER001");
        let pkt2 = build_discovery_pkt("SER001"); // same serial
        let pkt3 = build_discovery_pkt("SER002"); // different serial

        let sender = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let dest: std::net::SocketAddr = format!("127.0.0.1:{}", recv_port).parse().unwrap();
            sock.send_to(&pkt1, dest).await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
            sock.send_to(&pkt2, dest).await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
            sock.send_to(&pkt3, dest).await.unwrap();
        });

        let radios = discover_on_port(recv_port, Duration::from_millis(300))
            .await
            .unwrap();

        sender.await.unwrap();

        assert_eq!(radios.len(), 2, "should deduplicate by serial number");
    }
}
