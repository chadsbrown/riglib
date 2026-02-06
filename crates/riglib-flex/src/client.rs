//! SmartSDR TCP client for FlexRadio transceivers.
//!
//! [`SmartSdrClient`] manages the TCP command channel (port 4992) and
//! optionally a UDP VITA-49 receiver (port 4991). It handles the connection
//! handshake, command/response correlation via sequence numbers, background
//! status message processing, and meter data reception.
//!
//! This is a pure SmartSDR protocol client -- it does not know about the
//! riglib `Rig` trait. The trait mapping is handled separately in the Rig
//! implementation (WI-3.6).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, broadcast, oneshot};

use riglib_core::ReceiverId;
use riglib_core::audio::AudioBuffer;
use riglib_core::error::{Error, Result};
use riglib_core::events::RigEvent;

use crate::codec::{self, SmartSdrMessage, SmartSdrResponse, SmartSdrVersion, mhz_to_hz};
use crate::mode;
use crate::state::{RadioState, SliceState};
use crate::vita49;

/// Default command timeout (2 seconds).
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

/// Default handshake timeout (5 seconds).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Broadcast channel capacity for RigEvent subscribers.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Options for configuring the SmartSDR client connection.
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// Client program name sent during registration.
    pub client_name: String,
    /// Timeout for individual command responses.
    pub command_timeout: Duration,
    /// Automatically send subscription commands after connect.
    pub auto_subscribe: bool,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            client_name: "riglib".to_string(),
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            auto_subscribe: true,
        }
    }
}

/// SmartSDR TCP client for FlexRadio transceivers.
///
/// Manages the TCP command channel, background read loops, state caching,
/// and event broadcasting. The client is safe to share across tasks via
/// interior mutability (`Arc<Mutex<...>>`).
pub struct SmartSdrClient {
    /// Write half of the TCP stream, used for sending commands.
    tcp_writer: Arc<Mutex<Option<tokio::io::WriteHalf<TcpStream>>>>,

    /// Next sequence number for commands (starts at 1).
    next_seq: Arc<Mutex<u32>>,

    /// Pending command responses: sequence number -> oneshot sender.
    pending: Arc<Mutex<HashMap<u32, oneshot::Sender<SmartSdrResponse>>>>,

    /// Client handle assigned by the radio during handshake.
    handle: Arc<Mutex<Option<u32>>>,

    /// SmartSDR version from the handshake.
    version: Arc<Mutex<Option<SmartSdrVersion>>>,

    /// Cached radio state, updated from status messages and meter data.
    state: Arc<Mutex<RadioState>>,

    /// Event broadcast channel sender.
    event_tx: broadcast::Sender<RigEvent>,

    /// Background TCP read task handle.
    tcp_read_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,

    /// Background UDP read task handle.
    udp_read_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,

    /// Active DAX audio streams: stream_id -> mpsc sender for AudioBuffers.
    ///
    /// The UDP read loop checks this map for each DaxAudio VITA-49 packet
    /// and forwards parsed audio samples to the registered sender.
    dax_streams: Arc<Mutex<HashMap<u32, tokio::sync::mpsc::Sender<AudioBuffer>>>>,

    /// Connection state flag (atomic for lock-free reads).
    connected: Arc<AtomicBool>,

    /// Command timeout duration.
    command_timeout: Duration,
}

impl SmartSdrClient {
    /// Connect to a FlexRadio at the given host and port.
    ///
    /// Performs the TCP handshake (reads version and handle lines), then
    /// starts the background TCP read loop. If `auto_subscribe` is true
    /// (the default), subscription commands are sent automatically.
    pub async fn connect(host: &str, tcp_port: u16) -> Result<Self> {
        Self::connect_with_options(host, tcp_port, ClientOptions::default()).await
    }

