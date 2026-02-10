# Error Model Upgrades (Orthogonal)

**Depends on:** Phases A-E complete (IO task architecture stable across all backends)
**Deliverable:** Structured error types replacing stringly-typed errors.
**Estimated effort:** 5 sessions (sub-phases Err.1 through Err.4)

---

## Timing

Structured errors are valuable but **should not block** the IO-task migration. Do this after:

- Stable response matching
- Stable shutdown behavior
- A few Tier-1 rigs validated on hardware

Otherwise you'll spend time re-plumbing errors while still changing the execution model.

---

## Why This Matters for Contesting

During a contest, the operator has no time to debug. Error messages must be immediately actionable:

- "Protocol error: rig returned NAK" → command not supported (wrong mode, parameter out of range)
- "Protocol error: CI-V bus collision" → informational, system handled it
- "Protocol error: error response (?;)" → Yaesu/Kenwood rejected the command
- "Protocol error: SmartSDR error 0x12345678: Invalid parameter" → FlexRadio API error with numeric code preserved

The current `Error::Protocol(String)` loses this structure — callers cannot `match` on error kinds, only string-match on Display output.

---

## Current State (Audit)

### Error Enum

`riglib-core/src/error.rs` has 9 variants. `Protocol(String)` and `Transport(String)` are the stringly-typed targets:

```rust
pub enum Error {
    Transport(String),        // 30 construction sites — stringly-typed
    Protocol(String),         // 210 construction sites — stringly-typed
    Timeout,                  // unit — no context, but fine for now
    Unsupported(String),      // 56 sites — out of scope (not hot path)
    InvalidParameter(String), // 27 sites — out of scope (not hot path)
    NotConnected,             // unit — fine
    ConnectionLost,           // unit — fine
    StreamClosed,             // unit — fine
    Io(#[from] std::io::Error), // structured — fine
}
```

### Construction Site Counts

| Backend | Protocol | Transport | Total |
|---------|----------|-----------|-------|
| Icom (io, commands, rig, civ) | 26 | 0 | 26 |
| Kenwood (commands, rig) | 50 | 0 | 50 |
| Elecraft (commands, rig) | 44 | 0 | 44 |
| Yaesu (commands, rig) | 39 | 0 | 39 |
| FlexRadio (codec, client, vita49, rig, discovery, mode) | 35 | 8 | 43 |
| text-io (shared IO task) | 7 | 0 | 7 |
| transport (serial, tcp, audio) | 0 | 19 | 19 |
| core + test-harness | 5 | 2 | 7 |
| **Total** | **210** | **30** | **240** |

### Test Impact

- **~32 `matches!(..., Error::*)` assertions** — match on variant name, survive if variant names are preserved
- **~30 `.contains(...)` / `.to_string()` assertions** — match on Display strings, break if format changes

### Dependencies Already In Place

- `thiserror` v2 already a workspace dependency, used by riglib-core and all backends
- No `anyhow` in any library crate (only test-app and examples)

---

## Design Decisions

### Defer `CommandError` Context Wrapper

The original plan proposed a `CommandError` struct wrapping every error with `{model, command, attempt, max_attempts}`. This is **deferred** because:

1. The IO task already logs retries with full context via `tracing` (model, command, attempt count)
2. Threading context through ~100 parse functions in commands.rs would require changing most function signatures
3. The immediate value is typed enums for `match` — context can be layered on later without breaking the API

### Defer `Unsupported(String)` and `InvalidParameter(String)`

These 83 sites are not hot-path errors. They are stable messages used in default trait implementations and builder validation. Can be structured later if needed.

### `From<String>` Shim Strategy

When `Error::Protocol(String)` changes to `Error::Protocol(ProtocolError)`, all 210 construction sites break at once. To enable incremental migration:

1. Err.1 adds the new enums and changes the `Error` variants
2. Err.1 also adds `From<String>` impl on `ProtocolError` mapping to `ProtocolError::Parse { detail }` and on `TransportError` mapping to `TransportError::Other { detail }`
3. Existing `Error::Protocol("...".into())` call sites continue to compile via the shim
4. Err.2–Err.4 replace shim usage with proper typed variants per backend
5. After Err.4, remove the `From<String>` shims (any remaining usage becomes a compile error)

This avoids a workspace-wide flag day.

---

## Sub-Phase Err.1 — Define Structured Error Types + Core Migration

**Scope:** Type definitions + transport layer migration. One session.

### New Types in `riglib-core/src/error.rs`

