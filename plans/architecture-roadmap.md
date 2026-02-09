# riglib Architecture Roadmap

## End-State Goals

1. **One place reads from the wire** per rig connection
2. **All commands serialize through a scheduler** enabling priority, coalescing, and atomic sequences
3. **AI/transceive becomes a behavior toggle** ("emit unsolicited events + update caches"), not an architectural fork ("different I/O model")

## Invariants (preserve throughout all phases)

- **AI/transceive disabled must still fully work** (commands + responses)
- No backend should regress in supported operations (freq/mode/split/PTT/etc.)
- Flipping AI on/off changes only "extra features" (events/caching), never correctness

## Phase Summary

| Phase | Description | Backend | Sub-phases | Sessions | Depends On |
|-------|-------------|---------|------------|----------|------------|
| [A](phase-a-io-task-unification.md) | Universal IO task, remove direct path | Icom | A.1-A.6 | ~6 | — |
| [B](phase-b-priority-scheduling.md) | RT/BG priority scheduling | Icom | B.1-B.3 | ~3 | A |
| [C](phase-c-read-coalescing.md) | In-flight read coalescing (if needed) | Icom | C.1-C.2 | ~2 | B |
| [D](phase-d-text-protocol-migration.md) | Apply IO-task pattern to text-protocol rigs | Yaesu, Kenwood, Elecraft | D.1-D.4 | ~4-5 | A |
| [E](phase-e-flex-alignment.md) | Align Flex with new conventions | Flex | E.1-E.3 | ~3 | A |
| [F](phase-f-so2r-transactions.md) | Atomic command sequences for SO2R | All | F.1-F.3 | ~3-4 | B, hardware validation |

Orthogonal: [Error Model](error-model.md) (Err.1-Err.3, ~3 sessions) — can start after Phase A.3, should not block the IO-task migration.

**Total estimated effort: ~22-26 sessions**

## Dependency Graph

```
A.1 (Timeout Unification)
  |
  v
A.2 (IO types + Request/Response)
  |
  v
A.3 (IO task loop)                     Err.1 (Define error types)
  |                                       |
  +---> A.4 (Builder wiring)              v
  |       |                             Err.2 (Icom errors)
  |       v                               |
  |     A.5 (Delete direct path)          v
  |       |                             Err.3 (Text errors)
  |       v
  |     A.6 (Lifecycle / Drop)
  |
  +--- After A.5 is stable: --------------------------------+
       |                          |                          |
       v                          v                          v
  B.1 (Split channels)     D.1 (Generic text IO)      E.1 (Flex builder)
       |                          |                          |
       v                          v                          v
  B.2 (Route methods)      D.2 (Kenwood)              E.2 (Flex events)
       |                          |                          |
       v                          v                          v
  B.3 (Bounded buffers)    D.3 (Elecraft)              E.3 (Flex test infra)
       |                          |
       v                          v
  C.1 (Coalesce key)       D.4 (Yaesu)
       |                     [ONLY if profiling
       v                      shows need]
  C.2 (Fan-out)
       |
       +--- After B.2 + hardware validation:
            |
            v
       F.1 (Sequence request)
            |
            v
       F.2 (Rig-level atomics)
            |
            v
       F.3 (TX interlock)
```

## Migration Principles

- **One backend at a time.** Icom first (most mature transceive path), then text-protocol rigs, then Flex alignment.
- **No flag day.** Each backend migrates independently. Un-migrated backends continue working via their existing direct path.
- **Delete `execute_command_direct()` only after** parity tests pass for a backend.
- **Transport trait does not change.** The `&mut self` / `send` / `receive` shape is already correct for exclusive IO-task ownership. What changes is *how* the transport is held (moved into the IO task instead of `Arc<Mutex<_>>`).
- **Transport stays bytes-in/bytes-out.** Framing, buffering, resync, and response correlation live in the IO task. Do not push protocol frame parsing into `Transport`.
- **IO task lifecycle is explicit.** Dropping a rig must cancel/abort its IO task so processes exit cleanly and ports/sockets are not left busy. Graceful shutdown may be best-effort; `abort()` is an acceptable fallback.
- **Each sub-phase is independently committable and testable.** Sessions are sized to fit within Claude Code context limits.

## Key Architectural Concept

After migration, AI/transceive is no longer "the mode you must enable for the library to work." It becomes:

- **Off:** command/response only (no unsolicited event publishing)
- **On:** command/response + unsolicited decode into `RigEvent`s + optional caching

Removing `execute_command_direct()` does not remove AI-off support. It removes a second I/O pathway — eliminating a duplicated correctness surface that doubles debugging effort and blocks SO2R-grade stability.

## Key Risk Areas

- **A.3 (IO task loop)** is the most complex sub-phase and the foundation for everything else. Budget extra review time.
- **D.1 (Generic text IO extraction)** must avoid over-abstraction. The three text-protocol families are similar enough to share structure but different enough in prefix parsing to need manufacturer-specific hooks.
- **F.2 (Rig-level atomic operations)** requires hardware validation per manufacturer. Do not ship without testing on physical radios.
