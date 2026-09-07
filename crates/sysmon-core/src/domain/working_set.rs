//! Choosing which processes are materialised for display.

use crate::domain::types::{OthersRollup, ProcessSample};

pub const WORKING_SET_SIZE: usize = 50;

/// How many processes the table materialises.
///
/// Modelled as an enum rather than a number with `Infinity` in it, so "all"
/// cannot be confused with a very large cap and every `match` has to handle it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkingSetCap {
    Top(usize),
    All,
}

/// The three widths `+` and `-` step through. Each is a row count, and the cost
/// of the tier scales with it.
const DEFAULT_ROWS: usize = 50;
const WIDER_ROWS: usize = 150;
const WIDEST_ROWS: usize = 300;
/// Sorts below every real reading, so a row that was never measured sinks
/// rather than tying with a genuine zero. See I-1.
const NEVER_MEASURED: f64 = -1.0;
/// A row with no reading contributes nothing to the roll-up total.
const NOTHING: f64 = 0.0;
/// How far `+` / `-` can expand the working set, smallest first.
///
/// The first three steps are free, and measurably so. `ps -Ao pid,ppid,time,rss`
/// already returns every process and a sample is already built for each, so a
/// wider cut spawns nothing extra; and because the table is windowed to the
/// terminal height (I-26), it renders no extra rows either. Measured median cost
/// of one `processes()` call, interleaved A/B on a loaded machine: cap 50 =
/// 27.3 ms, 150 = 25.1 ms, 300 = 26.5 ms — all within noise.
///
/// `All` is the exception, at 90.4 ms (+0.63% of one core at the 10s tier). The
/// cost is not the extra rows: it is that the *static* `ps` (which resolves
/// executable paths and maps UIDs, and costs ~2x the hot one) is refetched
/// whenever a visible process has no cached name. With every process visible,
/// any newly spawned helper triggers that refetch, so it fires on essentially
/// every tick instead of rarely. See I-9c; the UI labels the step accordingly.
pub const WORKING_SET_STEPS: [WorkingSetCap; 4] = [
    WorkingSetCap::Top(DEFAULT_ROWS),
    WorkingSetCap::Top(WIDER_ROWS),
    WorkingSetCap::Top(WIDEST_ROWS),
    WorkingSetCap::All,
];

impl WorkingSetCap {
    /// Human label for the footer and the roll-up row.
    pub fn label(self) -> String {
        match self {
            Self::Top(n) => n.to_string(),
            Self::All => "all".to_string(),
        }
    }

    /// Cost warning, or empty when the step is free.
    ///
    /// Shown for the same reason `--energy=accurate` and the "est" suffix
    /// exist: a tool whose whole claim is that it does not drain your battery
    /// has to say so when you ask it to cost more. See I-9c.
    pub fn cost_note(self) -> &'static str {
        match self {
            Self::Top(_) => "",
            Self::All => " (+0.6% cpu)",
        }
    }
}

pub struct WorkingSet {
    pub visible: Vec<ProcessSample>,
    pub others: OthersRollup,
}

/// Processes outside the cap are counted but never materialised: no history
/// ring buffer, no render work. See C-7.
///
/// The union of top-N-by-CPU and top-N-by-RSS is deliberate — a memory hog at
/// 0% CPU (a leaked Electron helper) is exactly the thing you want to find, and
/// a CPU-only cut would hide it.
///
/// Note this trims *display*, not *tracking*: the delta tracker still sees every
/// PID (I-4b), so a process entering the set already has a real CPU% rather than
/// showing "—" for a full interval.
/// The PIDs worth keeping: the heaviest by CPU and by memory, unioned.
///
/// Half the budget to each list so the union stays within `n`. Taking the top
/// `n` of both would yield up to 2n rows.
fn top_by_cpu_and_memory(all: &[ProcessSample], n: usize) -> std::collections::HashSet<i32> {
    let half = (n / 2).max(1);

    let mut by_cpu: Vec<&ProcessSample> = all.iter().collect();
    by_cpu.sort_by(|a, b| {
        b.cpu_percent
            .unwrap_or(NEVER_MEASURED)
            .partial_cmp(&a.cpu_percent.unwrap_or(NEVER_MEASURED))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut by_mem: Vec<&ProcessSample> = all.iter().collect();
    by_mem.sort_by_key(|p| std::cmp::Reverse(p.rss_bytes));

    let mut keep: std::collections::HashSet<i32> = std::collections::HashSet::new();
    for p in by_cpu.iter().take(half) {
        keep.insert(p.pid);
    }
    for p in by_mem.iter().take(half) {
        keep.insert(p.pid);
    }
    keep
}

pub fn select_working_set(all: &[ProcessSample], cap: WorkingSetCap) -> WorkingSet {
    let WorkingSetCap::Top(n) = cap else {
        return WorkingSet {
            visible: all.to_vec(),
            others: OthersRollup::default(),
        };
    };
    if all.len() <= n {
        return WorkingSet {
            visible: all.to_vec(),
            others: OthersRollup::default(),
        };
    }

    let keep = top_by_cpu_and_memory(all, n);

    let mut visible = Vec::new();
    let mut others = OthersRollup::default();
    for p in all {
        if keep.contains(&p.pid) {
            visible.push(p.clone());
        } else {
            // C-9: the tail is aggregated, so the table still reconciles with
            // the CPU and MEM cards instead of silently under-reporting.
            others.count += 1;
            others.cpu_percent += p.cpu_percent.unwrap_or(NOTHING);
            others.rss_bytes += p.rss_bytes;
            others.energy += p.energy.unwrap_or(NOTHING);
        }
    }
    WorkingSet { visible, others }
}
