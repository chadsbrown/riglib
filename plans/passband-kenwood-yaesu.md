# Passband cleanup: Kenwood and Yaesu follow-up

## Background

`Passband` in riglib-core (`crates/riglib-core/src/types.rs`) is documented as
hertz. Every backend's `get_passband` should return true Hz and
`set_passband` should accept Hz. Icom was fixed in the same change set that
produced this plan — see `crates/riglib-icom/src/filter.rs` and the rewritten
handlers in `crates/riglib-icom/src/rig.rs`. Kenwood and Yaesu are still
wrong or unimplemented; this file tracks what's left.

Reference standard:

- **Elecraft** (`crates/riglib-elecraft/src/rig.rs`) — returns true Hz via
  `parse_bandwidth_response`. Gold standard, no change needed.
- **FlexRadio** (`crates/riglib-flex/src/rig.rs`) — computes
  `filter_hi - filter_lo` from cached slice state. Genuinely Hz, correct.
- **Icom** (`crates/riglib-icom/src/filter.rs`) — now uses per-mode index
  lookup tables matching hamlib's `filtericom[]`. Correct.

## Kenwood — current state

File: `crates/riglib-kenwood/src/rig.rs:362`

```rust
// Rough mapping: index 0 = narrowest (200 Hz), index 31 = widest (5000 Hz).
// Linear interpolation provides a reasonable approximation.
let hz = 200 + index * 150;
```

This is a **made-up linear approximation**. The `SH` command returns a
filter slot index (0–99 range on modern Kenwoods), and the real mapping is
non-linear and model-dependent:

- **TS-590S / TS-590SG** — `SH` returns a "high-cut" index (0–11 for SSB,
  different for CW). Combined with `SL` (low-cut) to compute effective
  bandwidth. The current "index × 150 + 200" formula is wrong for every
  valid index.
- **TS-890S** — different `SH`/`SL` range, and separate `FW` command on
  some modes.
- **TS-990S** — similar to TS-890 but with extra main/sub nuances.

## Kenwood — what needs doing

1. **Verify against hamlib.** Look at `rigs/kenwood/kenwood.c` and the
   per-model files (`ts590.c`, `ts890s.c`, `ts990s.c`) for their
   `SH`/`SL`/`FW` index tables. These are probably split per model because
   the mappings differ.
2. **Decide on the SH/SL combination policy.** Kenwood's bandwidth is not
   a single number — it's `high_cut_hz - low_cut_hz`. We need to either
   read both and compute the difference (two round-trips) or cache one.
   Match whatever hamlib does for the target model.
3. **Add per-model filter tables** analogous to
   `riglib-icom/src/filter.rs`. Kenwood supports more model variation
   than Icom does in this area, so the `KenwoodModel` enum likely needs to
   carry a filter-table pointer or model-family discriminant.
4. **Round-trip tests** for each model table.
5. **Bench-verify.** No Kenwood hardware is currently available
   (see `MEMORY.md`), so this change either waits for hardware access or
   ships with table values taken verbatim from hamlib + a clear note in
   the PR that it's unverified on real hardware.

The current linear fudge at least doesn't panic, so leaving it in place
until someone has a radio to test against is acceptable. The PR should
prefer "correct mapping verified on one model" over "wildly-guessed
mapping applied to all models." Start with one model (TS-590SG is
probably the most common) and expand.

## Yaesu — current state

File: `crates/riglib-yaesu/src/rig.rs:357`

```rust
async fn get_passband(&self, _rx: ReceiverId) -> Result<Passband> {
    Err(Error::Unsupported(
        "Yaesu passband read not yet implemented (model-dependent)".into(),
    ))
}
```

Both `get_passband` and `set_passband` return `Unsupported`. Nothing to
"fix" — just to implement.

## Yaesu — what needs doing

1. **Understand the Yaesu filter command family.** Yaesu CAT has
   `SH` (roofing filter select on FTDX-class rigs), `NA0`/`NA1` (narrow
   toggle on older models), and newer rigs also expose `BW`/`FW` or
   equivalent. The specific command and encoding varies significantly
   across:
   - **FT-DX10 / FTDX-101 family** — `SH` reads a 2-digit width index
     with a non-linear mapping similar in spirit to Icom.
   - **FT-991 / FT-991A** — different encoding again.
   - **FT-710** — more recent, check reference.
2. **Build per-model filter tables.** Yaesu varies more than Icom across
   the supported models; each needs its own table. Consider a
   `YaesuFilterTable` trait or enum matching the pattern used by Icom's
   `filter.rs`.
3. **Handle mode-aware decoding.** Like Icom, Yaesu's filter width
   interpretation depends on current mode (SSB vs CW vs AM). `get_mode`
   first, then `SH`, then decode.
4. **FT-DX10 bench validation.** Test hardware is available
   (see `MEMORY.md`) so at least one model can be properly verified.
5. **Other models stay `Unsupported` until implemented.** Avoid the
   temptation to ship a "generic" table that's wrong for every rig.

## Suggested PR sequencing

- **PR 1** — Yaesu FT-DX10 filter table + `get_passband` / `set_passband`
  implementation, bench-validated. Other Yaesu models remain `Unsupported`.
- **PR 2** — Kenwood TS-590SG filter table (from hamlib, unverified on
  real hardware), with a clear note in the PR description. Other Kenwood
  models remain on the linear fudge with a `debug!` warning added so it's
  obvious in logs that values are approximate.
- **PR 3+** — additional models as hardware becomes available.

## Open questions

- **FM passband control.** On Icom we return `Unsupported` for FM because
  `1A 03` doesn't apply to FM. Confirm the same is true (or not) for
  Yaesu and Kenwood before their handlers copy that pattern.
- **DATA mode filter independence.** On the IC-7610, data modes have
  independent filter widths from voice modes, and `1A 03` reads whichever
  is currently active. If Yaesu/Kenwood differ here, document it.
- **Shared abstraction.** After three backends have per-model filter
  tables, consider whether they should share a common trait/type in
  `riglib-core` rather than each crate rolling its own. Probably not
  worth it until we have three real implementations to compare.
