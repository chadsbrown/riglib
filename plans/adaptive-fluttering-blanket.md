# Passband (filter width) implementation — Yaesu FT-DX10 and Kenwood TS-590SG

## Context

`Passband` in `riglib-core` is documented as hertz and every backend's
`get_passband`/`set_passband` is supposed to honor that contract. Icom was
fixed in `crates/riglib-icom/src/filter.rs` using per-mode lookup tables.
Kenwood and Yaesu are still broken:

- **Kenwood** (`crates/riglib-kenwood/src/rig.rs:362`) — `get_passband`
  returns a made-up linear approximation `hz = 200 + index * 150` from the
  raw `SH` response. Wrong for every valid index. `set_passband` inverts
  the same wrong formula. CW/FSK modes use an entirely different CAT
  command (`FW`) and are not handled at all.
- **Yaesu** (`crates/riglib-yaesu/src/rig.rs:357`) — both directions return
  `Error::Unsupported`.

This plan delivers two independent PRs:

1. **PR 1 — Yaesu FT-DX10**: bench-verifiable, our primary test hardware.
2. **PR 2 — Kenwood TS-590SG**: hamlib-table port, unverified on hardware
   (no Kenwood in the test bench — see MEMORY.md), shipped with a clear
   "unverified" note in the PR description.

Other Yaesu/Kenwood models keep their current behavior (Yaesu
`Unsupported`, Kenwood linear fudge) with a `debug!` log entry flagging the
approximation for models without a real table. Expanding coverage to
additional models waits for hardware access.

---

## Architecture: per-model filter tables

Both backends need per-model tables because the index→Hz mapping varies
significantly across the supported rigs (verified from both the
manufacturer CAT reference PDFs and hamlib's source). The Icom single-
table pattern doesn't port directly.

### Dispatch approach

Add a `filter_table` field to `YaesuModel` and `KenwoodModel`, pointing to
a `&'static FilterTable` struct. The concrete `FilterTable` type is
backend-specific (Yaesu's differs from Kenwood's because the command
shapes differ).

- `YaesuModel::filter_table: Option<&'static YaesuFilterTable>` — `None`
  for models not yet implemented. `get_passband`/`set_passband` return
  `Error::Unsupported` when `None`.
- `KenwoodModel::filter_table: Option<&'static KenwoodFilterTable>` —
  same pattern. When `None`, fall through to the existing linear fudge
  with a `debug!` log entry. (Drop the fudge entirely once all models
  have real tables; not in this PR set.)

This keeps the `Rig` trait impl uncluttered — it just dispatches through
the model's table — and makes adding future models a pure-data change.

---

## PR 1 — Yaesu FT-DX10 filter implementation

### Source of truth

- **Primary**: Yaesu FTDX10 CAT Operation Reference Manual, revision
  2308-F (August 2023). Table 3 starting page 21.
- **Cross-reference**: hamlib's `rigs/yaesu/newcat.c` (`newcat_set_rx_bandwidth`
  lines ~8717–9578 and `newcat_get_rx_bandwidth` ~9747+).
- **Discrepancy flagged**: hamlib's SSB table has `2200/2300/2400` at
  indices 12–14 (inherited from the old FTDX101D table), but the Yaesu
  FTDX10 manual says `2250/2400/2450`. This plan follows the **Yaesu
  manual**, not hamlib. The FTDX10 bench test will confirm which is
  right; if hamlib is right, the tables are one-line fixes.

### FTDX10 command shape

- Set: `SH00NN;` (7 bytes, P1 fixed `0`, P2 fixed `0`, `NN` = P3 = 00–23)
- Read: `SH0;` (4 bytes, P1 fixed `0`)
- Answer: `SH00NN;` (7 bytes)
- **Not per-VFO** — FTDX10 has single `SH` state regardless of
  VFO A/B. `ReceiverId` is accepted but ignored (validate it's A or B,
  not an out-of-range id).
- SH applies in SSB / CW / RTTY / PSK only. In AM/FM the rig uses
  `MD` + `NA` (narrow toggle); SH is not applicable.

### FTDX10 filter tables (Hz, from Yaesu manual Table 3)

