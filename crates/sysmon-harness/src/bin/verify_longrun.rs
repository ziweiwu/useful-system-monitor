//! I-10b for the Rust build: resident memory must stay flat across a long run.
//!
//! This is the check the whole exercise started with. The TypeScript build died
//! after ~31 hours on `FATAL ERROR: Ineffective mark-compacts near heap limit`,
//! because React's development reconciler emitted a `performance.measure()` per
//! component per commit and Node never evicts user-timing entries — 173.7 KB
//! per render, held by the runtime rather than by anything the app owned.
//!
//! **Rust does not make that class of bug impossible.** It prevents
//! use-after-free and data races; it has no opinion on a cache that never
//! evicts, and this app has four of them — the CPU delta tracker, the process
//! name cache, the per-process history rings and the global histories. Each is
//! bounded by construction and unit-tested (I-10), and this is what proves the
//! whole assembly of them actually is.
//!
//! Measured as **KB per render**, not per second, because a render is the unit
//! that allocates and the tier only decides how fast renders arrive.
//!
//! Runs against `--mock` by default so the process list holds still: a real one
//! churns, which legitimately moves the name cache and would be noise here.
//!
//! Usage:
//!   cargo run -p sysmon-harness --bin verify_longrun -- [--secs N] [--tick MS]

use std::io::Read;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize};

/// Under this, the app is not the reason the machine ran out of memory.
///
/// Generous next to the 173.7 KB/render that actually shipped, and tight enough
/// that a genuinely unbounded cache cannot hide under it: at 5 KB/render and the
/// 10s default tier, reaching 4 GB would take over two thousand years.
const BUDGET_KB_PER_RENDER: f64 = 5.0;
const DEFAULT_SECS: u64 = 90;
const DEFAULT_TICK_MS: u64 = 100;
/// Big enough that draining the pty never becomes the thing being measured.
const DRAIN_BUFFER_BYTES: usize = 16384;
/// Let the caches fill before measuring, so their one-time growth is not read
/// as a slope.
const WARMUP: Duration = Duration::from_secs(15);
const REPORT_EVERY: Duration = Duration::from_secs(15);
/// Nothing was drawn, so there is no slope to report.
const NO_SLOPE: f64 = 0.0;
/// The 4 GB heap death this check exists to catch.
const HEAP_LIMIT_KB: f64 = 4.0 * 1024.0 * 1024.0;
const DEFAULT_TIER_SECS: f64 = 10.0;
const SECS_PER_DAY: f64 = 86_400.0;
/// Below this the run proves nothing, however flat the memory looks — an app
/// that never redraws never allocates, which is exactly how a dead build once
/// passed a heap check.
const MIN_RENDERS: usize = 200;

fn rss_kb(pid: u32) -> Option<f64> {
    let out = Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// `--secs N --tick N`, or `None` if the command line held something else.
fn parse_args(args: &[String]) -> Option<(u64, u64)> {
    let mut secs = DEFAULT_SECS;
    let mut tick_ms = DEFAULT_TICK_MS;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--secs" => {
                secs = args[i + 1].parse().unwrap_or(secs);
                i += 1;
            }
            "--tick" => {
                tick_ms = args[i + 1].parse().unwrap_or(tick_ms);
                i += 1;
            }
            other => {
                eprintln!("verify_longrun: unknown option {other}");
                return None;
            }
        }
        i += 1;
    }
    Some((secs, tick_ms))
}

/// The app under a pty, on a fast tier.
///
/// A fast tier buys renders rather than wall-clock: the question is cost per
/// render, so a 1s tier compresses hours of the 10s default into minutes.
fn spawn_app(
    pair: &portable_pty::PtyPair,
) -> Result<Box<dyn portable_pty::Child + Send + Sync>, String> {
    let bin = std::fs::canonicalize("target/release/sysmon")
        .or_else(|_| std::fs::canonicalize("target/debug/sysmon"))
        .map_err(|e| format!("build the binary first ({e})"))?;
    let mut cmd = CommandBuilder::new(bin);
    cmd.arg("--mock");
    cmd.arg("--interval");
    cmd.arg("1");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    pair.slave.spawn_command(cmd).map_err(|e| e.to_string())
}

/// Count frames by ratatui's synchronized-update marker.
///
/// ratatui's crossterm backend hides the cursor once per draw, so `ESC [ ? 2 5
/// l` is one frame. Counted rather than parsed: this check is about what the
/// frames *cost*, not what is in them.
///
/// The tail is carried between reads because the marker can straddle a buffer
/// boundary — and a counter that silently misses frames reports a larger
/// KB/render than the truth, which is the direction that turns a healthy build
/// into a failing one.
fn count_frames(mut reader: Box<dyn std::io::Read + Send>) -> mpsc::Receiver<usize> {
    let (tx, rx) = mpsc::channel::<usize>();
    std::thread::spawn(move || {
        const MARKER: &[u8] = b"\x1b[?25l";
        let mut buf = [0u8; DRAIN_BUFFER_BYTES];
        let mut tail: Vec<u8> = Vec::new();
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                return;
            }
            tail.extend_from_slice(&buf[..n]);
            let frames = tail.windows(MARKER.len()).filter(|w| *w == MARKER).count();
            // Keep just enough to catch a marker split across the boundary.
            let keep = MARKER.len() - 1;
            if tail.len() > keep {
                tail.drain(..tail.len() - keep);
            }
            if tx.send(frames).is_err() {
                return;
            }
        }
    });
    rx
}

