# Command Audit (Present vs Future)

Date: 2026-02-13

## Scope

This document audits commands currently implemented in:

- `crates/riglib-yaesu/src/commands.rs`
- `crates/riglib-kenwood/src/commands.rs`
- `crates/riglib-elecraft/src/commands.rs`
- `crates/riglib-icom/src/commands.rs`

against available vendor references.

## Status Legend

- `validated`: confirmed against vendor command reference.
- `incorrect`: confirmed mismatch with vendor reference.
- `likely incorrect`: strong mismatch signal, needs targeted implementation correction.
- `partial`: command family exists and mostly aligns, but details vary by model or are not fully proven yet.
- `future`: not currently implemented in riglib command surface.

## Sources Used

### Yaesu
- FT-991/FT-991A CAT tables (manual mirrors):
  - https://www.manualslib.com/manual/1026884/Yaesu-Ft-991.html
  - https://www.manualslib.com/manual/2394078/Yaesu-Ft-991a.html

### Kenwood
- TS-890S PC command reference:
  - https://www.kenwood.com/i/products/info/amateur/software_download/pdf/ts890_pc_command_e_rev10.pdf
- TS-590S/SG PC command reference:
  - https://www.kenwood.com/i/products/info/amateur/software_download/pdf/ts590_pc_command_e_rev5.pdf
- TS-990S PC command reference:
  - https://www.kenwood.com/i/products/info/amateur/software_download/pdf/ts990_pc_command_e_rev5.pdf

### Elecraft
- K4 command reference:
  - https://ftp.elecraft.com/K4/Owner's%20Manual/K4%20Command%20Reference%20R27.html
- K3/KX programmer reference:
  - https://ftp.elecraft.com/K3/Manuals%20Downloads/K3S%20K3%20KX3%20KX2%20Programmer%27s%20Reference%20R9A.pdf

### Icom (local PDFs provided by user)
- `ico-ic-7610_yj.pdf`
- `IC-7760_ENG_CI-V_0.pdf`

### Hamlib (secondary cross-check)
- Debian source package index:
  - https://sources.debian.org/src/hamlib/4.6.5-5/
- Hamlib NEWS/changelog (maintenance signals):
  - https://sources.debian.org/src/hamlib/4.6.5-5/NEWS/
- Yaesu backend tree showing FT-991/newcat layout:
  - https://sources.debian.org/src/hamlib/3.0.1-1/rigs/yaesu/
- FT-991 backend source path:
  - https://sources.debian.org/src/hamlib/3.0.1-1/rigs/yaesu/ft991.c/
- Icom CI-V command constants (historical Hamlib backend snapshot):
  - https://sources.debian.org/src/hamlib/4.0-7/rigs/icom/icom_defs.h/

## Confirmed Incorrect Items

1. Kenwood `RM` mapping swapped.
- Code:
  - `crates/riglib-kenwood/src/commands.rs:150`
  - `crates/riglib-kenwood/src/commands.rs:158`
- Current: `RM1`=SWR, `RM2`=ALC
- Reference: TS-890S/TS-990S tables indicate `RM1`=ALC, `RM2`=SWR.

2. Yaesu `RM` mapping swapped.
- Code:
  - `crates/riglib-yaesu/src/commands.rs:162`
  - `crates/riglib-yaesu/src/commands.rs:169`
- Current: `RM1`=SWR, `RM5`=ALC
- Reference (FT-991 family): `RM1`=IDC, `RM3`=ALC, `RM5`=SWR.

3. Yaesu AGC value model inconsistent.
- Code:
  - `crates/riglib-yaesu/src/commands.rs:225`
  - `crates/riglib-yaesu/src/rig.rs:543`
  - `crates/riglib-yaesu/src/models.rs:116`
- Current assumes `GT0` values map as `0=Off,1=Fast,2=Mid,3=Slow`.
- FT-991 family CAT tables include broader value set and different semantic mapping.

## Hamlib Cross-Check (Secondary Reference)

This section is a secondary implementation cross-check against Hamlib sources that were retrievable during this audit session.

Notes:
- Hamlib was used as corroboration, not as protocol authority.
- Vendor command references remain the primary source of truth.
- In this environment, only part of Hamlib source tree was directly retrievable in-machine-readable form, so some rows are marked inconclusive.