SSB (`LSB`/`USB`/`DataLSB`/`DataUSB`), indices 1–23:
```
 1: 300    2: 400    3: 600    4: 850    5: 1100   6: 1200
 7: 1500   8: 1650   9: 1800  10: 1950  11: 2100  12: 2250
13: 2400  14: 2450  15: 2500  16: 2600  17: 2700  18: 2800
19: 2900  20: 3000  21: 3200  22: 3500  23: 4000
```

CW/RTTY/PSK (`CW`/`CWR`/`RTTY`/`RTTYR`), indices 1–18:
```
 1:   50   2:  100   3:  150   4:  200   5:  250   6:  300
 7:  350   8:  400   9:  450  10:  500  11:  600  12:  800
13: 1200  14: 1400  15: 1700  16: 2000  17: 2400  18: 3000
```

Index 00 = "default" — meaning depends on the currently selected roofing
filter. **User-confirmed approach**: on get, when P3=00, issue a second
query (`RF0;` — read roofing filter) and return that width.

AM/FM widths are not SH-controlled; they come from mode+narrow:
- `AM` / `DataAM` → 9000 Hz
- `AM` narrow → 6000 Hz (requires `NA0;` query to detect narrow state)
- `FM` / `DataFM` → 16000 Hz
- `FM` narrow → 9000 Hz

PR1 scope for AM/FM: `get_passband` reads `NA0;` and returns the
appropriate fixed value. `set_passband(target)` picks narrow if `target
<= 6000` (AM) or `target <= 9000` (FM) else wide, issues the appropriate
`NA0N;` write.

### FTDX10 roofing filter (for P3=00 handling)

Command: `RF0;` read / `RF0N;` write. Values (from hamlib `ftdx10.c`
`ftdx10_priv_caps`):

| set char | get char | width Hz | notes   |
|----------|----------|----------|---------|
| '0'      | '0'      | 12000    | AUTO    |
| '1'      | '6'      | 12000    | 12 kHz  |
| '2'      | '7'      |  3000    | 3 kHz   |
| '4'      | '9'      |   500    | 500 Hz  |
| '5'      | 'A'      |   300    | 300 Hz  |

(FTDX10 has no 1.2 kHz roofing filter — FTDX101D does.)

The `RF` support is new — PR1 introduces a minimal private helper
(`read_roofing_filter_hz`) used only by `get_passband`. Not exposed on
the public `Rig` trait.

### Files to modify / create (PR 1)

- **New**: `crates/riglib-yaesu/src/filter.rs` — defines
  `YaesuFilterTable`, `FtDx10Mode` (or reuses `FilterFamily`-style enum),
  the FTDX10 tables as `pub static FTDX10: YaesuFilterTable = ...`, and
  the `index_to_hz` / `hz_to_index` helpers. Mirror the shape of
  `riglib-icom/src/filter.rs`. Includes exhaustive table tests
  (boundaries, round-trip, mode-aware dispatch).
- **Edit**: `crates/riglib-yaesu/src/commands.rs` — add
  `cmd_read_sh()` → `SH0;`, `cmd_set_sh(index: u8)` → `SH00{index:02};`,
  `cmd_read_na()` → `NA0;`, `cmd_set_na(on: bool)` → `NA00;`/`NA01;`,
  `cmd_read_rf()` → `RF0;`, plus a response parser
  `parse_sh_response` that strips the `SH00` prefix, parses the 2-digit
  index, and returns `u8`.
- **Edit**: `crates/riglib-yaesu/src/models.rs` — add
  `pub filter_table: Option<&'static YaesuFilterTable>` field to
  `YaesuModel`, set to `Some(&filter::FTDX10)` in `ft_dx10()`, `None` in
  all other model factories. Update `YaesuModel`'s Debug derive
  compatibility if needed.
- **Edit**: `crates/riglib-yaesu/src/rig.rs:319` `get_mode` — no change
  (already reads mode per-VFO correctly).
- **Edit**: `crates/riglib-yaesu/src/rig.rs:357`
  `get_passband`/`set_passband` — replace the `Unsupported` stubs with
  the full FTDX10 path. Use the existing `execute_command` helper to
  send SH / RF / NA bytes.
