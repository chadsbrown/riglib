# Contest Logging Features Implementation Plan

## Context

riglib now has core rig control (frequency, mode, PTT, split, S-meter, power, SWR, ALC) and DTR/RTS hardware keying. To fully support a contest logger, it needs: CW speed control, VFO operations (A=B, swap), antenna port selection, AGC, preamp/attenuator, RIT/XIT, CW message sending, and transceive (AI) mode for Kenwood/Elecraft/Yaesu. This plan breaks the work into 14 phases, each completable in a single session.

---

## Phase 1: Core Types and Events ✅ DONE

Add new enums, capability fields, and event variants to `riglib-core`.

### Files to modify:
- **`crates/riglib-core/src/types.rs`** — Add enums:
  - `AgcMode { Off, Fast, Medium, Slow }`
  - `PreampLevel { Off, Preamp1, Preamp2 }`
  - `AttenuatorLevel { Off, Db6, Db12, Db18 }`
  - `AntennaPort { Ant1, Ant2, Ant3, Ant4 }`
  - Add fields to `RigCapabilities`: `agc_modes`, `preamp_levels`, `attenuator_levels`, `antenna_ports`, `has_rit`, `has_xit`, `has_cw_keyer`, `has_cw_messages`, `has_vfo_ab_swap`, `has_vfo_ab_equal`, `has_transceive`

- **`crates/riglib-core/src/events.rs`** — Add event variants:
  - `AgcChanged { receiver, mode }`, `CwSpeedChanged { wpm }`, `RitChanged { enabled, offset_hz }`, `XitChanged { enabled, offset_hz }`

- **`crates/riglib/src/lib.rs`** — Ensure new types are re-exported

### Verification: `cargo check --workspace`

---

## Phase 2: Rig Trait — Tiny Features + CW Speed ✅ DONE

Add trait methods for CW speed, VFO A=B, VFO swap, antenna port.

### Files to modify:
- **`crates/riglib-core/src/rig.rs`** — Add to `Rig` trait:
  - `get_cw_speed() -> Result<u8>` (WPM)
  - `set_cw_speed(wpm: u8) -> Result<()>`
  - `set_vfo_a_eq_b(receiver: ReceiverId) -> Result<()>`
  - `swap_vfo(receiver: ReceiverId) -> Result<()>`
  - `get_antenna(receiver: ReceiverId) -> Result<AntennaPort>`
  - `set_antenna(receiver: ReceiverId, port: AntennaPort) -> Result<()>`

### Verification: `cargo check --workspace` (will fail until impls added — expected)

---

## Phase 3: Tiny Features — Commands (All 5 Manufacturers) ✅ DONE

Add low-level command functions for CW speed, VFO A=B, VFO swap, antenna port.

### Files to modify:
- **`crates/riglib-icom/src/commands.rs`** — CI-V: CW speed `0x14 0x0C`, VFO A=B `0x07 0xA0`, VFO swap `0x07 0xB0`, Antenna `0x12`
- **`crates/riglib-yaesu/src/commands.rs`** — `KS`, `AB`, `SV`, `AN`
- **`crates/riglib-kenwood/src/commands.rs`** — `KS`, `VV`/`AB`, `EX`/`VR`, `AN`
- **`crates/riglib-elecraft/src/commands.rs`** — `KS`, `SW11`, `SW00`, antenna returns Unsupported
- **`crates/riglib-flex/src/commands.rs`** — `transmit set cw_speed=`, slice copy/swap, `slice set ANT=`

### Verification: `cargo check --workspace`

---

## Phase 4: Tiny Features — Rig Wiring (All 5 Manufacturers) ✅ DONE

Wire Phase 3 commands into each `Rig` trait implementation.

### Files to modify:
- **`crates/riglib-icom/src/rig.rs`** — Implement all 6 methods
- **`crates/riglib-yaesu/src/rig.rs`** — Same
- **`crates/riglib-kenwood/src/rig.rs`** — Same
- **`crates/riglib-elecraft/src/rig.rs`** — Same (antenna returns Unsupported)
- **`crates/riglib-flex/src/rig.rs`** — Same

### Verification: `cargo check --workspace`, `cargo test --workspace`

---

## Phase 5a: AGC — Trait Methods ✅ DONE

Add AGC get/set to the `Rig` trait.

### Files to modify:
- **`crates/riglib-core/src/rig.rs`** — Add to `Rig` trait:
  - `get_agc(receiver: ReceiverId) -> Result<AgcMode>`
  - `set_agc(receiver: ReceiverId, mode: AgcMode) -> Result<()>`

### Verification: `cargo check -p riglib-core`

---

## Phase 5b: AGC — Commands (All 5 Manufacturers) ✅ DONE

Add low-level AGC command functions. Each manufacturer uses different value encodings for AGC modes — verify all mappings against CAT reference documents before implementing.

### Files to modify:
- **`crates/riglib-icom/src/commands.rs`** — CI-V sub-command `0x16 0x12`. Values: `0x01`=fast, `0x02`=mid, `0x03`=slow. **Caveat:** Not all Icom models support AGC off (`0x00`). The command layer should accept all `AgcMode` variants but models that don't support off should return `Unsupported` — this gating can live in the rig layer (5c) based on model info, or the command layer can be model-aware. Decide during implementation.
- **`crates/riglib-yaesu/src/commands.rs`** — `GT` command. Yaesu encodes AGC as numeric time constants, not named modes. Values differ by model (e.g. FT-DX10: `GT00;`=off, `GT01;`=fast, `GT02;`=mid, `GT03;`=slow). **Must verify against each supported model's CAT reference** — do not assume all Yaesu models share the same mapping.
- **`crates/riglib-kenwood/src/commands.rs`** — `GT` command. Kenwood uses numeric values similar to Yaesu (shared CAT heritage) but the scale may differ. **Verify per-model mappings** (TS-590SG, TS-890S, etc.).
- **`crates/riglib-elecraft/src/commands.rs`** — `GT` command. K3 uses a different numeric scale from Kenwood: `GT000;`=off, `GT002;`=fast, `GT004;`=slow. **Verify K4 values match K3** — they may not.
- **`crates/riglib-flex/src/commands.rs`** — `slice set agc_mode=<mode>` where `<mode>` is a string (`off`, `fast`, `med`, `slow`). Maps cleanly to `AgcMode` enum. **Note:** The command must include the slice index derived from `ReceiverId` — confirm this uses the same `ReceiverId`-to-slice mapping as existing FlexRadio commands.

### Verification: `cargo check --workspace`

---

## Phase 5c: AGC — Rig Wiring (All 5 Manufacturers)

This phase is split into five sub-phases (5c-1 through 5c-5), one per manufacturer. Each sub-phase is self-contained, touches only one manufacturer's `rig.rs`, and ends with `cargo check --workspace`. They can be done in any order.

**Shared context for all sub-phases:**
- The `Rig` trait already has default `get_agc` / `set_agc` methods returning `Error::Unsupported` (added in Phase 5a).
- The `RigEvent::AgcChanged { receiver: ReceiverId, mode: AgcMode }` event already exists (added in Phase 1).
- Low-level command functions and parsers already exist in each manufacturer's `commands.rs` (added in Phase 5b).
- `AgcMode` enum: `Off`, `Fast`, `Medium`, `Slow` (in `riglib-core/src/types.rs`).
- **In scope:** emit `AgcChanged` from `set_agc()` on success. **Deferred to Phases 9-12:** emitting `AgcChanged` from transceive/unsolicited responses.
- Phase 1 added `agc_modes` to `RigCapabilities`, but it remains empty until Phase 13 populates it per model. This is expected.

---

### Phase 5c-1: AGC Rig Wiring — Icom ✅ DONE

Wire AGC commands into the Icom `Rig` trait implementation.

#### File to modify:
- **`crates/riglib-icom/src/rig.rs`**

#### Implementation details:

**`get_agc(rx: ReceiverId) -> Result<AgcMode>`:**
1. Call `self.select_receiver(rx).await?` (same pattern as `get_mode`, `get_frequency`).
2. Build command: `commands::cmd_read_agc_mode(self.civ_address)`.
3. Execute: `let frame = self.execute_command(&cmd).await?`.
4. Extract payload: `let data = Self::frame_payload(&frame)?`.
5. Parse: `let raw = commands::parse_agc_mode_response(&data)?`.
6. Map raw byte to `AgcMode`: `0x01` → `Fast`, `0x02` → `Medium`, `0x03` → `Slow`.
7. **SDR-generation AGC off detection:** For rigs where AGC off is controlled via time constant (IC-7610, IC-7300, IC-7300MK2, IC-705, IC-9700, IC-7850/51), if the raw mode byte is `0x01` (fast), additionally read the time constant via `commands::cmd_read_agc_time_constant(self.civ_address)` and check if the parsed value is `0x00` — if so, return `AgcMode::Off` instead of `Fast`. **Decision point:** determine which models need this two-step check. Consider adding a `supports_agc_time_constant: bool` field to `IcomModel`, or match on `self.model.name`.

**`set_agc(rx: ReceiverId, mode: AgcMode) -> Result<()>`:**
1. Call `self.select_receiver(rx).await?`.
2. For `AgcMode::Off` on SDR-generation rigs: use `commands::cmd_set_agc_time_constant(self.civ_address, 0x00)` (sets time constant to zero, disabling AGC). For `AgcMode::Off` on non-SDR rigs that lack time constant control: return `Error::Unsupported`.
3. For `Fast`/`Medium`/`Slow`: map to byte (`0x01`/`0x02`/`0x03`), build `commands::cmd_set_agc_mode(self.civ_address, byte)`, execute via `self.execute_ack_command(&cmd).await?`. On SDR-generation rigs, also ensure time constant is non-zero if switching from Off to an active mode.
4. On success: `let _ = self.event_tx.send(RigEvent::AgcChanged { receiver: rx, mode });`.

**Existing patterns to follow:** Look at `get_mode` / `set_mode` for the `select_receiver` → `execute_command` → `parse` → `event` flow. Look at `get_passband` for an example of model-conditional behavior.