| Backend/Area | Hamlib cross-check | Evidence |
|---|---|---|
| Yaesu newcat family (FT-991 path) | partial corroboration | Hamlib includes FT-991 integration via newcat path (`ft991.c` + `newcat.c` in `3.0.1-1/yaesu` listing), indicating command-family parity for major operations. |
| Yaesu SWR/ALC selector mapping | inconclusive in-session | Direct `newcat.c` body could not be fetched through tool cache policy during this run, so no direct RM selector comparison from Hamlib code was possible here. |
| Yaesu AGC semantics | inconclusive in-session | Same retrieval limitation for `newcat.c`; no direct Hamlib AGC-value mapping extracted in this run. |
| Kenwood backend | partial corroboration | Hamlib release notes include continued Kenwood maintenance/fixes (e.g., `kenwood.c` items in 4.6.x notes), consistent with command-family maturity; direct current `kenwood.c` body extraction was blocked in-session. |
| Elecraft behavior | partial corroboration | In older Hamlib layouts Elecraft functionality is represented under Kenwood-compatible/kit paths; no direct contemporary Elecraft backend source extraction was completed in this run. |
| Icom CI-V family | partial corroboration | Hamlib has dedicated Icom CI-V backend structure (`icom` folder with `icom.c` and model files listed); direct `icom.c` body extraction was blocked in-session, so subcommand-level confirmation remained manual-driven from local vendor PDFs. |
| Icom data-mode command family (`1A 06`) | likely present in Hamlib, not confirmed line-level here | Hamlib Icom backend/model breadth suggests support patterns, but line-level verification for exact `1A 06` handling could not be pulled in this run. |
| Icom antenna command abstraction (`0x12`) | inconclusive in-session | Unable to retrieve Hamlib `icom.c` implementation lines here; no direct Hamlib-based resolution of `0x12` abstraction detail in this run. |

## Present Command Surface Audit

## Yaesu

| Command family | Code surface | Status | Notes | Hamlib cross-check |
|---|---|---|---|---|
| Frequency read/set (`FA`, `FB`) | `cmd_read_frequency_a/b`, `cmd_set_frequency_a/b` | validated | Core format aligns with CAT table shape. | partial corroboration (FT-991/newcat path present) |
| Mode read/set (`MD0`, `MD1`) | `cmd_read_mode_a/b`, `cmd_set_mode_a/b` | partial | Base mapping aligns; full model-by-model mode-code edge cases still pending. | partial corroboration (FT-991/newcat path present) |
| PTT (`TX`) | `cmd_read_ptt`, `cmd_set_ptt` | validated | Core on/off behavior aligns. | partial corroboration (FT-991/newcat path present) |
| Power (`PC`) | `cmd_read_power`, `cmd_set_power` | validated | Core read/set structure aligns. | partial corroboration (FT-991/newcat path present) |
| S-meter (`SM0`) | `cmd_read_s_meter` | validated | Selector and response shape align. | partial corroboration (FT-991/newcat path present) |
| SWR/ALC (`RM`) | `cmd_read_swr`, `cmd_read_alc` | incorrect | Mapping is reversed/misaligned (see above). | inconclusive (newcat selector lines not extracted) |
| AGC (`GT0`) | `cmd_read_agc`, `cmd_set_agc` | incorrect | Value mapping/model semantics not aligned. | inconclusive (newcat AGC-value lines not extracted) |
| Preamp (`PA0`) | `cmd_read_preamp`, `cmd_set_preamp` | partial | Command shape aligns; model-specific level semantics need per-model confirmation. | partial corroboration (FT-991/newcat path present) |
| Attenuator (`RA0`) | `cmd_read_attenuator`, `cmd_set_attenuator` | partial | Command shape aligns; exact level semantics can vary by model. | partial corroboration (FT-991/newcat path present) |
| Split (`FT`) | `cmd_read_split`, `cmd_set_split` | validated | Core split on/off behavior aligns. | partial corroboration (FT-991/newcat path present) |
| A=B / swap (`AB`, `SV`) | `cmd_vfo_a_eq_b`, `cmd_vfo_swap` | validated | Standard Yaesu CAT behavior. | partial corroboration (FT-991/newcat path present) |
| IF info (`IF`) | `cmd_read_information` | validated | Appropriate read path for clarifier/status extraction. | partial corroboration (FT-991/newcat path present) |
| RIT/XIT (`RT`, `XT`, `RU`, `RD`, `RC`) | corresponding builders | partial | Write-path semantics align; broader read-back behavior depends on IF parsing and model behavior. | partial corroboration (FT-991/newcat path present) |
| CW (`KS`, `KY`) | read/set CW speed and message commands | validated | Command family and payload shape align. | partial corroboration (FT-991/newcat path present) |
| AI (`AI`) | `cmd_set_ai` | validated | `AI0/AI2` pattern aligns. | partial corroboration (FT-991/newcat path present) |