```rust
/// Transport-layer error classification.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("timeout waiting for data ({context})")]
    Timeout { context: String },

    #[error("connection lost: {reason}")]
    ConnectionLost { reason: String },

    #[error("I/O error: {source}")]
    Io {
        #[source]
        source: std::io::Error,
    },

    #[error("transport error: {detail}")]
    Other { detail: String },
}

/// Protocol-layer error classification.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// Icom CI-V: rig returned 0xFA (NAK).
    #[error("rig returned NAK")]
    Nak,

    /// Icom CI-V: bus collision detected.
    #[error("CI-V bus collision")]
    Collision,

    /// Text protocol: rig returned `?;`.
    #[error("error response (?;)")]
    ErrorResponse,

    /// FlexRadio SmartSDR: structured error with numeric code.
    #[error("SmartSDR error 0x{code:08X}: {message}")]
    SmartSdrError { code: u32, message: String },

    /// Response parsing failure.
    #[error("parse error: {detail}")]
    Parse { detail: String },

    /// Response did not match expected command prefix.
    #[error("unexpected response: expected {expected}, got {got}")]
    UnexpectedResponse { expected: String, got: String },
}
```

### Changes

1. Add `TransportError` and `ProtocolError` enums
2. Change `Error::Protocol(String)` → `Error::Protocol(ProtocolError)`
3. Change `Error::Transport(String)` → `Error::Transport(TransportError)`
4. Add temporary `From<String>` impls on both enums (shim for incremental migration)
5. Migrate `riglib-transport` (19 `Transport(String)` sites in serial.rs, tcp.rs, audio.rs) to proper `TransportError` variants
6. Migrate `riglib-core` error tests (12 tests) to new types
7. Migrate `riglib-test-harness` (5 sites in mock_serial.rs, mock_tcp.rs)

### Error Classification Rules

The IO task should handle transport errors as follows:

- **`TransportError::Timeout`** → recoverable. Retry per configuration, then return `Error::Timeout` to caller.
- **`TransportError::ConnectionLost`** → terminal. Emit `RigEvent::Disconnected`, stop IO task, return error to all pending callers.
- **`TransportError::Io(std::io::Error)`** → classify by `ErrorKind`:
  - `BrokenPipe`, `ConnectionReset`, `ConnectionAborted` → treat as `ConnectionLost`
  - `TimedOut` → treat as `Timeout`
  - `PermissionDenied` → `TransportError::Other { detail: "serial port permission denied" }`

### Definition of Done

- New error types compile
- `From<String>` shims allow all downstream crates to compile without changes
- riglib-transport and riglib-core tests updated and passing
- `cargo test` passes workspace-wide

---

## Sub-Phase Err.2 — Migrate Icom Errors

**Scope:** Replace string errors with structured types in Icom backend. One session. Depends on Err.1.
**Sites:** 26 Protocol + 0 Transport = 26 total.

### Changes

| File | Sites | Pattern |
|------|-------|---------|
| `io.rs` | 5 | NAK → `Nak`, collision → `Collision`, unexpected response → `UnexpectedResponse` |
| `commands.rs` | 14 | All parse-related → `Parse { detail }` |
| `rig.rs` | 6 | Unexpected prefix → `UnexpectedResponse`, parse failures → `Parse` |
| `civ.rs` | 1 | Frame parse → `Parse` |

### Key Mappings

- `"rig returned NAK"` → `ProtocolError::Nak`
- `"CI-V bus collision"` → `ProtocolError::Collision`
- `"expected ACK, got cmd=0x{:02X}"` → `ProtocolError::UnexpectedResponse { expected: "ACK".into(), got: format!("cmd=0x{:02X}", cmd) }`
- `"frequency data too short"`, `"unknown mode byte"`, etc. → `ProtocolError::Parse { detail: "...".into() }`

### Definition of Done

- All 26 Icom Protocol sites use typed `ProtocolError` variants (no `From<String>` shim usage remaining in riglib-icom)
- Icom tests updated and passing
- ~5 test string assertions updated to match new Display format

---

## Sub-Phase Err.3a — Migrate Text-IO + Kenwood Errors

**Scope:** Shared text-IO layer + Kenwood backend. One session. Depends on Err.1.
**Sites:** 7 (text-io) + 50 (Kenwood) = 57 total.

### Changes

**text-io (`io.rs`, 7 sites):**
- `"rig returned error response (?;)"` (3 sites) → `ProtocolError::ErrorResponse`
- Response prefix mismatches → `ProtocolError::UnexpectedResponse`
- Parse failures → `ProtocolError::Parse`

**Kenwood (`commands.rs`, 35 sites + `rig.rs`, 15 sites):**
- Length checks (`"FA data too short"`) → `ProtocolError::Parse { detail }`
- Parse failures (`"invalid frequency digits"`) → `ProtocolError::Parse { detail }`
- Unexpected values (`"unexpected TX state"`) → `ProtocolError::UnexpectedResponse` or `Parse`