- **Edit**: `crates/riglib-yaesu/src/rig.rs` tests module — add
  `#[tokio::test]` coverage for:
  - SSB read at several indices (1 → 300, 13 → 2400, 23 → 4000)
  - CW read at several indices (1 → 50, 10 → 500, 18 → 3000)
  - SSB set rounding (2500 → idx 15, 2450 → idx 14)
  - `P3=00 → reads RF → returns roofing filter width` (mocked
    two-command sequence)
  - AM mode reads → returns 9000, narrow → 6000
  - FM mode reads → returns 16000, narrow → 9000
  - Unsupported model (`FT-710` factory returning `None` filter_table)
    still returns `Error::Unsupported`

### Bench validation (PR 1)

Before merging, run against the real FT-DX10 and verify:

1. Front panel shows 2400 Hz → `SH0;` returns `SH0013;`
2. Front panel 2450 Hz → `SH0014;` (confirms Yaesu manual vs hamlib)
3. Front panel 3000 Hz → `SH0020;`
4. Front panel 4000 Hz → `SH0023;`
5. CW mode 500 Hz → `SH0010;`
6. CW mode 3000 Hz → `SH0018;`
7. AM mode, narrow off → `get_passband` returns 9000 Hz (via NA check)
8. Write loop: `set_passband(2400)` → front panel updates to 2400
9. `get_passband` round-trip after `set_passband(2400)` returns 2400

If 2450/2400 index discrepancy resolves to hamlib's values, update the
SSB table constants accordingly — single-line change, no design impact.

---

## PR 2 — Kenwood TS-590SG filter implementation

### Source of truth

- **Primary**: Kenwood TS-590S/TS-590SG PC Control Command Reference rev 3.
- **Cross-reference**: hamlib `rigs/kenwood/ts590.c` —
  `ts590_set_mode` (lines 216–322) and `ts590_get_mode` (324–426). Note
  that hamlib's implementation has two known bugs (SSB-DATA doesn't
  subtract low-cut; FM path missing); we fix both.
- **Shipped unverified**: no Kenwood hardware on the bench. PR
  description must state this clearly.

### TS-590SG command shapes

- `SH;` read high-cut, `SH{nn};` set high-cut (2 digits).
- `SL;` read low-cut, `SL{nn};` set low-cut (2 digits).
- `FW;` read width, `FW{hhhh};` set width (4 literal Hz digits).
  FW applies in CW and FSK only. SH/SL apply in SSB/SSB-DATA/AM/AM-DATA/
  FM/FM-DATA.
- All commands are VFO-agnostic on TS-590SG (single receiver).

### TS-590SG tables (Hz, from Kenwood PC reference rev 3)

SSB / SSB-DATA / FM / FM-DATA **high-cut** (`SH`, indices 0–13):
```
 0: 1000   1: 1200   2: 1400   3: 1600   4: 1800   5: 2000   6: 2200
 7: 2400   8: 2600   9: 2800  10: 3000  11: 3400  12: 4000  13: 5000
```

SSB / SSB-DATA / FM / FM-DATA **low-cut** (`SL`, indices 0–11):
```
 0:  0    1:  50   2: 100   3: 200   4: 300   5: 400
 6: 500   7: 600   8: 700   9: 800  10: 900  11: 1000
```

AM / AM-DATA **high-cut** (`SH`, indices 0–3):
`0: 2500, 1: 3000, 2: 4000, 3: 5000`

AM / AM-DATA **low-cut** (`SL`, indices 0–3):
`0: 0, 1: 100, 2: 200, 3: 300`

CW (`FW` direct Hz, 14 valid widths):
`50, 80, 100, 150, 200, 250, 300, 400, 500, 600, 1000, 1500, 2000, 2500`

FSK / RTTY (`FW` direct Hz, 4 valid widths):
`250, 500, 1000, 1500`

### Bandwidth semantics

For SSB/AM/FM: `effective_bandwidth = SH[hi_idx] - SL[lo_idx]`.

`get_passband` on TS-590SG in SSB/AM/FM/data modes issues TWO commands:
`SH;` then `SL;`, looks up both indices, subtracts. Returns
`Passband::from_hz(hi - lo)`.

`set_passband` (user-chosen policy): **preserve current SL, adjust SH**.
Sequence:
1. Read current `SL;` → index → `current_lo_hz`
2. Compute `target_hi = target_bandwidth + current_lo_hz`
3. Find nearest SH index with `target_hi`, write `SH{nn};`

