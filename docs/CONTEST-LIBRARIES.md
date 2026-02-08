# Contest Logger Library Assessment

What other standalone libraries would be useful for a ham radio contest logging
application, beyond the three already planned (riglib, winkeyerlib, otrsp)?
Which pieces are substantial, well-bounded, and reusable enough to be their own
library?

**Strong candidates:**

1. **DX Cluster client** — Telnet protocol, spot parsing (the DX spot format is loosely standardized but full of edge cases), filtering by band/mode/entity, spot aging/expiry. This is a meaningful protocol library — similar in complexity to winkeyerlib. Every contest logger, DX logger, and bandmap tool needs this. Could also support the newer WebSocket-based cluster interfaces.

2. **Cabrillo / ADIF** — Log interchange formats. Cabrillo v3 is the contest submission format (every contest uses it), ADIF 3.1.4 is the general logging interchange format. Parsing and generation, field validation, contest-specific header handling. These two could be a single crate or two small crates. Well-defined specs, very reusable.

3. **CTY / entity database** — Prefix-to-DXCC-entity resolution. Every ham radio app needs to turn "W1AW" into "United States" with CQ zone 5, ITU zone 8, lat/long, continent, etc. The CTY.DAT format (maintained by AD1C) is the de facto standard. Includes prefix exceptions, call areas, and special callsign overrides. Used for multiplier tracking, bearing calculations, and display.

4. **Rotator control** — GS-232 (Yaesu), EasyComm, and similar protocols. Analogous to riglib but for antenna rotators. Smaller scope (just azimuth/elevation) but same transport concerns.

**Moderate candidates (could go either way):**

5. **Geodesic / Maidenhead** — Grid square ↔ lat/long conversion, great circle bearing and distance, short/long path calculation. Small but universally needed. Might be a tiny crate (like 200 lines) — almost too small for its own repo, but clean and very reusable.

6. **Super Check Partial (SCP)** — MASTER.SCP database parsing and fast partial-match lookup. Every contest logger uses this for call completion. The interesting part is the data structure for fast prefix/substring matching, not just simple parsing.

**Probably stays inside the logger:**

- Contest rules engine (scoring, exchanges, multipliers) — too contest-specific and tightly coupled to logger state
- Bandmap rendering — UI-specific
- Multi-op networking — application-specific protocol
- Audio recording — platform-specific, thin wrapper over system APIs
- Macro/message templating — logger-specific variable expansion

**Ranking by value:**

| Priority | Library | Why |
|----------|---------|-----|
| 1 | Cabrillo/ADIF | Every ham radio app needs this, well-defined specs |
| 2 | CTY/entity database | Universal need, non-trivial prefix resolution logic |
| 3 | DX Cluster client | Substantial protocol work, widely reusable |
| 4 | Rotator control | Natural companion to riglib |
| 5 | Geodesic/Maidenhead | Small but universal |
| 6 | SCP database | Niche but valuable |