/// KB per render between two readings, or zero when nothing was drawn.
fn kb_per_render(now_kb: f64, base: (f64, usize), renders: usize) -> (usize, f64) {
    let (base_kb, base_renders) = base;
    let n = renders.saturating_sub(base_renders);
    let per = if n > 0 {
        (now_kb - base_kb) / n as f64
    } else {
        NO_SLOPE
    };
    (n, per)
}

/// Drive the app for the whole run, and report the baseline it settled to.
fn drive(
    pid: u32,
    writer: &mut Box<dyn std::io::Write + Send>,
    frames: &mpsc::Receiver<usize>,
    run: (u64, u64),
) -> (Option<(f64, usize)>, usize) {
    let (secs, tick_ms) = run;
    // Keys, so the app is redrawing for input as well as for samples — the
    // combination a person actually produces.
    let keys = [b"1", b"2", b"3", b"4", b"5"];
    let start = Instant::now();
    let mut renders = 0usize;
    let mut baseline = None::<(f64, usize)>;
    let mut last_report = Instant::now();

    while start.elapsed() < Duration::from_secs(secs) + WARMUP {
        std::thread::sleep(Duration::from_millis(tick_ms));
        use std::io::Write;
        let _ = writer.write_all(keys[renders % keys.len()]);
        let _ = writer.flush();
        while let Ok(n) = frames.try_recv() {
            renders += n;
        }

        if baseline.is_none() && start.elapsed() >= WARMUP {
            baseline = rss_kb(pid).map(|kb| (kb, renders));
        }
        if last_report.elapsed() >= REPORT_EVERY {
            last_report = Instant::now();
            if let (Some(kb), Some(base)) = (rss_kb(pid), baseline) {
                let (n, per) = kb_per_render(kb, base, renders);
                println!(
                    "  {:>3}s  rss {kb:>8.0} KB  renders {n:>6}  {per:>7.3} KB/render",
                    start.elapsed().as_secs()
                );
            }
        }
    }
    (baseline, renders)
}

/// A terminal wide enough that no column is dropped, so every render draws the
/// whole frame.
fn open_pty() -> Result<portable_pty::PtyPair, String> {
    portable_pty::native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

/// The slope, and whether it clears I-10b.
fn verdict(end_kb: f64, base: (f64, usize), renders: usize) -> std::process::ExitCode {
    let (base_kb, base_renders) = base;
    let n = renders.saturating_sub(base_renders);
    let per = if n > 0 {
        (end_kb - base_kb) / n as f64
    } else {
        f64::INFINITY
    };

    println!("\n{base_kb:.0} KB -> {end_kb:.0} KB over {n} renders = {per:.3} KB/render");
    if per > NO_SLOPE {
        let renders_to_limit = HEAP_LIMIT_KB / per;
        let days = renders_to_limit * DEFAULT_TIER_SECS / SECS_PER_DAY;
        println!("at this rate and the 10s default tier, 4 GB in ~{days:.0} days");
    }

    let drew = n >= MIN_RENDERS;
    let flat = per <= BUDGET_KB_PER_RENDER;
    if !drew {
        println!("\nonly {n} renders — the app is not drawing, so flat memory means nothing.");
    }
    println!(
        "I-10b: {}  (renders {n} >= {MIN_RENDERS}: {}; {per:.3} <= {BUDGET_KB_PER_RENDER} KB/render: {})",
        if drew && flat { "PASS" } else { "FAIL" },
        if drew { "ok" } else { "NO" },
        if flat { "ok" } else { "NO" }
    );
    if drew && flat {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(1)
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((secs, tick_ms)) = parse_args(&args) else {
        return std::process::ExitCode::from(2);
    };

    let pair = match open_pty() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("verify_longrun: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let mut child = match spawn_app(&pair) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("verify_longrun: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let pid = child.process_id().unwrap_or(0);

    let frames = count_frames(pair.master.try_clone_reader().expect("a reader"));
    let mut writer = pair.master.take_writer().expect("a writer");

    println!("pid {pid}, {secs}s at a 1s tier, keys every {tick_ms}ms\n");
    // Let the caches fill before measuring, so their one-time growth is not
    // read as a slope.
    let (baseline, renders) = drive(pid, &mut writer, &frames, (secs, tick_ms));

    let end_kb = rss_kb(pid);
    let _ = child.kill();
    let _ = child.wait();

    let (Some(base), Some(end_kb)) = (baseline, end_kb) else {
        eprintln!("verify_longrun: no baseline, or the process was gone before the final reading");
        return std::process::ExitCode::from(1);
    };
    verdict(end_kb, base, renders)
}
