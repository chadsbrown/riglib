# Auxiliary Keying Port Support

**Status:** Not started
**Priority:** Low — assess after real-world contest validation
**Depends on:** Phases A-E complete (IO task architecture stable)

---

## Problem Statement

Currently, each `Rig` object owns a single transport. When the operator configures `PttMethod::Dtr` or `KeyLine::Rts`, the IO task toggles DTR/RTS on that same serial port that carries CAT commands. This covers the common case — most radios expose DTR/RTS on the same port as CAT.

However, some contest stations use a **separate physical port** for keying:

- **External keying adapter** (MicroHAM, RigExpert, plain FTDI breakout) wired to the radio's KEY/PTT jacks, while CAT goes over the radio's built-in USB
- **ACC port + USB** — radio's USB port for CAT, DB-9 or DIN ACC port for PTT/CW via a separate serial adapter
- **Dedicated CW interface** — WinKeyer or similar on its own USB port (out of scope — these have their own protocol, not just DTR/RTS)

riglib cannot currently address this. The builder has no option for a secondary keying transport.

---

## Assessment: Is This Actually Needed?

Several aspects of riglib's architecture reduce the urgency of this feature.

### RT Priority Already Minimizes Keying Latency

The IO task uses biased select with RT priority (Phase B). `SetLine` requests for DTR/RTS are routed through the RT channel and serviced at the next select point — they are never queued behind background polling. The only wait is for an in-flight CI-V or text-protocol frame to complete, which is typically 5-15ms on CI-V and 3-10ms on text protocols. This is well within acceptable keying latency for contest CW at any speed.

### CAT PTT Is the Common Default

Most modern radios support PTT via CAT command (`PttMethod::Cat`), which is the riglib default. CAT PTT goes through the same transport as other commands — no second port needed. DTR/RTS PTT is primarily used for:

- Radios without CAT PTT support (rare in 2010+ models)
- Operators who want DTR/RTS for QSK break-in timing (the radio's T/R relay responds faster to a hardware signal than to a CI-V command on some models)
- Legacy station wiring where PTT lines are already routed to a specific port

### CW Keying Is Predominantly via WinKeyer, Not riglib

In competitive CW contesting, the K1EL WinKeyer is the overwhelmingly dominant keying solution. Every serious contest logger (N1MM+, Win-Test, DXLog) has deep WinKeyer integration. WinKeyer is dedicated hardware that handles CW timing, weighting, speed control (physical knob + paddle input), and serial number generation — it connects via USB with its own protocol and keys the radio's KEY jack directly. This is entirely outside riglib's scope.

riglib's CW keying paths serve secondary roles:

- **`send_cw_message()` (CAT)** — sends text to the radio's built-in keyer (Icom CI-V cmd 0x17, Kenwood `KY;`, Yaesu `KM;`). Convenient for casual use or digital modes, but the radio's keyer has limited buffer depth and no paddle interleaving. No second port relevant — it's a CAT command.
- **`KeyLine::Dtr` / `KeyLine::Rts` (hardware)** — bit-bangs CW via serial line toggling. Used by software keyers or SO2R microcontrollers that switch keying lines between radios. This is where the auxiliary port question applies.

Since most contest CW keying bypasses riglib entirely (via WinKeyer), the auxiliary keying port is primarily relevant for **PTT** (where CAT PTT or same-port DTR/RTS usually suffices) and for niche hardware keying setups.

### Dual-USB Radios Don't Require Dual Ports

Radios like the IC-7610 and FT-DX10 expose two USB serial ports, but both carry the same protocol (CI-V or CAT). You pick one for riglib and DTR/RTS keying works on that same port. The second port is typically used by a second application (e.g., WSJT-X on port A, logger on port B) or for audio codec routing — not for separating CAT from keying.

---

## When It Would Be Needed

Despite the above, there are real scenarios where a separate keying port matters:

1. **External keying adapter is a different physical device.** The operator has a $10 FTDI cable wired to the radio's rear-panel KEY jack. CAT goes over the radio's USB. These are two different `/dev/ttyUSB*` devices. riglib cannot use the FTDI cable for DTR/RTS today.

2. **Latency-critical QSK at extreme speeds.** At 40+ WPM with full QSK, even 5-15ms of additional latency from waiting for an in-flight CI-V frame can clip the first dit. A dedicated keying port with no protocol framing has near-zero latency. (In practice, the radio's T/R relay switching time dominates, but the argument exists.)

3. **SO2R with shared keying hardware.** A MicroHAM Station Master or similar device manages PTT/CW routing between two radios. It presents as a serial port to the PC. riglib would need to toggle DTR/RTS on that device, not on the radio's CAT port.

---

## Proposed Design (When Implemented)

### Builder API

```rust
IcomRigBuilder::new(Model::Ic7610)
    .serial_port("/dev/ttyUSB0")       // CAT transport (existing)
    .keying_port("/dev/ttyUSB1")       // auxiliary keying transport (new)
    .ptt_method(PttMethod::Dtr)
    .key_line(KeyLine::Rts)
    .build()
```

If `keying_port` is set, `SetLine` requests route to the auxiliary transport instead of the CAT transport. If not set, behavior is unchanged (SetLine goes to the CAT transport).

### IO Task Changes

- The IO task holds an `Option<Box<dyn Transport>>` for the auxiliary keying port
- `SetLine` requests check: if aux transport exists, use it; otherwise use primary
- The auxiliary transport needs no framing, buffering, or protocol parsing — just `set_dtr()` / `set_rts()`
- Since there is no protocol interaction on the keying port, `SetLine` does not need to wait for any in-flight frame — it can execute immediately, even during a CI-V transaction on the primary port
- This would make keying latency truly independent of CAT traffic

### Lifecycle

- The auxiliary transport is opened at build time, closed at drop time
- If the auxiliary port disconnects, emit a warning event but do not tear down the primary connection — CAT still works, only hardware keying is lost

### Scope

- Small change: ~50-100 lines across builder, IO task, and `RigIo` handle
- One new builder method per backend (or a shared method in a common builder trait)
- Tests: mock a second transport, verify SetLine routes to it

---

## Recommendation

**Defer until real-world contest validation surfaces a need.** The RT priority architecture already provides low-latency keying on the primary port. Validate with the IC-7610 and FT-DX10 during actual contest operation (Field Day, CQWW, etc.). If keying latency proves insufficient or an operator needs an external keying adapter, implement this as a small incremental change. The design is straightforward and does not require architectural changes — it's plumbing, not restructuring.

If implemented, estimate **1 session** of effort.
