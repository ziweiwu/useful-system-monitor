//! Live macOS collectors.
//!
//! Each one is shaped by its cost. The tier notes in the plan carry the
//! numbers; the short version is that a render costs far more than any
//! collector, and the two `ps` calls differ by 2x, so names are cached and the
//! expensive call is made only when a *visible* process has no name yet.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use sysmon_core::domain::deltas::{core_utilisation, summarise, CpuDeltaTracker, CpuTimes};
use sysmon_core::domain::scoring::energy_proxy;
use sysmon_core::domain::types::{
    BatteryData, CpuData, DiskData, HostInfo, MemoryData, ProcessMeta, ProcessSample, RawProcess,
    StartTime,
};
use sysmon_core::domain::working_set::{select_working_set, WorkingSet, WorkingSetCap};
use sysmon_core::kill::guards::is_protected_name;
use sysmon_core::parse::battery::{parse_ioreg_battery, parse_pmset};
use sysmon_core::parse::df::{parse_df, parse_df_all};
use sysmon_core::parse::ps::{parse_lstart, parse_ps_hot, parse_ps_static};
use sysmon_core::parse::top_power::parse_top_power;
use sysmon_core::parse::vm_stat::parse_memory;
use sysmon_core::text::sanitize_text;

use crate::collect::cpu;
use crate::collect::exec::{bin, run, CommandError, DEFAULT_TIMEOUT};
use crate::collect::{Identity, ProcessesData};

#[allow(dead_code)]
pub struct DarwinCollector {
    accurate_energy: bool,
    ncpu: usize,
    deltas: CpuDeltaTracker,
    prev_cpu_times: Vec<CpuTimes>,
    /// Static metadata is expensive (it doubles the cost of `ps`: 22 ms -> 44 ms,
    /// because it resolves executable paths and maps UIDs to names) but never
    /// changes for a live PID, so it is cached and only refetched when a
    /// *visible* process has no name.
    meta_cache: HashMap<i32, ProcessMeta>,
    energy_impact: HashMap<i32, f64>,
    energy_sampled_at: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn sysctl(key: &str) -> Result<String, CommandError> {
    run(bin::SYSCTL, &["-n", key], DEFAULT_TIMEOUT).map(|s| s.trim().to_string())
}

#[allow(dead_code)]
impl DarwinCollector {
    pub fn new(accurate_energy: bool) -> Result<Self, String> {
        let prev = cpu::per_core_times()?;
        let ncpu = prev.len().max(1);
        Ok(Self {
            accurate_energy,
            ncpu,
            deltas: CpuDeltaTracker::new(FULL_SCALE_PCT * ncpu as f64),
            prev_cpu_times: prev,
            meta_cache: HashMap::new(),
            energy_impact: HashMap::new(),
            energy_sampled_at: 0,
        })
    }

    pub fn host(&self) -> HostInfo {
        let cores = self.ncpu;
        // Apple Silicon reports performance cores first; Intel has no perflevel
        // keys, and treating everything as a performance core is correct there.
        let perf_cores = sysctl("hw.perflevel0.logicalcpu")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0 && *n <= cores)
            .unwrap_or(cores);
        let model = sysctl("machdep.cpu.brand_string").unwrap_or_else(|_| "unknown".into());
        let total_mem_bytes = sysctl("hw.memsize")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        HostInfo {
            model: if model.is_empty() {
                "unknown".into()
            } else {
                model
            },
            cores,
            perf_cores,
            eff_cores: cores - perf_cores,
            total_mem_bytes,
            uptime_sec: uptime_sec(),
        }
    }

    /// Free: no spawn, so this can run every second. See I-6.
    pub fn cpu(&mut self) -> Result<CpuData, String> {
        let cur = cpu::per_core_times()?;
        let per_core = core_utilisation(&self.prev_cpu_times, &cur);

        let summary = summarise(&self.prev_cpu_times, &cur, &per_core);
        self.prev_cpu_times = cur;

        Ok(CpuData {
            per_core,
            system: summary.system,
            user_percent: summary.user_percent,
            sys_percent: summary.sys_percent,
            load_avg: cpu::load_average(),
        })
    }

