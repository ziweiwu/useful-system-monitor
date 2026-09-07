//! Rust against Node, on the four things that actually matter for a tool that
//! sits in a background pane all day.
//!
//! Everything here is measured on this machine, interleaved, in the same run.
//! Interleaving is not decoration: a laptop's CPU frequency and thermal state
//! drift over minutes, so running all of A then all of B measures the drift as
//! much as the difference. Alternating and taking the **median** cancels both.
//!
//! What is measured, and why each one:
//!
//! - **Startup to first output.** The one-shot path is what a script or a cron
//!   job pays, every invocation.
//! - **Resident memory.** The dashboard is meant to be left open; RSS is the
//!   rent it charges for that.
//! - **CPU per sample.** The app's own claim is that watching the machine
//!   costs ~1% of a core. This is the number behind it.
//! - **Install size.** What a user downloads, which for the Node build is the
//!   dependency tree and for the Rust one is a single binary.
//!
//! Usage: cargo run -p sysmon-harness --bin bench -- [--runs N]

use std::io::Read;
use std::process::Command;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize};

/// A measurement that did not happen. Distinct from a real zero only by
/// context, but every caller here treats it as "no reading".
const NOTHING_MEASURED: f64 = 0.0;
const MS_PER_SEC: f64 = 1000.0;
const SECS_PER_MIN: f64 = 60.0;
const MINS_PER_HOUR: f64 = 60.0;
const BYTES_PER_KB: f64 = 1024.0;
/// `/usr/bin/time -l`'s ` real ` line puts system time in this field.
const SYS_TIME_FIELD: usize = 4;
/// Big enough that draining the pty never becomes the thing being measured.
const DRAIN_BUFFER_BYTES: usize = 16384;
/// Startup and the first samples are not steady state, so they are skipped.
const SETTLE_SECS: u64 = 8;
const RSS_SAMPLE_INTERVAL_MS: u64 = 500;
/// Long enough to span several sampling ticks at the 10s default tier.
const STEADY_STATE_SECS: u64 = 60;
/// Odd, so the median is a measured run rather than an average of two.
const DEFAULT_RUNS: usize = 9;
/// Lets the machine settle between interleaved runs.
const BETWEEN_RUNS_MS: u64 = 50;
/// 100% of one core fully busy.
const PERCENT_SCALE: f64 = 100.0;

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if samples.is_empty() {
        return NOTHING_MEASURED;
    }
    let mid = samples.len() / 2;
    if samples.len() % 2 == 0 {
        f64::midpoint(samples[mid - 1], samples[mid])
    } else {
        samples[mid]
    }
}

/// Wall clock for one full `--json` run, including process start and exit.
fn time_oneshot(program: &str, args: &[&str]) -> Option<f64> {
    let start = Instant::now();
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(start.elapsed().as_secs_f64() * MS_PER_SEC)
}

