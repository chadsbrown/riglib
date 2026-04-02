# Plan: Detect AI/Transceive Mode in riglib, Expose to Consuming Apps

## Context

riglib has full transceive/AI frame parsing implemented but unreachable (`ai_enabled` hardcoded to `false`). clogger works around this by polling at 4 Hz. The design intent: **riglib should detect whether AI mode is enabled, expose it as a readable property, and let consuming apps decide whether to use events or poll.**

hamlib (the popular C library) avoids this problem entirely -- it disables AI mode on connect for Kenwood/Elecraft because its synchronous model can't handle unsolicited data, and passively ignores Icom transceive frames. riglib's IO task architecture already handles interleaved unsolicited frames cleanly, so we can do better.

### Current state

- `ai_enabled: false` hardcoded in all 4 backends (Icom, Elecraft, Kenwood, Yaesu)
- IO tasks always receive unsolicited data on the idle buffer but drain (discard) it when `ai_enabled` is false
- `process_idle_frames()` and `drain_idle_frames()` do identical frame parsing work -- the only difference is emitting vs discarding events. Zero cost to always processing.
- `enable_transceive()` / `disable_transceive()` exist but are disconnected from the IO task's flag
- No method exists to query the radio's current AI mode status
- Transceive frame parsing is fully implemented and tested in all backends -- just never runs in production
- clogger unconditionally polls at 4 Hz as a workaround

### How hamlib handles this (for reference)

- **Kenwood/Elecraft**: hamlib queries current AI state (`AI;`), saves it, then **turns AI OFF** (`AI0;`) on connect because its synchronous transaction model can't handle unsolicited data. Restores on disconnect. Partial mitigation: unexpected `FA`/`FB` responses mid-transaction are cached and the read is retried.
- **Icom**: hamlib does NOT control CI-V transceive at all. No set/get command. If the user enables it in the radio menu, hamlib's async thread processes broadcasts passively. But CI-V transceive frames don't identify which VFO changed, so hamlib still polls.
- **Public API**: `rig_set_trn()` / `rig_get_trn()` are **deprecated** and return `-RIG_EDEPRECATED`. No mechanism for consuming apps to query AI state or choose between polling and events.

## Design Decisions

1. **Icom**: Passive observation -- always process idle frames, track whether transceive frames have been seen, flip a property from false to true on first observation
2. **Kenwood/Elecraft/Yaesu**: Query only -- send `AI;` at connect, report the result, don't change it
3. **API shape**: Observable property -- detected at build time (text protocols) or dynamically (Icom), stored as a field, instant reads
4. **FlexRadio**: Always true (SmartSDR is inherently event-driven)

## Implementation

### Step 1: Flip `ai_enabled` to `true` in all backends

This is a prerequisite. The IO task must process (not drain) idle frames for both detection and event emission to work. Since `process_idle_frames()` and `drain_idle_frames()` do identical frame parsing, there is zero cost.

| File | Line | Change |
|------|------|--------|
| `riglib-icom/src/builder.rs` | 205 | `ai_enabled: false` -> `true` |
| `riglib-icom/src/io.rs` | 353 | Always pass `Some(event_tx)` (remove `ai_enabled` guard) |
| `riglib-elecraft/src/rig.rs` | 177 | `ai_enabled: false` -> `true` |
| `riglib-kenwood/src/rig.rs` | 174 | `ai_enabled: false` -> `true` |
| `riglib-yaesu/src/rig.rs` | 180 | `ai_enabled: false` -> `true` |

### Step 2: Add `is_transceive_active()` to the Rig trait

**File**: `riglib-core/src/rig.rs`

```rust
/// Whether transceive (AI) mode is currently active on the radio.
///
/// When `true`, the radio is sending unsolicited state updates and the
/// [`subscribe()`](Rig::subscribe) channel will receive real-time events
/// without polling. When `false`, the application should poll via
/// `get_frequency()` / `get_mode()` etc.
///
/// For text-protocol rigs (Kenwood, Elecraft, Yaesu), this is queried
/// at connect time. For Icom CI-V, this is detected passively by
/// observing whether transceive frames arrive on the bus.
fn is_transceive_active(&self) -> bool {
    false
}
```