    /// Connect with custom options.
    pub async fn connect_with_options(
        host: &str,
        tcp_port: u16,
        options: ClientOptions,
    ) -> Result<Self> {
        let addr = format!("{}:{}", host, tcp_port);
        tracing::debug!(addr = %addr, "Connecting to FlexRadio SmartSDR");

        let stream = tokio::time::timeout(HANDSHAKE_TIMEOUT, TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|e| Error::Transport(format!("TCP connect failed: {}", e)))?;

        // Disable Nagle for low-latency command/response.
        let _ = stream.set_nodelay(true);

        let (read_half, write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        // -- Handshake: read version line --
        let version_line = read_handshake_line(&mut reader).await?;
        let version_msg = codec::parse_message(&version_line)?;
        let version = match version_msg {
            SmartSdrMessage::Version(v) => v,
            other => {
                return Err(Error::Protocol(format!(
                    "expected version line, got: {:?}",
                    other
                )));
            }
        };
        tracing::debug!(
            major = version.major,
            minor = version.minor,
            patch = version.patch,
            build = version.build,
            "SmartSDR version received"
        );

        // -- Handshake: read handle line --
        let handle_line = read_handshake_line(&mut reader).await?;
        let handle_msg = codec::parse_message(&handle_line)?;
        let handle = match handle_msg {
            SmartSdrMessage::Handle(h) => h,
            other => {
                return Err(Error::Protocol(format!(
                    "expected handle line, got: {:?}",
                    other
                )));
            }
        };
        tracing::debug!(handle = format!("{:08X}", handle), "Client handle received");

        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        let pending: Arc<Mutex<HashMap<u32, oneshot::Sender<SmartSdrResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let state = Arc::new(Mutex::new(RadioState::default()));
        let connected = Arc::new(AtomicBool::new(true));

        // Mark connected in state.
        {
            let mut s = state.lock().await;
            s.connected = true;
        }

        // Start the TCP read loop.
        let tcp_read_handle = {
            let pending = Arc::clone(&pending);
            let state = Arc::clone(&state);
            let event_tx = event_tx.clone();
            let connected = Arc::clone(&connected);

            tokio::spawn(async move {
                tcp_read_loop(reader, pending, state, event_tx, connected).await;
            })
        };

        let dax_streams = Arc::new(Mutex::new(HashMap::new()));

        let client = SmartSdrClient {
            tcp_writer: Arc::new(Mutex::new(Some(write_half))),
            next_seq: Arc::new(Mutex::new(1)),
            pending,
            handle: Arc::new(Mutex::new(Some(handle))),
            version: Arc::new(Mutex::new(Some(version))),
            state,
            event_tx,
            tcp_read_handle: Mutex::new(Some(tcp_read_handle)),
            udp_read_handle: Mutex::new(None),
            dax_streams,
            connected,
            command_timeout: options.command_timeout,
        };

        // Send client registration and subscriptions if auto_subscribe.
        if options.auto_subscribe {
            let reg_cmd = codec::cmd_client_program(&options.client_name);
            client.send_command_no_wait(&reg_cmd).await?;

            for sub in &["slice all", "meter all", "tx all"] {
                let sub_cmd = codec::cmd_subscribe(sub);
                client.send_command_no_wait(&sub_cmd).await?;
            }
        }

        // Emit Connected event.
        let _ = client.event_tx.send(RigEvent::Connected);

        tracing::debug!(addr = %addr, "FlexRadio client connected");
        Ok(client)
    }

    /// Send a command and await the response.
    ///
    /// Auto-increments the sequence number. Returns the response data on
    /// success, or an error on timeout or non-zero error code.
    pub async fn send_command(&self, command: &str) -> Result<String> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(Error::NotConnected);
        }

