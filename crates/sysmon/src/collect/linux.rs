//! Live Linux collectors.
//!
//! Where macOS shells out to seven commands, Linux reads files. That is cheaper
//! — no spawn, no locale hazard, no `strftime` — and it removes the failure
//! mode that once disabled the kill path machine-wide (I-28). It also means
//! this file shares no parser with the macOS one, so it is **new code with new
//! tests, not a port with an oracle**: the differential harness has nothing to
//! compare it against. See the note on `sysmon_core::parse::linux`.

use std::collections::{HashMap, HashSet};
use std::fs;

use sysmon_core::domain::deltas::{core_utilisation, summarise, CpuDeltaTracker, CpuTimes};
use sysmon_core::domain::scoring::energy_proxy;
use sysmon_core::domain::types::{
    BatteryData, CpuData, DiskData, HostInfo, MemoryData, ProcessMeta, ProcessSample, RawProcess,
    StartTime, VolumeUsage,
};
use sysmon_core::domain::working_set::{select_working_set, WorkingSetCap};
use sysmon_core::kill::guards::process_name;
use sysmon_core::parse::linux::{
    parse_meminfo, parse_mounts, parse_power_supply, parse_proc_pid_stat,
    parse_proc_pid_status_uid, parse_proc_stat, Mount, LINUX_PROTECTED_NAMES,
};

use crate::collect::{Identity, ProcessesData};

pub struct LinuxCollector {
    ncpu: usize,
    deltas: CpuDeltaTracker,
    prev_cpu_times: Vec<CpuTimes>,
    /// Names, owners and start times never change for a live PID, so they are
    /// cached and only refetched when a *visible* process has none. Reading
    /// `/proc/[pid]/status` for 800 processes is the expensive half here, the
    /// way the static `ps` is on macOS.
    meta_cache: HashMap<i32, ProcessMeta>,
    users: HashMap<u32, String>,
    clock_ticks: u64,
    page_size: u64,
    boot_time_ms: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `USER_HZ`, which is 100 on every mainstream kernel but is not guaranteed.
fn clock_ticks() -> u64 {
    // SAFETY: `sysconf` takes an integer name and returns a long; no pointers
    // are involved and the call has no preconditions.
    #[allow(unsafe_code)]
    let v = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if v > 0 {
        v as u64
    } else {
        100
    }
}

/// What `sysconf` could not tell us. 4 KiB is the Linux default on every
/// architecture this ships for.
const FALLBACK_PAGE_SIZE: u64 = 4096;
/// One core fully busy.
const FULL_SCALE_PCT: f64 = 100.0;
/// A figure `/proc` did not report.
const NO_READING: f64 = 0.0;
/// Sized so a desktop's process table rarely reallocates.
const TYPICAL_PROCESS_COUNT: usize = 512;
/// `/proc/loadavg` carries the 1, 5 and 15 minute figures.
const LOAD_WINDOWS: usize = 3;

fn page_size() -> u64 {
    // SAFETY: as above.
    #[allow(unsafe_code)]
    let v = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if v > 0 {
        v as u64
    } else {
        FALLBACK_PAGE_SIZE
    }
}

/// When the machine booted, from `/proc/stat`'s `btime`.
///
/// Needed because `/proc/[pid]/stat` reports a process's start as ticks since
/// boot, and I-16 binds a kill to an *instant*.
fn boot_time_ms() -> i64 {
    fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("btime "))?
                .trim()
                .parse::<i64>()
                .ok()
        })
        .map(|secs| secs * 1000)
        .unwrap_or(0)
}

/// uid -> name, read once from `/etc/passwd`.
///
/// Deliberately not `getpwuid`: that call is not thread-safe in the form that
/// returns a borrowed struct, and this runs on a collector thread. The file is
/// the same source it reads, minus the hazard — and a uid with no entry keeps
/// its number rather than becoming "?".
fn read_users() -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let Ok(text) = fs::read_to_string("/etc/passwd") else {
        return out;
    };
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() > 2 {
            if let Ok(uid) = f[2].parse::<u32>() {
                out.insert(uid, f[0].to_string());
            }
        }
    }
    out
}

impl LinuxCollector {
    pub fn new(_accurate_energy: bool) -> Result<Self, String> {
        let prev = parse_proc_stat(
            &fs::read_to_string("/proc/stat").map_err(|e| format!("/proc/stat: {e}"))?,
        );
        if prev.is_empty() {
            return Err("/proc/stat reported no cores".to_string());
        }
        let ncpu = prev.len();
        Ok(Self {
            ncpu,
            deltas: CpuDeltaTracker::new(FULL_SCALE_PCT * ncpu as f64),
            prev_cpu_times: prev,
            meta_cache: HashMap::new(),
            users: read_users(),
            clock_ticks: clock_ticks(),
            page_size: page_size(),
            boot_time_ms: boot_time_ms(),
        })
    }

