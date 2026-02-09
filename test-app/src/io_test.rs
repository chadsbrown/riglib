// IO-test subcommand — architecture-level validation harness for the
// single-IO-task design.  Tests latency, throughput, response routing,
// priority scheduling, event delivery, and graceful shutdown against
// real hardware (or mock transport for smoke-testing).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rand::Rng;
use rand::SeedableRng;
use tokio_util::sync::CancellationToken;

use riglib::{ReceiverId, Rig, RigEvent};

use crate::RigAudio;

// ---------------------------------------------------------------------------
// CLI options (passed from main.rs)
// ---------------------------------------------------------------------------

pub struct IoTestOptions {
    pub phase_duration: u64,
    pub poll_rate: u32,
    pub roundtrip_count: u32,
    pub ptt_cycles: u32,
    pub rx: u8,
    pub phases: Vec<Phase>,
    pub skip_ptt: bool,
    pub ptt_safety_ms: u64,
    pub rt_threshold_ms: u64,
    pub max_error_rate: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    BgCorrectness,
    Throughput,
    Roundtrip,
    RtPriority,
    Transceive,
    Shutdown,
}

const ALL_PHASES: &[Phase] = &[
    Phase::BgCorrectness,
    Phase::Throughput,
    Phase::Roundtrip,
    Phase::RtPriority,
    Phase::Transceive,
    Phase::Shutdown,
];

pub fn parse_phases(s: &str) -> Result<Vec<Phase>> {
    if s.eq_ignore_ascii_case("all") {
        return Ok(ALL_PHASES.to_vec());
    }
    let mut phases = Vec::new();
    for part in s.split(',') {
        let p = match part.trim().to_lowercase().as_str() {
            "bg-correctness" => Phase::BgCorrectness,
            "throughput" => Phase::Throughput,
            "roundtrip" => Phase::Roundtrip,
            "rt-priority" => Phase::RtPriority,
            "transceive" => Phase::Transceive,
            "shutdown" => Phase::Shutdown,
            other => bail!(
                "unknown phase '{}'. Valid: bg-correctness, throughput, roundtrip, \
                 rt-priority, transceive, shutdown, all",
                other
            ),
        };
        phases.push(p);
    }
    Ok(phases)
}

fn phase_label(p: Phase) -> &'static str {
    match p {
        Phase::BgCorrectness => "bg-correctness",
        Phase::Throughput => "throughput",
        Phase::Roundtrip => "roundtrip",
        Phase::RtPriority => "rt-priority",
        Phase::Transceive => "transceive",
        Phase::Shutdown => "shutdown",
    }
}

// ---------------------------------------------------------------------------
// Latency statistics
// ---------------------------------------------------------------------------

struct LatencyStats {
    samples: Vec<Duration>,
}

struct ComputedStats {
    n: usize,
    min: Duration,
    avg: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    max: Duration,
}

impl LatencyStats {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    fn record(&mut self, d: Duration) {
        self.samples.push(d);
    }

    fn compute(&mut self) -> Option<ComputedStats> {
        let n = self.samples.len();
        if n == 0 {
            return None;
        }
        self.samples.sort();
        let sum: Duration = self.samples.iter().sum();
        let avg = sum / n as u32;
        Some(ComputedStats {
            n,
            min: self.samples[0],
            avg,
            p50: self.samples[n * 50 / 100],
            p95: self.samples[(n * 95 / 100).min(n - 1)],
            p99: self.samples[(n * 99 / 100).min(n - 1)],
            max: self.samples[n - 1],
        })
    }
}

impl ComputedStats {
    fn fmt_line(&self) -> String {
        format!(
            "n={}  min={:.1}ms  avg={:.1}ms  p50={:.1}ms  p95={:.1}ms  p99={:.1}ms  max={:.1}ms",
            self.n,
            self.min.as_secs_f64() * 1000.0,
            self.avg.as_secs_f64() * 1000.0,
            self.p50.as_secs_f64() * 1000.0,
            self.p95.as_secs_f64() * 1000.0,
            self.p99.as_secs_f64() * 1000.0,
            self.max.as_secs_f64() * 1000.0,
        )
    }
}