Costs one extra round-trip per `set_passband` but respects the user's
existing low-cut shaping. Document this in the method doc comment.

CW/FSK: single round-trip via `FW;` / `FW{hhhh};`.

### EX-028 / EX-029 menu assumption

The TS-590SG has a menu that flips SH/SL semantics between "HI/LO cut"
(the default and most common setting, matching the tables above) and
"WIDTH/SHIFT". Hamlib silently assumes HI/LO. We do the same and
**document the assumption** in the `get_passband`/`set_passband` doc
comments. We do NOT query the menu. If the user has configured WIDTH/
SHIFT, they get wrong values — same behavior as hamlib.

### Files to modify / create (PR 2)

- **New**: `crates/riglib-kenwood/src/filter.rs` — defines
  `KenwoodFilterTable`, the TS-590SG tables as `pub static TS590SG:
  KenwoodFilterTable = ...`, plus helpers:
  - `ssb_hi_index_to_hz(table, idx) -> Option<u32>`
  - `ssb_lo_index_to_hz(table, idx) -> Option<u32>`
  - `ssb_hi_hz_to_index(table, hz) -> u8` (nearest)
  - `ssb_lo_hz_to_index(table, hz) -> u8`
  - `am_hi_index_to_hz` / `am_lo_index_to_hz` / ...
  - `cw_width_hz_to_nearest(table, hz) -> u32` (direct Hz lookup)
  - `fsk_width_hz_to_nearest(table, hz) -> u32`
  - Mode-family classifier: `FilterFamily::{Ssb, Am, Cw, Fsk, Fm}`
  - Tests: table boundaries, round-trip for every valid index in every
    family, nearest-match rounding behavior.
- **Edit**: `crates/riglib-kenwood/src/commands.rs`
  - Update `cmd_read_passband` / `cmd_set_passband` docs.
  - Add: `cmd_read_sl() -> SL;`, `cmd_set_sl(idx: u8) -> SL{idx:02};`
  - Add: `cmd_read_fw() -> FW;`, `cmd_set_fw(hz: u16) -> FW{hz:04};`
  - Add parsers: `parse_sh_response`, `parse_sl_response`,
    `parse_fw_response`.
- **Edit**: `crates/riglib-kenwood/src/models.rs` — add
  `pub filter_table: Option<&'static KenwoodFilterTable>` field, set to
  `Some(&filter::TS590SG)` in `ts_590sg()` **and also `ts_590s()`**
  (the HI/LO tables are identical on the two models per the Kenwood rev
  3 manual; only the WIDTH/SHIFT mode tables differ, which we don't
  use). Leave TS-890S and TS-990S at `None`.
- **Edit**: `crates/riglib-kenwood/src/rig.rs:362`
  - `get_passband`: match on `self.model.filter_table`; if
    `Some(table)`, dispatch by `get_mode().await?` family:
    - Ssb/Am/Fm → read SH then SL, compute diff, return.
    - Cw/Fsk → read FW, return parsed Hz.
  - If `None`, fall through to the current linear fudge with a
    `debug!` log line noting the approximation.
  - `set_passband`: mirror the dispatch. SSB path reads current SL,
    computes new SH, writes. AM/FM same pattern (AM uses the small AM
    SH/SL table). CW/FSK writes FW directly.
- **Edit**: `crates/riglib-kenwood/src/rig.rs` tests module — add
  coverage:
  - SSB get: mock `SH;` → `SH06;` (2200) + `SL;` → `SL04;` (300) →
    result 1900 Hz
  - SSB get: mock `SH08;` (2600) + `SL04;` (300) → 2300 Hz
  - SSB set(2400): mock `SL;` → `SL04;` (300) + expect
    `SH07;` (target_hi = 2400 + 300 = 2700, nearest SH index is 07 → 2400)
  - AM get: mock `SH02;` (4000) + `SL02;` (200) → 3800 Hz
  - CW get: mock `FW;` → `FW0500;` → 500 Hz
  - CW set(400): expect `FW0400;`
  - FSK get: mock `FW;` → `FW0250;` → 250 Hz
  - TS-890S model (filter_table=None): verify debug log + linear fudge
    still returns