#### Verification: `cargo check --workspace`

---

### Phase 5c-2: AGC Rig Wiring — Yaesu ✅ DONE

Wire AGC commands into the Yaesu `Rig` trait implementation.

#### File to modify:
- **`crates/riglib-yaesu/src/rig.rs`**

#### Implementation details:

**`get_agc(rx: ReceiverId) -> Result<AgcMode>`:**
1. No receiver selection needed — Yaesu AGC applies to the currently selected VFO.
2. Build command: `let cmd = commands::cmd_read_agc()`.
3. Execute: `let (_prefix, data) = self.execute_command(&cmd).await?` (Yaesu pattern returns a `(String, String)` tuple).
4. Parse: `let raw = commands::parse_agc_response(&data)?`.
5. Map raw value to `AgcMode`: `0` → `Off`, `1` → `Fast`, `2` → `Medium`, `3` → `Slow`.

**`set_agc(rx: ReceiverId, mode: AgcMode) -> Result<()>`:**
1. Map `AgcMode` to Yaesu value: `Off` → `0`, `Fast` → `1`, `Medium` → `2`, `Slow` → `3`.
2. Build command: `let cmd = commands::cmd_set_agc(value)`.
3. Execute the command (Yaesu set commands typically do not return data).
4. On success: `let _ = self.event_tx.send(RigEvent::AgcChanged { receiver: rx, mode });`.

**Existing patterns to follow:** Look at `get_mode` / `set_mode` for the `execute_command` → parse → event flow. Yaesu is the simplest — the `GT` command has a clean 1:1 mapping to `AgcMode` with no model-dependent branching.

#### Verification: `cargo check --workspace`

---

### Phase 5c-3: AGC Rig Wiring — Kenwood ✅ DONE

Wire AGC commands into the Kenwood `Rig` trait implementation.

#### File to modify:
- **`crates/riglib-kenwood/src/rig.rs`**

#### Implementation details:

Kenwood has **three distinct AGC command variants** depending on model. The `get_agc`/`set_agc` implementations must dispatch based on `self.model`:

**Variant 1 — TS-890S** (uses `GC` command, simple mode byte):
- Read: `commands::cmd_read_agc_mode()` → `parse_agc_mode_response(&data)` returns `0`=Off, `1`=Slow, `2`=Medium, `3`=Fast.
- Write: `commands::cmd_set_agc_mode(value)`.

**Variant 2 — TS-990S** (uses `GC` with per-VFO addressing):
- Read: `commands::cmd_read_agc_mode_vfo(vfo)` where `vfo` is derived from `rx.index()` (0 = main, 1 = sub). Parse with `parse_agc_mode_vfo_response(&data)` which returns `(vfo, mode)`.
- Write: `commands::cmd_set_agc_mode_vfo(vfo, value)`.

**Variant 3 — TS-590S/SG** (uses `GT` time constant command):
- Read: `commands::cmd_read_agc_time_constant()` → `parse_agc_time_constant_response(&data)` returns a numeric time constant. Map: `0` → `Off`, `5` → `Fast`, `10` → `Medium`, `20` → `Slow`.
- Write: `commands::cmd_set_agc_time_constant(value)` where value is the numeric time constant.

**`get_agc(rx: ReceiverId) -> Result<AgcMode>`:**
1. Match on model name (or add an `agc_command_style` enum to `KenwoodModel`) to pick the correct command variant.
2. Build and execute the appropriate command via `self.execute_command(&cmd).await?`.
3. Parse response with the matching parser.
4. Map the raw value to `AgcMode`.

**`set_agc(rx: ReceiverId, mode: AgcMode) -> Result<()>`:**
1. Same model dispatch to pick the correct command variant.
2. Map `AgcMode` to the model-appropriate raw value.
3. Build and execute via `self.execute_set_command(&cmd).await?`.
4. On success: `let _ = self.event_tx.send(RigEvent::AgcChanged { receiver: rx, mode });`.

**Decision point:** How to dispatch between model variants. Options:
- (A) Match on `self.model.name` string (`"TS-890S"`, `"TS-990S"`, `"TS-590S"`, `"TS-590SG"`).
- (B) Add an `agc_command_style` enum to `KenwoodModel` (cleaner, but requires touching `models.rs` — minor and purely additive).

**Existing patterns to follow:** Look at any method that already branches on model for command selection.

#### Verification: `cargo check --workspace`

---

### Phase 5c-4: AGC Rig Wiring — Elecraft ✅ DONE

Wire AGC commands into the Elecraft `Rig` trait implementation.

#### File to modify:
- **`crates/riglib-elecraft/src/rig.rs`**

#### Implementation details:

Elecraft has **two command variants** dispatched by the existing `self.model.is_k4` flag:

**K3-family** (K3, K3S, KX3, KX2 — `is_k4 == false`):
- Read: `commands::cmd_read_agc_k3()` sends `GT;`. Parse with `commands::parse_agc_response_k3(&data)` which returns `(speed: u16, agc_on: Option<bool>)`. The `agc_on` field is `Some(false)` on K22-firmware rigs when AGC is off, `None` on older firmware.
- Mapping: If `agc_on == Some(false)` → `AgcMode::Off`. Otherwise map `speed`: `0` → `Off` (fallback for pre-K22 firmware with speed=0), low values → `Fast`, mid values → `Medium`, high values → `Slow`. **Decision point:** define speed thresholds. K3 GT speed range is 0-20; typical: `0`=Off, `1-4`=Fast, `5-12`=Medium, `13-20`=Slow.
- Write: `commands::cmd_set_agc_k3(speed, agc_on)`. Map `AgcMode` to `(speed, agc_on)` pair: `Off` → `(0, Some(false))`, `Fast` → `(2, Some(true))`, `Medium` → `(10, Some(true))`, `Slow` → `(18, Some(true))`.

**K4** (`is_k4 == true`):
- Read: `commands::cmd_read_agc_k4()` sends `GT$;`. Parse with `commands::parse_agc_response_k4(&data)` which returns `0`=Off, `1`=Slow, `2`=Fast.
- Mapping: `0` → `Off`, `1` → `Slow`, `2` → `Fast`. **Note:** K4 does not have a `Medium` mode. `set_agc(_, AgcMode::Medium)` should either map to `Slow` (closest) or return `Error::Unsupported`. Document the choice.
- Write: `commands::cmd_set_agc_k4(value)` where `0`=Off, `1`=Slow, `2`=Fast.

**`get_agc(rx: ReceiverId) -> Result<AgcMode>`:**
1. Branch on `self.model.is_k4`.
2. Build the appropriate command, execute via `self.execute_command(&cmd).await?`.
3. Parse with the matching parser.
4. Map to `AgcMode` using the rules above.

**`set_agc(rx: ReceiverId, mode: AgcMode) -> Result<()>`:**
1. Branch on `self.model.is_k4`.
2. Map `AgcMode` to the appropriate raw value(s).
3. Build the appropriate command, execute via `self.execute_set_command(&cmd).await?`.
4. On success: `let _ = self.event_tx.send(RigEvent::AgcChanged { receiver: rx, mode });`.

**Existing patterns to follow:** Look at `get_passband` / `set_passband` which already branches on `self.model.is_k4` for command dispatch.

#### Verification: `cargo check --workspace`

---

### Phase 5c-5: AGC Rig Wiring — FlexRadio ✅ DONE

Wire AGC commands into the FlexRadio `Rig` trait implementation. This sub-phase touches multiple files within `riglib-flex` to plumb AGC through the cached-state architecture.

#### Files to modify:
- **`crates/riglib-flex/src/rig.rs`** — Implement `get_agc()`, `set_agc()`.
- **`crates/riglib-flex/src/state.rs`** — Add `agc_mode: String` field to `SliceState`.
- **`crates/riglib-flex/src/codec.rs`** — Add `agc_mode: Option<String>` field to `SliceStatus` and parse the `agc_mode` key in `parse_slice_status()`.
- **`crates/riglib-flex/src/client.rs`** — In `process_status()`, apply `SliceStatus.agc_mode` to `SliceState.agc_mode` and emit `RigEvent::AgcChanged` when the value changes.

#### Implementation details:

**Step 1 — `state.rs`:** Add `pub agc_mode: String` to `SliceState`. Default is empty string (same pattern as the `mode` field).

**Step 2 — `codec.rs`:** Add `pub agc_mode: Option<String>` to `SliceStatus`. In `parse_slice_status()`, add an arm to the match block: `"agc_mode" => { status.agc_mode = Some(value.clone()); }`. Initialize `agc_mode: None` in the `SliceStatus` constructor.

**Step 3 — `client.rs`:** In `process_status()`, inside the `if object.starts_with("slice")` block, after the existing field applications, add AGC change detection and event emission following the same pattern used for `mode` (ModeChanged). Parse the string via `agc_str.parse::<AgcMode>()` (which uses `FromStr` added in Phase 1, accepting `"off"`, `"fast"`, `"med"`/`"medium"`, `"slow"`).

**Step 4 — `rig.rs`:**

**`get_agc(rx: ReceiverId) -> Result<AgcMode>`:**
1. `self.ensure_slice(rx).await?`.
2. `let state = self.client.state().await`.
3. Look up `state.slices.get(&rx.index())`, error if missing.
4. Parse `slice.agc_mode` via `slice.agc_mode.parse::<AgcMode>()`. If empty string or parse failure, return `Error::Protocol("unknown AGC mode")`.

**`set_agc(rx: ReceiverId, mode: AgcMode) -> Result<()>`:**
1. `self.ensure_slice(rx).await?`.
2. Map `AgcMode` to SmartSDR string: `Off` → `"off"`, `Fast` → `"fast"`, `Medium` → `"med"`, `Slow` → `"slow"`.
3. Build command: `codec::cmd_slice_set_agc_mode(rx.index(), mode_str)`.
4. Execute: `self.client.send_command(&cmd).await?`.
5. Event emission happens automatically in `process_status()` (Step 3) when the SmartSDR status update arrives. Optionally emit eagerly here too for lower latency.