    pub fn memory(&self) -> Result<MemoryData, CommandError> {
        let vm = run(bin::VM_STAT, &[], DEFAULT_TIMEOUT)?;
        let swap = sysctl("vm.swapusage")?;
        let total = sysctl("hw.memsize")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(parse_memory(&vm, &swap, total))
    }

    pub fn disk(&self) -> Result<DiskData, String> {
        let out = run(bin::DF, &["-k"], DEFAULT_TIMEOUT).map_err(|e| e.to_string())?;
        let root = parse_df(&out, "/")
            .ok_or_else(|| "no filesystem mounted at / in df output".to_string())?;
        // One df call feeds both the root card and the per-volume view.
        Ok(DiskData {
            mount: root.mount,
            total_bytes: root.total_bytes,
            used_bytes: root.used_bytes,
            free_bytes: root.free_bytes,
            volumes: parse_df_all(&out),
        })
    }

    pub fn battery(&self) -> Result<BatteryData, CommandError> {
        let pmset_out = run(bin::PMSET, &["-g", "batt"], DEFAULT_TIMEOUT)?;
        let ioreg_out = run(
            bin::IOREG,
            &["-rn", "AppleSmartBattery", "-w0"],
            DEFAULT_TIMEOUT,
        )
        .unwrap_or_default();

        let Some(base) = parse_pmset(&pmset_out) else {
            /* No battery is a normal state, not an error: every desktop Mac and
            every CI runner reports none. Erroring here would degrade the
            panel on a machine that is working perfectly. */
            return Ok(BatteryData {
                present: false,
                percent: 0.0,
                charging: false,
                on_ac_power: true,
                time_remaining_min: None,
                watts: None,
                cycle_count: None,
                health_percent: None,
                temperature_c: None,
            });
        };
        let detail = parse_ioreg_battery(&ioreg_out);
        Ok(BatteryData {
            present: base.present,
            percent: base.percent,
            charging: detail.charging.unwrap_or(base.charging),
            on_ac_power: base.on_ac_power,
            time_remaining_min: base.time_remaining_min,
            watts: detail.watts,
            cycle_count: detail.cycle_count,
            health_percent: detail.health_percent,
            temperature_c: detail.temperature_c,
        })
    }

    /// `ps -o lstart= -p PID`, read at signal time. See I-16.
    ///
    /// Deliberately its own tiny spawn rather than a lookup in the last sample:
    /// the whole point is that it is newer than the sample. ~15 ms, paid once,
    /// on an action the user is being asked to confirm anyway.
    pub fn identity(&self, pid: i32) -> Result<Identity, CommandError> {
        let out = match run(
            bin::PS,
            &["-o", "lstart=", "-p", &pid.to_string()],
            DEFAULT_TIMEOUT,
        ) {
            Ok(out) => out,
            Err(e) => {
                /* `ps -p` exits 1 when no process matches. That is an answer,
                and it means the process is gone. Anything else — the binary
                missing, or the timeout that is reachable on a loaded machine
                — is *no* answer, and must not be spelled the same way. It
                used to be: every failure returned "gone", so the panel told
                the user a live process had already exited. See I-16. */
                return if e.exit_code.is_some() {
                    Ok(Identity::Gone)
                } else {
                    Err(e)
                };
            }
        };
        if out.trim().is_empty() {
            return Ok(Identity::Gone);
        }
        Ok(Identity::Known(parse_lstart(&out)))
    }

    pub fn command_line(&self, pid: i32) -> Option<String> {
        let out = run(
            bin::PS,
            &["-o", "command=", "-p", &pid.to_string()],
            DEFAULT_TIMEOUT,
        )
        .ok()?;
        // Full argv, chosen by the process itself — the least trustworthy string
        // this app displays.
        let s = sanitize_text(out.trim()).into_owned();
        (!s.is_empty()).then_some(s)
    }

