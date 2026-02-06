# 004 — Audio Streaming Research

Research notes for riglib Phase 4 (audio streaming).
Conducted 2026-02-06.

---

## 1. USB Audio for Serial Manufacturers

### 1.1 General Pattern

All modern HF transceivers from the five supported manufacturers present USB Audio
Class 1.0 (UAC 1.0) compliant devices when connected via USB. UAC 1.0 is driverless
on all major operating systems (Linux, macOS, Windows 10+). The common configuration
across manufacturers is:

| Parameter      | Typical Value          |
|----------------|------------------------|
| USB Audio Class | 1.0 (class-compliant)  |
| Sample Rate     | 48,000 Hz              |
| Bit Depth       | 16-bit signed integer  |
| Channels        | 2 (stereo)             |
| Codec Chip      | TI/Burr-Brown PCM2902 or similar |

Some rigs support additional sample rates (44,100 Hz) but 48 kHz / 16-bit is the
universal lowest common denominator that all ham radio software targets. WSJT-X
explicitly requires 48,000 Hz, 16-bit, and this has become the de facto standard.

### 1.2 Icom IC-7610

**USB Architecture**: The IC-7610 has two rear-panel USB Type-B ports. USB-1 (closest
to the speaker jacks) connects to an internal USB hub that exposes three devices:

1. **USB Audio CODEC** (TI/Burr-Brown) -- line-level audio I/O, 48 kHz 16-bit stereo
2. **Silicon Labs CP210x #1** -- CI-V transceiver control (virtual COM port)
3. **Silicon Labs CP210x #2** -- RTTY/PSK decoder, CW keying, GPS (virtual COM port)

USB-2 is a separate port dedicated to I/Q baseband output (192 kHz, 24-bit, for use
with HDSDR and similar SDR software).

**Device Name**:
- Linux (ALSA/PulseAudio): `USB Audio CODEC` or `Burr-Brown from TI USB Audio CODEC`
  - PulseAudio name: `alsa_input.usb-Burr-Brown_from_TI_USB_Audio_CODEC-00.analog-stereo`
- macOS (CoreAudio): `USB Audio CODEC`
- Windows: `USB Audio CODEC` (under Microphone/Speakers in Sound settings)

**Audio Routing**: The single USB Audio CODEC device carries both Main and Sub receiver
audio (stereo -- left channel Main, right channel Sub). TX audio goes from computer
to the rig on the same audio device.

**Key Detail**: The audio device, CI-V serial port, and RTTY serial port all share
the same internal USB hub. This means they share a common USB device path prefix,
which can be used for device association (see Section 3).

