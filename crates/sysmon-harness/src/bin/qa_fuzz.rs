//! Seeded keyboard fuzzing against the reducer and the frame builder.
//!
//! The TypeScript fuzzer mounts a real Ink app and sends keys through a virtual
//! terminal, which costs hundreds of milliseconds per run and caps a CI sweep at
//! a few dozen seeds. Because the reducer and the frame are both pure functions
//! here, the same sweep runs entirely in memory and does tens of thousands of
//! steps a second — so the interesting axis stops being "can we afford another
//! seed" and becomes "what else should we assert".
//!
//! What it asserts, on every single step:
//!
//! - the reducer does not panic on any key at any size;
//! - **no frame ever overflows its terminal**, in rows or in cells (I-19, I-26);
//! - **no signal is ever requested against a confirmation that is not on the
//!   screen** (I-15) — checked against the frame that was actually built, not
//!   against the mode state;
//! - a kill is never requested from the same input burst that opened the
//!   confirmation, however the keys are grouped.
//!
//! Keys are sent in **unawaited bursts** of random length, because that is what
//! a paste and a fast typist look like and it is the shape that produced the
//! stale-closure bugs in the original.
//!
//! Usage: cargo run -p sysmon-harness --bin qa_fuzz -- [--seeds N] [--steps N]

use sysmon::render::{self, Size};
use sysmon::runtime::store::AppData;
use sysmon_core::app::input::{
    reconcile_mode, reconcile_selection, reduce, Effect, InputContext, Key, KeyEvent,
};
use sysmon_core::app::state::{Mode, UiState};
use sysmon_core::domain::types::{
    BatteryData, CpuData, DiskData, HostInfo, MemoryData, OthersRollup, Panel, ProcessSample,
    StartTime, VolumeUsage,
};
use sysmon_core::layout::screens::too_small;
use sysmon_core::text::width::display_width;

const NOW: i64 = 1_800_000_000_000;

// ---- the fixture's arbitrary-but-fixed shape ----
/// Spreads CPU, energy and the protected flag across the fake PIDs so every
/// column has a range of values and both branches of `protected` are drawn.
const CPU_SPREAD: i32 = 89;
const ENERGY_SPREAD: i32 = 37;
const PROTECTED_EVERY: i32 = 11;
const CORE_COUNT: usize = 10;
/// Fans the per-core figures from 0 up to near full, one step per core.
const CORE_STEP: usize = 9;
const LOAD_AVG: [f64; 3] = [4.1, 4.7, 5.2];
const MINUTES_REMAINING: i64 = 143;
const DRAW_WATTS: f64 = -12.4;
const CYCLE_COUNT: i64 = 203;
const HEALTH_PCT: i64 = 91;
const TEMPERATURE_C: f64 = 30.2;
/// Enough rows to fill any terminal the sweep resizes to.
const VISIBLE_PROCESSES: i32 = 48;
const PID_STRIDE: i32 = 7;
/// Steps the seeded history so the sparklines are not flat.
const HISTORY_STEP: usize = 3;

// ---- the sweep ----
/// One step in twelve resizes, so a run crosses many terminal shapes without
/// spending most of its steps redrawing after a resize.
const RESIZE_ODDS: usize = 12;
/// Deliberately spans the floor at which the app stops drawing.
const MIN_FUZZ_COLUMNS: usize = 30;
const FUZZ_COLUMN_SPREAD: usize = 120;
const MIN_FUZZ_ROWS: usize = 4;
const FUZZ_ROW_SPREAD: usize = 40;
/// Keys per step. More than one, so the anti-paste guard sees real batches.
const MAX_BURST: usize = 5;
const DEFAULT_SEEDS: usize = 200;
const DEFAULT_STEPS: usize = 300;

/// The same LCG the corpus generators use, so a failing seed reproduces exactly.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u32) as usize
    }
    fn pick<T: Copy>(&mut self, choices: &[T]) -> T {
        choices[self.below(choices.len())]
    }
}

