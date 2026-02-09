# Phase C — Read Coalescing (Optional, Profile First)

**Depends on:** [Phase B](phase-b-priority-scheduling.md)
**Deliverable:** Reduced wire traffic under high polling load, if profiling shows it's needed.
**Estimated effort:** 2 sessions (if implemented)

**Default: skip this phase until profiling shows a need.**

---

## Decision Framework

### When Coalescing Is Worth It

- UI/panadapter polling at high frequency (10-50 Hz)
- Multiple consumers (UI + logger + automation + CW skimmer)
- Visible command queue growth or latency spikes
- Targeting SO2R and need predictable RT response

### The Numbers

At 19200 baud on CI-V, each frequency read (6 bytes out, 11 bytes back) takes ~8.9ms on the wire. A contest logger polling at 10 Hz plus a band map at 20 Hz plus a skimmer checking mode = 30+ `get_frequency()` calls/second = 267ms of bus time/second. Feasible, but leaves little headroom for setters and PTT.

### When To Skip

- Still validating protocol correctness with real rigs
- Not seeing high polling load yet
- Want to keep the IO task simple until drivers are stable

---

## Coalescing Is Protocol-Aware

For CI-V, `get_frequency` on VFO A and `get_frequency` on VFO B are DIFFERENT commands (they may require a preceding `select_receiver()` call on the IC-7610). Coalescing must key on the full command bytes or a semantic key like `(CommandType::GetFrequency, ReceiverId)`, not just the function name.

For text-protocol rigs, this is simpler: `FA;` and `FB;` are distinct commands with distinct prefixes. Two `FA;` requests in-flight can trivially be coalesced.

---

## Sub-Phase C.1 — Add Coalesce Key to Request

**Scope:** Plumbing only, no behavioral change. Single session.

- Add an optional `CoalesceKey` to `Request::CivCommand`.
- Define `CoalesceKey` as an opaque hash/enum covering `get_frequency(VFO_A)`, `get_mode(VFO_A)`, etc.

---

## Sub-Phase C.2 — In-Flight Tracking + Fan-Out

**Scope:** Behavioral change. Single session. Depends on C.1.

- In the IO task, maintain a `HashMap<CoalesceKey, Vec<oneshot::Sender<Result<CivFrame>>>>`.
- When a request arrives with a coalesce key matching an in-flight command, attach the new sender as a waiter instead of queuing a new wire command.
- When the response arrives, fan it out to all waiters.
- Do **not** add time-window caching.
- Do **not** add "drop older reads."

### Scope — Keep Narrow

Start with only idempotent, high-frequency reads:

- `get_frequency(rx)`
- `get_mode(rx)`
- Maybe `get_ptt_state()`

**Never coalesce:**
- Setters
- Anything with side effects
- Reads whose results can legitimately differ between identical in-flight requests

### Definition of Done

- 1,000 concurrent `get_frequency()` calls produce ~1 wire transaction per in-flight window.
- No correctness change for non-coalesced commands.
- Coalescing can be disabled without code changes (config flag).

---

## Alternative: Caller-Side Rate Limiting (CachedRig)

Before implementing Phase C, consider a `CachedRig` wrapper that caches recent GET results with a configurable TTL (e.g., 50ms). This is much simpler:

```rust
pub struct CachedRig<R: Rig> {
    inner: R,
    freq_cache: DashMap<ReceiverId, (Instant, u64)>,
    cache_ttl: Duration,
}
```

This keeps the IO task simple and pushes caching to the consumer layer. It may provide 90% of the benefit with 10% of the complexity. Evaluate this first before committing to IO-task-level coalescing.
