//! Everything the renderer reads, in one place.

use std::collections::HashMap;

use sysmon_core::domain::ring::Ring;
use sysmon_core::domain::types::{BatteryData, CpuData, DiskData, HostInfo, MemoryData, Panel};

use crate::collect::ProcessesData;

/// How many samples each global history keeps. Fixed capacity, so memory never
/// grows with uptime. See I-10.
pub const HISTORY_LEN: usize = 60;
/// Per-process history is shorter: it is only ever drawn in the detail panel's
/// three sparklines.
pub const PROC_HISTORY_LEN: usize = 40;

pub struct Histories {
    pub cpu: Ring,
    pub memory: Ring,
    pub disk: Ring,
    pub battery: Ring,
}

/// A history point for a row that reported no reading. See I-1.
const NO_READING: f64 = 0.0;

impl Default for Histories {
    fn default() -> Self {
        Self {
            cpu: Ring::new(HISTORY_LEN),
            memory: Ring::new(HISTORY_LEN),
            disk: Ring::new(HISTORY_LEN),
            battery: Ring::new(HISTORY_LEN),
        }
    }
}

/// One process's history.
///
/// `start_time` is not decoration. A PID on its own is not an identity: recycle
/// it while the process is still inside the working set and the new one
/// inherits the dead one's sparklines, so the detail panel draws another
/// program's history under this name. See I-3.
pub struct ProcHistory {
    pub cpu: Ring,
    pub mem: Ring,
    pub energy: Ring,
    pub start_time: sysmon_core::domain::types::StartTime,
}

impl ProcHistory {
    fn new(start_time: sysmon_core::domain::types::StartTime) -> Self {
        Self {
            cpu: Ring::new(PROC_HISTORY_LEN),
            mem: Ring::new(PROC_HISTORY_LEN),
            energy: Ring::new(PROC_HISTORY_LEN),
            start_time,
        }
    }
}

#[derive(Default)]
pub struct AppData {
    pub host: Option<HostInfo>,
    pub cpu: Panel<CpuData>,
    pub memory: Panel<MemoryData>,
    pub disk: Panel<DiskData>,
    pub battery: Panel<BatteryData>,
    pub processes: Panel<ProcessesData>,
    pub histories: Histories,
    /// Kept only for the working set, and evicted the moment a process leaves
    /// it, so memory is bounded by (working set × history length) rather than
    /// growing with the ~800 live processes or with uptime. See I-10.
    pub proc_history: HashMap<i32, ProcHistory>,
    /// The detail panel's argv line, fetched on demand.
    pub command_line: Option<String>,
    /// Whether the numbers are scripted.
    ///
    /// Said on screen, not just in the flag: a dashboard showing invented
    /// numbers that looks exactly like one showing real ones is a trap, and
    /// screenshots outlive the terminal they were taken in.
    pub mock: bool,
}

impl AppData {
    /// Record one process sample into the per-process rings, and evict anything
    /// that has left the working set.
    pub fn record_processes(&mut self) {
        let Some(procs) = self.processes.sample() else {
            return;
        };
        let mut live: Vec<i32> = Vec::with_capacity(procs.visible.len());
        for p in &procs.visible {
            live.push(p.pid);
            let entry = self.proc_history.entry(p.pid);
            let h = entry.or_insert_with(|| ProcHistory::new(p.start_time));
            // A recycled PID is a different program, so its rings start over.
            if !h.start_time.same_process_as(p.start_time) {
                *h = ProcHistory::new(p.start_time);
            }
            h.cpu.push(p.cpu_percent.unwrap_or(NO_READING));
            h.mem.push(p.rss_bytes as f64);
            h.energy.push(p.energy.unwrap_or(NO_READING));
        }
        self.proc_history.retain(|pid, _| live.contains(pid));
    }
}