/// Every key the app binds, plus a few it does not — an unbound key must be
/// inert rather than surprising.
const KEYS: &[Key] = &[
    Key::Char('q'),
    Key::Char('k'),
    Key::Char('t'),
    Key::Char('c'),
    Key::Char('m'),
    Key::Char('e'),
    Key::Char('r'),
    Key::Char('/'),
    Key::Char('+'),
    Key::Char('-'),
    Key::Char('1'),
    Key::Char('2'),
    Key::Char('3'),
    Key::Char('4'),
    Key::Char('5'),
    Key::Char('x'),
    Key::Char('中'),
    Key::Enter,
    Key::Escape,
    Key::Backspace,
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::PageUp,
    Key::PageDown,
];

fn process(pid: i32, name: &str) -> ProcessSample {
    ProcessSample {
        pid,
        ppid: 1,
        start_time: StartTime::Known(NOW - 60_000),
        command: format!("/Applications/{name}.app/Contents/MacOS/{name}"),
        user: "ziweiwu".into(),
        state: "S".into(),
        cpu_percent: Some((pid % CPU_SPREAD) as f64),
        rss_bytes: (pid as u64) * 1_048_576,
        energy: Some((pid % ENERGY_SPREAD) as f64),
        protected: pid % PROTECTED_EVERY == 0,
    }
}

fn cpu_panel() -> Panel<CpuData> {
    let cores = 0..CORE_COUNT;
    Panel::Ok {
        data: CpuData {
            per_core: cores.map(|core| (core * CORE_STEP) as f64).collect(),
            system: 42.5,
            user_percent: 30.0,
            sys_percent: 12.5,
            load_avg: LOAD_AVG,
        },
        sampled_at_ms: NOW - 3_000,
    }
}

fn memory_panel() -> Panel<MemoryData> {
    Panel::Ok {
        data: MemoryData {
            total_bytes: 17_179_869_184,
            used_bytes: 12_884_901_888,
            free_bytes: 93_732_864,
            available_bytes: 4_294_967_296,
            wired_bytes: 3_716_710_400,
            active_bytes: 3_761_422_336,
            inactive_bytes: 3_750_412_288,
            compressed_bytes: 5_406_769_152,
            swap_total_bytes: 2_147_483_648,
            swap_used_bytes: 1_610_612_736,
        },
        sampled_at_ms: NOW - 3_000,
    }
}

fn disk_panel() -> Panel<DiskData> {
    Panel::Ok {
        data: DiskData {
            mount: "/".into(),
            total_bytes: 994_662_584_320,
            used_bytes: 362_546_425_856,
            free_bytes: 632_116_158_464,
            volumes: vec![VolumeUsage {
                mount: "/".into(),
                device: "/dev/disk3s5".into(),
                total_bytes: 994_662_584_320,
                used_bytes: 362_546_425_856,
                free_bytes: 632_116_158_464,
                network: false,
            }],
        },
        sampled_at_ms: NOW - 100_000,
    }
}

fn battery_panel() -> Panel<BatteryData> {
    Panel::Ok {
        data: BatteryData {
            present: true,
            percent: 87.0,
            charging: false,
            on_ac_power: false,
            time_remaining_min: Some(MINUTES_REMAINING),
            watts: Some(DRAW_WATTS),
            cycle_count: Some(CYCLE_COUNT),
            health_percent: Some(HEALTH_PCT),
            temperature_c: Some(TEMPERATURE_C),
        },
        sampled_at_ms: NOW - 20_000,
    }
}

fn fixture() -> AppData {
    let mut d = AppData {
        host: Some(HostInfo {
            model: "Apple M1 Pro".into(),
            cores: 10,
            perf_cores: 8,
            eff_cores: 2,
            total_mem_bytes: 17_179_869_184,
            uptime_sec: 264_000,
        }),
        ..AppData::default()
    };
    d.cpu = cpu_panel();
    d.memory = memory_panel();
    d.disk = disk_panel();
    d.battery = battery_panel();
    d.processes = processes_panel();
    seed_histories(&mut d);
    d.record_processes();
    d
}