    pub fn host(&self) -> HostInfo {
        let model = fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim() == "model name"))
                    .map(|(_, v)| v.trim().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());
        let uptime_sec = fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
            .unwrap_or(NO_READING) as u64;
        HostInfo {
            model,
            cores: self.ncpu,
            // Linux exposes no performance/efficiency split that is meaningful
            // across vendors, so every core is reported as one kind rather than
            // inventing a division. The strip then draws one group.
            perf_cores: self.ncpu,
            eff_cores: 0,
            total_mem_bytes: self.memory().map(|m| m.total_bytes).unwrap_or(0),
            uptime_sec,
        }
    }

    /// Free: a file read, no spawn. See I-6.
    pub fn cpu(&mut self) -> Result<CpuData, String> {
        let cur = parse_proc_stat(
            &fs::read_to_string("/proc/stat").map_err(|e| format!("/proc/stat: {e}"))?,
        );
        let per_core = core_utilisation(&self.prev_cpu_times, &cur);

        let summary = summarise(&self.prev_cpu_times, &cur, &per_core);
        self.prev_cpu_times = cur;

        Ok(CpuData {
            per_core,
            system: summary.system,
            user_percent: summary.user_percent,
            sys_percent: summary.sys_percent,
            load_avg: load_average(),
        })
    }

    pub fn memory(&self) -> Result<MemoryData, String> {
        Ok(parse_meminfo(
            &fs::read_to_string("/proc/meminfo").map_err(|e| format!("/proc/meminfo: {e}"))?,
        ))
    }

    pub fn disk(&self) -> Result<DiskData, String> {
        let mounts = parse_mounts(
            &fs::read_to_string("/proc/mounts").map_err(|e| format!("/proc/mounts: {e}"))?,
        );
        let volumes: Vec<VolumeUsage> = mounts.iter().filter_map(usage).collect();
        let root = volumes
            .iter()
            .find(|v| v.mount == "/")
            .ok_or_else(|| "no filesystem mounted at /".to_string())?;
        Ok(DiskData {
            mount: root.mount.clone(),
            total_bytes: root.total_bytes,
            used_bytes: root.used_bytes,
            free_bytes: root.free_bytes,
            volumes,
        })
    }

    pub fn battery(&self) -> Result<BatteryData, String> {
        let Ok(entries) = fs::read_dir("/sys/class/power_supply") else {
            return Ok(no_battery());
        };
        for entry in entries.flatten() {
            let Ok(text) = fs::read_to_string(entry.path().join("uevent")) else {
                continue;
            };
            if let Some(b) = parse_power_supply(&text) {
                return Ok(b);
            }
        }
        // A desktop with no battery is a fact, not an error.
        Ok(no_battery())
    }

    /// The identity as it is **right now**, re-read rather than taken from the
    /// last sample. See I-16.
    pub fn identity(&self, pid: i32) -> Result<Identity, String> {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(text) => {
                match parse_proc_pid_stat(
                    &text,
                    self.clock_ticks,
                    self.page_size,
                    self.boot_time_ms,
                ) {
                    Some((_, meta)) => Ok(Identity::Known(meta.start_time)),
                    // The file exists but could not be read back to an instant,
                    // which is not the same as the process being gone.
                    None => Err(format!("/proc/{pid}/stat was unreadable")),
                }
            }
            // ENOENT is an answer: the process is gone. Anything else — EACCES,
            // an I/O error — is *no* answer, and must not be spelled the same
            // way. Conflating them told users a live process had already exited.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Identity::Gone),
            Err(e) => Err(format!("/proc/{pid}/stat: {e}")),
        }
    }

    pub fn command_line(&self, pid: i32) -> Option<String> {
        let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        // argv is NUL-separated, and the last entry usually has a trailing NUL.
        let joined = raw
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        let s = sysmon_core::text::sanitize_text(joined.trim()).into_owned();
        (!s.is_empty()).then_some(s)
    }

    /// One `/proc/[pid]` entry, and the owner lookup that only a PID the cache
    /// has not seen before pays for.
    fn read_one(&self, pid: i32) -> Option<(RawProcess, ProcessMeta)> {
        // A process can exit between the readdir and the read; that is normal
        // on a busy machine and is not an error.
        let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (raw, mut meta) =
            parse_proc_pid_stat(&text, self.clock_ticks, self.page_size, self.boot_time_ms)?;
        if !self.meta_cache.contains_key(&pid) {
            meta.user = fs::read_to_string(format!("/proc/{pid}/status"))
                .ok()
                .and_then(|s| parse_proc_pid_status_uid(&s))
                .map(|uid| {
                    self.users
                        .get(&uid)
                        .cloned()
                        .unwrap_or_else(|| uid.to_string())
                })
                .unwrap_or_else(|| "?".to_string());
        }
        Some((raw, meta))
    }

    /// Refresh the name cache: drop recycled and exited PIDs, add new ones.
    fn refresh_meta_cache(&mut self, fresh: Vec<ProcessMeta>, hot: &[RawProcess]) {
        // A recycled PID is a different program, so its cached name is wrong.
        let recycled: Vec<i32> = self.deltas.recycled().iter().copied().collect();
        for pid in recycled {
            self.meta_cache.remove(&pid);
        }
        for m in fresh {
            self.meta_cache.insert(m.pid, m);
        }
        // I-10: exited PIDs are evicted, so the cache cannot grow without bound.
        let live: HashSet<i32> = hot.iter().map(|p| p.pid).collect();
        self.meta_cache.retain(|pid, _| live.contains(pid));
    }

    fn to_sample(&self, raw: &RawProcess, cpu_percent: Option<f64>) -> ProcessSample {
        let meta = self.meta_cache.get(&raw.pid);
        let command = meta.map_or_else(|| format!("pid {}", raw.pid), |m| m.command.clone());
        ProcessSample {
            pid: raw.pid,
            ppid: raw.ppid,
            start_time: meta.map_or(StartTime::Unreadable, |m| m.start_time),
            user: meta.map_or_else(|| "?".to_string(), |m| m.user.clone()),
            state: meta.map_or_else(|| "?".to_string(), |m| m.state.clone()),
            cpu_percent,
            rss_bytes: raw.rss_bytes,
            // There is no Linux equivalent of macOS's Energy Impact, so the
            // column is always the CPU-time estimate here and the header always
            // says "est". Inventing a second unit would be worse than admitting
            // there is one.
            energy: energy_proxy(cpu_percent),
            protected: meta
                .is_none_or(|_| LINUX_PROTECTED_NAMES.contains(&process_name(&command).as_str())),
            command,
        }
    }

    pub fn processes(&mut self, cap: WorkingSetCap) -> Result<ProcessesData, String> {
        let mut hot: Vec<RawProcess> = Vec::with_capacity(TYPICAL_PROCESS_COUNT);
        let mut fresh: Vec<ProcessMeta> = Vec::new();
        let entries = fs::read_dir("/proc").map_err(|e| format!("/proc: {e}"))?;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|s| s.parse::<i32>().ok()) else {
                continue;
            };
            let seen_before = self.meta_cache.contains_key(&pid);
            let Some((raw, meta)) = self.read_one(pid) else {
                continue;
            };
            if !seen_before {
                fresh.push(meta);
            }
            hot.push(raw);
        }

        let now = now_ms();
        let cpu_by_pid = self.deltas.update(&hot, now);
        self.refresh_meta_cache(fresh, &hot);

        let all: Vec<ProcessSample> = hot
            .iter()
            .map(|p| self.to_sample(p, cpu_by_pid.get(&p.pid).copied().flatten()))
            .collect();

        let ws = select_working_set(&all, cap);
        Ok(ProcessesData {
            total: all.len(),
            visible: ws.visible,
            others: ws.others,
            // Every PID, so the ancestor guard can walk out of the working set.
            parents: hot.iter().map(|p| (p.pid, p.ppid)).collect(),
            energy_accurate: false,
        })
    }
}