        let seq = {
            let mut next = self.next_seq.lock().await;
            let seq = *next;
            *next = next.wrapping_add(1);
            seq
        };

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(seq, tx);
        }

        let encoded = codec::encode_command(seq, command);
        tracing::trace!(seq = seq, command = %command, "Sending command");

        {
            let mut writer = self.tcp_writer.lock().await;
            let w = writer.as_mut().ok_or(Error::NotConnected)?;
            w.write_all(&encoded)
                .await
                .map_err(|e| Error::Transport(format!("failed to send command: {}", e)))?;
            w.flush()
                .await
                .map_err(|e| Error::Transport(format!("failed to flush command: {}", e)))?;
        }

        // Await the response with timeout.
        let response = tokio::time::timeout(self.command_timeout, rx).await;

        match response {
            Ok(Ok(resp)) => {
                tracing::trace!(
                    seq = seq,
                    error_code = resp.error_code,
                    message = %resp.message,
                    "Response received"
                );
                if resp.error_code != 0 {
                    Err(Error::Protocol(format!(
                        "SmartSDR error 0x{:08X}: {}",
                        resp.error_code, resp.message
                    )))
                } else {
                    Ok(resp.message)
                }
            }
            Ok(Err(_)) => {
                // Oneshot sender was dropped (read loop exited).
                let mut pending = self.pending.lock().await;
                pending.remove(&seq);
                Err(Error::ConnectionLost)
            }
            Err(_) => {
                // Timeout -- remove pending entry.
                let mut pending = self.pending.lock().await;
                pending.remove(&seq);
                Err(Error::Timeout)
            }
        }
    }

    /// Send a command without waiting for a response (fire-and-forget).
    ///
    /// Still assigns a sequence number so the radio can process it, but
    /// no oneshot channel is created and the caller does not block.
    pub async fn send_command_no_wait(&self, command: &str) -> Result<()> {
        if !self.connected.load(Ordering::SeqCst) {
            return Err(Error::NotConnected);
        }

        let seq = {
            let mut next = self.next_seq.lock().await;
            let seq = *next;
            *next = next.wrapping_add(1);
            seq
        };

        let encoded = codec::encode_command(seq, command);
        tracing::trace!(seq = seq, command = %command, "Sending command (no-wait)");

        let mut writer = self.tcp_writer.lock().await;
        let w = writer.as_mut().ok_or(Error::NotConnected)?;
        w.write_all(&encoded)
            .await
            .map_err(|e| Error::Transport(format!("failed to send command: {}", e)))?;
        w.flush()
            .await
            .map_err(|e| Error::Transport(format!("failed to flush command: {}", e)))?;

        Ok(())
    }

    /// Start the VITA-49 UDP receiver on the specified port.
    ///
    /// Binds a UDP socket and spawns a background task that receives
    /// VITA-49 packets, parses meter data, routes DAX audio to registered
    /// streams, and updates the radio state.
    pub async fn start_udp_receiver(&self, udp_port: u16) -> Result<()> {
        let bind_addr = format!("0.0.0.0:{}", udp_port);
        let socket = tokio::net::UdpSocket::bind(&bind_addr).await.map_err(|e| {
            Error::Transport(format!("failed to bind UDP socket on {}: {}", bind_addr, e))
        })?;

        tracing::debug!(port = udp_port, "UDP VITA-49 receiver started");

        let state = Arc::clone(&self.state);
        let event_tx = self.event_tx.clone();
        let dax_streams = Arc::clone(&self.dax_streams);

        let handle = tokio::spawn(async move {
            udp_read_loop(socket, state, event_tx, dax_streams).await;
        });

        let mut udp_handle = self.udp_read_handle.lock().await;
        *udp_handle = Some(handle);

        Ok(())
    }

    /// Get a snapshot of the current radio state.
    pub async fn state(&self) -> RadioState {
        let s = self.state.lock().await;
        s.clone()
    }

    /// Get the current meter value by name.
    ///
    /// Returns `None` if the meter is not registered or has no value.
    pub async fn meter_value(&self, name: &str) -> Option<i16> {
        let s = self.state.lock().await;
        let id = s.meters.id_for_name(name)?;
        s.meter_values.get(&id).copied()
    }

    /// Get the event broadcast receiver.
    ///
    /// Multiple subscribers can be created; each gets an independent copy
    /// of every event.
    pub fn subscribe(&self) -> broadcast::Receiver<RigEvent> {
        self.event_tx.subscribe()
    }

    /// Disconnect and clean up background tasks.
    pub async fn disconnect(&self) -> Result<()> {
        if !self.connected.load(Ordering::SeqCst) {
            return Ok(());
        }

        tracing::debug!("Disconnecting SmartSDR client");
        self.connected.store(false, Ordering::SeqCst);

        // Close the TCP writer to signal the read loop.
        {
            let mut writer = self.tcp_writer.lock().await;
            if let Some(mut w) = writer.take() {
                let _ = w.shutdown().await;
            }
        }

        // Abort background tasks.
        {
            let mut handle = self.tcp_read_handle.lock().await;
            if let Some(h) = handle.take() {
                h.abort();
            }
        }
        {
            let mut handle = self.udp_read_handle.lock().await;
            if let Some(h) = handle.take() {
                h.abort();
            }
        }

        // Clear pending commands.
        {
            let mut pending = self.pending.lock().await;
            pending.clear();
        }

        // Update state.
        {
            let mut s = self.state.lock().await;
            s.connected = false;
        }

        let _ = self.event_tx.send(RigEvent::Disconnected);
        tracing::debug!("SmartSDR client disconnected");
        Ok(())
    }

    /// Whether the client is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Get the client handle assigned by the radio.
    pub async fn handle(&self) -> Option<u32> {
        let h = self.handle.lock().await;
        *h
    }

    /// Get the SmartSDR version from the handshake.
    pub async fn version(&self) -> Option<SmartSdrVersion> {
        let v = self.version.lock().await;
        v.clone()
    }

    /// Register a DAX audio stream for routing.
    ///
    /// After registration, incoming VITA-49 DaxAudio packets whose
    /// `stream_id` matches will be parsed and forwarded as [`AudioBuffer`]s
    /// through the provided sender.
    pub async fn register_dax_stream(
        &self,
        stream_id: u32,
        tx: tokio::sync::mpsc::Sender<AudioBuffer>,
    ) {
        let mut streams = self.dax_streams.lock().await;
        streams.insert(stream_id, tx);
        tracing::debug!(
            stream_id = format!("0x{:08X}", stream_id),
            "Registered DAX audio stream"
        );
    }

    /// Unregister a DAX audio stream.
    ///
    /// After unregistration, incoming VITA-49 packets for this stream_id
    /// will be silently discarded. The sender is dropped, which will cause
    /// any associated `AudioReceiver` to return `None` on the next recv.
    pub async fn unregister_dax_stream(&self, stream_id: u32) {
        let mut streams = self.dax_streams.lock().await;
        streams.remove(&stream_id);
        tracing::debug!(
            stream_id = format!("0x{:08X}", stream_id),
            "Unregistered DAX audio stream"
        );
    }
}