/// Names chosen to cover the width cases: ASCII, an emoji with a variation
/// selector, and CJK.
fn processes_panel() -> Panel<sysmon::collect::ProcessesData> {
    let names = ["Chrome", "Slack", "⚠️ Warn", "アプリ", "node", "Finder"];
    let indices = 1..=VISIBLE_PROCESSES;
    let visible: Vec<ProcessSample> = indices
        .map(|i| process(i * PID_STRIDE, names[(i as usize) % names.len()]))
        .collect();
    let parents = visible.iter().map(|p| (p.pid, 1)).collect();
    Panel::Ok {
        data: sysmon::collect::ProcessesData {
            total: 800,
            visible,
            others: OthersRollup {
                count: 752,
                cpu_percent: 22.5,
                rss_bytes: 9_000_000_000,
                energy: 30.0,
            },
            parents,
            energy_accurate: false,
        },
        sampled_at_ms: NOW - 3_000,
    }
}

/// Fills the rings so the sparklines are drawn rather than skipped.
fn seed_histories(app: &mut AppData) {
    for ring in [
        &mut app.histories.cpu,
        &mut app.histories.memory,
        &mut app.histories.disk,
    ] {
        for i in 0..60 {
            ring.push((i * HISTORY_STEP % 100) as f64);
        }
    }
}

struct Failure {
    seed: u32,
    step: usize,
    what: String,
}

/// What the app had told the user about the confirmation when a key landed.
struct Confirmation {
    /// The modal was on the last frame the user actually saw.
    drawn: bool,
    /// The modal was open in the state the key was applied to.
    was_open: bool,
}

/// A kill that reached the signal stage without a confirmation on screen is the
/// one thing this sweep must never see.
fn kill_without_confirmation(effects: &[Effect], seen: &Confirmation) -> Option<&'static str> {
    let signalled = effects
        .iter()
        .any(|effect| matches!(effect, Effect::RequestKill { .. }));
    if !signalled {
        return None;
    }
    if !seen.drawn {
        return Some("signalled with no confirmation on screen");
    }
    if !seen.was_open {
        return Some("signalled without the confirmation being open");
    }
    None
}

/// A frame that overflows its terminal, either down or across.
fn overflow(frame: &render::Frame, size: Size) -> Option<String> {
    if frame.lines.len() > size.rows {
        return Some(format!(
            "{} lines in {} rows at {}x{}",
            frame.lines.len(),
            size.rows,
            size.columns,
            size.rows
        ));
    }
    frame.lines.iter().enumerate().find_map(|(i, line)| {
        let cells: usize = line.spans.iter().map(|s| display_width(&s.content)).sum();
        (cells > size.columns).then(|| {
            format!(
                "line {i} is {cells} cells at {}x{}",
                size.columns, size.rows
            )
        })
    })
}

/// One seed's run, as the state it carries between steps.
struct Sweep<'a> {
    rng: Rng,
    ui: UiState,
    size: Size,
    kill_modal_drawn: bool,
    fixture: &'a AppData,
}