**Existing patterns to follow:** Look at `get_frequency` / `set_frequency` for the `ensure_slice` → read cached state / send command pattern. Look at how `process_status()` in `client.rs` handles `mode` changes for the event emission pattern.

#### Verification: `cargo check --workspace`, `cargo test -p riglib-flex`

---

## Phase 6a: Preamp / Attenuator — Events ✅ DONE

Add `PreampChanged` and `AttenuatorChanged` events to `riglib-core`. The trait methods (`get_preamp`, `set_preamp`, `get_attenuator`, `set_attenuator`) already exist with default `Error::Unsupported` returns (added in Phase 2). The types `PreampLevel` and `AttenuatorLevel` already exist (added in Phase 1). This sub-phase only adds the missing event variants.

### Files to modify:
- **`crates/riglib-core/src/events.rs`** — Add two new event variants:
  - `PreampChanged { receiver: ReceiverId, level: PreampLevel }` — emitted when the preamp level changes.
  - `AttenuatorChanged { receiver: ReceiverId, level: AttenuatorLevel }` — emitted when the attenuator level changes.
  - Update the `use` import at the top to include `PreampLevel` and `AttenuatorLevel` from `crate::types`.

### What already exists (no changes needed):
- `PreampLevel { Off, Preamp1, Preamp2 }` in `types.rs` with `Display`, `FromStr`.
- `AttenuatorLevel { Off, Db6, Db12, Db18 }` in `types.rs` with `Display`, `FromStr`.
- Trait methods `get_preamp(rx) -> Result<PreampLevel>`, `set_preamp(rx, level) -> Result<()>`, `get_attenuator(rx) -> Result<AttenuatorLevel>`, `set_attenuator(rx, level) -> Result<()>` in `rig.rs`.
- `RigCapabilities` fields `preamp_levels: Vec<PreampLevel>` and `attenuator_levels: Vec<AttenuatorLevel>` (remain empty until Phase 13).

### Verification: `cargo check -p riglib-core`

---

## Phase 6b: Preamp / Attenuator — Commands (All 5 Manufacturers) ✅ DONE

Add low-level preamp and attenuator command builders and response parsers. Each manufacturer uses different protocols — verify all mappings against CAT reference documents before implementing.

### Files to modify:

- **`crates/riglib-icom/src/commands.rs`** — Two separate CI-V command groups:

  **Preamp** — CI-V function command `0x16` sub-command `0x02`:
  - Add constant: `SUB_PREAMP: u8 = 0x02` (under `CMD_FUNC` section).
  - `cmd_read_preamp(addr) -> Vec<u8>`: sends `CMD_FUNC` (`0x16`) + sub `0x02` with no data.
  - `cmd_set_preamp(addr, level: u8) -> Vec<u8>`: sends `CMD_FUNC` (`0x16`) + sub `0x02` + data byte. Values: `0x00`=Off, `0x01`=Preamp1, `0x02`=Preamp2.
  - `parse_preamp_response(data: &[u8]) -> Result<u8>`: expects 1 byte, returns raw preamp level.
  - **Per-model note:** Most Icom HF rigs have Preamp1 only (10 dB). The IC-7600, IC-7700, IC-7800, IC-7850/51, IC-7610 have both Preamp1 and Preamp2. The IC-7300, IC-7300MK2, IC-705, IC-7100, IC-7410, IC-9100 have Preamp1 only. The IC-9700 and IC-905 (VHF/UHF) have Preamp1 only. Model gating happens in the rig layer (Phase 6c-1), not here.

  **Attenuator** — CI-V command `0x11`:
  - Add constant: `CMD_ATTENUATOR: u8 = 0x11`.
  - `cmd_read_attenuator(addr) -> Vec<u8>`: sends `CMD_ATTENUATOR` (`0x11`) with no sub-command or data.
  - `cmd_set_attenuator(addr, level: u8) -> Vec<u8>`: sends `CMD_ATTENUATOR` (`0x11`) with a single data byte. Values: `0x00`=Off, `0x20`=20dB on most models. **Per-model note:** All supported Icom HF rigs have a single attenuator level (approximately 20 dB, sometimes labeled as 12 dB on older models). The attenuator is binary (on/off) on all models — there is no multi-step attenuator. The raw value `0x20` represents "attenuator on". Map `AttenuatorLevel::Off` to `0x00`, any other `AttenuatorLevel` variant to `0x20`.
  - `parse_attenuator_response(data: &[u8]) -> Result<u8>`: expects 1 byte, returns raw attenuator level (`0x00` or `0x20`).

- **`crates/riglib-yaesu/src/commands.rs`** — Text protocol `PA` and `RA` commands:

  **Preamp** — `PA0` command:
  - `cmd_read_preamp() -> Vec<u8>`: sends `PA0;`. Response is `PA0{level};` where level is `00`=Off, `01`=IPO (bypass), varies by model.
  - `cmd_set_preamp(level: u8) -> Vec<u8>`: sends `PA0{level:02};`. **Yaesu preamp encoding:** Yaesu inverts the terminology from other manufacturers. `PA0` parameter `00` = preamp off (IPO/bypass), `01` = preamp on (Amp 1), `02` = preamp on (Amp 2) on models that have two stages. On the FT-DX10/FT-710/FT-891/FT-991A: `00`=Off, `01`=Amp1 (only one stage). On the FT-DX101D/FT-DX101MP: `00`=Off, `01`=Amp1, `02`=Amp2.
  - `parse_preamp_response(data: &str) -> Result<u8>`: expects 2 characters (after the `PA0` prefix is stripped by the protocol layer), returns raw level.

  **Attenuator** — `RA0` command:
  - `cmd_read_attenuator() -> Vec<u8>`: sends `RA0;`. Response is `RA0{level:02};` where level is `00`=Off, `01`=On (typically 12 dB).
  - `cmd_set_attenuator(level: u8) -> Vec<u8>`: sends `RA0{level:02};`. Values: `00`=Off, `01`=On. All supported Yaesu models have a single-step attenuator.
  - `parse_attenuator_response(data: &str) -> Result<u8>`: expects 2 characters, returns raw level.

- **`crates/riglib-kenwood/src/commands.rs`** — Text protocol `PA` and `RA` commands:

  **Preamp** — `PA` command:
  - `cmd_read_preamp() -> Vec<u8>`: sends `PA;`. Response is `PA{level};` where level is a single digit.
  - `cmd_set_preamp(level: u8) -> Vec<u8>`: sends `PA{level};`. Values: `0`=Off, `1`=Preamp1. The TS-890S has only one preamp level. The TS-990S has `0`=Off, `1`=Preamp1, `2`=Preamp2. The TS-590S/SG has `0`=Off, `1`=Preamp1.
  - `parse_preamp_response(data: &str) -> Result<u8>`: expects 1 character, returns raw level.

  **Attenuator** — `RA` command:
  - `cmd_read_attenuator() -> Vec<u8>`: sends `RA;`. Response is `RA{level:02};` where level is a 2-digit value.
  - `cmd_set_attenuator(level: u8) -> Vec<u8>`: sends `RA{level:02};`. Values: `00`=Off. On the TS-590S/SG: `00`=Off, `12`=12dB, `06`=6dB, `18`=18dB. On the TS-890S: `00`=Off, `06`=6dB, `12`=12dB, `18`=18dB. On the TS-990S: `00`=Off, `06`=6dB, `12`=12dB, `18`=18dB. **Note:** Kenwood is the only manufacturer with multi-step attenuator values that map cleanly to `AttenuatorLevel` enum variants.
  - `parse_attenuator_response(data: &str) -> Result<u8>`: expects 2 characters, returns raw level.

- **`crates/riglib-elecraft/src/commands.rs`** — Extended Kenwood text protocol `PA` and `RA`:

  **Preamp** — `PA` command:
  - `cmd_read_preamp() -> Vec<u8>`: sends `PA;`. Response is `PA{level};`.
  - `cmd_set_preamp(level: u8) -> Vec<u8>`: sends `PA{level};`. Values: `0`=Off, `1`=Preamp on. All K3-family and K4 have a single preamp stage.
  - `parse_preamp_response(data: &str) -> Result<u8>`: expects 1 character, returns raw level.

  **Attenuator** — `RA` command:
  - K3-family: `RA;` reads the attenuator. Response is `RA{level:02};`. Values: `00`=Off. The K3/K3S/KX3/KX2 support an attenuator (typically 10-15 dB, single step). `RA00;`=Off, `RA01;`=On.
  - K4: The K4 has `RA$` extended format. `cmd_read_attenuator_k4() -> Vec<u8>` sends `RA$;`. Response is `RA${level};`. Values: `0`=Off, `1`=On.
  - **Decision point:** Use `self.model.is_k4` dispatch in rig layer (Phase 6c-4), or provide both command variants in commands.rs. Recommend providing both variants (`cmd_read_attenuator_k3()` / `cmd_read_attenuator_k4()`) and let the rig layer dispatch.
  - `parse_attenuator_response_k3(data: &str) -> Result<u8>`: expects 2 characters, returns raw level.
  - `parse_attenuator_response_k4(data: &str) -> Result<u8>`: expects `$n` format, returns raw level.

- **`crates/riglib-flex/src/codec.rs`** — No preamp/attenuator commands needed. FlexRadio SDRs do not have discrete preamp or attenuator hardware. The `rfgain` slice parameter controls RF front-end gain but has a different semantic from preamp/attenuator. **Decision point:** Return `Error::Unsupported` for all preamp/attenuator operations (recommended), or optionally map `rfgain` to attenuator levels (stretch goal, not recommended for this phase).

### Verification: `cargo check --workspace`

---

## Phase 6c: Preamp / Attenuator — Rig Wiring (All 5 Manufacturers)

This phase is split into five sub-phases (6c-1 through 6c-5), one per manufacturer. Each sub-phase is self-contained, touches only one manufacturer's `rig.rs` (and optionally `models.rs`), and ends with `cargo check --workspace`. They can be done in any order.