    /// Refreshes the measured-energy lane. Never awaited by the hot path.
    fn refresh_energy(&mut self) {
        if !self.accurate_energy {
            return;
        }
        // -l 2 is mandatory: a single sample reports 0.0 for everything.
        let Ok(out) = run(
            bin::TOP,
            &["-l", "2", "-o", "power", "-n", "60", "-stats", "pid,power"],
            TOP_TIMEOUT,
        ) else {
            return;
        };
        self.energy_impact = parse_top_power(&out);
        self.energy_sampled_at = now_ms();
    }

    pub fn processes(&mut self, cap: WorkingSetCap) -> Result<ProcessesData, CommandError> {
        // Hot columns only: 22 ms rather than 48 ms.
        let hot = parse_ps_hot(&run(
            bin::PS,
            &["-Ao", "pid,ppid,time,rss"],
            DEFAULT_TIMEOUT,
        )?);
        let now = now_ms();
        if self.accurate_energy && self.energy_sampled_at == 0 {
            self.refresh_energy();
        }

        // I-4b: the tracker sees every PID, so a process entering the working
        // set already has a real CPU% instead of showing "—" for an interval.
        let cpu_by_pid = self.deltas.update(&hot, now);
        self.evict_recycled();

        let lane = self.energy_lane();

        let tick = Tick {
            hot: &hot,
            cpu_by_pid: &cpu_by_pid,
            lane,
        };
        let (all, ws) = self.rows(&tick, cap)?;

        Ok(ProcessesData {
            total: all.len(),
            visible: ws.visible,
            others: ws.others,
            // Built from every row, so the ancestor guard can walk out of the
            // working set. See I-13.
            parents: hot.iter().map(|p| (p.pid, p.ppid)).collect(),
            energy_accurate: self.accurate_energy && lane == EnergyLane::Measured,
        })
    }

    /// Every row, and the working set chosen from it.
    ///
    /// The second `ps` is only worth its 26 ms when a visible row has no name,
    /// so the rows are built once optimistically and only rebuilt if that
    /// turns out to be true.
    fn rows(&mut self, tick: &Tick, cap: WorkingSetCap) -> Result<Rows, CommandError> {
        let all = self.build_all(tick.hot, tick.cpu_by_pid, tick.lane);
        let ws = select_working_set(&all, cap);
        if !ws
            .visible
            .iter()
            .any(|p| !self.meta_cache.contains_key(&p.pid))
        {
            return Ok((all, ws));
        }
        self.rebuild_meta_cache(tick.hot)?;
        let all = self.build_all(tick.hot, tick.cpu_by_pid, tick.lane);
        let ws = select_working_set(&all, cap);
        Ok((all, ws))
    }

    /// A PID the tracker just flagged as recycled (I-3) is a different program
    /// now, so its cached name, user and protected flag are wrong. A "do I have
    /// this pid" check is not an identity check.
    fn evict_recycled(&mut self) {
        let recycled: Vec<i32> = self.deltas.recycled().iter().copied().collect();
        for pid in recycled {
            self.meta_cache.remove(&pid);
        }
    }

    /// Which unit the energy column can carry this tick.
    fn energy_lane(&self) -> EnergyLane {
        if self.accurate_energy && !self.energy_impact.is_empty() {
            EnergyLane::Measured
        } else {
            EnergyLane::Estimated
        }
    }

    /// Names, users and start times for every live PID, from the slower `ps`.
    fn rebuild_meta_cache(&mut self, hot: &[RawProcess]) -> Result<(), CommandError> {
        let metas = parse_ps_static(&run(
            bin::PS,
            &["-Ao", "pid,lstart,user,state,comm"],
            DEFAULT_TIMEOUT,
        )?);
        let live: HashSet<i32> = hot.iter().map(|p| p.pid).collect();
        // I-10: rebuilding from the live set also evicts exited PIDs, so the
        // cache cannot grow without bound.
        self.meta_cache = metas
            .into_iter()
            .filter(|m| live.contains(&m.pid))
            .map(|m| (m.pid, m))
            .collect();
        Ok(())
    }