### No bench validation for PR 2

Explicitly stated in the PR description: tables are ported verbatim from
the Kenwood TS-590S/SG PC Control Command Reference rev 3 (and
cross-checked against hamlib ts590.c). No hardware verification was
possible. Any user who tests against a real TS-590SG and finds a
discrepancy should file an issue.

---

## Out of scope

Explicitly deferred until hardware or clear user demand:

- Yaesu FTDX101D/MP, FT-991A, FT-710, FT-891 filter tables. The
  research report has the tables and the command-shape differences
  documented in a form ready to lift into `filter.rs`, but we won't
  commit them un-bench-verified. (Each is a small follow-up PR.)
- Kenwood TS-890S, TS-990S filter tables. Hamlib doesn't even implement
  them, so we'd be writing the first Rust-side port from the Kenwood
  manuals with no reference implementation to cross-check. Not
  shipping unverified.
- Shared filter abstraction in `riglib-core`. After three backends
  (Icom, Yaesu, Kenwood) all have per-model tables we can revisit
  whether to factor out a common trait. Not now — the shapes differ
  enough (Icom is single-table, Yaesu is per-model per-mode, Kenwood
  is dual SH/SL plus separate FW) that a premature abstraction would
  fight us.
- Kenwood EX-028/029 HI/LO vs WIDTH/SHIFT auto-detection.
- `Passband::Default` / `Passband::Auto` sentinel values (would need a
  core-type change).

---

## Critical files to modify

### PR 1 (Yaesu FT-DX10)
- `crates/riglib-yaesu/src/filter.rs` (new)
- `crates/riglib-yaesu/src/commands.rs` (add SH/NA/RF builders + parsers)
- `crates/riglib-yaesu/src/models.rs` (add `filter_table` field)
- `crates/riglib-yaesu/src/rig.rs:357` (rewrite `get_passband`/`set_passband`)
- `crates/riglib-yaesu/src/lib.rs` (re-export `filter` module if needed)

### PR 2 (Kenwood TS-590SG)
- `crates/riglib-kenwood/src/filter.rs` (new)
- `crates/riglib-kenwood/src/commands.rs` (add SL/FW builders + parsers)
- `crates/riglib-kenwood/src/models.rs` (add `filter_table` field)
- `crates/riglib-kenwood/src/rig.rs:362` (rewrite `get_passband`/`set_passband`)
- `crates/riglib-kenwood/src/lib.rs` (re-export)

### Reference (read-only)
- `crates/riglib-icom/src/filter.rs` — pattern to mirror
- `crates/riglib-icom/src/rig.rs:327` — handler-side pattern
- `crates/riglib-core/src/types.rs` — `Passband`, `Mode`, `ReceiverId`
- `crates/riglib-test-harness/src/mock_serial.rs` — `MockTransport.expect()`

---

## Verification

### PR 1 (Yaesu) — unit tests

```
cargo test -p riglib-yaesu filter
cargo test -p riglib-yaesu passband
```

All tests in `crates/riglib-yaesu/src/filter.rs` and the
passband-related tests in `crates/riglib-yaesu/src/rig.rs` pass.

### PR 1 — bench validation on FT-DX10

Connect the FT-DX10 to the dev machine via USB. Using
`riglib`'s `examples/` (or a small throwaway binary):

1. Read current passband in SSB mode at several front-panel widths —
   confirm reported Hz matches front panel.
2. Write 2400 Hz → front panel updates to 2400.
3. Read back → 2400.
4. Switch to CW → read passband → matches front panel.
5. Write 500 Hz CW → front panel updates to 500.
6. Switch to AM → read passband → 9000 Hz. Toggle narrow on radio →
   read → 6000 Hz.

**Specific discrepancy resolution test**: in SSB, use the front panel
width knob to set `2250 Hz`, `2400 Hz`, and `2450 Hz` in turn; for each,
read the raw `SH0;` response. Confirm whether the manual's indices
(12/13/14) or hamlib's (12/13 for 2200/2300) are correct.

### PR 2 (Kenwood) — unit tests only

```
cargo test -p riglib-kenwood filter
cargo test -p riglib-kenwood passband
```

No hardware validation possible. PR description must call this out
explicitly.

### Both PRs — full workspace build

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