impl Sweep<'_> {
    /// Occasionally resize, including through the floor where the app stops
    /// drawing — the size class the original's sweep never explored.
    fn maybe_resize(&mut self) {
        if self.rng.below(RESIZE_ODDS) == 0 {
            self.size = Size {
                columns: MIN_FUZZ_COLUMNS + self.rng.below(FUZZ_COLUMN_SPREAD),
                rows: MIN_FUZZ_ROWS + self.rng.below(FUZZ_ROW_SPREAD),
            };
        }
    }

    /// Every key in one batch shares a batch id, which is exactly what the
    /// anti-paste guard reads.
    fn press_keys(&mut self, batch: u64) -> Option<&'static str> {
        let burst = 1 + self.rng.below(MAX_BURST);
        for _ in 0..burst {
            let key = self.rng.pick(KEYS);
            let rows = render::visible_rows(self.fixture, &self.ui);
            let ctx = InputContext {
                too_small: too_small(self.size.columns, self.size.rows),
                filtered: &rows,
                // The guards are exercised for real by the kill tests; here the
                // question is whether the *frame* gates the signal.
                kill_allowed: true,
                kill_modal_drawn: self.kill_modal_drawn,
                now_ms: NOW,
            };
            let seen = Confirmation {
                drawn: self.kill_modal_drawn,
                was_open: matches!(self.ui.mode, Mode::Kill(_)),
            };
            let (next, effects) = reduce(&self.ui, KeyEvent { key, batch }, &ctx);
            self.ui = next;

            if let Some(what) = kill_without_confirmation(&effects, &seen) {
                return Some(what);
            }
        }
        None
    }

    /// Settle the selection against the rows that survived, then draw.
    fn draw(&mut self) -> Option<String> {
        let rows = render::visible_rows(self.fixture, &self.ui);
        reconcile_selection(&mut self.ui, &rows);
        reconcile_mode(&mut self.ui, &rows);

        let frame = render::build(self.fixture, &self.ui, self.size, NOW);
        self.kill_modal_drawn = frame.kill_modal_drawn;
        overflow(&frame, self.size)
    }
}

fn run_seed(seed: u32, steps: usize, fixture: &AppData) -> Option<Failure> {
    let mut sweep = Sweep {
        rng: Rng(seed),
        ui: UiState::default(),
        size: Size {
            columns: 100,
            rows: 32,
        },
        kill_modal_drawn: false,
        fixture,
    };

    // One burst per step, so the step index *is* the batch id.
    for (step, batch) in (0..steps).map(|s| (s, s as u64 + 1)) {
        sweep.maybe_resize();
        if let Some(what) = sweep.press_keys(batch) {
            return Some(Failure {
                seed,
                step,
                what: what.into(),
            });
        }
        if let Some(what) = sweep.draw() {
            return Some(Failure { seed, step, what });
        }
    }
    None
}

/// `--seeds N --steps N`, or `None` if the command line held something else.
fn parse_args(args: &[String]) -> Option<(usize, usize)> {
    let mut seeds = DEFAULT_SEEDS;
    let mut steps = DEFAULT_STEPS;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seeds" => {
                seeds = args[i + 1].parse().unwrap_or(seeds);
                i += 1;
            }
            "--steps" => {
                steps = args[i + 1].parse().unwrap_or(steps);
                i += 1;
            }
            other => {
                eprintln!("qa_fuzz: unknown option {other}");
                return None;
            }
        }
        i += 1;
    }
    Some((seeds, steps))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((seeds, steps)) = parse_args(&args) else {
        return std::process::ExitCode::from(2);
    };

    let d = fixture();
    let start = std::time::Instant::now();
    let mut failures = Vec::new();
    for seed in 0..seeds as u32 {
        if let Some(f) = run_seed(seed.wrapping_mul(2_654_435_761).wrapping_add(1), steps, &d) {
            failures.push(f);
        }
    }
    let elapsed = start.elapsed();

    println!(
        "{seeds} seeds x {steps} steps = {} steps in {:.2}s ({:.0} steps/s)",
        seeds * steps,
        elapsed.as_secs_f64(),
        (seeds * steps) as f64 / elapsed.as_secs_f64()
    );
    for f in &failures {
        println!("  FAIL seed {} step {}: {}", f.seed, f.step, f.what);
    }
    println!(
        "qa-fuzz: {}",
        if failures.is_empty() { "PASS" } else { "FAIL" }
    );
    if failures.is_empty() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(1)
    }
}