### Definition of Done

- All 57 sites use typed `ProtocolError` variants
- Kenwood + text-io tests updated and passing

---

## Sub-Phase Err.3b — Migrate Elecraft + Yaesu Errors

**Scope:** Elecraft and Yaesu backends. One session. Depends on Err.1.
**Sites:** 44 (Elecraft) + 39 (Yaesu) = 83 total.

### Changes

Identical patterns to Kenwood — these three backends share the same text-protocol structure. Mostly mechanical find-and-replace of `Error::Protocol(format!("..."))` → `Error::Protocol(ProtocolError::Parse { detail: format!("...") })` in commands.rs and rig.rs for both crates.

### Definition of Done

- All 83 sites use typed `ProtocolError` variants
- Elecraft + Yaesu tests updated and passing
- Remove `From<String>` shim on `ProtocolError` (all text-protocol backends migrated)

---

## Sub-Phase Err.4 — Migrate FlexRadio Errors

**Scope:** FlexRadio backend. One session. Depends on Err.1.
**Sites:** 35 Protocol + 8 Transport = 43 total.

### Changes

| File | Sites | Pattern |
|------|-------|---------|
| `codec.rs` | 19 | Parse version/response/status lines → `Parse { detail }` |
| `client.rs` | 4 Protocol + 7 Transport | SmartSDR error → `SmartSdrError { code, message }`, handshake → `Parse`, TCP errors → `TransportError::Io` or `ConnectionLost` |
| `vita49.rs` | 4 | Packet parsing → `Parse { detail }` |
| `rig.rs` | 5 | State errors → `Parse`, unsupported → leave as `Unsupported` |
| `discovery.rs` | 2 | Discovery protocol → `Parse { detail }` |
| `mode.rs` | 1 | Unknown mode string → `Parse { detail }` |

### SmartSDR Error Code Preservation

Current:
```rust
Err(Error::Protocol(format!("SmartSDR error 0x{:08X}: {}", resp.error_code, resp.message)))
```

After:
```rust
Err(Error::Protocol(ProtocolError::SmartSdrError {
    code: resp.error_code,
    message: resp.message.clone(),
}))
```

Callers can now `match` on the error code programmatically instead of parsing Display output.

### Definition of Done

- All 43 FlexRadio sites use typed variants
- FlexRadio tests updated and passing
- Remove `From<String>` shim on `TransportError` (all backends migrated)
- `cargo test` passes workspace-wide with zero shim usage remaining

---

## Protocol-Specific Gotchas

**CI-V NAK does not always mean "command rejected."** On some Icom rigs, NAK can mean the rig is busy (e.g., during TX/RX transition). The error model should ideally distinguish `Nak` from `Busy`, though CI-V doesn't provide a separate busy indicator. Log NAK context for debugging.

**Yaesu `?;` with no prefix.** Gives no indication of which command failed. In the IO-task model (one command in-flight at a time), associate with the most recently sent command.

**FlexRadio errors are structured.** SmartSDR responses include numeric error codes (e.g., `R12345|E|12345678|Invalid parameter`). The `SmartSdrError` variant preserves the code for programmatic handling.

**Audio transport wraps cpal errors.** The 15 `Transport(format!(...))` sites in `audio.rs` wrap cpal error messages. These become `TransportError::Other { detail }` since cpal errors don't implement `std::error::Error` consistently. Acceptable for now.

---

## Deferred Work

### `CommandError` Context Wrapper (Future)

The IO task has full context (model, command, attempt count) when an error occurs. A future phase could wrap errors with this context:

```rust
pub struct CommandError {
    pub kind: CommandErrorKind,
    pub model: String,
    pub command: String,
    pub attempt: u32,
    pub max_attempts: u32,
}
```

This requires threading context through parse functions or wrapping at the IO-task level. Deferred because `tracing` already provides this context in logs.

### `Unsupported(String)` and `InvalidParameter(String)` (Future)

83 sites total, not hot-path. Could be structured if callers need to match on specific unsupported operations. Low priority.

---

## Definition of Done (Error Model — all sub-phases)

- All backends use typed `ProtocolError` / `TransportError` variants
- Zero `From<String>` shim usage remaining in library crates
- Callers can `match` on error kinds (NAK, Collision, ErrorResponse, SmartSdrError, Parse, etc.)
- Errors carry enough context for contest-time debugging via Display output
- No `anyhow` or bare `String`-only errors in the hot path
- All ~1907 tests passing