// ---------------------------------------------------------------------------
// Handshake helper
// ---------------------------------------------------------------------------

/// Read a single line from the TCP stream during the handshake phase.
async fn read_handshake_line(
    reader: &mut BufReader<tokio::io::ReadHalf<TcpStream>>,
) -> Result<String> {
    let mut line = String::new();
    let result = tokio::time::timeout(HANDSHAKE_TIMEOUT, reader.read_line(&mut line)).await;

    match result {
        Ok(Ok(0)) => Err(Error::ConnectionLost),
        Ok(Ok(_)) => {
            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
            Ok(trimmed.to_string())
        }
        Ok(Err(e)) => Err(Error::Transport(format!("handshake read error: {}", e))),
        Err(_) => Err(Error::Timeout),
    }
}

// ---------------------------------------------------------------------------
// TCP read loop
// ---------------------------------------------------------------------------

/// Background task that reads lines from the TCP stream and dispatches them.
async fn tcp_read_loop(
    mut reader: BufReader<tokio::io::ReadHalf<TcpStream>>,
    pending: Arc<Mutex<HashMap<u32, oneshot::Sender<SmartSdrResponse>>>>,
    state: Arc<Mutex<RadioState>>,
    event_tx: broadcast::Sender<RigEvent>,
    connected: Arc<AtomicBool>,
) {
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) => {
                // Connection closed by remote.
                tracing::debug!("TCP connection closed by radio");
                connected.store(false, Ordering::SeqCst);
                {
                    let mut s = state.lock().await;
                    s.connected = false;
                }
                let _ = event_tx.send(RigEvent::Disconnected);
                break;
            }
            Ok(_) => {
                let trimmed = line_buf.trim_end_matches('\n').trim_end_matches('\r');
                if trimmed.is_empty() {
                    continue;
                }

                match codec::parse_message(trimmed) {
                    Ok(SmartSdrMessage::Response(resp)) => {
                        let seq = resp.sequence;
                        let mut p = pending.lock().await;
                        if let Some(sender) = p.remove(&seq) {
                            let _ = sender.send(resp);
                        } else {
                            tracing::trace!(seq = seq, "Response for unknown/expired sequence");
                        }
                    }
                    Ok(SmartSdrMessage::Status(status)) => {
                        process_status(&status, &state, &event_tx).await;
                    }
                    Ok(SmartSdrMessage::Message(text)) => {
                        tracing::debug!(message = %text, "SmartSDR message");
                    }
                    Ok(SmartSdrMessage::Version(v)) => {
                        tracing::warn!(
                            version = format!("{}.{}.{}.{}", v.major, v.minor, v.patch, v.build),
                            "Unexpected version line after handshake"
                        );
                    }
                    Ok(SmartSdrMessage::Handle(h)) => {
                        tracing::warn!(
                            handle = format!("{:08X}", h),
                            "Unexpected handle line after handshake"
                        );
                    }
                    Ok(SmartSdrMessage::Unknown(line)) => {
                        tracing::trace!(line = %line, "Unknown line from radio");
                    }
                    Err(e) => {
                        tracing::trace!(error = %e, line = %trimmed, "Failed to parse line");
                    }
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "TCP read error");
                connected.store(false, Ordering::SeqCst);
                {
                    let mut s = state.lock().await;
                    s.connected = false;
                }
                let _ = event_tx.send(RigEvent::Disconnected);
                break;
            }
        }
    }

    // Drop all pending oneshot senders so waiters get errors.
    let mut p = pending.lock().await;
    p.clear();
}