**Sources**:
- [W7AY IC-7610 Discovery](https://www.w7ay.net/site/Applications/IC-7610/Contents/Discover.html)
- [Ham Radio Deluxe IC-7610 USB Setup](https://support.hamradiodeluxe.com/support/solutions/articles/51000083466-ic-7610-usb-setup-configuration)
- [DXLab Suite Connecting Icom](https://www.dxlabsuite.com/dxlabwiki/ConnectingIcom72007600)

### 1.3 Icom IC-7300 (and IC-7100, IC-9700, IC-705)

Same architecture as the IC-7610 but with a single USB port. The internal hub exposes
the same three devices (Audio CODEC + 2x CP210x serial). The IC-7300 uses the same
Burr-Brown USB Audio CODEC at 48 kHz 16-bit.

The IC-705 uses USB-C but presents the same device classes.

**Sources**:
- [K0PIR Icom 7300 COM Port and USB Audio CODEC](https://k0pir.us/icom-7300-com-port-and-usb-audio-codec-fix/)
- [SM7IUN Configuring Icom Radios for USB](https://media.sm7iun.se/2019/10/Configuring-Icom-HF-Radios-with-USB-for-Digital-Operation.pdf)

### 1.4 Yaesu FT-DX10

**USB Architecture**: The FT-DX10 has a built-in USB Audio CODEC. A single USB Type-B
cable provides both CAT control (virtual COM port) and audio I/O. No external
interface (SCU-17) is needed. The SCU-17 is an external USB audio + serial interface
for older Yaesu rigs that lack built-in USB audio (FT-891, etc.).

**Device Name**: `USB Audio CODEC` (same generic name as Icom, since both use
similar TI/Burr-Brown codec chips).

**Sample Rate**: 48 kHz, 16-bit stereo.

**CAT Baud Rate**: 38,400 (default) -- this is the serial control baud rate, not
audio. Separate USB endpoint.

**Sources**:
- [Galanto FT-DX10 FT8 Setup](https://www.galanto.com/en/yaesu-ftdx-10-ft8-jtdx/)
- [PD2TX FT-DX10 FT8 Settings](https://pd2tx.com/archives/84)
- [WSJTX Groups.io FT-DX10 Codec Discussion](https://wsjtx.groups.io/g/main/topic/yaesu_dx10_codec_audio/91252375)

### 1.5 Yaesu FT-DX101D/MP and FT-991A

Both have built-in USB audio CODECs similar to the FT-DX10. The FT-891 and FT-710
also have built-in USB audio. All Yaesu models in our catalog (post-2015) include
built-in USB audio; the SCU-17 is only needed for much older rigs.

### 1.6 Elecraft K3 / K3S

**USB Architecture**: The K3S has a USB port providing both CAT control and
line-level audio I/O on the same cable. The USB audio device name varies by OS but
appears generically as `USB Audio CODEC` or similar.

**Sample Rate**: 48 kHz appears to be the standard rate based on S/PDIF and digital
mode configuration guides, though Elecraft documentation is less explicit about
the internal codec specifications than Icom's.

**K3 (original)**: The original K3 requires the KIO3 interface module for USB audio.
Without it, an external sound card interface (SignaLink, etc.) is needed.

**Sources**:
- [K3 and USB Audio Codec Discussion](https://elecraft.mailman.qth.narkive.com/7oRQJgS7/k3-and-usb-audio-codec)
- [K3S Owner's Manual](https://ftp.elecraft.com/K3S/Manuals%20Downloads/K3S%20Owner's%20man%20A1.pdf)

### 1.7 Elecraft K4

**USB Architecture**: The K4 provides a USB Audio CODEC when connected via USB.
Windows creates two input and two output audio devices from the USB connection.
The K4 also supports Ethernet connectivity, but audio over Ethernet is not
documented as a feature -- USB audio is the primary computer audio path.

**Sample Rate**: 48 kHz, 16-bit (standard for WSJT-X and digital modes).

**Sources**:
- [WT8P K4D WSJT-X Configuration](https://www.wt8p.com/configuring-elecraft-k4d-for-ft8/)
- [K4 Operating Manual Rev D6](https://ftp.elecraft.com/K4/Manuals%20Downloads/K4%20Built-In%20Operating%20Manual%20rev%20D6/Operating%20Manual%20Rev%20D6%20printable.pdf)

### 1.8 Kenwood TS-890S

**USB Architecture**: The TS-890S has a USB Type-B connector providing both CAT
control and USB audio. The device presents as `USB AUDIO CODEC` in Windows
(under "Line (USB AUDIO CODEC)").

**Sample Rate**: Configurable from Windows audio properties Advanced tab. Supports
48 kHz 16-bit as the standard configuration.

**Kenwood USB Audio Manual**: Kenwood provides a dedicated "USB Audio Setting Manual"
for the TS-890S that walks through Windows audio configuration.

**Sources**:
- [Kenwood TS-890S USB Audio Manual](https://www.kenwood.com/i/products/info/amateur/ts_890/pdf/ts890_usb_audio_manual_e.pdf)
- [Kenwood TS-890S FT8 Settings](https://www.kenwood.com/i/products/info/amateur/ts_890/pdf/ts890_ft8_settings_en.pdf)

### 1.9 Summary Table

| Rig            | Built-in USB Audio | Device Name Pattern      | Sample Rate | Bit Depth | Channels |
|----------------|-------------------|--------------------------|-------------|-----------|----------|
| IC-7610        | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2 (L=Main, R=Sub) |
| IC-7300        | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| IC-9700        | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| IC-705         | Yes (USB-C)       | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| FT-DX10        | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| FT-DX101D/MP   | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| FT-991A        | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| FT-891         | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| FT-710         | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| K4             | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| K3S            | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| K3             | KIO3 required     | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| KX3            | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| KX2            | Yes               | USB Audio CODEC          | 48 kHz      | 16-bit    | 2        |
| TS-890S        | Yes               | USB AUDIO CODEC          | 48 kHz      | 16-bit    | 2        |
| TS-590S/SG     | Yes               | USB AUDIO CODEC          | 48 kHz      | 16-bit    | 2        |
| TS-990S        | Yes               | USB AUDIO CODEC          | 48 kHz      | 16-bit    | 2        |

All devices are USB Audio Class 1.0 compliant (driverless on modern OSes).

---

## 2. Rust Audio Crate Landscape

### 2.1 cpal (Cross-Platform Audio Library)

**Version**: 0.17.1 (released 2026-01-04)
**License**: Apache-2.0
**Repository**: https://github.com/RustAudio/cpal
**Downloads**: 8.7M+ total
**MSRV**: Rust 1.82

**This is the clear winner for riglib.** It is the most mature, most widely used,
and best maintained Rust audio crate. All other audio crates in the Rust ecosystem
either build on top of cpal or are less capable.

#### Platform Backends

| Platform     | Backend       | Build Dependency                                     |
|-------------|---------------|------------------------------------------------------|
| Linux/BSD   | ALSA (default)| `libasound2-dev` (Debian/Ubuntu), `alsa-lib-devel` (Fedora) |
| Linux/BSD   | JACK (optional)| JACK development libraries                          |
| macOS/iOS   | CoreAudio     | Xcode toolchain                                      |
| Windows     | WASAPI (default)| None (uses `windows` crate)                        |
| Windows     | ASIO (optional)| LLVM/Clang + ASIO SDK                               |
| Android     | AAudio        | NDK                                                  |
| Web         | WebAudio      | wasm-bindgen                                         |

#### PipeWire Compatibility

cpal does not have a native PipeWire backend. However, PipeWire on modern Linux
distributions provides compatibility layers for both ALSA and JACK, so cpal works
with PipeWire through those shims transparently. The PipeWire developers explicitly
recommend using existing ALSA/JACK APIs rather than the native PipeWire API for
applications that already work through those backends.

**Source**: [cpal PipeWire issue #554](https://github.com/RustAudio/cpal/issues/554)

#### API Model: Callback-Based

cpal uses a **callback model**, not async streams. The application provides a closure
that receives audio data (for input) or fills a buffer (for output). The callback
runs on a high-priority audio thread managed by the OS audio backend.

Key types:
```rust
// Device enumeration
let host = cpal::default_host();
let devices = host.devices()?;              // all devices
let input = host.default_input_device()?;   // default mic
let output = host.default_output_device()?; // default speaker

// Device info
let name: String = device.name()?;
let configs = device.supported_input_configs()?;

// Stream creation (callback-based)
let stream = device.build_input_stream(
    &config,
    move |data: &[f32], _info: &InputCallbackInfo| {
        // Process audio samples here -- runs on audio thread
    },
    |err| { eprintln!("stream error: {}", err); },
    None, // timeout
)?;
stream.play()?;
```

#### Bridging cpal Callbacks to Tokio Async

Since riglib is async (tokio), and cpal is callback-based, we need a bridge.
The standard approach is:

1. **Ring buffer / bounded channel**: The cpal callback writes samples into a
   `tokio::sync::mpsc` channel or a lock-free ring buffer.
2. **Async receiver**: The riglib audio stream reads from the channel using
   `async`/`.await`.
3. **Direction**:
   - RX audio: cpal input callback -> mpsc::Sender -> async Receiver
   - TX audio: async Sender -> mpsc::Receiver -> cpal output callback

A `tokio::sync::mpsc::channel` with a bounded capacity (e.g., 4-8 audio buffers
worth of samples) provides backpressure without blocking the audio thread. The
cpal callback should use `try_send()` to avoid blocking. If the consumer falls
behind, samples are dropped (preferable to blocking the audio thread and causing
underruns).

For the reverse direction (TX audio), the cpal output callback uses `try_recv()`
to pull samples. If no samples are available, it fills the buffer with silence.

#### USB Audio Device Support

cpal has no USB-specific code -- it relies on the OS audio subsystem (ALSA,
CoreAudio, WASAPI) to enumerate and manage USB audio devices. USB Audio Class 1.0
devices appear as normal audio devices in all three backends. This means:

- USB audio devices are enumerated alongside built-in audio devices
- Device names come from the OS audio subsystem (may differ across platforms)
- No special configuration needed for USB audio

Known issue: On Linux with ALSA, device enumeration can sometimes miss USB devices
that were plugged in after the program started. A re-enumeration is needed.
(See [cpal issue #357](https://github.com/RustAudio/cpal/issues/357))

#### License Compatibility

cpal is Apache-2.0. riglib is MIT. Apache-2.0 is a permissive license compatible
with MIT projects -- an MIT-licensed project can depend on Apache-2.0 code without
issues. The resulting binary simply needs to include the Apache-2.0 notice.

### 2.2 rodio

**Version**: 0.21.1
**License**: MIT/Apache-2.0
**Repository**: https://github.com/RustAudio/rodio
**Downloads**: 5.3M+ total
**MSRV**: Rust 1.87

rodio is a **playback-only** library built on top of cpal. It provides a high-level
API for playing audio files (WAV, MP3, FLAC, Vorbis) with mixing and effects.

**Not suitable for riglib** because:
- Playback only -- no audio input/recording support
- Higher-level abstraction than we need (we want raw PCM samples)
- Adds unnecessary dependencies (audio decoders)
- We would still need cpal directly for input

### 2.3 portaudio-rs / rust-portaudio

**portaudio-rs**: Last updated ~2020, 36K downloads total.
**rust-portaudio**: In **maintenance mode** -- maintainers recommend using cpal instead.

Both are bindings to the C PortAudio library. **Not recommended** because:
- Requires native C library (PortAudio) as a build dependency
- Maintenance mode / abandoned
- cpal is pure Rust and actively maintained

**Sources**:
- [rust-portaudio GitHub](https://github.com/RustAudio/rust-portaudio)
- [portaudio-rs crates.io](https://crates.io/crates/portaudio-rs)

### 2.4 Other Notable Crates

| Crate          | Notes                                            | Suitable? |
|---------------|--------------------------------------------------|-----------|
| `hound`       | WAV file reading/writing. Pure Rust. Useful for saving/loading audio files. | Yes (for WAV I/O) |
| `dasp`        | Digital Audio Signal Processing library. Sample format conversion, interpolation. | Maybe (for format conversion) |
| `ringbuf`     | Lock-free SPSC ring buffer. Could bridge cpal callbacks to async. | Maybe |

### 2.5 Recommendation

**Use cpal 0.17 as the audio backend, behind an `audio` feature flag.** Optionally
depend on `hound` for WAV file I/O in the test application.

---

## 3. USB Audio Device to Rig Association

This is the most challenging UX problem in Phase 4. When a user has multiple rigs
connected via USB, each rig presents both serial ports and an audio device. The
question is: given a serial port `/dev/ttyUSB0` connected to an IC-7610, which of
the system's audio devices is the IC-7610's USB Audio CODEC?

### 3.1 The Core Problem

All USB-connected transceivers present their audio device with a nearly identical
generic name: "USB Audio CODEC". When multiple rigs are connected, there may be
multiple devices with the same name. The OS appends a disambiguator (e.g.,
"USB Audio CODEC #2") but the mapping to a specific rig is not obvious.

### 3.2 Linux: udev and USB Topology

On Linux, the most reliable approach uses the USB device tree. Devices on the same
internal USB hub share a common device path prefix.

**Example**: An IC-7610 on USB bus 1, port 4 might have:
```
/sys/devices/pci0000:00/.../usb1/1-4/1-4.1/   -> audio device
/sys/devices/pci0000:00/.../usb1/1-4/1-4.2/   -> serial port (CI-V)
/sys/devices/pci0000:00/.../usb1/1-4/1-4.3/   -> serial port (RTTY)
```

All three share the prefix `1-4`, which is the external USB port the rig is
connected to. By examining the sysfs path of both the serial port and the ALSA
audio device, we can determine which audio device is on the same USB hub.

**Implementation approach**:
1. Given a serial port path (e.g., `/dev/ttyUSB0`), resolve its sysfs path
   via `/sys/class/tty/ttyUSB0/device/../../..` to find the USB parent device.
2. Extract the bus-port path prefix (e.g., `1-4`).
3. Enumerate ALSA audio devices, find the one whose sysfs path shares the same
   bus-port prefix.

This is the approach wfview uses. wfview's "Auto" serial port selection
automatically detects OEM Icom radios by walking the USB device tree.

**Sources**:
- [wfview Serial Port Management](https://wfview.org/wfview-user-manual/serial-port-management/)
- [dh1tw Persistent USB Mapping of Audio Devices](https://github.com/dh1tw/remoteAudio/wiki/Persistent-USB-Mapping-of-Audio-devices-(Linux))
- [Linux udev rules for USB serial devices](https://dev.to/enbis/how-udev-rules-can-help-us-to-recognize-a-usb-to-serial-device-over-dev-tty-interface-pbk)

### 3.3 macOS: CoreAudio Device UID

On macOS, the USB Audio class driver sets the `kAudioDevicePropertyDeviceUID`
property in the format:
```
AppleUSBAudioEngine:<manufacturer>:<device_name>:<serial_or_location>:<interfaces>
```

The `serial_or_location` component comes from the IOUSBHostDevice service and
encodes the physical USB port location. By querying this property for audio
devices and matching the location component against the serial port's USB
location, we can associate them.

The process:
1. Enumerate CoreAudio devices, extract `kAudioDevicePropertyDeviceUID`.
2. Parse the location ID from the UID string.
3. Enumerate serial ports, find matching USB location IDs.

This is more complex than Linux but feasible.

**Source**: [Apple Developer Forums - Convert CoreAudio AudioObjectID](https://developer.apple.com/forums/thread/801522)

### 3.4 Windows: WMI PnP Device Path

On Windows, WMI (Windows Management Instrumentation) can query USB device topology.
Both serial ports and audio devices have PnP device IDs that encode the USB hub and
port information.

The approach:
1. Query `Win32_PnPEntity` for serial port devices, extract the USB device path.
2. Query `Win32_PnPEntity` for audio devices, extract USB device paths.
3. Match devices sharing the same USB parent hub.

Windows also assigns "Container IDs" to USB composite devices, which group
multiple interfaces (serial + audio) from the same physical device. This is
potentially the most reliable matching mechanism on Windows.

**Source**: [Microsoft Learn - How USB Devices are Assigned Container IDs](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/how-usb-devices-are-assigned-container-ids)

### 3.5 How Existing Software Solves This

**WSJT-X**: Does not solve it. Users manually select the audio device and the
serial port independently. No automatic association.

**fldigi**: Same -- manual configuration of audio device and rig control.

**wfview**: On Linux, uses USB topology (sysfs) to auto-detect Icom radios and
associate their serial ports and audio devices. This is the most sophisticated
solution in the ham radio ecosystem.

**SDR Console**: Manual configuration.

**Conclusion**: No mainstream ham radio software automatically associates USB audio
devices with serial ports cross-platform. wfview does it on Linux for Icom only.

### 3.6 Practical Recommendation for riglib

**Phase 4 should require manual audio device specification**, with an optional
auto-detection helper:

1. **Primary API**: The builder accepts an explicit audio device name string.
   ```rust
   IcomBuilder::new(IcomModel::IC7610)
       .serial_port("/dev/ttyUSB0")
       .audio_device("USB Audio CODEC")  // explicit
       .build().await?
   ```

2. **Helper function**: A platform-specific utility that, given a serial port path,
   attempts to find the associated audio device using USB topology.
   ```rust
   // Linux only initially
   let audio_device = riglib::audio::find_associated_audio_device("/dev/ttyUSB0")?;
   ```

3. **Enumeration**: Expose cpal's device enumeration so users can list available
   audio devices and pick the right one.
   ```rust
   let devices = riglib::audio::list_audio_devices()?;
   for d in &devices { println!("{}", d.name); }
   ```

This matches how every other ham radio application works (manual selection) while
leaving the door open for smarter auto-detection later. The Linux auto-detection
via USB topology can be implemented first since we have hardware to test with.

---

## 4. FlexRadio DAX Audio

### 4.1 DAX Architecture Overview

FlexRadio radios do not use USB audio at all. Instead, they stream demodulated
audio over the network using VITA-49 encapsulated packets on UDP port 4991. This
system is called DAX (Digital Audio eXchange).

DAX audio is fundamentally different from USB audio:
- **Network-based**: UDP packets, not OS audio devices
- **Integrated with the API**: Controlled via the same TCP command channel (port 4992)
- **Per-slice**: Each DAX channel can be associated with a specific slice receiver
- **Bidirectional**: Supports both RX audio (from radio) and TX audio (to radio)

### 4.2 DAX Audio Format

| Parameter      | Value                  |
|----------------|------------------------|
| Sample Rate    | 24,000 Hz (24 ksps)   |
| Bit Depth      | 32-bit IEEE 754 float  |
| Channels       | 2 (stereo)             |
| Byte Order     | Little-endian (within big-endian VITA-49 framing) |
| Encapsulation  | VITA-49.0 Extension Data packets |
| Packet Class   | 0x03E3                 |
| Transport      | UDP port 4991          |

The native DAX sample rate is always 24 ksps regardless of the slice's IF bandwidth.
The radio internally resamples as needed. This is lower than the 48 kHz used by
USB audio -- consumers that need 48 kHz (e.g., for compatibility with digital mode
software) must upsample.

### 4.3 DAX TCP Commands

Based on the SmartSDR TCP/IP API documentation:

**Create an RX audio stream**:
```
C<seq>|stream create type=dax_rx dax_channel=<1-8>
```
Response includes the stream handle (hex), which is also the VITA-49 stream ID.

**Create a TX audio stream**:
```
C<seq>|stream create type=dax_tx
```
Only one TX stream per client. The stream handle in the response is used as the
VITA-49 stream ID when sending TX audio packets to the radio.

**Create a microphone audio stream**:
```
C<seq>|stream create type=dax_mic
```
Captures microphone audio from the radio.

**Remove a stream**:
```
C<seq>|stream remove <stream_handle>
```

**Associate a DAX channel with a slice**:
```
C<seq>|slice set <slice_index> dax=<channel>
```
Where `<channel>` is 1-8, or 0 to disconnect.

**Enable DAX TX**:
```
C<seq>|dax tx <T|F|0|1>
```
OR:
```
C<seq>|transmit set dax=<0|1>
```

**Configure DAX audio channel** (alternative syntax):
```
C<seq>|dax audio set <channel> slice=<slice> tx=<0|1>
```

**Sources**:
- [SmartSDR TCPIP API - stream](https://github.com/flexradio/smartsdr-api-docs/wiki/TCPIP-stream)
- [SmartSDR TCPIP API - dax](https://github.com/flexradio/smartsdr-api-docs/wiki/TCPIP-dax)
- [FlexRadio Community - DAX](https://community.flexradio.com/discussion/8024999/dax)
- [FlexRadio Community - TX Audio](https://community.flexradio.com/discussion/7789005/using-txaudio-class-to-send-iq-samples-to-flex-6500-to-transmit)

### 4.4 DAX RX Audio Flow

1. Client sends: `stream create type=dax_rx dax_channel=1`
2. Radio responds with stream handle (e.g., `0x20000001`)
3. Client sends: `slice set 0 dax=1` (associate DAX channel 1 with slice 0)
4. Radio begins sending VITA-49 packets with stream_id=0x20000001 to UDP port 4991
5. Client receives packets, parses header (class code 0x03E3 = DaxAudio), extracts
   float32 stereo samples from payload

### 4.5 DAX TX Audio Flow

1. Client sends: `stream create type=dax_tx`
2. Radio responds with stream handle
3. Client sends: `transmit set dax=1` to enable DAX TX
4. Client constructs VITA-49 packets with the assigned stream handle, fills payload
   with float32 stereo samples at 24 ksps, sends to radio's UDP port 4991
5. When PTT is asserted, the radio transmits the DAX audio

The TX audio VITA-49 packet format is the same as RX: float32 stereo at 24 ksps,
little-endian samples, big-endian VITA-49 framing.

### 4.6 Current riglib Support

The existing code already handles much of the DAX audio infrastructure:

- **`vita49.rs`**: Already parses DAX audio packets (`StreamType::DaxAudio`, class
  code `0x03E3`). The `parse_dax_audio_payload()` function extracts `AudioSample`
  structs with `left: f32` and `right: f32` fields. This is complete and tested.

- **`client.rs`**: The `start_udp_receiver()` method binds UDP port 4991 and
  receives VITA-49 packets. Currently it only processes `StreamType::MeterData` --
  the `DaxAudio` match arm is a no-op (`_ => { /* Other stream types not handled */ }`).
  Adding DAX audio handling here is straightforward.

- **Missing**: Stream creation commands (`stream create type=dax_rx`), DAX channel
  configuration, TX audio packet construction and sending, and the async stream
  interface to expose audio samples to consumers.

### 4.7 Latency Considerations

DAX audio has inherent network latency:
- VITA-49 packet assembly: ~21ms per packet (512 samples at 24 ksps)
- Network transit: <1ms on LAN
- Buffering on receive side: implementation-dependent

Total one-way latency is typically 25-50ms on a local network. This is acceptable
for digital modes (FT8 has multi-second timing) but may be noticeable for CW
sidetone or voice monitoring. FlexRadio's own SmartSDR client has similar latency.

**Source**: [FlexRadio DAX Audio Specifications](https://community.flexradio.com/discussion/6346756/what-are-the-audio-stream-specifications-for-dax-and-how-do-they-compare-to-vac-in-the-3000-5000)

---

## 5. AudioStream Trait Design

### 5.1 Design Constraints

1. **Two fundamentally different backends**: USB audio (via cpal, callback-based,
   OS audio device) and FlexRadio DAX (network, VITA-49, integrated with TCP API).
2. **riglib is async (tokio)**: Consumers expect `async`/`.await` APIs.
3. **cpal is callback-based**: Must bridge to async.
4. **Sample formats differ**: USB audio is typically i16; DAX is f32.
5. **Sample rates differ**: USB audio is 48 kHz; DAX is 24 kHz.
6. **Not all rigs support audio**: The Rig trait should not require audio.

### 5.2 Proposed Design: Separate from Rig Trait

Audio streaming should NOT be part of the `Rig` trait. Rationale:
- Not all rigs support it (especially older models without USB audio, or rigs
  connected via CI-V level converter without audio)
- USB audio requires an OS audio device, which is a completely different resource
  than the serial port used for rig control
- FlexRadio DAX is conceptually part of the same connection, but still involves
  separate UDP streams

Instead, audio should be a **separate trait** that rig types can optionally
implement, or a standalone struct created alongside the rig:

```rust
/// Audio configuration for a stream.
#[derive(Debug, Clone)]
pub struct AudioStreamConfig {
    /// Sample rate in Hz (typically 48000 for USB, 24000 for DAX).
    pub sample_rate: u32,
    /// Number of channels (typically 2 for stereo).
    pub channels: u16,
    /// Sample format.
    pub sample_format: AudioSampleFormat,
}

/// Supported sample formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSampleFormat {
    /// 32-bit IEEE 754 floating point, range [-1.0, 1.0].
    F32,
    /// 16-bit signed integer.
    I16,
}

/// A chunk of audio samples.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Interleaved samples in f32 format.
    /// For stereo: [L0, R0, L1, R1, ...].
    pub samples: Vec<f32>,
    /// Number of channels.
    pub channels: u16,
    /// Sample rate of this buffer.
    pub sample_rate: u32,
}

/// Receiver for RX audio from the rig.
pub struct AudioReceiver {
    rx: tokio::sync::mpsc::Receiver<AudioBuffer>,
    config: AudioStreamConfig,
}

impl AudioReceiver {
    /// Receive the next audio buffer. Returns None when the stream ends.
    pub async fn recv(&mut self) -> Option<AudioBuffer> {
        self.rx.recv().await
    }

    /// The stream configuration (sample rate, channels, format).
    pub fn config(&self) -> &AudioStreamConfig {
        &self.config
    }
}

/// Sender for TX audio to the rig.
pub struct AudioSender {
    tx: tokio::sync::mpsc::Sender<AudioBuffer>,
    config: AudioStreamConfig,
}

impl AudioSender {
    /// Send an audio buffer for transmission. Returns Err if the stream
    /// has been closed.
    pub async fn send(&self, buf: AudioBuffer) -> Result<()> {
        self.tx.send(buf).await.map_err(|_| Error::StreamClosed)
    }

    /// The expected stream configuration.
    pub fn config(&self) -> &AudioStreamConfig {
        &self.config
    }
}
```

### 5.3 Rig Integration: AudioCapable Trait

Rather than modifying the existing `Rig` trait, add a marker/extension trait:

```rust
/// Trait for rigs that support audio streaming.
///
/// Not all rigs support audio (depends on connection type and model).
/// Use `rig.audio_supported()` or attempt a downcast to check.
#[async_trait]
pub trait AudioCapable: Rig {
    /// Start receiving RX audio from the specified receiver.
    ///
    /// Returns an AudioReceiver that yields audio buffers asynchronously.
    /// For USB audio rigs, this opens the OS audio input device.
    /// For FlexRadio, this creates a DAX RX stream.
    async fn start_rx_audio(
        &self,
        rx: ReceiverId,
        config: Option<AudioStreamConfig>,
    ) -> Result<AudioReceiver>;

    /// Start sending TX audio to the rig.
    ///
    /// Returns an AudioSender for submitting audio buffers.
    /// The rig must be in the correct mode and PTT asserted separately.
    async fn start_tx_audio(
        &self,
        config: Option<AudioStreamConfig>,
    ) -> Result<AudioSender>;

    /// Stop all audio streams.
    async fn stop_audio(&self) -> Result<()>;

    /// Query whether audio streaming is available in the current configuration.
    fn audio_supported(&self) -> bool;
}
```

### 5.4 Why Async Channels Over Callbacks

Callbacks (like cpal's native API) are efficient but create API friction in an
async codebase. Using `tokio::sync::mpsc` channels as the public API has these
advantages:

1. **Natural async integration**: `receiver.recv().await` works in any async context.
2. **Composable**: Can be selected/joined with other futures.
3. **Backpressure**: Bounded channels prevent unbounded memory growth.
4. **Thread safety**: mpsc channels are Send + Sync.

The cpal callback thread pushes into the channel; the async consumer pulls from it.
The small overhead of the channel (one allocation per buffer, one atomic operation
per send/recv) is negligible at audio rates (48000 samples/sec = ~94 buffers/sec
with 512-sample buffers).

### 5.5 Sample Format Normalization

The trait should normalize all audio to **f32** at the public API boundary:

- USB audio (i16) is converted to f32 in the cpal callback before sending to the channel.
- FlexRadio DAX audio is already f32.
- Conversion: `i16_sample as f32 / 32768.0`

This simplifies consumer code -- they always work with f32 regardless of backend.
The `AudioStreamConfig` reports the actual sample rate (which differs: 48 kHz vs
24 kHz), and consumers can resample if needed.

### 5.6 What About Format Conversion?

Should riglib provide sample rate conversion (24 kHz <-> 48 kHz)?

**Recommendation: No, not in Phase 4.** Sample rate conversion is a substantial
DSP task (requires a resampling filter). Consumers that need a specific sample rate
(like 48 kHz for WSJT-X) should use a dedicated resampling crate (`rubato`,
`samplerate`, or `dasp`). riglib should provide samples at the native rate and
document the rate in `AudioStreamConfig`.

---

## 6. Practical Recommendations

### 6.1 Minimum Viable Feature Set

The Phase 4 MVP should support:

1. **RX audio capture from USB audio devices** (Icom, Yaesu, Kenwood, Elecraft)
2. **RX audio capture from FlexRadio DAX**
3. **Audio device enumeration**
4. **Manual audio device selection** (by name)
5. **Save captured audio to WAV file** (in test app)

TX audio can be deferred to a follow-up. Rationale:
- RX audio has clear immediate use cases (digital mode decoding, audio recording)
- TX audio requires careful PTT coordination and is riskier to test
- Most digital mode software handles TX audio independently
- Implementing RX first validates the architecture without TX complexity

### 6.2 Feature Flag Strategy

```toml
[features]
audio = ["dep:cpal"]
```

The `audio` feature enables:
- `AudioCapable` trait in riglib-core
- USB audio implementation in each manufacturer crate
- cpal as a dependency (only when audio feature is enabled)

FlexRadio DAX audio should NOT require the `audio` feature flag, since it uses
no OS audio devices -- it is purely network-based. The DAX `AudioReceiver` and
`AudioSender` are always available in riglib-flex.

### 6.3 Unified vs Separate Traits

**Recommendation: Unified trait, separate implementations.**

The `AudioCapable` trait and `AudioReceiver`/`AudioSender` types should be the same
regardless of whether the backend is cpal (USB audio) or DAX (network). Consumers
get an `AudioReceiver` and call `.recv().await` without caring about the transport.

However, the implementations are completely different:
- USB audio: Opens cpal device, runs callback, bridges to mpsc channel
- FlexRadio DAX: Sends TCP stream-create command, receives VITA-49 UDP packets,
  parses audio, sends to mpsc channel

### 6.4 What Can Be Tested Without Hardware

A surprising amount:

- **AudioCapable trait definition**: Compiles and is documented
- **AudioReceiver/AudioSender/AudioBuffer types**: Pure Rust, no I/O
- **cpal device enumeration**: Works on any computer with audio devices (even
  built-in mic/speakers)
- **FlexRadio DAX stream creation**: Mock TCP server + mock UDP sender (extending
  existing test infrastructure in client.rs)
- **VITA-49 DAX audio parsing**: Already tested with synthetic packets
- **i16-to-f32 conversion**: Trivial unit test
- **WAV file writing**: Pure Rust (hound crate)
- **Channel bridging logic**: Mock producer + async consumer

Hardware is only needed for:
- Verifying that the correct USB audio device is opened (device name matching)
- End-to-end RX audio quality validation
- TX audio validation (requires dummy load + monitoring)

### 6.5 Revised Work Item Structure

The original WI-4.1 through WI-4.4 should be revised as follows:

1. **WI-4.1**: Core audio types + AudioCapable trait (riglib-core) -- no I/O
2. **WI-4.2**: cpal USB audio backend -- cpal integration, device enumeration, RX audio
3. **WI-4.3**: FlexRadio DAX audio -- stream commands, VITA-49 audio routing, RX audio
4. **WI-4.4**: TX audio for both backends
5. **WI-4.5**: Test application v4 -- audio commands, WAV recording
6. **WI-4.6**: USB audio device association helper (Linux first)

This separates the purely mechanical work (types, traits) from the I/O integration
work (cpal, DAX) and puts TX audio as its own step rather than mixed with RX.

### 6.6 Reasons to Simplify

Phase 4 can be simplified by:

1. **Deferring TX audio**: RX-only is significantly simpler and covers the primary
   use cases. TX can be Phase 4b or Phase 5.
2. **Deferring device auto-association**: Manual device selection is fine for v1.
   Auto-detection can be a later enhancement.
3. **Deferring sample rate conversion**: Let consumers handle resampling.
4. **Not worrying about FlexRadio Opus audio**: The 0x8005 Opus audio stream is
   for compressed remote-operation audio, not for digital modes. DAX audio (0x03E3)
   is the right path and is already parsed.

### 6.7 Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| cpal device enumeration misses USB devices | Low | Medium | Re-enumerate on demand; document |
| Multiple rigs with same audio device name | Medium | High | Manual selection; auto-detect later |
| FlexRadio DAX latency too high for CW | Medium | Low | Acceptable for digital modes; document |
| cpal callback thread blocks on channel send | Low | High | Use try_send(); drop samples if full |
| No FlexRadio test hardware | Certain | Medium | Mock server tests; defer hardware validation |
| Cross-platform audio differences | Medium | Medium | Test on all three platforms |

---

## 7. References

### Manufacturer Documentation
- [Icom CI-V Reference Guide](https://static.dxengineering.com/global/images/technicalarticles/ico-ic-7610_yj.pdf)
- [W7AY IC-7610 Discovery](https://www.w7ay.net/site/Applications/IC-7610/Contents/Discover.html)
- [Kenwood TS-890S USB Audio Manual](https://www.kenwood.com/i/products/info/amateur/ts_890/pdf/ts890_usb_audio_manual_e.pdf)
- [Elecraft K4 Operating Manual](https://ftp.elecraft.com/K4/Manuals%20Downloads/K4%20Built-In%20Operating%20Manual%20rev%20D6/Operating%20Manual%20Rev%20D6%20printable.pdf)
- [FlexRadio SmartSDR API Wiki](https://github.com/flexradio/smartsdr-api-docs/wiki)
- [FlexRadio SmartSDR TCP/IP API](https://github.com/flexradio/smartsdr-api-docs/wiki/SmartSDR-TCPIP-API)

### Rust Crates
- [cpal 0.17.1 - GitHub](https://github.com/RustAudio/cpal)
- [cpal - docs.rs](https://docs.rs/cpal/latest/cpal/)
- [cpal - crates.io](https://crates.io/crates/cpal)
- [rodio - GitHub](https://github.com/RustAudio/rodio)
- [hound - WAV library](https://crates.io/crates/hound)

### Community / Third-Party
- [wfview Serial Port Management](https://wfview.org/wfview-user-manual/serial-port-management/)
- [wfview Audio Configuration](https://wfview.org/wfview-user-manual/audio-configuration/)
- [flexlib-go - GitHub](https://github.com/hb9fxq/flexlib-go)
- [dh1tw Persistent USB Mapping](https://github.com/dh1tw/remoteAudio/wiki/Persistent-USB-Mapping-of-Audio-devices-(Linux))
- [WSJT-X User Guide](https://wsjt.sourceforge.io/wsjtx-doc/wsjtx-main-2.6.1.html)
- [FlexRadio DAX Audio Specs Discussion](https://community.flexradio.com/discussion/6346756/what-are-the-audio-stream-specifications-for-dax-and-how-do-they-compare-to-vac-in-the-3000-5000)
- [MicroHAM DXP - UAC 1.0 Reference](https://www.microham.com/contents/en-us/d206_DXP.html)