/// Peak resident set and total CPU time of a finished process, from
/// `/usr/bin/time -l`, which reports both without needing to sample.
fn measure_run(program: &str, args: &[&str]) -> Option<(f64, f64)> {
    let mut cmd = Command::new("/usr/bin/time");
    cmd.arg("-l").arg(program).args(args);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let field = |needle: &str| -> Option<f64> {
        text.lines()
            .find(|l| l.contains(needle))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    let rss_bytes = field("maximum resident set size")?;
    let user: f64 = field("user")
        .or_else(|| {
            text.lines()
                .find(|l| l.contains(" real "))?
                .split_whitespace()
                .nth(2)?
                .parse()
                .ok()
        })
        .unwrap_or(NOTHING_MEASURED);
    let sys: f64 = text
        .lines()
        .find(|l| l.contains(" real "))
        .and_then(|l| l.split_whitespace().nth(SYS_TIME_FIELD)?.parse().ok())
        .unwrap_or(NOTHING_MEASURED);
    Some((rss_bytes / BYTES_PER_KB, (user + sys) * MS_PER_SEC))
}

/// Bytes under a path, following nothing.
fn dir_size(path: &str) -> u64 {
    fn walk(entry: &std::path::Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(entry) else {
            return 0;
        };
        if meta.is_file() {
            return meta.len();
        }
        if !meta.is_dir() {
            return 0;
        }
        std::fs::read_dir(entry)
            .map(|it| it.flatten().map(|e| walk(&e.path())).sum())
            .unwrap_or(0)
    }
    walk(std::path::Path::new(path))
}

/// An absolute path for a program name, the way a shell would find one.
fn which(name: &str) -> Option<std::path::PathBuf> {
    if name.contains('/') {
        return std::fs::canonicalize(name).ok();
    }
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .map(|d| std::path::Path::new(d).join(name))
        .find(|p| p.is_file())
}

/// Cumulative CPU time of a live process, in milliseconds.
///
/// `ps -o time` is centisecond-resolution and cumulative, so sampling it twice
/// and dividing by the wall clock between gives true utilisation — the same
/// reason this app's own CPU column is a delta rather than `%cpu`, which is a
/// lifetime average and would report a long-running dashboard as idle no matter
/// what it just did.
fn cpu_time_ms(pid: u32) -> Option<f64> {
    let out = Command::new("/bin/ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let parts: Vec<&str> = t.split(':').collect();
    const MINS_SECS: usize = 2;
    const HOURS_MINS_SECS: usize = 3;
    let (mins, rest) = match parts.len() {
        MINS_SECS => (parts[0].parse::<f64>().ok()?, parts[1]),
        HOURS_MINS_SECS => (
            parts[0].parse::<f64>().ok()? * MINS_PER_HOUR + parts[1].parse::<f64>().ok()?,
            parts[2],
        ),
        _ => return None,
    };
    Some((mins * SECS_PER_MIN + rest.parse::<f64>().ok()?) * MS_PER_SEC)
}

fn rss_kb(pid: u32) -> Option<f64> {
    let out = Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Run a dashboard under a real pty for `secs` and report what it cost while
/// simply sitting there — which is the whole point of the app.
struct Steady {
    rss_kb: f64,
    cpu_percent_of_core: f64,
}

/// The app running under a real pty, with its output already being drained.
///
/// The frames have to be drained or the pty buffer fills and the app blocks on
/// write, which would measure the harness rather than the app.
fn spawn_under_pty(argv: &[&str]) -> Option<Box<dyn portable_pty::Child + Send + Sync>> {
    // `CommandBuilder` does no PATH lookup, so a bare "node" spawns nothing and
    // the measurement silently reports "could not measure" rather than failing.
    let program = which(argv[0])?;
    let pty = portable_pty::native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .ok()?;
    let mut cmd = CommandBuilder::new(program);
    for a in &argv[1..] {
        cmd.arg(a);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    let child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  bench: could not start {}: {e}", argv[0]);
            return None;
        }
    };

    let mut reader = pair.master.try_clone_reader().ok()?;
    std::thread::spawn(move || {
        let mut buf = [0u8; DRAIN_BUFFER_BYTES];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                return;
            }
        }
    });
    Some(child)
}

fn steady_state(argv: &[&str], secs: u64) -> Option<Steady> {
    let mut child = spawn_under_pty(argv)?;
    let pid = child.process_id()?;

    // Let startup and the first samples settle before measuring.
    std::thread::sleep(Duration::from_secs(SETTLE_SECS));
    let t0 = Instant::now();
    let cpu0 = cpu_time_ms(pid).unwrap_or(NOTHING_MEASURED);
    let mut rss_samples = Vec::new();
    while t0.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(RSS_SAMPLE_INTERVAL_MS));
        if let Some(kb) = rss_kb(pid) {
            rss_samples.push(kb);
        }
    }
    let cpu1 = cpu_time_ms(pid).unwrap_or(cpu0);
    let elapsed = t0.elapsed().as_secs_f64();
    let _ = child.kill();
    let _ = child.wait();

    Some(Steady {
        rss_kb: median(rss_samples),
        // 100% here means one core fully busy.
        cpu_percent_of_core: (cpu1 - cpu0) / (elapsed * MS_PER_SEC) * PERCENT_SCALE,
    })
}