/// Process a parsed status message, updating cached state and emitting events.
async fn process_status(
    status: &codec::SmartSdrStatus,
    state: &Arc<Mutex<RadioState>>,
    event_tx: &broadcast::Sender<RigEvent>,
) {
    let object = &status.object;

    if object.starts_with("slice") {
        // Parse the slice status fields.
        match codec::parse_slice_status(&status.params, object) {
            Ok(ss) => {
                let mut s = state.lock().await;
                let slice = s.slices.entry(ss.index).or_insert_with(|| SliceState {
                    index: ss.index,
                    ..SliceState::default()
                });

                if let Some(freq_mhz) = ss.frequency_mhz {
                    let freq_hz = mhz_to_hz(freq_mhz);
                    if slice.frequency_hz != freq_hz {
                        slice.frequency_hz = freq_hz;
                        let _ = event_tx.send(RigEvent::FrequencyChanged {
                            receiver: ReceiverId::from_index(ss.index),
                            freq_hz,
                        });
                    }
                }

                if let Some(ref mode_str) = ss.mode {
                    if slice.mode != *mode_str {
                        slice.mode = mode_str.clone();
                        if let Ok(m) = mode::flex_to_mode(mode_str) {
                            let _ = event_tx.send(RigEvent::ModeChanged {
                                receiver: ReceiverId::from_index(ss.index),
                                mode: m,
                            });
                        }
                    }
                }

                if let Some(lo) = ss.filter_lo {
                    slice.filter_lo = lo;
                }
                if let Some(hi) = ss.filter_hi {
                    slice.filter_hi = hi;
                }
                if let Some(tx) = ss.tx {
                    slice.is_tx = tx;
                }
                if let Some(active) = ss.active {
                    slice.active = active;
                }
            }
            Err(e) => {
                tracing::trace!(error = %e, "Failed to parse slice status");
            }
        }
    } else if object == "tx" {
        let mut s = state.lock().await;
        for (key, value) in &status.params {
            match key.as_str() {
                "state" => {
                    let was_tx = s.tx_state.transmitting;
                    s.tx_state.transmitting = value == "1";
                    if s.tx_state.transmitting != was_tx {
                        let _ = event_tx.send(RigEvent::PttChanged {
                            on: s.tx_state.transmitting,
                        });
                    }
                }
                "power" | "rfpower" => {
                    if let Ok(p) = value.parse::<u16>() {
                        s.tx_state.power = p;
                    }
                }
                _ => {}
            }
        }
    } else if object.starts_with("meter") {
        // Meter status updates populate the meter map.
        match codec::parse_meter_status(&status.params) {
            Ok(ms) => {
                let mut s = state.lock().await;
                if let Some(name) = &ms.name {
                    s.meters.insert(ms.id, name);
                }
            }
            Err(e) => {
                tracing::trace!(error = %e, "Failed to parse meter status");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// UDP read loop
// ---------------------------------------------------------------------------

/// Background task that receives VITA-49 UDP datagrams and updates state.
async fn udp_read_loop(
    socket: tokio::net::UdpSocket,
    state: Arc<Mutex<RadioState>>,
    _event_tx: broadcast::Sender<RigEvent>,
    dax_streams: Arc<Mutex<HashMap<u32, tokio::sync::mpsc::Sender<AudioBuffer>>>>,
) {
    use crate::dax::DAX_SAMPLE_RATE;

    let mut buf = [0u8; 8192];

    loop {
        match socket.recv(&mut buf).await {
            Ok(n) => {
                if let Ok(packet) = vita49::parse_packet(&buf[..n]) {
                    match packet.header.stream_type {
                        vita49::StreamType::MeterData => {
                            if let Ok(readings) = vita49::parse_meter_payload(packet.payload) {
                                let mut s = state.lock().await;
                                for reading in readings {
                                    s.meter_values.insert(reading.meter_id, reading.value);
                                }
                            }
                        }
                        vita49::StreamType::DaxAudio => {
                            let stream_id = packet.header.stream_id;
                            let streams = dax_streams.lock().await;
                            if let Some(tx) = streams.get(&stream_id) {
                                if let Ok(audio_samples) =
                                    vita49::parse_dax_audio_payload(packet.payload)
                                {
                                    // Convert Vec<AudioSample> to interleaved f32 Vec.
                                    let mut samples = Vec::with_capacity(audio_samples.len() * 2);
                                    for s in &audio_samples {
                                        samples.push(s.left);
                                        samples.push(s.right);
                                    }

                                    let buffer = AudioBuffer::new(
                                        samples,
                                        2, // stereo
                                        DAX_SAMPLE_RATE,
                                    );

                                    // Use try_send to avoid blocking the UDP loop.
                                    // If the consumer is too slow, drop the buffer.
                                    if tx.try_send(buffer).is_err() {
                                        tracing::trace!(
                                            stream_id = format!("0x{:08X}", stream_id),
                                            "DAX audio buffer dropped (consumer too slow)"
                                        );
                                    }
                                }
                            }
                        }
                        _ => {
                            // Other stream types are not handled yet.
                        }
                    }
                }
            }
            Err(e) => {
                tracing::trace!(error = %e, "UDP recv error");
                // Non-fatal for UDP -- just continue.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    /// Helper: create a mock SmartSDR server that sends handshake lines
    /// and returns the listener for further interaction.
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

    #[tokio::test]
    async fn test_connect_handshake() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            // Hold connection open for a moment.
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let options = ClientOptions {
            auto_subscribe: false,
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        // Verify handshake data was captured.
        let version = client.version().await.unwrap();
        assert_eq!(version.major, 1);
        assert_eq!(version.minor, 4);
        assert_eq!(version.patch, 0);
        assert_eq!(version.build, 0);

        let handle = client.handle().await.unwrap();
        assert_eq!(handle, 0x12345678);

        assert!(client.is_connected());

        client.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_send_command_and_receive_response() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            // Read commands and consume them until we see one we want to
            // respond to. The client may send subscription commands first.
            let mut reader = BufReader::new(&mut stream);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let trimmed = line.trim();
                // Look for the test command "info" from the client.
                if trimmed.contains("info") {
                    // Extract sequence number.
                    let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
                    let resp = format!("R{}|00000000|slice_data\n", seq_str);
                    // Get raw stream reference back. We need to get the
                    // underlying stream to write.
                    let inner = reader.into_inner();
                    inner.write_all(resp.as_bytes()).await.unwrap();
                    inner.flush().await.unwrap();
                    break;
                }
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let options = ClientOptions {
            auto_subscribe: false,
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        let result = client.send_command("info").await.unwrap();
        assert_eq!(result, "slice_data");

        client.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_command_timeout() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            // Accept but never respond to commands.
            tokio::time::sleep(Duration::from_secs(10)).await;
        });

        let options = ClientOptions {
            auto_subscribe: false,
            command_timeout: Duration::from_millis(100),
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        let result = client.send_command("info").await;
        assert!(matches!(result, Err(Error::Timeout)));

        client.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_command_error_response() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            let mut reader = BufReader::new(&mut stream);
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let trimmed = line.trim();
                if trimmed.contains("slice remove") {
                    let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
                    let resp = format!("R{}|50000015|Invalid slice\n", seq_str);
                    let inner = reader.into_inner();
                    inner.write_all(resp.as_bytes()).await.unwrap();
                    inner.flush().await.unwrap();
                    break;
                }
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let options = ClientOptions {
            auto_subscribe: false,
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        let result = client.send_command("slice remove 99").await;
        assert!(result.is_err());
        match result {
            Err(Error::Protocol(msg)) => {
                assert!(
                    msg.contains("50000015"),
                    "expected error code in message: {}",
                    msg
                );
                assert!(
                    msg.contains("Invalid slice"),
                    "expected error text in message: {}",
                    msg
                );
            }
            other => panic!("expected Protocol error, got: {:?}", other),
        }

        client.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_status_message_updates_state() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            // Give the client time to start the read loop.
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send a slice status message.
            stream
                .write_all(
                    b"S12345678|slice 0 RF_frequency=14.250000 mode=USB filter_lo=100 filter_hi=2900\n",
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let options = ClientOptions {
            auto_subscribe: false,
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        // Wait for the status message to be processed.
        tokio::time::sleep(Duration::from_millis(150)).await;

        let state = client.state().await;
        let slice = state.slices.get(&0).expect("slice 0 should exist");
        assert_eq!(slice.frequency_hz, 14_250_000);
        assert_eq!(slice.mode, "USB");
        assert_eq!(slice.filter_lo, 100);
        assert_eq!(slice.filter_hi, 2900);

        client.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_multiple_commands_sequencing() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            // Read and respond to 3 commands, potentially out of order.
            let mut reader = BufReader::new(&mut stream);
            let mut responses = Vec::new();

            for _ in 0..3 {
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let trimmed = line.trim().to_string();
                let seq_str = &trimmed[1..trimmed.find('|').unwrap()];
                let seq: u32 = seq_str.parse().unwrap();
                responses.push(seq);
            }

            // Respond in reverse order to test correlation.
            let inner = reader.into_inner();
            for seq in responses.iter().rev() {
                let resp = format!("R{}|00000000|ok{}\n", seq, seq);
                inner.write_all(resp.as_bytes()).await.unwrap();
            }
            inner.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let options = ClientOptions {
            auto_subscribe: false,
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        // Send 3 commands concurrently.
        let (r1, r2, r3) = tokio::join!(
            client.send_command("cmd1"),
            client.send_command("cmd2"),
            client.send_command("cmd3"),
        );

        // All should succeed. The responses contain the seq number, so
        // we verify each got the correct one.
        let v1 = r1.unwrap();
        let v2 = r2.unwrap();
        let v3 = r3.unwrap();

        // Each response should be "ok<seq>" where seq was assigned to that
        // specific send_command call. The key invariant is that all 3
        // responses arrived and were correctly correlated.
        assert!(v1.starts_with("ok"), "r1 = {}", v1);
        assert!(v2.starts_with("ok"), "r2 = {}", v2);
        assert!(v3.starts_with("ok"), "r3 = {}", v3);

        // Verify the seq numbers are different (each got a unique one).
        let s1: u32 = v1.strip_prefix("ok").unwrap().parse().unwrap();
        let s2: u32 = v2.strip_prefix("ok").unwrap().parse().unwrap();
        let s3: u32 = v3.strip_prefix("ok").unwrap().parse().unwrap();
        let mut seqs = vec![s1, s2, s3];
        seqs.sort();
        seqs.dedup();
        assert_eq!(seqs.len(), 3, "expected 3 unique sequence numbers");

        client.disconnect().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn test_disconnect() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let _stream = accept_and_handshake(&listener).await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let options = ClientOptions {
            auto_subscribe: false,
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        assert!(client.is_connected());

        client.disconnect().await.unwrap();
        assert!(!client.is_connected());

        // Verify state reflects disconnection.
        let state = client.state().await;
        assert!(!state.connected);

        // Sending commands after disconnect should fail.
        let result = client.send_command("info").await;
        assert!(matches!(result, Err(Error::NotConnected)));

        server.abort();
    }

    #[tokio::test]
    async fn test_subscribe_events() {
        let (listener, addr) = mock_smartsdr_server().await;
        let parts: Vec<&str> = addr.split(':').collect();
        let host = parts[0];
        let port: u16 = parts[1].parse().unwrap();

        let server = tokio::spawn(async move {
            let mut stream = accept_and_handshake(&listener).await;

            tokio::time::sleep(Duration::from_millis(50)).await;

            // Send a slice frequency change status.
            stream
                .write_all(b"S12345678|slice 0 RF_frequency=7.074000\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();

            tokio::time::sleep(Duration::from_millis(500)).await;
        });

        let options = ClientOptions {
            auto_subscribe: false,
            ..ClientOptions::default()
        };
        let client = SmartSdrClient::connect_with_options(host, port, options)
            .await
            .unwrap();

        // The first event is Connected (from the connect call).
        let mut rx = client.subscribe();

        // Wait for the status message to be processed.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Drain events and look for FrequencyChanged.
        let mut found_freq_change = false;
        while let Ok(event) = rx.try_recv() {
            if let RigEvent::FrequencyChanged { receiver, freq_hz } = event {
                assert_eq!(receiver, ReceiverId::from_index(0));
                assert_eq!(freq_hz, 7_074_000);
                found_freq_change = true;
            }
        }
        assert!(found_freq_change, "expected FrequencyChanged event");

        client.disconnect().await.unwrap();
        server.abort();
    }
}
