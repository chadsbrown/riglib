# Attenuator API Rework

## Problem

The `Rig` trait API uses dB values (`get_attenuator() -> u8`, `set_attenuator(db: u8)`),
and `RigCapabilities::attenuator_levels` lists the valid dB values a caller can pass.

This works correctly for **Kenwood**, where the CAT command `RA` takes actual dB values
(`RA00`, `RA06`, `RA12`, `RA18`) — the dB value passes straight through.

But for **Yaesu**, the `RA0` command is binary on/off (`0` or `1`). There's no way to
select a specific dB step via CAT. The radio has one physical attenuator button that cycles
through steps on the front panel, but CAT only sees on/off.

**Elecraft** is also binary on/off, and already correctly has `attenuator_levels: vec![]`.

## Current Behavior (Broken)

Yaesu models currently declare `attenuator_levels: vec![0, 6, 12, 18]`, implying 4
selectable steps. But CAT only provides 2: off and on.

- `set_attenuator(18)` → sends `RA01;` (on) → radio activates its single fixed attenuator
- `get_attenuator()` → reads `RA01;` (on) → maps to 6dB (first non-zero entry in capabilities)
- Result: user asked for 18dB, radio reports 6dB — inconsistent

The `get_attenuator` mapping picks the first non-zero entry from `attenuator_levels` (6),
which doesn't match the hardware. The FT-DX10's attenuator is actually ~12dB.

## Proposed Fix

Each Yaesu model's `attenuator_levels` should list only `[0, <actual_dB>]`, reflecting
the binary on/off nature and the actual hardware dB level:

| Model       | Current                | Proposed     | Actual HW  |
|-------------|------------------------|--------------|------------|
| FT-DX10     | `[0, 6, 12, 18]`      | `[0, 12]`    | ~12dB ATT  |
| FT-891      | `[]`                   | TBD          | needs research |
| FT-991A     | `[0, 6, 12, 18]`      | `[0, 12]`    | ~12dB ATT  |
| FT-DX101D   | `[0, 6, 12, 18]`      | `[0, 20]`    | ~20dB ATT  |
| FT-DX101MP  | `[0, 6, 12, 18]`      | `[0, 20]`    | ~20dB ATT  |
| FT-710      | `[0, 6, 12, 18]`      | `[0, 12]`    | ~12dB ATT  |

**Note:** The actual dB values need to be verified against Yaesu specs/manuals for each model.

### Code Changes

1. **`models.rs`** — Update `attenuator_levels` per table above
2. **`rig.rs` `get_attenuator`** — The existing logic already maps on→first non-zero entry,
   so once capabilities are correct it will return the right dB value
3. **`rig.rs` `set_attenuator`** — Already maps non-zero→1, so no change needed
4. **`rig.rs` tests** — Update test assertions for the new dB values

### Elecraft

Elecraft models have `attenuator_levels: vec![]` (empty), which means the attenuator trait
methods return `Unsupported`. If we want to support them, we'd need to research the actual
dB values and add entries. The K3/K3S has a 10dB attenuator, the K4 has a switchable
attenuator. The set/get implementation already exists but is unreachable due to empty
capabilities.

### Kenwood

No changes needed — Kenwood CAT sends actual dB values and the current implementation is
correct.

## Alternative Approaches to Consider

1. **Keep dB-based API but validate against capabilities** — `set_attenuator(db)` could
   reject values not in `attenuator_levels`. Currently any non-zero value is silently
   accepted and mapped to "on".

2. **Binary API for binary rigs** — Could have the trait expose an `AttenuatorMode` enum
   (Off, On, or specific dB levels) instead of raw dB. More type-safe but more complex.

3. **Status quo with correct capabilities** — Simplest fix. Just correct the
   `attenuator_levels` to match reality. Callers check capabilities to know what values
   are valid.

Option 3 (correct capabilities) seems like the right balance of simplicity and correctness.