fn row(label: &str, node: String, rust: String, better: &str) {
    println!("  {label:<26} {node:>16}  {rust:>16}   {better}");
}

fn ratio(node: f64, rust: f64) -> String {
    if rust <= NOTHING_MEASURED || node <= NOTHING_MEASURED {
        return String::new();
    }
    if node >= rust {
        format!("{:.1}x less", node / rust)
    } else {
        format!("{:.1}x MORE", rust / node)
    }
}

/// Wall clock, peak RSS and CPU time for one full `--json` run of each.
///
/// Interleaved, alternating which goes first, so a warming cache favours
/// neither.
#[derive(Default)]
struct Samples {
    wall_ms: Vec<f64>,
    rss_kb: Vec<f64>,
    cpu_ms: Vec<f64>,
}

impl Samples {
    fn take(&mut self, program: &str, args: &[&str]) {
        if let Some(ms) = time_oneshot(program, args) {
            self.wall_ms.push(ms);
        }
        if let Some((rss, cpu)) = measure_run(program, args) {
            self.rss_kb.push(rss);
            self.cpu_ms.push(cpu);
        }
    }
}

fn report_oneshot(node_cli: &str, rust_cli: &str, runs: usize) {
    let (mut node, mut rust) = (Samples::default(), Samples::default());
    for i in 0..runs {
        if i % 2 == 0 {
            node.take("node", &[node_cli, "--json"]);
            rust.take(rust_cli, &["--json"]);
        } else {
            rust.take(rust_cli, &["--json"]);
            node.take("node", &[node_cli, "--json"]);
        }
        std::thread::sleep(Duration::from_millis(BETWEEN_RUNS_MS));
    }

    let (n_ms, r_ms) = (median(node.wall_ms), median(rust.wall_ms));
    let (n_rss, r_rss) = (median(node.rss_kb), median(rust.rss_kb));
    let (n_cpu, r_cpu) = (median(node.cpu_ms), median(rust.cpu_ms));

    println!("\n  one-shot `--json` (the whole run, start to exit)");
    row(
        "wall clock",
        format!("{n_ms:.0} ms"),
        format!("{r_ms:.0} ms"),
        &ratio(n_ms, r_ms),
    );
    row(
        "peak RSS",
        format!("{n_rss:.0} KB"),
        format!("{r_rss:.0} KB"),
        &ratio(n_rss, r_rss),
    );
    row(
        "CPU time",
        format!("{n_cpu:.0} ms"),
        format!("{r_cpu:.0} ms"),
        &ratio(n_cpu, r_cpu),
    );
}

/// Pure startup: no sampling, no priming sleep — just how long the runtime
/// takes to get to the first byte.
fn report_startup(node_cli: &str, rust_cli: &str, runs: usize) {
    let (mut node_ms, mut rust_ms) = (Vec::new(), Vec::new());
    for i in 0..runs {
        if i % 2 == 0 {
            node_ms.extend(time_oneshot("node", &[node_cli, "--version"]));
            rust_ms.extend(time_oneshot(rust_cli, &["--version"]));
        } else {
            rust_ms.extend(time_oneshot(rust_cli, &["--version"]));
            node_ms.extend(time_oneshot("node", &[node_cli, "--version"]));
        }
    }
    let (n_v, r_v) = (median(node_ms), median(rust_ms));
    println!("\n  startup only (`--version`, no sampling)");
    row(
        "wall clock",
        format!("{n_v:.0} ms"),
        format!("{r_v:.0} ms"),
        &ratio(n_v, r_v),
    );
}

