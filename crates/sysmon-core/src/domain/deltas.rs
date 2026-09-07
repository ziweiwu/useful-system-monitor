//! Turning cumulative CPU-time counters into instantaneous CPU%.
//!
//! `ps` reports lifetime-average %CPU, which cannot answer "what is hogging the
//! CPU right now". Its TIME column has centisecond resolution, so sampling
//! twice and dividing by the wall-clock gap gives true instantaneous
//! utilisation.
//!
//! Invariants: I-1 (always a delta, `None` on first sight), I-3 (monotonicity /
//! PID-reuse detection), I-4b (tracks every PID, not just the working set).

use std::collections::{HashMap, HashSet};

use crate::domain::types::RawProcess;

/// A core, or a machine, that did no work in the sampling window.
const IDLE: f64 = 0.0;
/// One core fully busy.
const FULL_SCALE_PCT: f64 = 100.0;
/// The same, as a 0..1 fraction.
const FULLY_BUSY: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
struct Prev {
    cpu_time_ms: u64,
    at: i64,
}

/// Cumulative CPU time to a percentage, per PID.
#[derive(Debug)]
pub struct CpuDeltaTracker {
    prev: HashMap<i32, Prev>,
    /// PIDs whose cumulative counter went *backwards* on the last update.
    ///
    /// That is the signature of PID reuse (I-3), and it is different from a PID
    /// simply being seen for the first time — both yield no CPU%, but only this
    /// one means "the thing behind this number is a different program now".
    /// Exposed because anything else caching per-PID has to hear about it: a
    /// name cache keyed on "do I have this pid" will happily keep showing the
    /// dead process's name, which is what it used to do.
    recycled: HashSet<i32>,
    max_percent: f64,
}

impl CpuDeltaTracker {
    /// `max_percent` is the physical ceiling: 100 × the number of cores.
    pub fn new(max_percent: f64) -> Self {
        Self {
            prev: HashMap::new(),
            recycled: HashSet::new(),
            max_percent,
        }
    }

    /// PIDs the last [`Self::update`] flagged as recycled.
    pub fn recycled(&self) -> &HashSet<i32> {
        &self.recycled
    }

    /// Number of tracked PIDs. Exposed for the memory-bound test (I-10).
    pub fn tracked_count(&self) -> usize {
        self.prev.len()
    }

    pub fn reset(&mut self) {
        self.prev.clear();
        self.recycled.clear();
    }

    /// `now_ms` is wall-clock for this sample.
    ///
    /// Returns pid -> CPU%, with `None` where there is no usable previous
    /// sample. Never `Some(0.0)` for a first observation — see I-1.
    pub fn update(&mut self, procs: &[RawProcess], now_ms: i64) -> HashMap<i32, Option<f64>> {
        let mut out = HashMap::with_capacity(procs.len());
        let mut next = HashMap::with_capacity(procs.len());
        self.recycled.clear();

        for p in procs {
            let prev = self.prev.get(&p.pid).copied();
            next.insert(
                p.pid,
                Prev {
                    cpu_time_ms: p.cpu_time_ms,
                    at: now_ms,
                },
            );
            out.insert(p.pid, self.percent_since(prev, p, now_ms));
        }

        // I-4b/I-10: drop entries for processes that have exited, so the map
        // tracks live PIDs only and cannot grow without bound.
        self.prev = next;
        out
    }

    /// CPU% for one process against its previous sample, or `None` when there
    /// is no usable one — a first sighting, a zero-length window, or a counter
    /// that went backwards.
    fn percent_since(
        &mut self,
        prev: Option<Prev>,
        sample: &RawProcess,
        now_ms: i64,
    ) -> Option<f64> {
        // I-1: first observation yields nothing, never zero.
        let prev = prev?;

        let dt = now_ms - prev.at;
        if dt <= 0 {
            return None;
        }

        let Some(d_cpu) = sample.cpu_time_ms.checked_sub(prev.cpu_time_ms) else {
            // I-3: cumulative CPU time went backwards, so this PID was
            // recycled. Discard the delta and treat it as unseen.
            self.recycled.insert(sample.pid);
            return None;
        };

        // I-2: clamp to the physical ceiling; scheduler jitter can otherwise
        // produce a hair over 100% per core across a short window.
        Some(((d_cpu as f64 / dt as f64) * FULL_SCALE_PCT).min(self.max_percent))
    }
}

/// One core's cumulative time counters.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CpuTimes {
    pub user: f64,
    pub nice: f64,
    pub sys: f64,
    pub idle: f64,
    pub irq: f64,
}

/// The whole-machine CPU figures both collectors report.
pub struct CpuSummary {
    /// The mean across cores (I-6).
    pub system: f64,
    pub user_percent: f64,
    pub sys_percent: f64,
}

/// The user/system split and the machine-wide mean, from two samples of the
/// per-core counters.
///
/// Shared by both platform collectors: the arithmetic is identical on each, and
/// keeping two copies is how the two drift apart on a number the `--json`
/// differential harness compares directly.
pub fn summarise(prev: &[CpuTimes], cur: &[CpuTimes], per_core: &[f64]) -> CpuSummary {
    let (mut d_user, mut d_sys, mut d_total) = (IDLE, IDLE, IDLE);
    for (i, b) in cur.iter().enumerate() {
        let Some(a) = prev.get(i) else {
            continue;
        };
        d_user += (b.user - a.user) + (b.nice - a.nice);
        d_sys += b.sys - a.sys;
        d_total += (b.user - a.user)
            + (b.nice - a.nice)
            + (b.sys - a.sys)
            + (b.idle - a.idle)
            + (b.irq - a.irq);
    }
    // I-6: the mean across cores, never a sum of process rows — kernel_task
    // (PID 0) is invisible to `ps`.
    let system = if per_core.is_empty() {
        IDLE
    } else {
        per_core.iter().sum::<f64>() / per_core.len() as f64
    };
    let share = |part: f64| {
        if d_total > IDLE {
            (part / d_total) * FULL_SCALE_PCT
        } else {
            IDLE
        }
    };
    CpuSummary {
        system,
        user_percent: share(d_user),
        sys_percent: share(d_sys),
    }
}

/// Per-core utilisation from two samples.
///
/// System CPU is the mean across cores and never a sum of process rows (I-6),
/// because kernel_task (PID 0) is invisible to `ps`.
pub fn core_utilisation(prev: &[CpuTimes], cur: &[CpuTimes]) -> Vec<f64> {
    let mut out = Vec::with_capacity(cur.len());
    for (i, b) in cur.iter().enumerate() {
        let Some(a) = prev.get(i) else {
            out.push(IDLE);
            continue;
        };
        let d_idle = b.idle - a.idle;
        let d_total =
            (b.user - a.user) + (b.nice - a.nice) + (b.sys - a.sys) + d_idle + (b.irq - a.irq);
        if d_total <= IDLE {
            out.push(IDLE);
            continue;
        }
        // I-2: clamp into [0, 100].
        let busy = FULLY_BUSY - d_idle / d_total;
        out.push((busy * FULL_SCALE_PCT).clamp(IDLE, FULL_SCALE_PCT));
    }
    out
}