**Shared context for all sub-phases:**
- The `Rig` trait already has default `get_preamp` / `set_preamp` / `get_attenuator` / `set_attenuator` methods returning `Error::Unsupported` (added in Phase 2).
- The `RigEvent::PreampChanged { receiver, level }` and `RigEvent::AttenuatorChanged { receiver, level }` events exist (added in Phase 6a).
- Low-level command functions and parsers already exist in each manufacturer's `commands.rs` (added in Phase 6b).
- `PreampLevel` enum: `Off`, `Preamp1`, `Preamp2` (in `riglib-core/src/types.rs`).
- `AttenuatorLevel` enum: `Off`, `Db6`, `Db12`, `Db18` (in `riglib-core/src/types.rs`).
- **In scope:** emit `PreampChanged` / `AttenuatorChanged` from `set_preamp()` / `set_attenuator()` on success. **Deferred to Phases 9-12:** emitting these events from transceive/unsolicited responses.
- Phase 1 added `preamp_levels` and `attenuator_levels` to `RigCapabilities`, but they remain empty until Phase 13 populates them per model. This is expected.

---

### Phase 6c-1: Preamp / Attenuator Rig Wiring — Icom ✅ DONE

Wire preamp and attenuator commands into the Icom `Rig` trait implementation.

#### Files to modify:
- **`crates/riglib-icom/src/rig.rs`**
- **`crates/riglib-icom/src/models.rs`** (optional, see decision point below)

#### Implementation details:

**Preamp — `get_preamp(rx: ReceiverId) -> Result<PreampLevel>`:**
1. Call `self.select_receiver(rx).await?` (same pattern as `get_agc`).
2. Build command: `commands::cmd_read_preamp(self.civ_address)`.
3. Execute: `let frame = self.execute_command(&cmd).await?`.
4. Extract payload: `let data = Self::frame_payload(&frame)?`.
5. Parse: `let raw = commands::parse_preamp_response(&data)?`. Skip the sub-command echo byte (byte at index 0 will be `0x02`, the sub-command echo — start parsing from index 1, same pattern as AGC). **Verify:** Confirm the frame_payload includes or excludes the sub-command echo by checking how `get_agc` handles it.
6. Map raw byte to `PreampLevel`: `0x00` -> `Off`, `0x01` -> `Preamp1`, `0x02` -> `Preamp2`.

**Preamp — `set_preamp(rx: ReceiverId, level: PreampLevel) -> Result<()>`:**
1. Call `self.select_receiver(rx).await?`.
2. Map `PreampLevel` to raw byte: `Off` -> `0x00`, `Preamp1` -> `0x01`, `Preamp2` -> `0x02`.
3. **Model gating for Preamp2:** If `level == Preamp2` and the model only supports Preamp1, return `Error::Unsupported`. **Decision point:** Add a `has_preamp2: bool` field to `IcomModel` (similar to `has_agc_time_constant`), or hard-code a check based on model name. Models with Preamp2: IC-7600, IC-7700, IC-7800, IC-7850, IC-7851, IC-7610. Models with Preamp1 only: IC-7300, IC-7300MK2, IC-705, IC-7100, IC-9100, IC-7410, IC-9700, IC-905.
4. Build command: `commands::cmd_set_preamp(self.civ_address, raw_byte)`.
5. Execute: `self.execute_ack_command(&cmd).await?`.
6. On success: `let _ = self.event_tx.send(RigEvent::PreampChanged { receiver: rx, level });`.

**Attenuator — `get_attenuator(rx: ReceiverId) -> Result<AttenuatorLevel>`:**
1. Call `self.select_receiver(rx).await?`.
2. Build command: `commands::cmd_read_attenuator(self.civ_address)`.
3. Execute: `let frame = self.execute_command(&cmd).await?`.
4. Extract payload: `let data = Self::frame_payload(&frame)?`.
5. Parse: `let raw = commands::parse_attenuator_response(&data)?`.
6. Map raw byte to `AttenuatorLevel`: `0x00` -> `Off`, any non-zero value (`0x20`) -> `AttenuatorLevel::Db12` (closest match for the ~20 dB Icom attenuator; alternatively use `Db18` — document the choice).

**Attenuator — `set_attenuator(rx: ReceiverId, level: AttenuatorLevel) -> Result<()>`:**
1. Call `self.select_receiver(rx).await?`.
2. Map `AttenuatorLevel` to raw byte: `Off` -> `0x00`, any other value -> `0x20` (Icom attenuator is binary on/off). **Note:** Setting `Db6`, `Db12`, or `Db18` all map to `0x20` (attenuator on). This is the expected behavior for Icom rigs.
3. Build command: `commands::cmd_set_attenuator(self.civ_address, raw_byte)`.
4. Execute: `self.execute_ack_command(&cmd).await?`.
5. On success: emit `RigEvent::AttenuatorChanged { receiver: rx, level }`. **Note:** Emit the level the user requested, not the mapped binary value. If user sets `Db18`, emit `Db18` even though the hardware only does on/off.

**Existing patterns to follow:** Look at `get_agc` / `set_agc` for the `select_receiver` -> `execute_command` -> parse -> event flow. Look at `has_agc_time_constant` for the model-conditional field pattern.

#### Verification: `cargo check --workspace`, `cargo test -p riglib-icom`

---

### Phase 6c-2: Preamp / Attenuator Rig Wiring — Yaesu ✅ DONE

Wire preamp and attenuator commands into the Yaesu `Rig` trait implementation.

#### File to modify:
- **`crates/riglib-yaesu/src/rig.rs`**
- **`crates/riglib-yaesu/src/models.rs`** (optional, see decision point below)

#### Implementation details:

**Preamp — `get_preamp(rx: ReceiverId) -> Result<PreampLevel>`:**
1. No receiver selection needed — Yaesu preamp applies to the currently selected VFO.
2. Build command: `let cmd = commands::cmd_read_preamp()`.
3. Execute: `let (_prefix, data) = self.execute_command(&cmd).await?`.
4. Parse: `let raw = commands::parse_preamp_response(&data)?`.
5. Map raw value to `PreampLevel`: `0` -> `Off`, `1` -> `Preamp1`, `2` -> `Preamp2`.

**Preamp — `set_preamp(rx: ReceiverId, level: PreampLevel) -> Result<()>`:**
1. Map `PreampLevel` to Yaesu value: `Off` -> `0`, `Preamp1` -> `1`, `Preamp2` -> `2`.
2. **Model gating for Preamp2:** If `level == Preamp2` and the model only supports one preamp stage, return `Error::Unsupported`. Models with Preamp2: FT-DX101D, FT-DX101MP. Models with Preamp1 only: FT-DX10, FT-891, FT-991A, FT-710. **Decision point:** Add a `has_preamp2: bool` field to `YaesuModel`, or match on `self.model.name`.
3. Build command: `let cmd = commands::cmd_set_preamp(value)`.
4. Execute the command.
5. On success: `let _ = self.event_tx.send(RigEvent::PreampChanged { receiver: rx, level });`.

**Attenuator — `get_attenuator(rx: ReceiverId) -> Result<AttenuatorLevel>`:**
1. Build command: `let cmd = commands::cmd_read_attenuator()`.
2. Execute: `let (_prefix, data) = self.execute_command(&cmd).await?`.
3. Parse: `let raw = commands::parse_attenuator_response(&data)?`.
4. Map raw value to `AttenuatorLevel`: `0` -> `Off`, `1` -> `AttenuatorLevel::Db12` (all Yaesu models have a single ~12 dB attenuator step).

**Attenuator — `set_attenuator(rx: ReceiverId, level: AttenuatorLevel) -> Result<()>`:**
1. Map `AttenuatorLevel` to Yaesu value: `Off` -> `0`, any other value -> `1` (Yaesu attenuator is binary on/off, ~12 dB).
2. Build command: `let cmd = commands::cmd_set_attenuator(value)`.
3. Execute the command.
4. On success: `let _ = self.event_tx.send(RigEvent::AttenuatorChanged { receiver: rx, level });`.

**Existing patterns to follow:** Look at `get_agc` / `set_agc` for the `execute_command` -> parse -> event flow. Yaesu is straightforward — the `PA0` and `RA0` commands have clean mappings with no model-dependent command variants (only model-dependent level ranges).

#### Verification: `cargo check --workspace`, `cargo test -p riglib-yaesu`

---

### Phase 6c-3: Preamp / Attenuator Rig Wiring — Kenwood ✅ DONE

Wire preamp and attenuator commands into the Kenwood `Rig` trait implementation.

#### File to modify:
- **`crates/riglib-kenwood/src/rig.rs`**

#### Implementation details:

Unlike AGC (which required three command variants), preamp and attenuator use the same `PA` / `RA` commands across all Kenwood models. The only per-model difference is the range of supported values.

**Preamp — `get_preamp(rx: ReceiverId) -> Result<PreampLevel>`:**
1. Build command: `let cmd = commands::cmd_read_preamp()`.
2. Execute: `let (_prefix, data) = self.execute_command(&cmd).await?`.
3. Parse: `let raw = commands::parse_preamp_response(&data)?`.
4. Map raw value to `PreampLevel`: `0` -> `Off`, `1` -> `Preamp1`, `2` -> `Preamp2`.

**Preamp — `set_preamp(rx: ReceiverId, level: PreampLevel) -> Result<()>`:**
1. Map `PreampLevel` to Kenwood value: `Off` -> `0`, `Preamp1` -> `1`, `Preamp2` -> `2`.
2. **Model gating for Preamp2:** Only the TS-990S supports Preamp2. The TS-590S/SG and TS-890S support only Preamp1. If `level == Preamp2` and the model does not support it, return `Error::Unsupported`. **Decision point:** Match on model name, or add a `has_preamp2: bool` field to `KenwoodModel`.
3. Build command: `let cmd = commands::cmd_set_preamp(value)`.
4. Execute via `self.execute_set_command(&cmd).await?`.
5. On success: `let _ = self.event_tx.send(RigEvent::PreampChanged { receiver: rx, level });`.