fn no_battery() -> BatteryData {
    BatteryData {
        present: false,
        percent: 0.0,
        charging: false,
        on_ac_power: true,
        time_remaining_min: None,
        watts: None,
        cycle_count: None,
        health_percent: None,
        temperature_c: None,
    }
}

/// The 1, 5 and 15 minute figures, in that order.
fn load_average() -> [f64; LOAD_WINDOWS] {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|s| {
            let n: Vec<f64> = s
                .split_whitespace()
                .take(LOAD_WINDOWS)
                .filter_map(|v| v.parse().ok())
                .collect();
            [
                n.first().copied().unwrap_or(NO_READING),
                n.get(1).copied().unwrap_or(NO_READING),
                n.get(2).copied().unwrap_or(NO_READING),
            ]
        })
        .unwrap_or([NO_READING; LOAD_WINDOWS])
}

/// Sizes for one mount, via `statvfs`.
///
/// Usage is total minus **available to an unprivileged user**, not total minus
/// free: ext4 reserves 5% for root by default, and reporting that reserve as
/// free space tells the user they have room they cannot actually use. This is
/// the same judgement `parse_df` makes about APFS snapshots on macOS.
fn usage(mount: &Mount) -> Option<VolumeUsage> {
    let path = std::ffi::CString::new(mount.mount.as_str()).ok()?;
    // SAFETY: `statvfs` takes a NUL-terminated path and a pointer to a struct
    // it fills; both are valid here, and the result is only read on success.
    #[allow(unsafe_code)]
    let stat = unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut s) != 0 {
            return None;
        }
        s
    };
    let block = stat.f_frsize.max(1) as u64;
    let total = stat.f_blocks as u64 * block;
    if total == 0 {
        return None;
    }
    let free = stat.f_bavail as u64 * block;
    Some(VolumeUsage {
        mount: mount.mount.clone(),
        device: mount.device.clone(),
        total_bytes: total,
        used_bytes: total.saturating_sub(free),
        free_bytes: free,
        network: mount.network,
    })
}
