# Per-Model Command Profile Migration Plan

## Objective

Replace ad-hoc model conditionals in manufacturer backends with explicit, testable
per-model command profiles, while preserving backend separation (Yaesu, Kenwood,
Elecraft, Icom) and avoiding a leaky cross-vendor abstraction.

This plan is intentionally incremental:
- Phase 1 creates profile scaffolding with no behavior change.
- Phase 2 migrates audited high-risk command families.
- Phase 3 expands coverage family-by-family.

## Why this architecture

Current state:
- `models.rs` captures model identity/capabilities.
- `rig.rs` contains model-branching for command semantics in several places.
- `commands.rs` mostly encodes backend-wide assumptions.

Target state:
- `models.rs`: model metadata + `profile_id` selection.
- `profiles.rs` (new per backend): declarative command-semantic differences.
- `rig.rs`: command selection reads `self.profile` fields, not scattered `if/match`.
- `commands.rs`: low-level builders/parsers remain reusable; profile decides which
  builders/parsers are used and how values map.

This keeps complexity local to each backend and makes model behavior auditable.

## Non-goals

- Do not build a single cross-manufacturer "universal profile" trait.
- Do not attempt full protocol completeness in one pass.
- Do not change transport, io task, or event architecture.

## Scope (initial)

Phase-2 migration scope comes from existing audit findings:
- Yaesu: `RM` selector mapping and `GT` AGC mapping semantics.
- Kenwood: `RM` selector mapping and AGC command-style semantics.
- Icom: antenna command semantics, AGC dual-path behavior, data-mode readiness.
- Elecraft: formalize existing K3/K4 split under profile fields.

## Proposed backend-local types

Each backend gets a `profiles.rs` with:

```rust
// crates/riglib-<backend>/src/profiles.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileId {
    // backend-specific variants, e.g.:
    // YaesuFt991Family,
    // KenwoodGcSimple,
    // IcomSdrCiv,
}

#[derive(Debug, Clone, Copy)]
pub struct CommandProfile {
    pub id: ProfileId,

    // Meter family behavior
    pub meter: MeterProfile,

    // AGC behavior (command family + value mapping)
    pub agc: AgcProfile,

    // Antenna command behavior (shape, subcommands, mapping)
    pub antenna: AntennaProfile,

    // Mode/data-mode strategy
    pub mode: ModeProfile,

    // Optional: attenuator/preamp semantics where model meaning diverges
    pub gain: GainProfile,
}

pub fn profile_for_model(model: &<BackendModelType>) -> CommandProfile {
    // single mapping table here
}
```

Recommended sub-structures (backend-specific fields allowed):

```rust
pub enum MeterSelectorStyle { /* ... */ }
pub struct MeterProfile {
    pub swr_selector: u8,
    pub alc_selector: u8,
    pub s_meter_selector: Option<u8>,
}

pub enum AgcCommandStyle { /* existing enums can be reused */ }
pub struct AgcProfile {
    pub style: AgcCommandStyle,
    pub read_mapping: &'static [(u8, riglib_core::AgcMode)],
    pub write_mapping: &'static [(riglib_core::AgcMode, u8)],
}

pub enum AntennaStyle { SimpleByte, Subcommanded, Unsupported }
pub struct AntennaProfile {
    pub style: AntennaStyle,
    pub supports_rx_ant: bool,
}

pub enum DataModeStyle { None, SharedModeByte, SeparateCommand }
pub struct ModeProfile {
    pub data_mode: DataModeStyle,
}

pub struct GainProfile {
    pub has_preamp2: bool,
    pub attenuator_binary: bool,
}
```

Notes:
- Prefer existing enums/fields when they already model reality (example:
  Kenwood `AgcCommandStyle`, Elecraft `is_k4`).
- Use static tables for raw<->semantic value conversion so mappings are explicit
  and testable.

## File-level change plan

### Yaesu

1. Add `crates/riglib-yaesu/src/profiles.rs`.
2. Export module in `crates/riglib-yaesu/src/lib.rs`.
3. Add `profile_id` (or derive via model name) in `crates/riglib-yaesu/src/models.rs`.
4. Add `profile: CommandProfile` field to `YaesuRig` in
   `crates/riglib-yaesu/src/rig.rs` and initialize in builder/new.
5. Update AGC/meter/antenna command selection in `rig.rs` to use profile fields.
6. Update `commands.rs` only where new builders are required; keep builders
   generic and dumb.

### Kenwood

1. Add `crates/riglib-kenwood/src/profiles.rs`.
2. Reuse `AgcCommandStyle` from model or mirror in profile.
3. Move RM selector semantics into profile.
4. Add `profile` to `KenwoodRig` and route AGC+meter paths through it.
5. Keep existing behavior unchanged in Phase 1; flip to corrected mapping in
   Phase 2 with tests.

### Elecraft

1. Add `crates/riglib-elecraft/src/profiles.rs`.
2. Map K3-family and K4-family to profiles.
3. Replace direct `if self.model.is_k4` branches in passband/AGC/attenuator
   with profile-driven dispatch (behavior-preserving).
