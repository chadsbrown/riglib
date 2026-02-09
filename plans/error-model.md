# Error Model Upgrades (Orthogonal)

**Depends on:** Nothing specific, but recommended after [Phase A](phase-a-io-task-unification.md) stabilizes
**Deliverable:** Structured error types replacing stringly-typed errors.
**Estimated effort:** 3 sessions (sub-phases Err.1 through Err.3)

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

- "Timeout on IC-7610 (0x98) cmd 0x03 (read frequency), attempt 3/3" → check USB cable
- "Protocol error: rig returned NAK" → command not supported (wrong mode, parameter out of range)
- "CI-V collision on attempt 2, recovered on attempt 3" → informational, system handled it

The current `Error::Protocol(String)` loses this context.

---

## Sub-Phase Err.1 — Define Structured Error Types

**Scope:** Type definitions, backward compatible. One session. Can start after Phase A.3.

```rust
// In riglib-core/src/error.rs

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
}

/// Protocol-layer error classification.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("rig returned NAK")]
    Nak,

    #[error("CI-V bus collision")]
    Collision,

    #[error("error response (?;)")]
    ErrorResponse,

    #[error("parse error: {detail}")]
    Parse { detail: String },

    #[error("unexpected response: expected {expected}, got {got}")]
    UnexpectedResponse { expected: String, got: String },
}

/// Full command error with context for debugging.
#[derive(Debug, thiserror::Error)]
#[error("{kind} [model={model}, cmd={command}, attempt={attempt}/{max_attempts}]")]
pub struct CommandError {
    pub kind: CommandErrorKind,
    pub model: String,
    pub command: String,
    pub attempt: u32,
    pub max_attempts: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandErrorKind {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}
```

### Error Classification Rules

The IO task should handle errors as follows:

- **`Timeout`** → recoverable. Retry per configuration, then return `Error::Timeout` to caller.
- **`ConnectionLost`** → terminal. Emit `RigEvent::Disconnected`, stop IO task, return error to all pending callers.
- **`Io(std::io::Error)`** → classify by `ErrorKind`:
  - `BrokenPipe`, `ConnectionReset`, `ConnectionAborted` → `ConnectionLost`
  - `TimedOut` → `Timeout`
  - `PermissionDenied` → `Transport("serial port permission denied")`

### Definition of Done

- New error types compile, existing `Error` variants still work.
- Error display messages include context.

---

## Sub-Phase Err.2 — Migrate Icom Errors

**Scope:** Replace string errors with structured types in Icom backend. One session. Depends on Err.1.

- Replace `Error::Protocol("rig returned NAK".into())` with `Error::Protocol(ProtocolError::Nak)`.
- Add command context to timeout errors.

### Definition of Done

- Error messages in existing Icom tests still match (or are updated to match structured format).

---

## Sub-Phase Err.3 — Migrate Text-Protocol Errors

**Scope:** Replace string errors in Yaesu/Kenwood/Elecraft. One session. Depends on Err.1.

- Replace `Error::Protocol("rig returned error response (?;)".into())` with `ProtocolError::ErrorResponse`.

### Definition of Done

- All backends use structured error types in the hot path.

---

## Protocol-Specific Gotchas

**CI-V NAK does not always mean "command rejected."** On some Icom rigs, NAK can mean the rig is busy (e.g., during TX/RX transition). The error model should ideally distinguish `Nak` from `Busy`, though CI-V doesn't provide a separate busy indicator. Log NAK context for debugging.

**Yaesu `?;` with no prefix.** Gives no indication of which command failed. In the IO-task model (one command in-flight at a time), associate with the most recently sent command.

**FlexRadio errors are structured.** SmartSDR responses include numeric error codes (e.g., `R12345|E|12345678|Invalid parameter`). Map to `ProtocolError` variants with the error code preserved.

---

## Definition of Done (Error Model — all sub-phases)

- All backends use structured error types
- Errors carry enough context for contest-time debugging
- No `anyhow` or `String`-only errors in the hot path
