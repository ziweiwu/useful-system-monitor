//! Live macOS collectors.
//!
//! Each one is shaped by its cost. The tier notes in the plan carry the
//! numbers; the short version is that a render costs far more than any
//! collector, and that `ps` costs per process listed, so it lists only the
//! processes libproc cannot read (`proc_table`).

use std::collections::{BTreeSet, HashMap, HashSet};
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
use crate::collect::proc_table;
use crate::collect::vm;
use crate::collect::{Identity, ProcessesData};

#[allow(dead_code)]
pub struct DarwinCollector {
    accurate_energy: bool,
    ncpu: usize,
    deltas: CpuDeltaTracker,
    prev_cpu_times: Vec<CpuTimes>,
    /// Static metadata, re-read for every PID only when a *visible* process
    /// has no name: it costs twice the hot columns (`ps`: 22 ms -> 44 ms,
    /// because it resolves executable paths and maps UIDs to names).
    meta_cache: HashMap<i32, ProcessMeta>,
    /// libproc's view of the process table; `None` if it could not be set up.
    table: Option<proc_table::Reader>,
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
            table: proc_table::Reader::new(),
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

    /// Read in-process (`vm`); the three commands only if that fails.
    pub fn memory(&self) -> Result<MemoryData, CommandError> {
        if let Some(memory) = vm::read() {
            return Ok(memory);
        }
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
        let (hot, split) = self.sample_table()?;
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
            split: split.as_ref(),
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

    /// Hot columns for every process.
    ///
    /// libproc reads this user's processes without a spawn; `ps` reads the
    /// rest, restricted to them, and the `Split` records which were which so a
    /// name lookup can divide the table the same way. Without libproc it is the
    /// hot `ps` call over every process, as it was.
    fn sample_table(&mut self) -> Result<(Vec<RawProcess>, Option<Split>), CommandError> {
        let Some(table) = self.table.as_ref().and_then(proc_table::Reader::read) else {
            // Hot columns only: 22 ms rather than 48 ms.
            let hot = parse_ps_hot(&run(bin::PS, &["-Ao", HOT_COLUMNS], DEFAULT_TIMEOUT)?);
            return Ok((hot, None));
        };
        let proc_table::Table {
            mut hot,
            other_uids,
            own_unreadable,
        } = table;
        let split = Split {
            native: hot.iter().map(|p| p.pid).collect(),
            other_uids,
            ps_pids: own_unreadable,
        };
        let read: HashSet<i32> = split.native.iter().copied().collect();
        let out = ps_selected(HOT_COLUMNS, &split.other_uids, &split.ps_pids)?;
        hot.extend(
            parse_ps_hot(&out)
                .into_iter()
                .filter(|p| !read.contains(&p.pid)),
        );
        hot.sort_by_key(|p| p.pid);
        Ok((hot, Some(split)))
    }

    /// Every row, and the working set chosen from it.
    ///
    /// The static columns are only worth reading when a visible row has no
    /// name, so the rows are built once optimistically and only rebuilt if that
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
        self.rebuild_meta_cache(tick)?;
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

    /// Names, users, states and start times for every live PID, divided the
    /// way the sample was: libproc for this user's, `ps` for the rest and for
    /// any whose argument area libproc could not read. Without libproc, the
    /// slower `ps` over every process.
    fn rebuild_meta_cache(&mut self, tick: &Tick) -> Result<(), CommandError> {
        let metas = match (tick.split, self.table.as_mut()) {
            (Some(split), Some(reader)) => {
                let (mut metas, unread) = reader.metas(&split.native);
                let pids: Vec<i32> = split.ps_pids.iter().chain(&unread).copied().collect();
                let named: HashSet<i32> = metas.iter().map(|m| m.pid).collect();
                let out = ps_selected(STATIC_COLUMNS, &split.other_uids, &pids)?;
                metas.extend(
                    parse_ps_static(&out)
                        .into_iter()
                        .filter(|m| !named.contains(&m.pid)),
                );
                metas
            }
            _ => parse_ps_static(&run(bin::PS, &["-Ao", STATIC_COLUMNS], DEFAULT_TIMEOUT)?),
        };
        let live: HashSet<i32> = tick.hot.iter().map(|p| p.pid).collect();
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
    /// `None` when libproc is down and `ps` read every process.
    split: Option<&'a Split>,
}

/// How a sample divided the process table between libproc and `ps`.
struct Split {
    /// The PIDs libproc read.
    native: Vec<i32>,
    /// Real UIDs, other than this user's, whose processes `ps` read.
    other_uids: BTreeSet<u32>,
    /// This user's PIDs that `ps` read: setuid programs and zombies.
    ps_pids: Vec<i32>,
}

/// Which unit the energy column is carrying this tick.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EnergyLane {
    /// `top`'s Energy Impact, for the PIDs it ranked.
    Measured,
    /// The CPU-time proxy, for every row.
    Estimated,
}

/// `columns` for the processes libproc left to `ps`: other users' by real
/// UID, and this user's unreadable ones by PID — or for every process, when
/// that selection fails, for the caller to keep the ones libproc did not read.
///
/// Users go by name where `/etc/passwd` has one, read afresh on every call:
/// `ps` resolves each `-U` entry with `getpwnam` first, and a number misses
/// that lookup before `getpwuid` finds it — 13 ms of `ps` CPU against 8 ms
/// for 37 other users here, most of the call. `ps` exits 1 both when nothing
/// matched and when it was given a user that does not exist, which is why 1
/// falls back to every process rather than to an empty answer.
fn ps_selected(columns: &str, uids: &BTreeSet<u32>, pids: &[i32]) -> Result<String, CommandError> {
    let join = |items: Vec<String>| items.join(",");
    let mut args = vec!["-o".to_string(), columns.to_string()];
    if !uids.is_empty() {
        let names = passwd_names();
        let user = |uid: &u32| names.get(uid).cloned().unwrap_or_else(|| uid.to_string());
        args.push("-U".into());
        args.push(join(uids.iter().map(user).collect()));
    }
    if !pids.is_empty() {
        args.push("-p".into());
        args.push(join(pids.iter().map(i32::to_string).collect()));
    }
    if args.len() == PS_FORMAT_ARGS {
        return Ok(String::new());
    }
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(bin::PS, &argv, DEFAULT_TIMEOUT) {
        Err(e) if e.exit_code == Some(PS_NO_MATCH) => {
            run(bin::PS, &["-Ao", columns], DEFAULT_TIMEOUT)
        }
        other => other,
    }
}

/// Account names by UID, from `/etc/passwd`, which lists every system
/// account — the users whose processes libproc cannot read. Empty if it cannot
/// be read, and then `ps` gets numbers.
fn passwd_names() -> HashMap<u32, String> {
    parse_passwd(&std::fs::read_to_string(PASSWD).unwrap_or_default())
}

/// The first name for each UID in `passwd(5)` text. `nobody`'s `-2` is no
/// `u32`, so it stays out and is passed to `ps` as a number.
fn parse_passwd(passwd: &str) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for line in passwd.lines().filter(|line| !line.starts_with('#')) {
        let mut fields = line.split(':');
        let (Some(name), Some(uid)) = (fields.next(), fields.nth(1)) else {
            continue;
        };
        if let (false, Ok(uid)) = (name.is_empty(), uid.parse::<u32>()) {
            names.entry(uid).or_insert_with(|| name.to_string());
        }
    }
    names
}