## Kenwood

| Command family | Code surface | Status | Notes | Hamlib cross-check |
|---|---|---|---|---|
| Frequency read/set (`FA`, `FB`) | read/set builders | validated | 11-digit format aligns. | inconclusive (kenwood.c not extracted in-session) |
| Mode (`MD`) | read/set builders | partial | Core mapping aligns; some model-specific data-mode handling remains nuanced. | inconclusive (kenwood.c not extracted in-session) |
| PTT/RX (`TX`, `RX`) | read/set/force RX | validated | Standard behavior. | inconclusive (kenwood.c not extracted in-session) |
| Power (`PC`) | read/set builders | validated | Core behavior aligns. | inconclusive (kenwood.c not extracted in-session) |
| S-meter (`SM0`) | `cmd_read_s_meter` | validated | Selector format aligns. | inconclusive (kenwood.c not extracted in-session) |
| SWR/ALC (`RM`) | `cmd_read_swr`, `cmd_read_alc` | incorrect | Mapping swapped (see above). | inconclusive (kenwood.c not extracted in-session) |
| RX/TX VFO and split (`FR`, `FT`) | read/set builders | validated | Sequence used for split is correct. | inconclusive (kenwood.c not extracted in-session) |
| AGC mode (`GC`) | TS-890/TS-990 builders | validated | Command forms align with model-specific docs. | inconclusive (kenwood.c not extracted in-session) |
| AGC time const (`GT`) | TS-590 builders | validated | 3-digit time constant form aligns. | inconclusive (kenwood.c not extracted in-session) |
| Preamp/attenuator (`PA`, `RA`) | read/set builders | partial | Command forms align; exact levels differ by model. | inconclusive (kenwood.c not extracted in-session) |
| Passband (`SH`) | read/set builders | partial | Valid family, but exact index/value mapping is model-specific. | inconclusive (kenwood.c not extracted in-session) |
| RIT/XIT (`RT`, `XT`, `RO`, `RU`, `RD`, `RC`) | corresponding builders | validated | Command family and offset strategy align with refs. | inconclusive (kenwood.c not extracted in-session) |
| CW (`KS`, `KY`) | speed + message functions | validated | Format and fixed-length/padding behavior align. | inconclusive (kenwood.c not extracted in-session) |
| AI (`AI`) | `cmd_set_ai` | validated | Core behavior aligns. | inconclusive (kenwood.c not extracted in-session) |
| A=B (`AB`) | `cmd_vfo_a_eq_b` | validated | Standard behavior. | inconclusive (kenwood.c not extracted in-session) |

## Elecraft

| Command family | Code surface | Status | Notes | Hamlib cross-check |
|---|---|---|---|---|
| Kenwood-compatible core (`FA/FB/MD/TX/RX/PC/SM/RM/FR/FT/AI/RT/XT/RO/RU/RD/RC/KS/KY`) | main builders | validated | Core command set aligns with K3/KX references. | inconclusive (no direct Elecraft backend extraction in-session) |
| SWR/ALC (`RM1`, `RM2`) | `cmd_read_swr`, `cmd_read_alc` | validated | Matches Elecraft references checked. | inconclusive (no direct Elecraft backend extraction in-session) |
| A=B / swap (`SWT11`, `SWT00`) | `cmd_vfo_a_eq_b`, `cmd_vfo_swap` | validated | Elecraft-specific mapping aligns. | inconclusive (no direct Elecraft backend extraction in-session) |
| AGC K3 (`GTnnn[x]`) | `cmd_read_agc_k3`, `cmd_set_agc_k3` | partial | Family aligns; firmware variation (`K22` extensions) requires runtime tolerance. | inconclusive (no direct Elecraft backend extraction in-session) |
| AGC K4 (`GT$`) | `cmd_read_agc_k4`, `cmd_set_agc_k4` | validated | K4 form aligns. | inconclusive (no direct Elecraft backend extraction in-session) |
| Attenuator K3/K4 (`RA`, `RA$`) | builders | validated | Family split aligns with references. | inconclusive (no direct Elecraft backend extraction in-session) |
| Bandwidth (`BW` K4, `FW` K3-family) | read/set builders | validated | Model split aligns. | inconclusive (no direct Elecraft backend extraction in-session) |
| Identify (`K3`, `K4`) | identify builders | validated | Expected behavior for model probing. | inconclusive (no direct Elecraft backend extraction in-session) |

