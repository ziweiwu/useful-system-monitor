//! Sorting and the energy estimate.

use std::cmp::Ordering;

use crate::domain::types::ProcessSample;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortKey {
    Cpu,
    Mem,
    Energy,
}

/// Energy-impact proxy.
///
/// macOS's real "Energy Impact" is only available from `top -stats power`,
/// which costs ~1s of CPU per call — 100 ms/s even on a slow lane, which would
/// dominate this app's entire budget. CPU time is the dominant term in Apple's
/// own formula, so the proxy is derived from data already collected for free,
/// and labelled as an estimate in the UI.
///
/// Kept as a named function so `--energy=accurate` can swap in real numbers
/// without touching the table, the sort or the battery view.
pub fn energy_proxy(cpu_percent: Option<f64>) -> Option<f64> {
    cpu_percent
}

/// Sorts below every real reading, so I-1's "never measured" rows sink rather
/// than tying with a genuine zero.
const NEVER_MEASURED: f64 = -1.0;
/// No energy was attributed at all, so there is nothing to divide.
const NO_ENERGY: f64 = 0.0;
/// Estimated watts attributable to a process, given the current total draw.
pub fn estimate_watts(
    energy: Option<f64>,
    total_energy: f64,
    total_watts: Option<f64>,
) -> Option<f64> {
    match (energy, total_watts) {
        (Some(share), Some(watts)) if total_energy > NO_ENERGY => {
            Some((share / total_energy) * watts.abs())
        }
        _ => None,
    }
}

/// Missing values sort last, not as zero — a process whose CPU% is not yet
/// known must not outrank one measured at 0%.
fn key_value(sample: &ProcessSample, key: SortKey) -> f64 {
    match key {
        SortKey::Cpu => sample.cpu_percent.unwrap_or(NEVER_MEASURED),
        SortKey::Mem => sample.rss_bytes as f64,
        SortKey::Energy => sample.energy.unwrap_or(NEVER_MEASURED),
    }
}

/// Total order, descending by `key`, ties broken by ascending PID.
///
/// Totality matters: with only the metric as key, two processes at an identical
/// value can swap places between frames and the table visibly jitters. See I-20.
pub fn compare(left: &ProcessSample, right: &ProcessSample, key: SortKey) -> Ordering {
    key_value(right, key)
        .partial_cmp(&key_value(left, key))
        .unwrap_or(Ordering::Equal)
        .then_with(|| left.pid.cmp(&right.pid))
}

pub fn sort_processes(procs: &[ProcessSample], key: SortKey) -> Vec<ProcessSample> {
    let mut out = procs.to_vec();
    out.sort_by(|a, b| compare(a, b, key));
    out
}
