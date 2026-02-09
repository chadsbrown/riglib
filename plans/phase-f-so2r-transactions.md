# Phase F — SO2R Atomic Sequences (Deferred)

**Depends on:** [Phase B](phase-b-priority-scheduling.md) (priority scheduling stable), hardware validation on multiple rigs
**Deliverable:** Atomic multi-command sequences for SO2R operations.
**Estimated effort:** 3-4 sessions (sub-phases F.1 through F.3)

---

## Why This Is Deferred

This phase should **not begin** until:

1. IO-task-only is stable on at least one backend (Icom)
2. Priority scheduling (RT/BG) is in place
3. Hardware validation has been done on real rigs

Atomic sequences are backend-specific, easy to implement incorrectly, and less valuable if base command latency is still unstable.

---

## Real SO2R Operating Scenarios

### Scenario 1: QSY + CQ on Radio 2

In CQWW CW, the operator finishes a QSO on Radio 1 and needs to CQ on Radio 2:

1. Set Radio 2 frequency to 14.025 MHz (BG)
2. Set Radio 2 mode to CW (BG)
3. Key Radio 2 PTT (RT)
4. Send CW message "CQ TEST K1XX" on Radio 2 (RT)
5. Unkey Radio 2 PTT when message completes (RT)
6. Switch audio focus to Radio 2 (external)

Steps 1-2 must complete atomically — no BG polling interleaving that could read a stale frequency.

### Scenario 2: Split Operation Setup

1. Enable split on Radio 1
2. Set TX frequency on VFO B to 14.035 MHz
3. Set RX frequency on VFO A to 14.025 MHz
4. Verify split state

If another BG poll reads frequency between steps 2 and 3, it gets confusing intermediate state.

### Scenario 3: TX Interlock

A physical SO2R box (microHAM, YCCC SO2R Box) provides hardware interlock. The software must also enforce: `set_ptt(true)` on Radio 1 should verify Radio 2 is in RX. This is application-layer logic, not IO-task logic, but the IO task must expose enough state for the decision.

---

## VFO A/B Semantics Per Manufacturer

| Manufacturer | VFO A | VFO B | Split TX | Dual RX |
|-------------|-------|-------|----------|---------|
| Icom IC-7610 | Main receiver | Sub receiver | Split uses separate VFOs | Full dual RX |
| Yaesu FTDX101D | VFO A | VFO B | Split TX on VFO B | Dual RX |
| Kenwood TS-890 | VFO A | VFO B | Split TX on VFO B | No dual RX |
| Elecraft K3/K4 | VFO A | VFO B | Split TX on VFO B | Sub RX (K3 option) |
| FlexRadio | Slice 0 | Slice 1 | TX slice designation | Up to 8 slices |

The `Sequence` request must abstract over these differences. The IO task executes raw commands; the rig-specific `Rig` trait implementation builds the correct command sequence.

---

## Sub-Phase F.1 — Add Sequence Request Variant

**Scope:** IO task plumbing. One session.

Add `Request::Sequence` to the IO task:

```rust
enum Request {
    Single { cmd: Vec<u8>, reply: oneshot::Sender<Response> },
    Sequence { cmds: Vec<Request>, reply: oneshot::Sender<Vec<Response>> },
    Shutdown { reply: oneshot::Sender<Box<dyn Transport>> },
}
```

IO task executes commands in order, blocking BG between steps but still checking RT.

### Definition of Done

- Send a sequence of 3 commands, verify they execute in order without BG interleaving.

---

## Sub-Phase F.2 — Rig-Level Atomic Operations

**Scope:** One session per backend. Depends on F.1.

Add default methods to the `Rig` trait:

- `set_frequency_and_mode(rx, freq, mode)` — builds a sequence
- `setup_split(tx_freq, rx_freq)` — builds a sequence

Each backend implements these using the correct command ordering for that manufacturer.

### Definition of Done

- Command sequence on the wire matches expected manufacturer-specific order.
- Tested per backend.

---

## Sub-Phase F.3 — TX Interlock Support

**Scope:** Application-layer safety. One session. Depends on F.1.

Add `TxInterlock` as a concept in riglib-core: a shared state that prevents both radios from transmitting. This wraps two `Rig` instances, not the IO task.

### Definition of Done

- `set_ptt(true)` on Radio 2 while Radio 1 is transmitting returns error or blocks.

---

## Protocol-Specific Gotchas

**Icom VFO selection is implicit.** On the IC-7610, you can't send `set_frequency(VFO_B, freq)` as one command. You send: (1) select sub receiver (0xD1), (2) set frequency (0x05 + BCD). If a BG command interleaves between steps 1 and 2, the frequency goes to the wrong VFO. Sequences must prevent this.

**Yaesu VFO selection is explicit.** `FA014025000;` sets VFO A directly. `FB014025000;` sets VFO B directly. No preceding selection needed. Yaesu sequences are simpler.

**Elecraft K3 "hold" commands.** `K31;` disables front-panel control during a sequence, preventing the operator from accidentally changing state mid-sequence. Consider exposing as an optional sequence wrapper.

**FlexRadio slice locking.** `slice lock` prevents other clients from modifying a slice — useful to prevent a panadapter from changing the slice the logger is using.

---

## Testing Scenarios

1. **Atomic frequency + mode change.** Send a sequence of `set_frequency` + `set_mode`. Simultaneously, BG task polls `get_frequency` at 50 Hz. Verify BG polls never see a state where frequency changed but mode has not.

2. **RT preemption during sequence.** Send a 5-step BG sequence. After step 2, inject an RT PTT command. Verify PTT executes between steps 2 and 3 (not after step 5).

3. **Two-radio interlock.** Create two IcomRig instances. Set PTT on rig 1. Attempt PTT on rig 2 with interlock. Verify rig 2 returns error or blocks.

---

## Definition of Done (Phase F — all sub-phases)

- Atomic sequences execute without BG interleaving
- RT preemption (PTT) still works during sequence execution
- TX interlock prevents simultaneous transmission
- Tested on real hardware for each manufacturer used in SO2R