**Attenuator — `get_attenuator(rx: ReceiverId) -> Result<AttenuatorLevel>`:**
1. Build command: `let cmd = commands::cmd_read_attenuator()`.
2. Execute: `let (_prefix, data) = self.execute_command(&cmd).await?`.
3. Parse: `let raw = commands::parse_attenuator_response(&data)?`.
4. Map raw value to `AttenuatorLevel`: `0` -> `Off`, `6` -> `Db6`, `12` -> `Db12`, `18` -> `Db18`. **Note:** Kenwood is the only manufacturer with multi-step attenuation that maps directly to `AttenuatorLevel` variants. Unknown values should return `Error::Protocol`.

**Attenuator — `set_attenuator(rx: ReceiverId, level: AttenuatorLevel) -> Result<()>`:**
1. Map `AttenuatorLevel` to Kenwood value: `Off` -> `0`, `Db6` -> `6`, `Db12` -> `12`, `Db18` -> `18`.
2. Build command: `let cmd = commands::cmd_set_attenuator(value)`.
3. Execute via `self.execute_set_command(&cmd).await?`.
4. On success: `let _ = self.event_tx.send(RigEvent::AttenuatorChanged { receiver: rx, level });`.

**Existing patterns to follow:** Look at `get_agc` / `set_agc` for the `execute_command` -> parse -> event flow. The preamp/attenuator commands are simpler than AGC because there is no model-dependent command *variant* — only model-dependent value ranges.

#### Verification: `cargo check --workspace`, `cargo test -p riglib-kenwood`

---

### Phase 6c-4: Preamp / Attenuator Rig Wiring — Elecraft ✅ DONE

Wire preamp and attenuator commands into the Elecraft `Rig` trait implementation.

#### File to modify:
- **`crates/riglib-elecraft/src/rig.rs`**

#### Implementation details:

Elecraft has a single preamp stage across all models. The attenuator has **two command variants** dispatched by the existing `self.model.is_k4` flag (same pattern as AGC).

**Preamp — `get_preamp(rx: ReceiverId) -> Result<PreampLevel>`:**
1. Build command: `let cmd = commands::cmd_read_preamp()`.
2. Execute: `let (_prefix, data) = self.execute_command(&cmd).await?`.
3. Parse: `let raw = commands::parse_preamp_response(&data)?`.
4. Map raw value to `PreampLevel`: `0` -> `Off`, `1` -> `Preamp1`.

**Preamp — `set_preamp(rx: ReceiverId, level: PreampLevel) -> Result<()>`:**
1. If `level == Preamp2`, return `Error::Unsupported` — no Elecraft model supports two preamp stages.
2. Map `PreampLevel` to Elecraft value: `Off` -> `0`, `Preamp1` -> `1`.
3. Build command: `let cmd = commands::cmd_set_preamp(value)`.
4. Execute via `self.execute_set_command(&cmd).await?`.
5. On success: `let _ = self.event_tx.send(RigEvent::PreampChanged { receiver: rx, level });`.

**Attenuator — `get_attenuator(rx: ReceiverId) -> Result<AttenuatorLevel>`:**
1. Branch on `self.model.is_k4`:
   - **K3-family** (`is_k4 == false`): `let cmd = commands::cmd_read_attenuator_k3()`. Parse with `commands::parse_attenuator_response_k3(&data)`.
   - **K4** (`is_k4 == true`): `let cmd = commands::cmd_read_attenuator_k4()`. Parse with `commands::parse_attenuator_response_k4(&data)`.
2. Map raw value to `AttenuatorLevel`: `0` -> `Off`, `1` (or non-zero) -> `AttenuatorLevel::Db12` (K3 attenuator is ~12 dB, K4 attenuator is ~12 dB). All Elecraft models have a single-step attenuator.

**Attenuator — `set_attenuator(rx: ReceiverId, level: AttenuatorLevel) -> Result<()>`:**
1. Map `AttenuatorLevel` to raw value: `Off` -> `0`, any other value -> `1` (binary on/off).
2. Branch on `self.model.is_k4`:
   - **K3-family**: `let cmd = commands::cmd_set_attenuator_k3(value)`.
   - **K4**: `let cmd = commands::cmd_set_attenuator_k4(value)`.
3. Execute via `self.execute_set_command(&cmd).await?`.
4. On success: `let _ = self.event_tx.send(RigEvent::AttenuatorChanged { receiver: rx, level });`.

**Existing patterns to follow:** Look at `get_agc` / `set_agc` for the `is_k4` dispatch pattern. Look at `get_passband` / `set_passband` for another example of K3/K4 command branching.

#### Verification: `cargo check --workspace`, `cargo test -p riglib-elecraft`

---

### Phase 6c-5: Preamp / Attenuator Rig Wiring — FlexRadio ✅ DONE

Document the unsupported status for preamp/attenuator on FlexRadio.

#### Files to modify:
- **`crates/riglib-flex/src/rig.rs`** — No implementation needed.

#### Implementation details:

FlexRadio SDRs do not have discrete preamp or attenuator hardware. The direct-sampling SDR architecture handles front-end gain internally through the `rfgain` parameter, which controls the ADC input level. This has a different semantic from traditional preamp/attenuator controls:

- **Preamp:** FlexRadio has no equivalent. The default `Error::Unsupported` return from the trait is correct. No override needed.
- **Attenuator:** FlexRadio has no equivalent. The default `Error::Unsupported` return from the trait is correct. No override needed.

**No code changes required.** The `Rig` trait default implementations already return `Error::Unsupported` for all four methods. This sub-phase exists only for documentation completeness and to confirm the design decision.

**Future consideration:** If a user requests mapping `rfgain` to attenuator levels, this could be added as a FlexRadio-specific extension method on the concrete `FlexRadio` type, but it should NOT map to the generic `get_attenuator`/`set_attenuator` trait methods because the semantics are different.

#### Verification: `cargo check --workspace`, `cargo test -p riglib-flex`

---

## Phase 7: RIT / XIT

### Phase 7a: Core Infrastructure — ✅ DONE

No work needed. The following already exist:

- **Rig trait methods** in `crates/riglib-core/src/rig.rs`: `get_rit()`, `set_rit()`, `get_xit()`, `set_xit()` with default `Unsupported` error returns
- **Events** in `crates/riglib-core/src/events.rs`: `RitChanged { enabled, offset_hz }` and `XitChanged { enabled, offset_hz }`
- **Capability flags** in `crates/riglib-core/src/types.rs`: `has_rit: bool` and `has_xit: bool` in `RigCapabilities`, defaulting to `false`

---

### Phase 7b-1: Icom CI-V RIT/XIT Command Builders — ✅ DONE

**File:** `crates/riglib-icom/src/commands.rs`

**What to add:**

1. New constant: `const CMD_RIT_XIT: u8 = 0x21;`
2. Sub-command constants:
   - `const SUB_RIT_ON_OFF: u8 = 0x01;`
   - `const SUB_RIT_OFFSET: u8 = 0x02;`
   - `const SUB_XIT_ON_OFF: u8 = 0x03;`
   - `const SUB_XIT_OFFSET: u8 = 0x04;`
3. Command builders:
   - `cmd_read_rit_on(rig_addr) -> Vec<u8>` — reads RIT on/off state
   - `cmd_set_rit_on(rig_addr, on: bool) -> Vec<u8>` — sets RIT on/off
   - `cmd_read_rit_offset(rig_addr) -> Vec<u8>` — reads RIT offset
   - `cmd_set_rit_offset(rig_addr, offset_hz: i32) -> Vec<u8>` — sets RIT offset
   - `cmd_read_xit_on(rig_addr) -> Vec<u8>` — reads XIT on/off state
   - `cmd_set_xit_on(rig_addr, on: bool) -> Vec<u8>` — sets XIT on/off
   - `cmd_read_xit_offset(rig_addr) -> Vec<u8>` — reads XIT offset
   - `cmd_set_xit_offset(rig_addr, offset_hz: i32) -> Vec<u8>` — sets XIT offset
4. Response parsers:
   - `parse_rit_on_response(data: &[u8]) -> Result<bool>`
   - `parse_rit_offset_response(data: &[u8]) -> Result<i32>`
   - `parse_xit_on_response(data: &[u8]) -> Result<bool>`
   - `parse_xit_offset_response(data: &[u8]) -> Result<i32>`

**Protocol details (CI-V 0x21):**
- On/off: sub-command byte followed by 0x01 (on) or 0x00 (off)
- Offset encoding: sign byte (0x00 = positive, 0x01 = negative) + 2-byte BCD magnitude (e.g., +150 Hz = `0x00 0x01 0x50`, -300 Hz = `0x01 0x03 0x00`)
- Offset range: typically +/-9999 Hz
- Transceive mode: rigs broadcast 0x21 sub-command responses unsolicited

**Verification:** `cargo test -p riglib-icom`, `cargo check --workspace`

---

### Phase 7b-2: Yaesu CAT RIT/XIT Command Builders — ✅ DONE

**File:** `crates/riglib-yaesu/src/commands.rs`

**What to add:**

1. Command builders:
   - `cmd_read_rit() -> Vec<u8>` — `RT0;` reads RIT state (on/off + offset)
   - `cmd_set_rit_on(on: bool) -> Vec<u8>` — `RT01;` (on) or `RT00;` (off)
   - `cmd_read_xit() -> Vec<u8>` — `XT0;` reads XIT state
   - `cmd_set_xit_on(on: bool) -> Vec<u8>` — `XT01;` (on) or `XT00;` (off)
   - `cmd_rit_up(hz: u32) -> Vec<u8>` — `RU{hz:04};` increment offset
   - `cmd_rit_down(hz: u32) -> Vec<u8>` — `RD{hz:04};` decrement offset
   - `cmd_rit_clear() -> Vec<u8>` — `RC;` clear offset to zero
2. Response parsers:
   - `parse_rit_response(data: &str) -> Result<(bool, i32)>` — parses `RT0P+XXXX` or `RT0P-XXXX`
   - `parse_xit_response(data: &str) -> Result<(bool, i32)>` — parses `XT0P+XXXX`