/// The case the app is actually for: left open in a pane.
///
/// Absolute paths: a pty child does not necessarily inherit this process's
/// working directory, so a relative script path resolves to nothing and the
/// measurement quietly reports zero.
fn report_idle(node_cli: &str, rust_cli: &str) -> bool {
    println!("\n  dashboard, idle in a pane (60s at the 10s default tier)");
    let node_abs = std::fs::canonicalize(node_cli).ok();
    let rust_abs = std::fs::canonicalize(rust_cli).ok();
    let (Some(node_abs), Some(rust_abs)) = (node_abs, rust_abs) else {
        eprintln!("bench: could not resolve the binaries");
        return false;
    };
    let node_abs = node_abs.to_string_lossy().to_string();
    let rust_abs = rust_abs.to_string_lossy().to_string();

    let node_steady = steady_state(&["node", &node_abs, "--mock"], STEADY_STATE_SECS);
    let rust_steady = steady_state(&[&rust_abs, "--mock"], STEADY_STATE_SECS);
    let (Some(node), Some(rust)) = (&node_steady, &rust_steady) else {
        println!("  (could not measure the dashboard on this machine)");
        return true;
    };
    row(
        "resident memory",
        format!("{:.0} KB", node.rss_kb),
        format!("{:.0} KB", rust.rss_kb),
        &ratio(node.rss_kb, rust.rss_kb),
    );
    row(
        "CPU (of one core)",
        format!("{:.2} %", node.cpu_percent_of_core),
        format!("{:.2} %", rust.cpu_percent_of_core),
        &ratio(node.cpu_percent_of_core, rust.cpu_percent_of_core),
    );
    true
}

/// Files under a path, counting a directory's contents rather than itself.
fn file_count(path: &str) -> usize {
    fn walk(entry: &std::path::Path) -> usize {
        let Ok(meta) = std::fs::symlink_metadata(entry) else {
            return 0;
        };
        if !meta.is_dir() {
            return 1;
        }
        std::fs::read_dir(entry)
            .map(|it| it.flatten().map(|e| walk(&e.path())).sum())
            .unwrap_or(0)
    }
    walk(std::path::Path::new(path))
}

/// What a user downloads, and what they need installed to run it.
fn report_install_cost(rust_cli: &str) {
    println!("\n  what an install costs");
    let node_install = dir_size("node_modules") + dir_size("dist");
    let rust_install = std::fs::metadata(rust_cli).map(|m| m.len()).unwrap_or(0);
    row(
        "shipped size",
        format!("{:.1} MB", node_install as f64 / 1e6),
        format!("{:.1} MB", rust_install as f64 / 1e6),
        &ratio(node_install as f64, rust_install as f64),
    );
    println!(
        "  {:<26} {:>16}  {:>16}",
        "runtime needed", "node >= 22", "none"
    );
    row(
        "files installed",
        file_count("node_modules").to_string(),
        "1".to_string(),
        "",
    );
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let runs = args
        .iter()
        .position(|a| a == "--runs")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_RUNS);

    let node_cli = "dist/cli.js";
    let rust_cli = "target/release/sysmon";
    for p in [node_cli, rust_cli] {
        if !std::path::Path::new(p).exists() {
            eprintln!("bench: {p} is missing — run `npm run build` and a release build first");
            return std::process::ExitCode::from(1);
        }
    }

    println!("interleaved, {runs} runs each, median reported\n");
    println!("  {:<26} {:>16}  {:>16}", "", "node", "rust");

    report_oneshot(node_cli, rust_cli, runs);
    report_startup(node_cli, rust_cli, runs);

    if !report_idle(node_cli, rust_cli) {
        return std::process::ExitCode::from(1);
    }

    report_install_cost(rust_cli);

    println!(
        "\n  Notes: the Node figures include its own startup, which is most of the\n  \
         wall clock and cannot be separated from it — that cost is real and paid\n  \
         on every invocation. `node_modules` is the production tree as installed\n  \
         here, so it is an upper bound on what a user downloads."
    );
    std::process::ExitCode::SUCCESS
}