## Icom (validated against IC-7610 + IC-7760 local CI-V references)

| Command family | Code surface | Status | Notes | Hamlib cross-check |
|---|---|---|---|---|
| Frequency (`0x03`, `0x05`) | `cmd_read_frequency`, `cmd_set_frequency` | validated | Matches CI-V table/formats. | partial corroboration (Hamlib Icom command-family constants) |
| Mode (`0x04`, `0x06`) | `cmd_read_mode`, `cmd_set_mode` | partial | Core mode commands align; data-mode handling is likely incomplete vs `1A 06`. | partial corroboration (Hamlib Icom command-family constants) |
| PTT/status (`1C 00`) | `cmd_read_ptt`, `cmd_set_ptt` | validated | Using status path with 00/01 is aligned with manuals. | partial corroboration (Hamlib Icom command-family constants) |
| Meter (`0x15 0x02/0x12/0x13`) | S-meter/SWR/ALC builders | validated | Subcommands align with tables. | partial corroboration (Hamlib Icom command-family constants) |
| Power level (`0x14 0x0A`) | read/set power builders | validated | 0000..0255 scale aligns. | partial corroboration (Hamlib Icom command-family constants) |
| Split (`0x0F`) | read/set split builders | validated | On/off semantics align. | partial corroboration (Hamlib Icom command-family constants) |
| VFO/main-sub (`0x07`) | VFO select + main/sub + A=B/swap | validated | Subcommand usage aligns (including `D0/D1`, `B0`, `B1`). | partial corroboration (Hamlib Icom command-family constants) |
| IF filter (`1A 03`) | `cmd_read_if_filter` | validated | Family/subcommand align. | partial corroboration (Hamlib Icom command-family constants) |
| AGC mode (`16 12`) + AGC time const (`1A 04`) | AGC builders | validated | Matches documented dual-path model. | partial corroboration (Hamlib Icom command-family constants) |
| RIT/XIT (`21 00/01/02`) | read/set builders | validated | Shared offset + on/off split aligns. | partial corroboration (Hamlib Icom command-family constants) |
| CW message (`0x17`) | send/stop builders | partial | Family aligns; upper payload/chunk behavior may vary by model/firmware. | partial corroboration (Hamlib Icom command-family constants) |
| Preamp (`16 02`) | read/set builders | validated | Command mapping aligns. | partial corroboration (Hamlib Icom command-family constants) |
| Attenuator (`0x11`) | read/set builders | validated | Command mapping aligns. | partial corroboration (Hamlib Icom command-family constants) |
| Antenna (`0x12`) | `cmd_read_antenna`, `cmd_set_antenna` | likely incorrect | Manuals show `0x12` with subcommands and ANTx/RX-ANT semantics; current abstraction is oversimplified single-byte model. | partial corroboration (Hamlib Icom command-family constants) |

## Future Command Surface (Not Implemented Yet)

These command areas appear in vendor references but are not exposed as first-class riglib command APIs yet.

1. Icom `1A 06` data-mode/filter command family (explicit data mode command path).
2. Icom `29` command wrapper for direct Main/Sub targeting across supported commands (IC-7760 docs).
3. Broader Icom `1A 05 ...` settings surface (many menu-backed controls, mostly intentionally out-of-scope today).
4. Yaesu model-specific CAT divergences beyond the current common abstraction (e.g., differing meter/AGC semantics across model families).
5. Kenwood model-specific optional command surfaces not currently represented (beyond shared HF control set).

## Summary

Current implementation is broadly solid at the core CAT/CI-V control layer, but not fully correct yet:

- `incorrect`: Kenwood RM SWR/ALC, Yaesu RM SWR/ALC, Yaesu AGC mapping.
- `likely incorrect`: Icom antenna abstraction for 7610/7760 behavior.
- `partial`: a handful of model-specific mode/data/extended setting areas.

This document should be reviewed before implementation changes, then updated after each fix.