**Protocol details (Yaesu CAT):**
- `RT0;` reads, response is `RT0P+XXXX;` where P is 0/1, +/- is sign, XXXX is 4-digit offset
- `XT` command: same format as RT
- `RU`/`RD`: relative increment/decrement (e.g., `RU0050;` adds 50 Hz)
- `RC`: resets offset to zero
- **Quirk:** Yaesu shares a single offset register between RIT and XIT. To set absolute offset: `RC;` then `RU`/`RD`.

**Verification:** `cargo test -p riglib-yaesu`, `cargo check --workspace`

---

### Phase 7b-3: Kenwood CAT RIT/XIT Command Builders — ✅ DONE

**File:** `crates/riglib-kenwood/src/commands.rs`

**What to add:**

1. Command builders:
   - `cmd_read_rit() -> Vec<u8>` — `RT;` reads RIT on/off
   - `cmd_set_rit_on(on: bool) -> Vec<u8>` — `RT1;` (on) or `RT0;` (off)
   - `cmd_read_xit() -> Vec<u8>` — `XT;` reads XIT on/off
   - `cmd_set_xit_on(on: bool) -> Vec<u8>` — `XT1;` (on) or `XT0;` (off)
   - `cmd_read_rit_xit_offset() -> Vec<u8>` — `RO;` reads current offset
   - `cmd_rit_up() -> Vec<u8>` — `RU;` increment by one step
   - `cmd_rit_down() -> Vec<u8>` — `RD;` decrement by one step
   - `cmd_rit_clear() -> Vec<u8>` — `RC;` clear offset to zero
2. Response parsers:
   - `parse_rit_response(data: &str) -> Result<bool>` — parses `RT0`/`RT1`
   - `parse_xit_response(data: &str) -> Result<bool>` — parses `XT0`/`XT1`
   - `parse_rit_xit_offset_response(data: &str) -> Result<i32>` — parses `RO+XXXXX`/`RO-XXXXX` (5-digit signed)

**Protocol details (Kenwood CAT):**
- `RT`/`XT`: simple on/off toggle
- `RO`: read-only, returns shared offset as `RO+XXXXX;` or `RO-XXXXX;` (5-digit Hz, signed)
- `RU`/`RD`: no parameter — each call steps by rig's configured step size (typically 10 Hz on TS-590, 1 Hz on TS-890)
- `RC`: clears offset to zero
- **Quirk:** Kenwood RIT/XIT share a single offset. To set absolute: `RC;` then N x `RU;`/`RD;`.
- AI mode: rig broadcasts `RT`/`XT` changes unsolicited

**Verification:** `cargo test -p riglib-kenwood`, `cargo check --workspace`

---

### Phase 7b-4: Elecraft CAT RIT/XIT Command Builders — ✅ DONE

**File:** `crates/riglib-elecraft/src/commands.rs`

**What to add:**

Same Kenwood-compatible commands in separate crate:

1. Command builders: `cmd_read_rit()`, `cmd_set_rit_on()`, `cmd_read_xit()`, `cmd_set_xit_on()`, `cmd_read_rit_xit_offset()`, `cmd_rit_up()`, `cmd_rit_down()`, `cmd_rit_clear()`
2. Response parsers: `parse_rit_response()`, `parse_xit_response()`, `parse_rit_xit_offset_response()`

**Protocol details:** Identical to Kenwood. K3/K3S/K4 use RT/XT/RO/RU/RD/RC. K3/K3S range: +/-9999 Hz.

**Verification:** `cargo test -p riglib-elecraft`, `cargo check --workspace`

---

### Phase 7b-5: FlexRadio SmartSDR RIT/XIT Command Builders — ✅ DONE

**Files:** `crates/riglib-flex/src/codec.rs`, `crates/riglib-flex/src/state.rs`

**What to add in `codec.rs`:**

1. Command builders:
   - `cmd_slice_set_rit_on(slice_index: u8, on: bool) -> String` — `slice set {slice_index} rit_on={0|1}`
   - `cmd_slice_set_rit_freq(slice_index: u8, offset_hz: i32) -> String` — `slice set {slice_index} rit_freq={offset_hz}`
   - `cmd_slice_set_xit_on(slice_index: u8, on: bool) -> String` — `slice set {slice_index} xit_on={0|1}`
   - `cmd_slice_set_xit_freq(slice_index: u8, offset_hz: i32) -> String` — `slice set {slice_index} xit_freq={offset_hz}`
2. Add to `SliceStatus`: `rit_on`, `rit_freq`, `xit_on`, `xit_freq` fields
3. Update `parse_slice_status()` with match arms for `"rit_on"`, `"rit_freq"`, `"xit_on"`, `"xit_freq"`

**What to add in `state.rs`:**

- `pub rit_on: bool` (default false), `pub rit_freq_hz: i32` (default 0)
- `pub xit_on: bool` (default false), `pub xit_freq_hz: i32` (default 0)

**Protocol details:** RIT/XIT are per-slice, fully independent (no shared offset). Status updates via `S<handle>|slice <index> rit_on=<0|1> rit_freq=<hz>`.

**Verification:** `cargo test -p riglib-flex`, `cargo check --workspace`

---

### Phase 7c-1: Icom RIT/XIT Rig Wiring — ✅ DONE

**Files:** `crates/riglib-icom/src/rig.rs`, `crates/riglib-icom/src/models.rs`

**What to add in `rig.rs`:**

1. `get_rit()`: send `cmd_read_rit_on()` + `cmd_read_rit_offset()`, return `(enabled, offset_hz)`
2. `set_rit()`: send `cmd_set_rit_on()` + `cmd_set_rit_offset()`, emit `RitChanged`
3. `get_xit()`: send `cmd_read_xit_on()` + `cmd_read_xit_offset()`
4. `set_xit()`: send `cmd_set_xit_on()` + `cmd_set_xit_offset()`, emit `XitChanged`
5. Transceive: match CMD_RIT_XIT (0x21) sub-commands in CI-V response router, emit events

**What to change in `models.rs`:** Set `has_rit: true`, `has_xit: true` for all models.

**Verification:** `cargo test -p riglib-icom`, `cargo check --workspace`

---

### Phase 7c-2: Yaesu RIT/XIT Rig Wiring — ✅ DONE

**Files:** `crates/riglib-yaesu/src/rig.rs`, `crates/riglib-yaesu/src/models.rs`

**What to add in `rig.rs`:**

1. `get_rit()`: send `cmd_read_rit()`, parse response
2. `set_rit()`: send `cmd_set_rit_on()`, then `cmd_rit_clear()` + `cmd_rit_up()`/`cmd_rit_down()`, emit `RitChanged`
3. `get_xit()` / `set_xit()`: same pattern
4. **Shared offset:** setting RIT offset also changes XIT offset

**What to change in `models.rs`:** Set `has_rit: true`, `has_xit: true` for all models.

**Verification:** `cargo test -p riglib-yaesu`, `cargo check --workspace`

---

### Phase 7c-3: Kenwood RIT/XIT Rig Wiring — ✅ DONE

**Files:** `crates/riglib-kenwood/src/rig.rs`, `crates/riglib-kenwood/src/models.rs`

**What to add in `rig.rs`:**

1. `get_rit()`: send `cmd_read_rit()` + `cmd_read_rit_xit_offset()`
2. `set_rit()`: send `cmd_set_rit_on()`, then `cmd_rit_clear()` + N x `cmd_rit_up()`/`cmd_rit_down()`, emit `RitChanged`
3. `get_xit()` / `set_xit()`: same pattern
4. **Clear-and-step:** `RC;` then loop `RU;`/`RD;` N times (step size is model-dependent)
5. AI mode: parse unsolicited `RT`/`XT` messages, emit events

**What to change in `models.rs`:** Set `has_rit: true`, `has_xit: true` for all models.

**Verification:** `cargo test -p riglib-kenwood`, `cargo check --workspace`

---

### Phase 7c-4: Elecraft RIT/XIT Rig Wiring — ✅ DONE

**Files:** `crates/riglib-elecraft/src/rig.rs`, `crates/riglib-elecraft/src/models.rs`

**What to add:** Same pattern as Kenwood (Phase 7c-3). Implement `get_rit/set_rit/get_xit/set_xit` with clear-and-step offset strategy.

**What to change in `models.rs`:** Set `has_rit: true`, `has_xit: true` for all models.

**Verification:** `cargo test -p riglib-elecraft`, `cargo check --workspace`

---

### Phase 7c-5: FlexRadio RIT/XIT Rig Wiring — ✅ DONE

**Files:** `crates/riglib-flex/src/rig.rs`, `crates/riglib-flex/src/client.rs`, `crates/riglib-flex/src/models.rs`

**What to add in `rig.rs`:**

1. `get_rit()`: read from cached `SliceState`
2. `set_rit()`: send `cmd_slice_set_rit_on()` + `cmd_slice_set_rit_freq()`, emit `RitChanged`
3. `get_xit()` / `set_xit()`: same pattern

**What to add in `client.rs`:** In status handler, emit `RitChanged`/`XitChanged` when slice RIT/XIT fields change.

**What to change in `models.rs`:** Set `has_rit: true`, `has_xit: true` for all models.

**Verification:** `cargo test -p riglib-flex`, `cargo check --workspace`

---

### Phase 7 Implementation Order

1. **7b-1 through 7b-5** (command builders) — independent, can be done in any order
2. **7c-1 through 7c-5** (rig wiring) — each depends on its respective 7b sub-phase
3. Recommended: 7c-1 (Icom, has test hardware) → 7c-2 (Yaesu, has test hardware) → 7c-3/7c-4/7c-5

### Done-When Criteria

- All 5 manufacturer command modules have RIT/XIT builders and parsers with unit tests
- All 5 manufacturer rig modules implement get_rit/set_rit/get_xit/set_xit
- All model definitions set has_rit/has_xit = true
- RitChanged/XitChanged events emitted on state changes (explicit sets + transceive/AI broadcasts)
- `cargo test --workspace` passes, `cargo clippy --workspace` clean

---

## Phase 8: CW Message Sending

### Phase 8a: Core Infrastructure — DONE

No work needed. The following already exist:

- **Rig trait methods** in `crates/riglib-core/src/rig.rs`: `send_cw_message(&self, message: &str)` and `stop_cw_message(&self)` with default `Unsupported` error returns
- **Capability flags** in `crates/riglib-core/src/types.rs`: `has_cw_messages: bool` in `RigCapabilities`, defaulting to `false`
- **No new events needed.** CW message sending is fire-and-forget from the rig control perspective. The rig handles keying internally. There is no reliable way to detect when the rig finishes sending, and contest loggers do not need this information — they track message timing locally.

---

### Phase 8b-1: Icom CI-V CW Message Command Builders — ✅ DONE

**File:** `crates/riglib-icom/src/commands.rs`

**What to add:**

1. New constant: `const CMD_SEND_CW_MESSAGE: u8 = 0x17;`
2. Command builders:
   - `cmd_send_cw_message(addr: u8, text: &str) -> Vec<u8>` — builds a CI-V 0x17 frame with ASCII text payload
   - `cmd_stop_cw_message(addr: u8) -> Vec<u8>` — builds a CI-V 0x17 frame with a single `0xFF` byte to abort

**Protocol details (CI-V 0x17):**
- Frame: `FE FE <addr> <ctrl> 0x17 <ascii bytes> FD`
- The text payload is raw ASCII — each character is sent as its standard ASCII byte value (e.g., `'C'` = 0x43, `'Q'` = 0x51)
- Maximum payload per frame: 30 characters. Longer messages must be chunked by the rig wiring layer (Phase 8c-1).
- To abort a message in progress: send 0x17 with a single `0xFF` data byte
- The rig ACKs (`FB`) each frame. No sub-command byte — 0x17 has no sub-commands.
- Supported characters: A-Z, 0-9, space, and standard CW prosigns (varies by model). The rig silently ignores unsupported characters.
- **No `encode_frame` sub-command:** Pass `None` as the `sub_cmd` parameter. The data portion is the raw ASCII bytes (or `[0xFF]` for stop).

**Verification:** `cargo test -p riglib-icom`, `cargo check --workspace`

---

### Phase 8b-2: Yaesu CAT CW Message Command Builders — ✅ DONE

**File:** `crates/riglib-yaesu/src/commands.rs`

**What to add:**

1. Command builders:
   - `cmd_send_cw_message(text: &str) -> Vec<u8>` — `KY{text};` where text is space-padded to fill the field
   - `cmd_read_cw_buffer() -> Vec<u8>` — `KY;` to query buffer status
   - `cmd_stop_cw_message() -> Vec<u8>` — sends `KY` with all-spaces payload to flush/abort
2. Response parser:
   - `parse_cw_buffer_response(data: &str) -> Result<bool>` — returns `true` if buffer can accept more text (`0` = buffer not full, `1` = buffer full)

**Protocol details (Yaesu KY):**
- Format: `KY{text};` — text field is up to 24 characters
- A single space must separate `KY` from the first character of text (i.e., `KY message;` not `KYmessage;`). Use `encode_command("KY", &format!(" {text}"))` or equivalent.
- Buffer status query: send `KY;`, response is `KY0;` (buffer ready) or `KY1;` (buffer full)
- To clear/abort: send `KY` with 24 spaces as the text payload
- Characters supported: A-Z, 0-9, space, `/`, `?`, `-`, `.`, `,` and some prosigns
- The rig buffers text and keys it at the configured keyer speed. When the buffer is full, the rig returns `KY1;` on a buffer query — the caller must wait and retry.

**Verification:** `cargo test -p riglib-yaesu`, `cargo check --workspace`

---

### Phase 8b-3: Kenwood CAT CW Message Command Builders — ✅ DONE

**File:** `crates/riglib-kenwood/src/commands.rs`

**What to add:**

1. Command builders:
   - `cmd_send_cw_message(text: &str) -> Vec<u8>` — `KY {text};` where text is exactly 24 characters, right-padded with spaces
   - `cmd_read_cw_buffer() -> Vec<u8>` — `KY;` to query buffer status
   - `cmd_stop_cw_message() -> Vec<u8>` — sends `KY` with 24 spaces to flush/clear buffer
2. Response parser:
   - `parse_cw_buffer_response(data: &str) -> Result<bool>` — returns `true` if buffer can accept more text

**Protocol details (Kenwood KY):**
- Format: `KY {text};` — **exactly one space** between `KY` and the text field
- Text field: fixed 24 characters, right-padded with spaces if shorter. Example: `KY CQ CQ DE W1AW          ;`
- Buffer status: `KY;` response is `KY0;` (ready) or `KY1;` (full)
- To abort: send `KY` with 24 spaces, or wait for the buffer to drain
- **Quirk:** The TS-590 requires the text field to be exactly 24 characters (space-padded). The TS-890/TS-990 are more lenient but space-padding works universally.
- Characters: A-Z, 0-9, common punctuation, `/`, `?`, `=`, prosigns via special sequences (model-dependent)

**Verification:** `cargo test -p riglib-kenwood`, `cargo check --workspace`

---

### Phase 8b-4: Elecraft CAT CW Message Command Builders — ✅ DONE

**File:** `crates/riglib-elecraft/src/commands.rs`

**What to add:**

1. Command builders:
   - `cmd_send_cw_message(text: &str) -> Vec<u8>` — `KY {text};` with 24-character space-padded text field
   - `cmd_read_cw_buffer() -> Vec<u8>` — `KY;` to query buffer status
   - `cmd_stop_cw_message() -> Vec<u8>` — sends `KY` with 24 spaces
2. Response parser:
   - `parse_cw_buffer_response(data: &str) -> Result<bool>` — returns `true` if buffer ready

**Protocol details:** Identical to Kenwood (Phase 8b-3). K3/K3S/K4 all implement the standard Kenwood `KY` command. The 24-character fixed-width format applies.

**Verification:** `cargo test -p riglib-elecraft`, `cargo check --workspace`

---

### Phase 8b-5: FlexRadio SmartSDR CW Message Command Builders — ✅ DONE

**File:** `crates/riglib-flex/src/codec.rs`

**What to add:**

1. Command builders:
   - `cmd_cwx_send(text: &str) -> String` — `cwx send "{escaped_text}"` where spaces in text are replaced with `\x7F` (DEL character, 0x7F) per SmartSDR convention
   - `cmd_cwx_clear() -> String` — `cwx clear` to abort any in-progress CW message
2. Helper:
   - `escape_cwx_text(text: &str) -> String` — replaces spaces with `\x7F` for CWX protocol encoding

**Protocol details (SmartSDR CWX):**
- Send: `cwx send "{text}"` — the text is enclosed in double quotes. Spaces within the text must be replaced with the DEL character (0x7F) because SmartSDR uses space as a parameter delimiter.
- Clear/abort: `cwx clear` — immediately stops CW output and clears the buffer
- The radio ACKs the command via the standard SmartSDR response mechanism (`R<seq>|0|` for success)
- CWX is **not** per-slice — it uses the radio's global CW transmitter
- SmartSDR also supports `cwx wpm <speed>` (handled separately by CW speed commands) and `cwx delay <ms>` (hang time, not needed for basic message sending)
- Maximum message length: effectively unlimited (SmartSDR buffers internally), but practical limit around 200 characters per command

**Verification:** `cargo test -p riglib-flex`, `cargo check --workspace`

---

### Phase 8c-1: Icom CW Message Rig Wiring — ✅ DONE

**Files:** `crates/riglib-icom/src/rig.rs`, `crates/riglib-icom/src/models.rs`

**What to add in `rig.rs`:**

1. `send_cw_message(&self, message: &str) -> Result<()>`:
   - Chunk the message into segments of at most 30 characters each
   - For each chunk: build frame via `commands::cmd_send_cw_message(self.civ_address, chunk)`, then `self.execute_ack_command(&cmd).await?`
   - No inter-chunk delay needed — the rig buffers internally and ACKs each frame
2. `stop_cw_message(&self) -> Result<()>`:
   - Build frame via `commands::cmd_stop_cw_message(self.civ_address)`, then `self.execute_ack_command(&cmd).await?`

**What to change in `models.rs`:** Set `has_cw_messages: true` for all models (IC-7300, IC-7600, IC-7610, IC-7851, IC-905 all support 0x17).

**Verification:** `cargo test -p riglib-icom`, `cargo check --workspace`

---

### Phase 8c-2: Yaesu CW Message Rig Wiring — ✅ DONE

**Files:** `crates/riglib-yaesu/src/rig.rs`, `crates/riglib-yaesu/src/models.rs`

**What to add in `rig.rs`:**

1. `send_cw_message(&self, message: &str) -> Result<()>`:
   - Chunk the message into segments of at most 24 characters each
   - For each chunk:
     - Query buffer status: `commands::cmd_read_cw_buffer()` via `self.execute_command()`, parse with `commands::parse_cw_buffer_response()`
     - If buffer full, wait briefly (50-100ms) and retry (with a reasonable retry limit, e.g. 50 retries = ~5 seconds)
     - If buffer ready, send: `commands::cmd_send_cw_message(chunk)` via `self.execute_command()`
2. `stop_cw_message(&self) -> Result<()>`:
   - Send `commands::cmd_stop_cw_message()` via `self.execute_command()`

**What to change in `models.rs`:** Set `has_cw_messages: true` for all models (FT-DX10, FT-DX101D, FT-710, FTDX5000 all support KY).

**Verification:** `cargo test -p riglib-yaesu`, `cargo check --workspace`

---

### Phase 8c-3: Kenwood CW Message Rig Wiring — ✅ DONE

**Files:** `crates/riglib-kenwood/src/rig.rs`, `crates/riglib-kenwood/src/models.rs`

**What to add in `rig.rs`:**

1. `send_cw_message(&self, message: &str) -> Result<()>`:
   - Chunk the message into segments of at most 24 characters each
   - For each chunk:
     - Query buffer: `commands::cmd_read_cw_buffer()` via `self.execute_command()`, parse response
     - If buffer full, sleep briefly and retry (same pattern as Yaesu)
     - If buffer ready, send: `commands::cmd_send_cw_message(chunk)` via `self.execute_set_command()`