Note: synchronous, not async. It reads a cached/observable field, not the radio.

### Step 3: Text protocol backends -- query `AI;` at build time

**New commands** (identical in all three `commands.rs`):

Files: `riglib-elecraft/src/commands.rs`, `riglib-kenwood/src/commands.rs`, `riglib-yaesu/src/commands.rs`

```rust
pub fn cmd_read_ai() -> Vec<u8> {
    encode_command("AI", "")  // sends "AI;"
}

pub fn parse_ai_response(data: &str) -> Result<bool> {
    match data.trim() {
        "2" | "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(Error::Protocol(format!("unexpected AI response: {data:?}")))
    }
}
```

**Builder changes** -- query AI mode after construction:

Files: `riglib-elecraft/src/builder.rs`, `riglib-kenwood/src/builder.rs`, `riglib-yaesu/src/builder.rs`

In `build_with_transport()`, after constructing the rig:

```rust
let mut rig = ElecraftRig::new(transport, ...);
if rig.capabilities().has_transceive {
    match rig.query_ai_mode().await {
        Ok(active) => rig.set_transceive_active(active),
        Err(e) => tracing::warn!("failed to query AI mode: {e}"),
    }
}
Ok(rig)
```

This requires:
- Adding `query_ai_mode()` -- a `pub(crate)` async method that sends `AI;` and parses the response
- Adding `set_transceive_active()` -- a `pub(crate)` setter for the internal field
- Adding a `transceive_active: bool` field to each rig struct
- Implementing `is_transceive_active()` on the Rig trait to return `self.transceive_active`

**Rig struct addition** (each text-protocol rig):
```rust
pub struct ElecraftRig {
    // ... existing fields ...
    transceive_active: bool,
}
```

### Step 4: Icom backend -- passive observation

**Approach**: The IO task already processes transceive frames when `ai_enabled` is true. We need it to also signal when it has seen one.

Add an `AtomicBool` shared between the IO task and `IcomRig`:

**File**: `riglib-icom/src/builder.rs`

```rust
let transceive_observed = Arc::new(AtomicBool::new(false));
let io = spawn_io_task(
    transport,
    IoConfig { ai_enabled: true, ... },
    event_tx.clone(),
    Arc::clone(&transceive_observed),  // new parameter
);
Ok(IcomRig::new(
    io, ..., transceive_observed,
))
```

**File**: `riglib-icom/src/io.rs`

`spawn_io_task()` accepts `Arc<AtomicBool>` and passes it to `io_loop()`. In the idle read branch, after `process_idle_frames()` processes any frames, set the flag:

```rust
if config.ai_enabled {
    let before = idle_buf.len();
    process_idle_frames(&mut idle_buf, config.civ_address, &event_tx);
    if before > idle_buf.len() {
        // Frames were consumed -- transceive is active
        transceive_observed.store(true, Ordering::Relaxed);
    }
}
```

Also set it when interleaved transceive frames are detected during command execution (around line 452-461):

```rust
if transceive::is_transceive_frame(&frame, civ_address) {
    transceive::process_single_transceive_frame(&frame, civ_address, tx);
    transceive_observed.store(true, Ordering::Relaxed);
    continue;
}
```

**File**: `riglib-icom/src/rig.rs`

```rust
pub struct IcomRig {
    // ... existing fields ...
    transceive_observed: Arc<AtomicBool>,
}

impl Rig for IcomRig {
    fn is_transceive_active(&self) -> bool {
        self.transceive_observed.load(Ordering::Relaxed)
    }
}
```

**Behavior**: Starts as `false`. Flips to `true` the moment the IO task sees any transceive frame (either in the idle buffer or interleaved during a command). Never flips back to `false`. This is correct because CI-V transceive is a radio menu setting that doesn't change during a session.

### Step 5: FlexRadio -- always true

**File**: `riglib-flex/src/rig.rs`