// ---------------------------------------------------------------------------
// Phase result
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Pass,
    Fail,
    Skip,
}

struct PhaseResult {
    phase: Phase,
    outcome: Outcome,
    detail: String,
}

fn print_results(model_name: &str, results: &[PhaseResult]) {
    println!();
    println!("============================================================");
    println!("  IO-Test Results — {}", model_name);
    println!("============================================================");
    println!();

    for r in results {
        let tag = match r.outcome {
            Outcome::Pass => "[PASS]",
            Outcome::Fail => "[FAIL]",
            Outcome::Skip => "[SKIP]",
        };
        println!("{} {}", tag, phase_label(r.phase));
        for line in r.detail.lines() {
            println!("  {}", line);
        }
        println!();
    }

    let run_count = results.iter().filter(|r| r.outcome != Outcome::Skip).count();
    let pass_count = results.iter().filter(|r| r.outcome == Outcome::Pass).count();
    let skip_count = results.iter().filter(|r| r.outcome == Outcome::Skip).count();

    println!("------------------------------------------------------------");
    if skip_count > 0 {
        println!(
            "  {}/{} phases passed ({} skipped)",
            pass_count, run_count, skip_count
        );
    } else {
        println!("  {}/{} phases passed", pass_count, run_count);
    }
    println!("============================================================");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn receiver_id(rx: u8) -> ReceiverId {
    ReceiverId::from_index(rx)
}

/// Save the current frequency and mode so we can restore after modifying phases.
async fn save_state(rig: &dyn Rig, rx: ReceiverId) -> Result<(u64, riglib::Mode)> {
    let freq = rig.get_frequency(rx).await?;
    let mode = rig.get_mode(rx).await?;
    Ok((freq, mode))
}

/// Restore previously saved frequency and mode.
async fn restore_state(rig: &dyn Rig, rx: ReceiverId, freq: u64, mode: riglib::Mode) {
    if let Err(e) = rig.set_frequency(rx, freq).await {
        eprintln!("  warning: failed to restore frequency: {e}");
    }
    if let Err(e) = rig.set_mode(rx, mode).await {
        eprintln!("  warning: failed to restore mode: {e}");
    }
}

/// Ensure PTT is off with up to 3 retry attempts.
async fn ensure_ptt_off(rig: &dyn Rig) {
    for attempt in 1..=3 {
        match rig.set_ptt(false).await {
            Ok(()) => return,
            Err(e) => {
                eprintln!(
                    "  warning: ensure_ptt_off attempt {}/3 failed: {e}",
                    attempt
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    eprintln!("  ERROR: could not turn PTT off after 3 attempts!");
}

// ---------------------------------------------------------------------------
// Poller task — shared background polling used by multiple phases
// ---------------------------------------------------------------------------

struct PollerResult {
    commands: u64,
    errors: u64,
    stats: LatencyStats,
}

async fn poller_task(
    rig: Arc<Box<dyn RigAudio>>,
    rx: ReceiverId,
    rate_hz: u32,
    cancel: CancellationToken,
) -> PollerResult {
    let period = Duration::from_secs_f64(1.0 / rate_hz as f64);
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut result = PollerResult {
        commands: 0,
        errors: 0,
        stats: LatencyStats::new(),
    };

    // Cycle through freq, mode, s-meter
    let mut cmd_index: u8 = 0;

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {}
        }

        let t = Instant::now();
        let ok = match cmd_index % 3 {
            0 => rig.get_frequency(rx).await.is_ok(),
            1 => rig.get_mode(rx).await.is_ok(),
            _ => rig.get_s_meter(rx).await.is_ok(),
        };
        let elapsed = t.elapsed();

        result.commands += 1;
        if ok {
            result.stats.record(elapsed);
        } else {
            result.errors += 1;
        }
        cmd_index = cmd_index.wrapping_add(1);
    }

    result
}

// ---------------------------------------------------------------------------
// Phase 1: bg-correctness
// ---------------------------------------------------------------------------

async fn phase_bg_correctness(
    rig: &Arc<Box<dyn RigAudio>>,
    opts: &IoTestOptions,
) -> PhaseResult {
    println!("  running bg-correctness ({} seconds)...", opts.phase_duration);

    let rx = receiver_id(opts.rx);
    let poll_rate = opts.poll_rate;
    let cancel = CancellationToken::new();
    let rig_clone = Arc::clone(rig);
    let cancel_clone = cancel.clone();

    let handle = tokio::spawn(async move {
        poller_task(rig_clone, rx, poll_rate, cancel_clone).await
    });

    tokio::time::sleep(Duration::from_secs(opts.phase_duration)).await;
    cancel.cancel();

    let mut pr = handle.await.unwrap();
    let elapsed = opts.phase_duration as f64;
    let rate = pr.commands as f64 / elapsed;
    let error_rate = if pr.commands > 0 {
        pr.errors as f64 / pr.commands as f64
    } else {
        0.0
    };

    let pass = error_rate <= opts.max_error_rate;
    let mut detail = format!(
        "{} commands in {:.1}s ({:.1} cmd/s), {} errors ({:.2}%)\n",
        pr.commands, elapsed, rate, pr.errors, error_rate * 100.0,
    );
    if let Some(stats) = pr.stats.compute() {
        detail.push_str(&stats.fmt_line());
    }

    PhaseResult {
        phase: Phase::BgCorrectness,
        outcome: if pass { Outcome::Pass } else { Outcome::Fail },
        detail,
    }
}

// ---------------------------------------------------------------------------
// Phase 2: throughput
// ---------------------------------------------------------------------------

async fn phase_throughput(
    rig: &Arc<Box<dyn RigAudio>>,
    opts: &IoTestOptions,
) -> PhaseResult {
    println!("  running throughput ({} seconds)...", opts.phase_duration);

    let rx = receiver_id(opts.rx);
    let mut stats = LatencyStats::new();
    let mut commands: u64 = 0;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(opts.phase_duration);

    while Instant::now() < deadline {
        let t = Instant::now();
        let _ = rig.get_frequency(rx).await;
        stats.record(t.elapsed());
        commands += 1;
    }

    let elapsed = start.elapsed().as_secs_f64();
    let rate = commands as f64 / elapsed;

    let mut detail = format!(
        "{} commands in {:.1}s ({:.1} cmd/s)\n",
        commands, elapsed, rate,
    );
    if let Some(s) = stats.compute() {
        detail.push_str(&s.fmt_line());
    }

    PhaseResult {
        phase: Phase::Throughput,
        outcome: Outcome::Pass, // informational only
        detail,
    }
}

// ---------------------------------------------------------------------------
// Phase 3: roundtrip — set/get consistency under concurrent load
// ---------------------------------------------------------------------------

async fn phase_roundtrip(
    rig: &Arc<Box<dyn RigAudio>>,
    opts: &IoTestOptions,
) -> PhaseResult {
    println!(
        "  running roundtrip ({} cycles, poller at {} Hz)...",
        opts.roundtrip_count, opts.poll_rate
    );

    let rx = receiver_id(opts.rx);

    // Save baseline state
    let baseline = match save_state(rig.as_ref().as_ref(), rx).await {
        Ok(b) => b,
        Err(e) => {
            return PhaseResult {
                phase: Phase::Roundtrip,
                outcome: Outcome::Fail,
                detail: format!("failed to save baseline state: {e}"),
            };
        }
    };

    let poll_rate = opts.poll_rate;
    let cancel = CancellationToken::new();

    // Spawn poller
    let rig_poller = Arc::clone(rig);
    let cancel_poller = cancel.clone();
    let poller_handle = tokio::spawn(async move {
        poller_task(rig_poller, rx, poll_rate, cancel_poller).await
    });

    // Writer task
    let rig_writer = Arc::clone(rig);
    let count = opts.roundtrip_count;
    let base_freq = baseline.0;

    let writer_handle = tokio::spawn(async move {
        let mut rng = rand::rngs::StdRng::from_entropy();
        let mut stats = LatencyStats::new();
        let mut matched: u32 = 0;
        let mut mismatched: u32 = 0;

        for _ in 0..count {
            let offset: i64 = rng.gen_range(-100_000..=100_000);
            let target = (base_freq as i64 + offset).max(0) as u64;

            let t = Instant::now();
            if let Err(_) = rig_writer.set_frequency(rx, target).await {
                mismatched += 1;
                continue;
            }

            tokio::time::sleep(Duration::from_millis(50)).await;

            match rig_writer.get_frequency(rx).await {
                Ok(readback) => {
                    stats.record(t.elapsed());
                    if readback == target {
                        matched += 1;
                    } else {
                        mismatched += 1;
                    }
                }
                Err(_) => {
                    mismatched += 1;
                }
            }
        }

        (matched, mismatched, stats)
    });

    let (matched, mismatched, mut write_stats) = writer_handle.await.unwrap();
    cancel.cancel();
    let poller_result = poller_handle.await.unwrap();

    // Restore state
    restore_state(rig.as_ref().as_ref(), rx, baseline.0, baseline.1).await;

    let poller_error_rate = if poller_result.commands > 0 {
        poller_result.errors as f64 / poller_result.commands as f64
    } else {
        0.0
    };

    let pass = mismatched == 0 && poller_error_rate <= opts.max_error_rate;

    let mut detail = format!(
        "{}/{} matched, poller: {} cmds, {} errors\n",
        matched, opts.roundtrip_count, poller_result.commands, poller_result.errors,
    );
    if let Some(s) = write_stats.compute() {
        detail.push_str(&format!("write: {}", s.fmt_line()));
    }

    PhaseResult {
        phase: Phase::Roundtrip,
        outcome: if pass { Outcome::Pass } else { Outcome::Fail },
        detail,
    }
}

// ---------------------------------------------------------------------------
// Phase 4: rt-priority — RT priority under BG load
// ---------------------------------------------------------------------------

async fn phase_rt_priority(
    rig: &Arc<Box<dyn RigAudio>>,
    opts: &IoTestOptions,
) -> PhaseResult {
    if opts.skip_ptt {
        return PhaseResult {
            phase: Phase::RtPriority,
            outcome: Outcome::Skip,
            detail: "skipped (--skip-ptt)".into(),
        };
    }

    println!(
        "  running rt-priority ({} PTT cycles, poller at {} Hz)...",
        opts.ptt_cycles, opts.poll_rate
    );

    let rx = receiver_id(opts.rx);
    let poll_rate = opts.poll_rate;
    let cancel = CancellationToken::new();

    // Spawn poller
    let rig_poller = Arc::clone(rig);
    let cancel_poller = cancel.clone();
    let poller_handle = tokio::spawn(async move {
        poller_task(rig_poller, rx, poll_rate, cancel_poller).await
    });

    let ptt_safety = Duration::from_millis(opts.ptt_safety_ms);
    let mut ptt_on_stats = LatencyStats::new();
    let mut ptt_off_stats = LatencyStats::new();
    let mut aborted = false;

    for cycle in 0..opts.ptt_cycles {
        // PTT ON
        let t = Instant::now();
        match tokio::time::timeout(ptt_safety, rig.set_ptt(true)).await {
            Ok(Ok(())) => {
                ptt_on_stats.record(t.elapsed());
            }
            Ok(Err(e)) => {
                eprintln!("  PTT ON failed at cycle {}: {e}", cycle);
                ensure_ptt_off(rig.as_ref().as_ref()).await;
                aborted = true;
                break;
            }
            Err(_) => {
                eprintln!("  PTT ON timed out at cycle {} ({}ms safety limit)", cycle, opts.ptt_safety_ms);
                ensure_ptt_off(rig.as_ref().as_ref()).await;
                aborted = true;
                break;
            }
        }

        // PTT OFF immediately
        let t = Instant::now();
        match tokio::time::timeout(ptt_safety, rig.set_ptt(false)).await {
            Ok(Ok(())) => {
                ptt_off_stats.record(t.elapsed());
            }
            Ok(Err(e)) => {
                eprintln!("  PTT OFF failed at cycle {}: {e}", cycle);
                ensure_ptt_off(rig.as_ref().as_ref()).await;
                aborted = true;
                break;
            }
            Err(_) => {
                eprintln!("  PTT OFF timed out at cycle {} ({}ms safety limit)", cycle, opts.ptt_safety_ms);
                ensure_ptt_off(rig.as_ref().as_ref()).await;
                aborted = true;
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    cancel.cancel();
    let poller_result = poller_handle.await.unwrap();

    if aborted {
        return PhaseResult {
            phase: Phase::RtPriority,
            outcome: Outcome::Fail,
            detail: "aborted due to PTT error/timeout".into(),
        };
    }

    let threshold = Duration::from_millis(opts.rt_threshold_ms);
    let on_stats = ptt_on_stats.compute();
    let off_stats = ptt_off_stats.compute();

    let pass = on_stats
        .as_ref()
        .is_some_and(|s| s.p95 <= threshold);

    let mut detail = String::new();
    if let Some(s) = &on_stats {
        detail.push_str(&format!("PTT on:  {}\n", s.fmt_line()));
    }
    if let Some(s) = &off_stats {
        detail.push_str(&format!("PTT off: {}\n", s.fmt_line()));
    }
    detail.push_str(&format!(
        "poller during test: {} cmds, {} errors",
        poller_result.commands, poller_result.errors,
    ));

    PhaseResult {
        phase: Phase::RtPriority,
        outcome: if pass { Outcome::Pass } else { Outcome::Fail },
        detail,
    }
}

// ---------------------------------------------------------------------------
// Phase 5: transceive — event delivery under load
// ---------------------------------------------------------------------------

async fn phase_transceive(
    rig: &Arc<Box<dyn RigAudio>>,
    opts: &IoTestOptions,
) -> PhaseResult {
    let caps = rig.capabilities();
    if !caps.has_transceive {
        return PhaseResult {
            phase: Phase::Transceive,
            outcome: Outcome::Skip,
            detail: "rig does not support transceive mode".into(),
        };
    }

    let poll_rate = opts.poll_rate;
    println!("  running transceive (20 freq changes, poller at {} Hz)...", poll_rate);

    let rx = receiver_id(opts.rx);

    // Save baseline
    let baseline = match save_state(rig.as_ref().as_ref(), rx).await {
        Ok(b) => b,
        Err(e) => {
            return PhaseResult {
                phase: Phase::Transceive,
                outcome: Outcome::Fail,
                detail: format!("failed to save baseline state: {e}"),
            };
        }
    };

    // Subscribe to events
    let mut event_rx = match rig.subscribe() {
        Ok(rx) => rx,
        Err(e) => {
            return PhaseResult {
                phase: Phase::Transceive,
                outcome: Outcome::Fail,
                detail: format!("failed to subscribe: {e}"),
            };
        }
    };

    let cancel = CancellationToken::new();

    // Spawn poller
    let rig_poller = Arc::clone(rig);
    let cancel_poller = cancel.clone();
    let poller_handle = tokio::spawn(async move {
        poller_task(rig_poller, rx, poll_rate, cancel_poller).await
    });

    let iterations = 20u32;
    let mut events_received = 0u32;
    let mut rng = rand::rngs::StdRng::from_entropy();
    let base_freq = baseline.0;

    for _ in 0..iterations {
        let offset: i64 = rng.gen_range(-100_000..=100_000);
        let target = (base_freq as i64 + offset).max(0) as u64;

        if rig.set_frequency(rx, target).await.is_err() {
            continue;
        }

        // Drain event_rx for up to 500ms looking for matching FrequencyChanged
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, event_rx.recv()).await {
                Ok(Ok(RigEvent::FrequencyChanged { freq_hz, .. })) if freq_hz == target => {
                    events_received += 1;
                    break;
                }
                Ok(Ok(_)) => continue,                  // different event, keep draining
                Ok(Err(_)) => break,                     // channel error
                Err(_) => break,                         // timeout
            }
        }
    }

    cancel.cancel();
    let _ = poller_handle.await;

    // Restore state
    restore_state(rig.as_ref().as_ref(), rx, baseline.0, baseline.1).await;

    let pct = (events_received as f64 / iterations as f64) * 100.0;
    let pass = events_received >= (iterations * 90 / 100);

    PhaseResult {
        phase: Phase::Transceive,
        outcome: if pass { Outcome::Pass } else { Outcome::Fail },
        detail: format!("{}/{} events received ({:.0}%)", events_received, iterations, pct),
    }
}

// ---------------------------------------------------------------------------
// Phase 6: shutdown — graceful shutdown under load
// ---------------------------------------------------------------------------

async fn phase_shutdown(
    rig: Arc<Box<dyn RigAudio>>,
    opts: &IoTestOptions,
    is_standalone: bool,
) -> PhaseResult {
    if !is_standalone {
        return PhaseResult {
            phase: Phase::Shutdown,
            outcome: Outcome::Skip,
            detail: "Run separately: io-test --phases shutdown".into(),
        };
    }

    println!("  running shutdown (2s polling then drop)...");

    let rx = receiver_id(opts.rx);
    let poll_rate = opts.poll_rate;
    let cancel = CancellationToken::new();

    let rig_poller = Arc::clone(&rig);
    let cancel_poller = cancel.clone();
    let poller_handle = tokio::spawn(async move {
        poller_task(rig_poller, rx, poll_rate, cancel_poller).await
    });

    tokio::time::sleep(Duration::from_secs(2)).await;

    // Drop the rig — the Arc will be the last strong reference once
    // poller is cancelled.
    cancel.cancel();
    let _ = poller_handle.await;
    drop(rig);

    PhaseResult {
        phase: Phase::Shutdown,
        outcome: Outcome::Pass,
        detail: "process exited cleanly".into(),
    }
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

pub async fn cmd_io_test(rig: Arc<Box<dyn RigAudio>>, opts: IoTestOptions) -> Result<()> {
    let info = rig.info().clone();
    let model_name = format!("{} {}", info.manufacturer, info.model_name);

    println!();
    println!("IO-Test — {}", model_name);
    println!();

    let is_standalone = opts.phases.len() == 1 && opts.phases[0] == Phase::Shutdown;
    let mut results = Vec::new();

    for &phase in &opts.phases {
        let result = match phase {
            Phase::BgCorrectness => phase_bg_correctness(&rig, &opts).await,
            Phase::Throughput => phase_throughput(&rig, &opts).await,
            Phase::Roundtrip => phase_roundtrip(&rig, &opts).await,
            Phase::RtPriority => phase_rt_priority(&rig, &opts).await,
            Phase::Transceive => phase_transceive(&rig, &opts).await,
            Phase::Shutdown => phase_shutdown(Arc::clone(&rig), &opts, is_standalone).await,
        };
        results.push(result);
    }

    print_results(&model_name, &results);

    let any_failed = results.iter().any(|r| r.outcome == Outcome::Fail);
    if any_failed {
        std::process::exit(1);
    }

    Ok(())
}