2. `stop_cw_message(&self) -> Result<()>`:
   - Send `commands::cmd_stop_cw_message()` via `self.execute_set_command()`

**What to change in `models.rs`:** Set `has_cw_messages: true` for all models (TS-590SG, TS-890S, TS-990S all support KY).

**Verification:** `cargo test -p riglib-kenwood`, `cargo check --workspace`

---

### Phase 8c-4: Elecraft CW Message Rig Wiring — ✅ DONE

**Files:** `crates/riglib-elecraft/src/rig.rs`, `crates/riglib-elecraft/src/models.rs`

**What to add:** Same pattern as Kenwood (Phase 8c-3). Implement `send_cw_message` with 24-character chunking and buffer polling, `stop_cw_message` with 24-space payload.

**What to change in `models.rs`:** Set `has_cw_messages: true` for all models (K3, K3S, K4 all support KY).

**Verification:** `cargo test -p riglib-elecraft`, `cargo check --workspace`

---

### Phase 8c-5: FlexRadio CW Message Rig Wiring — ✅ DONE

**Files:** `crates/riglib-flex/src/rig.rs`, `crates/riglib-flex/src/models.rs`

**What to add in `rig.rs`:**

1. `send_cw_message(&self, message: &str) -> Result<()>`:
   - Build command via `codec::cmd_cwx_send(message)`
   - Send via `self.client.send_command(&cmd).await?`
   - No chunking needed — SmartSDR handles arbitrary-length messages internally
2. `stop_cw_message(&self) -> Result<()>`:
   - Build command via `codec::cmd_cwx_clear()`
   - Send via `self.client.send_command(&cmd).await?`

**What to change in `models.rs`:** Set `has_cw_messages: true` for all models (FLEX-6400, FLEX-6600, FLEX-6700 all support CWX).

**Verification:** `cargo test -p riglib-flex`, `cargo check --workspace`

---

### Phase 8 Implementation Order

1. **8b-1 through 8b-5** (command builders) — independent, can be done in any order
2. **8c-1 through 8c-5** (rig wiring) — each depends on its respective 8b sub-phase
3. Recommended: 8c-1 (Icom, has test hardware) -> 8c-2 (Yaesu, has test hardware) -> 8c-3/8c-4/8c-5

### Done-When Criteria

- All 5 manufacturer command modules have CW message builders (and buffer parsers where applicable) with unit tests
- All 5 manufacturer rig modules implement `send_cw_message` and `stop_cw_message`
- All model definitions set `has_cw_messages = true`
- Icom implementation handles chunking for messages > 30 characters
- Yaesu/Kenwood/Elecraft implementations handle chunking for messages > 24 characters with buffer-full polling
- FlexRadio implementation escapes spaces to 0x7F in CWX text
- `cargo test --workspace` passes, `cargo clippy --workspace` clean

---

## Phase 9: Kenwood Transceive (AI Mode) — ✅ DONE

`cmd_set_ai()` already exists in Kenwood commands.

### Files to create/modify:
- **`crates/riglib-kenwood/src/transceive.rs`** (NEW) — Following Icom transceive pattern:
  - `CommandRequest` enum, `TransceiveHandle` with mpsc sender
  - `spawn_reader_task()` taking transport ownership
  - Reader loop: `tokio::select! { biased; }` over command channel + transport reads
  - Parse unsolicited AI responses (`FA00014250000;`, `MD01;`) → emit `RigEvent`s
  - Send `AI2` on connect, `AI0` on disconnect
- **`crates/riglib-kenwood/src/rig.rs`** — Transport handoff via `std::mem::replace` with `DisconnectedTransport`, route commands through handle when active
- **`crates/riglib-kenwood/src/lib.rs`** — Add `mod transceive;`

### Verification: `cargo check --workspace`, `cargo test --workspace`

---

## Phase 10: Elecraft Transceive (AI Mode) — ✅ DONE

Same pattern as Phase 9. `cmd_set_ai()` already exists. Elecraft uses Kenwood-style AI protocol.

### Files to create/modify:
- **`crates/riglib-elecraft/src/transceive.rs`** (NEW)
- **`crates/riglib-elecraft/src/rig.rs`** — Add transceive support
- **`crates/riglib-elecraft/src/lib.rs`** — Add `mod transceive;`

### Verification: `cargo check --workspace`, `cargo test --workspace`

---

## Phase 11: Yaesu Transceive (AI Mode) — ✅ DONE

Need to add AI command first (not in commands.rs yet).

### Files to create/modify:
- **`crates/riglib-yaesu/src/commands.rs`** — Add `cmd_set_ai()` (`AI0;` / `AI2;`)
- **`crates/riglib-yaesu/src/transceive.rs`** (NEW) — Parse Yaesu-format unsolicited responses
- **`crates/riglib-yaesu/src/rig.rs`** — Add transceive support
- **`crates/riglib-yaesu/src/lib.rs`** — Add `mod transceive;`

### Verification: `cargo check --workspace`, `cargo test --workspace`

---

## Phase 12a: Transceive Trait — enable_transceive() Wiring — ✅ DONE

Wire each backend's existing `enable_transceive()` / `enable_ai_mode()` inherent method
to the `Rig` trait's `enable_transceive()` method (already defined with default
`Err(Unsupported)` impl). `disable_transceive()` returns `Err(Unsupported)` for now.

### Files to modify:
- **`crates/riglib-icom/src/rig.rs`** — Override `enable_transceive` in `impl Rig`, delegate to inherent method. `disable_transceive` → `Err(Unsupported)`.
- **`crates/riglib-elecraft/src/rig.rs`** — Same pattern, delegate to `enable_ai_mode()`.
- **`crates/riglib-kenwood/src/rig.rs`** — Same pattern, delegate to `enable_ai_mode()`.
- **`crates/riglib-yaesu/src/rig.rs`** — Same pattern, delegate to `enable_ai_mode()`.
- **`crates/riglib-flex/src/rig.rs`** — `enable_transceive` → `Ok(())` (always-on). `disable_transceive` → `Err(Unsupported)`.
- **`test-app/src/main.rs`** — Update call sites to use trait method with `Result` return.

### Verification: `cargo check --workspace`, `cargo test --workspace`

---

## Phase 12b: Transceive Trait — disable_transceive() Implementation — ✅ DONE

Implement real `disable_transceive()` for each serial backend: shut down the background
reader task, send "AI off" command (CI-V transceive off for Icom, `AI0;` for
Kenwood/Elecraft/Yaesu), and return transport ownership to direct mode.

### Files to modify:
- **`crates/riglib-icom/src/transceive.rs`** — Add `shutdown()` to `TransceiveHandle` that signals the reader task to stop, sends CI-V transceive-off, and returns the transport.
- **`crates/riglib-icom/src/rig.rs`** — Override `disable_transceive` in `impl Rig`, call `TransceiveHandle::shutdown()`, restore transport to direct mode.
- **`crates/riglib-elecraft/src/transceive.rs`** — Same pattern, send `AI0;` on shutdown.
- **`crates/riglib-elecraft/src/rig.rs`** — Override `disable_transceive`.
- **`crates/riglib-kenwood/src/transceive.rs`** — Same pattern, send `AI0;` on shutdown.
- **`crates/riglib-kenwood/src/rig.rs`** — Override `disable_transceive`.
- **`crates/riglib-yaesu/src/transceive.rs`** — Same pattern, send `AI0;` on shutdown.
- **`crates/riglib-yaesu/src/rig.rs`** — Override `disable_transceive`.
- **`crates/riglib-flex/src/rig.rs`** — Stays `Err(Unsupported)` (always-on, no shutdown).

### Verification: `cargo check --workspace`, `cargo test --workspace`

---

## Phase 13: RigCapabilities Updates — ✅ DONE

Populate the new `RigCapabilities` fields (from Phase 1) for every model definition.

### Files to modify:
- **`crates/riglib-icom/src/models.rs`** — All 13 Icom models
- **`crates/riglib-yaesu/src/models.rs`** — All Yaesu models
- **`crates/riglib-kenwood/src/models.rs`** — All Kenwood models
- **`crates/riglib-elecraft/src/models.rs`** — All Elecraft models
- **`crates/riglib-flex/src/models.rs`** — All FlexRadio models

### Verification: `cargo check --workspace`, `cargo test --workspace`

---

## Phase 14: Test App Updates — ✅ DONE

Add CLI options and interactive commands for all new features.

### Files to modify:
- **`test-app/src/main.rs`** — Add commands:
  - `speed [wpm]` — get/set CW speed
  - `vfo-eq` / `vfo-swap` — VFO operations
  - `ant <1-4>` — antenna port
  - `agc <off|fast|med|slow>` — AGC mode
  - `preamp <off|1|2>` / `att <off|6|12|18>` — preamp/attenuator
  - `rit <on|off> [offset]` / `xit <on|off> [offset]` — RIT/XIT
  - `cw <message>` / `cw-stop` — CW message sending
  - `transceive <on|off>` — AI mode

### Verification: `cargo build --workspace`, `cargo clippy --workspace`, `cargo fmt --all -- --check`, `cargo test --workspace`

---

## Phase Dependency Order

```
Phase 1  (core types)
   ↓
Phase 2  (trait methods) ←──┐
Phase 3  (commands)     ←───┤ can be done together
   ↓                        │
Phase 4  (wiring)       ────┘ depends on 2 + 3
   ↓
Phase 5a (AGC trait) → 5b (AGC commands) → 5c-1 through 5c-5 (AGC wiring, one per manufacturer, independent of each other)
Phase 6-8 (preamp/att, RIT/XIT, CW messages) — sequential, each self-contained
   ↓
Phase 9-11 (transceive: Kenwood, Elecraft, Yaesu) — independent of each other
   ↓
Phase 12a (transceive enable wiring) — depends on 9+10+11
   ↓
Phase 12b (transceive disable impl) — depends on 12a
   ↓
Phase 13 (capabilities) — can happen anytime after Phase 1
   ↓
Phase 14 (test app) — depends on everything
```

## Session Strategy

Each phase ends with a clean `cargo check`/`cargo test`. If a session runs out of context, start a new session and say "continue from Phase N".