4. Keep `is_k4` in model for compatibility, but avoid using it directly from
   control paths after migration.

### Icom

1. Add `crates/riglib-icom/src/profiles.rs`.
2. Profile fields for antenna style and AGC/data-mode strategies.
3. Add `profile` to `IcomRig`; use profile for choosing:
   - AGC read/write path (`16 12` vs `1A 04` interactions).
   - Antenna encoding strategy.
   - Data-mode command path readiness (`1A 06` where supported).
4. Add targeted builders/parsers only where profile requires variant command
   frames.

## Migration phases

## Phase 1: Scaffolding (no semantic change)

Goal:
- Introduce profiles and wire them in without changing emitted command bytes.

Tasks:
1. Add `profiles.rs` + `profile_for_model` in each backend.
2. Add `profile` field to each `*Rig` struct.
3. Replace direct model flags in a few low-risk call sites with equivalent
   profile fields (mechanical).
4. Run full test suite and ensure no behavior changes.

Acceptance:
- No command snapshot changes.
- Existing tests pass.

## Phase 2: High-risk audited fixes

Goal:
- Correct known incorrect mappings using profile data.

Tasks:
1. Yaesu:
   - Fix RM SWR/ALC selectors via profile table.
   - Fix AGC mapping semantics using model-family profile mapping table.
2. Kenwood:
   - Fix RM SWR/ALC selector mapping.
   - Ensure AGC style selection fully profile-driven.
3. Icom:
   - Introduce profile-driven antenna strategy and correct abstraction gaps.
   - Ensure AGC off/read semantics handled per profile.
4. Elecraft:
   - Normalize K3/K4 branching under profile and verify parity.

Acceptance:
- Regression tests prove fixed command bytes and parse behavior.
- Audit known incorrect rows updated accordingly.

## Phase 3: Expand coverage

Goal:
- Migrate remaining command families from ad-hoc conditionals to profile-driven
  dispatch in small batches.

Batch order recommendation:
1. Mode/data-mode.
2. Meter and gain controls.
3. RIT/XIT and split/VFO operations.
4. CW/message related edges.

Acceptance:
- For each batch: new profile tests + command fixtures + no unrelated regressions.

## Test strategy (required)

## 1) Profile mapping tests

Per backend, add tests in `profiles.rs`:
- Every supported model maps to exactly one `ProfileId`.
- Profile fields match intended family behavior.

Example:
- `ft_991a()` -> `YaesuFt991FamilyProfile`.
- `ts_890s()` -> `KenwoodGcSimpleProfile`.

## 2) Command byte snapshot tests

In `commands.rs`/`rig.rs` tests:
- Verify emitted bytes for high-risk commands using model+profile pairs.
- Add one test per affected model family, not only one representative model.

## 3) Parse/mapping round-trip tests

- Raw protocol values -> semantic enums -> raw values where applicable.
- AGC mapping tables should be tested both read and write directions.

## 4) Behavior tests with mock transport

Using `riglib-test-harness`:
- `set_*` with verify mode should read back and compare profile-aware semantics.
- Errors for unsupported profile features should be deterministic.

## 5) Coverage guardrails

Add a test asserting profile table completeness:
- `all_<backend>_models()` every element has a valid profile.

## Audit + docs integration

Update `docs/command-audit.md` after each phase:
- Add a `Profile coverage` section:
  - `not wired`
  - `wired/no behavior change`
  - `wired+validated`
- For each command family row, include profile coverage note and evidence source.

Also add backend docs snippets:
- `crates/riglib-<backend>/README`/module docs describing how to add a new model:
  1. model definition
  2. profile mapping
  3. command tests
  4. audit update

## Risks and mitigations

1. Risk: Overfitting profiles to one model.
- Mitigation: profile IDs represent model families; require >1 model evidence
  where possible.

2. Risk: Hidden behavior changes during refactor.
- Mitigation: Phase 1 must be behavior-preserving with command snapshot parity.

3. Risk: Incomplete evidence for some command families.
- Mitigation: keep those profile fields marked conservative/default; do not
  change semantics until evidence is upgraded.

4. Risk: Profile sprawl.
- Mitigation: add fields only for proven divergence points; reject speculative
  fields.

## Deliverables checklist

- [ ] `profiles.rs` in each serial backend.
- [ ] `profile` wired into each `*Rig`.
- [ ] Phase-1 parity tests green.
- [ ] Phase-2 fixes merged with regression tests.
- [ ] Audit updated (present vs future + profile coverage).
- [ ] Contributor note for adding model/profile.

## Suggested execution order (PR slices)

1. PR1: Yaesu + Kenwood profile scaffolding (no behavior change).
2. PR2: Elecraft + Icom profile scaffolding (no behavior change).
3. PR3: Yaesu/Kenwood audited RM+AGC fixes.
4. PR4: Icom antenna + AGC profile migration.
5. PR5+: Remaining families in small, test-backed increments.

## Exit criteria

- High-risk audited command families are profile-driven and validated.
- New model onboarding path is deterministic (model -> profile -> tests).
- No new backend logic introduces raw model conditionals for migrated families.
- Audit shows which commands are validated now and which remain future/pending.