/// `-o <columns>`: the arguments before any selector.
const PS_FORMAT_ARGS: usize = 2;
const PASSWD: &str = "/etc/passwd";
/// What `ps` exits with when no process matched its selectors, or a user it
/// was given does not exist.
const PS_NO_MATCH: i32 = 1;
/// Read every sample.
const HOT_COLUMNS: &str = "pid,ppid,time,rss";
/// Read only when a visible process has no name.
const STATIC_COLUMNS: &str = "pid,lstart,user,state,comm";

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

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD_FIXTURE: &str = include_str!("../../../../test/fixtures/passwd.txt");
    /// `_windowserver` in the fixture.
    const WINDOWSERVER_UID: u32 = 88;
    /// Any UID, given two names.
    const SHARED_UID: u32 = 7;

    #[test]
    fn passwd_names_skip_comments_and_negative_uids() {
        let names = parse_passwd(PASSWD_FIXTURE);
        assert_eq!(names.get(&0).map(String::as_str), Some("root"));
        assert_eq!(
            names.get(&WINDOWSERVER_UID).map(String::as_str),
            Some("_windowserver")
        );
        assert!(!names
            .values()
            .any(|name| name == "nobody" || name.starts_with('#')));
    }

    #[test]
    fn passwd_names_keep_the_first_name_for_a_uid() {
        let passwd =
            format!("first:*:{SHARED_UID}:0::/:/bin/sh\nsecond:*:{SHARED_UID}:0::/:/bin/sh\n");
        let names = parse_passwd(&passwd);
        assert_eq!(names.get(&SHARED_UID).map(String::as_str), Some("first"));
    }
}