    /// The energy figure for one row.
    ///
    /// A measured number belongs to the process that was running when `top`
    /// sampled it. `top` reports a bare PID, so bind it the way everything else
    /// binds identity: a process that started *after* the sample cannot be the
    /// one it measured, so it gets the estimate instead. Same reasoning as I-16
    /// on the kill path.
    ///
    /// One unit per column, or an honest gap — never a silent swap. `top -o
    /// power -n 60` ranks at most 60 PIDs, so while the measured lane is live a
    /// row it does not cover reports nothing and renders "—"; that is I-1's rule
    /// applied to the column I-1's sibling governs. When the lane is off, the
    /// whole column reverts to the estimate together.
    fn energy_for(
        &self,
        row: &RawProcess,
        cpu_percent: Option<f64>,
        lane: EnergyLane,
    ) -> Option<f64> {
        match lane {
            EnergyLane::Estimated => energy_proxy(cpu_percent),
            EnergyLane::Measured => self
                .meta_cache
                .get(&row.pid)
                .and_then(|m| m.start_time.epoch_ms())
                .filter(|start| *start <= self.energy_sampled_at)
                .and_then(|_| self.energy_impact.get(&row.pid).copied()),
        }
    }

    fn build_all(
        &self,
        hot: &[RawProcess],
        cpu_by_pid: &HashMap<i32, Option<f64>>,
        lane: EnergyLane,
    ) -> Vec<ProcessSample> {
        hot.iter()
            .map(|p| {
                let meta = self.meta_cache.get(&p.pid);
                let command = meta.map_or_else(|| format!("pid {}", p.pid), |m| m.command.clone());
                let cpu_percent = cpu_by_pid.get(&p.pid).copied().flatten();
                ProcessSample {
                    pid: p.pid,
                    ppid: p.ppid,
                    start_time: meta.map_or(StartTime::Unreadable, |m| m.start_time),
                    user: meta.map_or_else(|| "?".to_string(), |m| m.user.clone()),
                    state: meta.map_or_else(|| "?".to_string(), |m| m.state.clone()),
                    cpu_percent,
                    rss_bytes: p.rss_bytes,
                    energy: self.energy_for(p, cpu_percent, lane),
                    // Unnamed processes are treated as protected: refusing to
                    // kill something we cannot identify is the safe default.
                    // See I-14.
                    protected: meta.is_none_or(|_| is_protected_name(&command)),
                    command,
                }
            })
            .collect()
    }
}

/// Every row this sample produced, and the working set chosen from it.
type Rows = (Vec<ProcessSample>, WorkingSet);

/// What one sampling tick has gathered so far, before the rows are built.
struct Tick<'a> {
    hot: &'a [RawProcess],
    cpu_by_pid: &'a HashMap<i32, Option<f64>>,
    lane: EnergyLane,
}

/// Which unit the energy column is carrying this tick.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EnergyLane {
    /// `top`'s Energy Impact, for the PIDs it ranked.
    Measured,
    /// The CPU-time proxy, for every row.
    Estimated,
}

/// One core fully busy.
const FULL_SCALE_PCT: f64 = 100.0;
/// `top -l 2` has to take two samples before it reports anything, so it needs
/// far longer than the other collectors.
const TOP_TIMEOUT: Duration = Duration::from_secs(15);

/// Seconds since boot, from `kern.boottime`.
#[allow(dead_code)]
fn uptime_sec() -> u64 {
    // "{ sec = 1754079004, usec = 123456 } Fri Aug  1 ..."
    let Ok(out) = sysctl("kern.boottime") else {
        return 0;
    };
    let Some(rest) = out.split("sec = ").nth(1) else {
        return 0;
    };
    let secs: i64 = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    if secs == 0 {
        return 0;
    }
    ((now_ms() / 1000) - secs).max(0) as u64
}