```rust
fn is_transceive_active(&self) -> bool {
    true  // SmartSDR is inherently event-driven
}
```

### Step 6: clogger -- conditionally poll based on property

**File**: `logger-runtime/src/rig_adapter.rs`

Replace the unconditional poll task with a conditional one. The subscription task stays unchanged (it always runs -- events come from both AI mode and polling).

```rust
// Subscribe and forward events (unchanged)
let mut events = rig.subscribe()?;
tokio::spawn(async move { /* ... existing event forwarding ... */ });

// Poll only if transceive is not active
if !rig.is_transceive_active() {
    info!("AI mode not active, polling at 4 Hz");
    let poll_rig = Arc::clone(&rig);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        loop {
            interval.tick().await;
            if let Err(e) = poll_rig.get_frequency(primary).await { ... }
            if let Err(e) = poll_rig.get_mode(primary).await { ... }
        }
    });
} else {
    info!("AI mode active, using transceive events");
}
```

**Note for Icom**: At startup, `is_transceive_active()` will be `false` (no transceive frames observed yet), so clogger will start polling. This is correct -- if the user has CI-V transceive enabled, the polling will also trigger transceive broadcasts, causing the flag to flip. A future optimization could check the flag periodically and stop the poll task, but that's not required for the initial implementation. The poll + transceive running together is harmless (duplicate events are just overwrites in the reducer).

## Files Modified

### riglib
| File | Change |
|------|--------|
| `riglib-core/src/rig.rs` | Add `fn is_transceive_active(&self) -> bool` to Rig trait |
| `riglib-icom/src/builder.rs` | `ai_enabled: true`, pass `Arc<AtomicBool>` to IO task |
| `riglib-icom/src/io.rs` | Accept `Arc<AtomicBool>`, set on transceive frame detection |
| `riglib-icom/src/rig.rs` | Add `transceive_observed` field, implement `is_transceive_active()` |
| `riglib-elecraft/src/builder.rs` | Query `AI;` after construction, set property |
| `riglib-elecraft/src/rig.rs` | Add `transceive_active` field, implement `is_transceive_active()`, add `query_ai_mode()` |
| `riglib-elecraft/src/commands.rs` | Add `cmd_read_ai()`, `parse_ai_response()` |
| `riglib-kenwood/src/builder.rs` | Query `AI;` after construction, set property |
| `riglib-kenwood/src/rig.rs` | Add `transceive_active` field, implement `is_transceive_active()`, add `query_ai_mode()` |
| `riglib-kenwood/src/commands.rs` | Add `cmd_read_ai()`, `parse_ai_response()` |
| `riglib-yaesu/src/builder.rs` | Query `AI;` after construction, set property |
| `riglib-yaesu/src/rig.rs` | Add `transceive_active` field, implement `is_transceive_active()`, add `query_ai_mode()` |
| `riglib-yaesu/src/commands.rs` | Add `cmd_read_ai()`, `parse_ai_response()` |
| `riglib-flex/src/rig.rs` | Implement `is_transceive_active()` returning `true` |

### clogger
| File | Change |
|------|--------|
| `logger-runtime/src/rig_adapter.rs` | Conditionally start poll task based on `is_transceive_active()` |

## Test Impact

- Existing tests unaffected by `ai_enabled` flip (MockTransport sends no unsolicited data)
- Text protocol builder tests need `AI;` expectation added to MockTransport (query happens during build)
- New unit tests for `cmd_read_ai()` / `parse_ai_response()` in each commands.rs
- New test for Icom `transceive_observed` AtomicBool flip when transceive frame is processed
- Icom IO task tests that construct IoConfig with `ai_enabled: false` should be updated to `true`

## Verification

1. `cargo test` in riglib workspace
2. Hardware: connect Kenwood/Elecraft with AI mode on/off, verify `is_transceive_active()` reports correctly
3. Hardware: connect Icom with CI-V transceive on, turn VFO, verify property flips to true
4. clogger: run `logger-tui`, verify polling starts/stops based on AI mode detection
