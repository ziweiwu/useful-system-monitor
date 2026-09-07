//! Linux `/proc` and `/sys` parsers.
//!
//! Pure, and compiled on every platform so their tests run everywhere — which
//! matters more here than for the macOS parsers, because of the gap this port
//! cannot close:
//!
//! # There is no Linux oracle
//!
//! The shipping TypeScript build is macOS-only. The differential harness that
//! makes the rest of this port checkable — run both binaries, compare the
//! answers — **does not exist for any of this**. There is nothing to diff
//! against.
//!
//! So the fixtures in `test/fixtures/linux/` are written from the documented
//! `proc(5)` formats rather than captured from a running machine, and they
//! should be **replaced with real captures from more than one distro and kernel
//! version** before anyone calls Linux support finished. Until then these
//! parsers are new code with new tests, not a port with an oracle, and they
//! should be read that way.

use std::collections::HashMap;

use crate::domain::deltas::CpuTimes;
use crate::domain::types::{BatteryData, MemoryData, ProcessMeta, RawProcess, StartTime};
use crate::text::sanitize_text;

/// Per-core counters from `/proc/stat`.
///
/// The aggregate `cpu` line is skipped: system CPU is the mean across cores
/// (I-6), and taking the kernel's own total instead would quietly change what
/// the number means between platforms.
///
/// Fields are `user nice system idle iowait irq softirq steal guest guest_nice`.
/// `iowait` is folded into idle — a core waiting on disk is not doing work, and
/// counting it as busy makes an idle machine that is copying a file read as
/// pegged. `irq` carries `irq + softirq`, which is what `os.cpus()` reports on
/// this platform.
pub fn parse_proc_stat(content: &str) -> Vec<CpuTimes> {
    let mut out = Vec::new();
    for line in content.lines() {
        let Some(rest) = line.strip_prefix("cpu") else {
            continue;
        };
        // "cpu " is the aggregate; "cpu0" and friends are the cores.
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let n: Vec<f64> = rest
            .split_whitespace()
            .skip(1)
            .filter_map(|f| f.parse::<f64>().ok())
            .collect();
        // user nice system idle iowait irq softirq …
        const IDLE: usize = 3;
        const IOWAIT: usize = 4;
        const IRQ: usize = 5;
        const SOFTIRQ: usize = 6;
        if n.len() <= SOFTIRQ {
            continue;
        }
        out.push(CpuTimes {
            user: n[0],
            nice: n[1],
            sys: n[2],
            idle: n[IDLE] + n[IOWAIT],
            irq: n[IRQ] + n[SOFTIRQ],
        });
    }
    out
}

/// `/proc/meminfo` reports in kB.
const BYTES_PER_KB: u64 = 1024;
const MS_PER_SEC: u64 = 1000;
/// A battery field the driver did not report.
const NO_READING: f64 = 0.0;
const FULL_SCALE_PCT: f64 = 100.0;
/// µW, µA and µV all need the same divisor to reach base units.
const MICRO_PER_UNIT: f64 = 1_000_000.0;
/// POWER_SUPPLY_TEMP is in tenths of a degree.
const DECICELSIUS_PER_CELSIUS: f64 = 10.0;

/// `/proc/meminfo`, in kB.
fn meminfo_fields(content: &str) -> HashMap<&str, u64> {
    let mut kb: HashMap<&str, u64> = HashMap::new();
    for line in content.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(v) = rest
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        kb.insert(key, v);
    }
    kb
}

pub fn parse_meminfo(content: &str) -> MemoryData {
    let kb = meminfo_fields(content);
    let get = |k: &str| kb.get(k).copied().unwrap_or(0) * BYTES_PER_KB;

    let total = get("MemTotal");
    /*
     * `MemAvailable` is the kernel's own estimate of what can be handed out
     * without swapping, and it is the only honest denominator here: it already
     * accounts for the reclaimable half of the page cache, which
     * `free + buffers + cached` over-counts and `MemFree` alone wildly
     * under-counts. Using MemFree would report a healthy machine as 97% full
     * for the same reason `top`'s total-minus-free does on macOS.
     */
    let available = get("MemAvailable").min(total);
    let used = total.saturating_sub(available);

    MemoryData {
        total_bytes: total,
        used_bytes: used,
        // True free, kept distinct from available — listing both `inactive` and
        // an "available" that already contains it would double-count.
        free_bytes: get("MemFree"),
        // I-5: used + available == total by construction.
        available_bytes: available,
        // Linux has no "wired". The nearest honest equivalent is memory the
        // kernel cannot reclaim: unreclaimable slab plus locked pages.
        wired_bytes: get("SUnreclaim") + get("Mlocked"),
        active_bytes: get("Active"),
        inactive_bytes: get("Inactive"),
        // zswap when it is on, and zero when it is not — never a guess.
        compressed_bytes: get("Zswapped"),
        swap_total_bytes: get("SwapTotal"),
        swap_used_bytes: get("SwapTotal").saturating_sub(get("SwapFree")),
    }
}

/// One row of `/proc/[pid]/stat`.
///
/// # The parse everyone gets wrong
///
/// Field 2 is the executable name **in parentheses, and it may contain both
/// spaces and parentheses** — `(Web Content (tab))` is a real Firefox process.
/// Splitting the line on whitespace therefore corrupts every field after it,
/// which shifts `utime`, `rss` and `starttime` by an unpredictable amount and
/// produces plausible-looking nonsense rather than an error. The name is
/// delimited by the **last** `)` on the line, not the first.
pub fn parse_proc_pid_stat(
    content: &str,
    clock_ticks: u64,
    page_size: u64,
    boot_time_ms: i64,
) -> Option<(RawProcess, ProcessMeta)> {
    // The name is delimited by the **last** `)` on the line, not the first.
    let open = content.find('(')?;
    let close = content.rfind(')')?;
    if close < open {
        return None;
    }
    let pid: i32 = content[..open].trim().parse().ok()?;
    let comm = &content[open + 1..close];
    let tail: Vec<&str> = content[close + 1..].split_whitespace().collect();
    let stat = StatFields::read(&tail)?;

    let ticks = clock_ticks.max(1);
    Some((
        RawProcess {
            pid,
            ppid: stat.ppid,
            cpu_time_ms: (stat.utime + stat.stime) * MS_PER_SEC / ticks,
            rss_bytes: stat.rss_pages * page_size,
        },
        ProcessMeta {
            pid,
            start_time: StartTime::Known(
                boot_time_ms + (stat.start_ticks * MS_PER_SEC / ticks) as i64,
            ),
            command: sanitize_text(comm).into_owned(),
            user: String::new(),
            state: sanitize_text(&stat.state).into_owned(),
        },
    ))
}

/// The fields this app reads from the tail of `/proc/[pid]/stat`.
struct StatFields {
    state: String,
    ppid: i32,
    utime: u64,
    stime: u64,
    start_ticks: u64,
    rss_pages: u64,
}

impl StatFields {
    /// `rest` starts at `state`, so every index here is three less than its
    /// `proc(5)` field number: state ppid pgrp session tty tpgid flags minflt
    /// cminflt majflt cmajflt utime stime …
    fn read(rest: &[&str]) -> Option<Self> {
        const UTIME: usize = 11;
        const STIME: usize = 12;
        /// Field 22 overall.
        const STARTTIME: usize = 19;
        /// Field 24 overall, and in **pages**.
        const RSS_PAGES: usize = 21;
        if rest.len() <= RSS_PAGES {
            return None;
        }
        Some(Self {
            state: rest[0].to_string(),
            ppid: rest[1].parse().ok()?,
            utime: rest[UTIME].parse().ok()?,
            stime: rest[STIME].parse().ok()?,
            start_ticks: rest[STARTTIME].parse().ok()?,
            rss_pages: rest[RSS_PAGES].parse().ok()?,
        })
    }
}

/// The owning uid from `/proc/[pid]/status`.
///
/// Only the real uid is read: a setuid binary's effective uid is not who is
/// running it, and the table is answering "whose process is this".
pub fn parse_proc_pid_status_uid(content: &str) -> Option<u32> {
    content
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// A user-facing mount from `/proc/mounts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub device: String,
    pub mount: String,
    pub fs_type: String,
    pub network: bool,
}

/// Filesystems that exist for the kernel's benefit rather than the user's.
///
/// The same judgement `parse_df_all` makes on macOS: a list that shows `proc`,
/// `sysfs` and eleven `tmpfs` entries alongside the disk is not a disk panel.
const PSEUDO_FS: &[&str] = &[
    "sysfs",
    "proc",
    "devtmpfs",
    "devpts",
    "tmpfs",
    "securityfs",
    "cgroup",
    "cgroup2",
    "pstore",
    "efivarfs",
    "bpf",
    "debugfs",
    "tracefs",
    "fusectl",
    "configfs",
    "ramfs",
    "hugetlbfs",
    "mqueue",
    "autofs",
    "binfmt_misc",
    "rpc_pipefs",
    "nsfs",
    "squashfs",
    "overlay",
    "fuse.gvfsd-fuse",
    "fuse.portal",
];

const NETWORK_FS: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "afs",
    "sshfs",
    "fuse.sshfs",
    "9p",
    "ceph",
];

/// Every user-facing mount in `/proc/mounts`, deduplicated by device.
///
/// A bind mount and a btrfs subvolume both report the same device with the same
/// totals, which is the Linux shape of the APFS-container problem: listing them
/// verbatim multiplies the machine's apparent storage. The outermost mount
/// represents the device, `/` always winning.
pub fn parse_mounts(content: &str) -> Vec<Mount> {
    let mut seen: Vec<Mount> = Vec::new();
    for line in content.lines() {
        // device mountpoint fstype …
        const FS_TYPE: usize = 2;
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() <= FS_TYPE {
            continue;
        }
        let (device, mount, fs_type) = (fields[0], unescape_mount(fields[1]), fields[FS_TYPE]);
        if PSEUDO_FS.contains(&fs_type) {
            continue;
        }
        let network = NETWORK_FS.contains(&fs_type);
        let Some(existing) = seen.iter_mut().find(|m| m.device == device) else {
            seen.push(Mount {
                device: device.to_string(),
                mount,
                fs_type: fs_type.to_string(),
                network,
            });
            continue;
        };
        // Keep the outermost path; `/` wins outright.
        if mount == "/" || (existing.mount != "/" && mount.len() < existing.mount.len()) {
            existing.mount = mount;
        }
    }
    seen.sort_by(|a, b| match (a.mount.as_str(), b.mount.as_str()) {
        ("/", "/") => std::cmp::Ordering::Equal,
        ("/", _) => std::cmp::Ordering::Less,
        (_, "/") => std::cmp::Ordering::Greater,
        _ => a.mount.cmp(&b.mount),
    });
    seen
}

/// `/proc/mounts` escapes space, tab, newline and backslash as octal.
fn unescape_mount(escaped: &str) -> String {
    const OCTAL_DIGITS: usize = 3;
    const OCTAL_RADIX: u32 = 8;
    let mut out = String::with_capacity(escaped.len());
    let mut chars = escaped.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let digits = 0..OCTAL_DIGITS;
        let octal: String = digits.filter_map(|_| chars.next()).collect();
        match u32::from_str_radix(&octal, OCTAL_RADIX)
            .ok()
            .and_then(char::from_u32)
        {
            Some(decoded) => out.push(decoded),
            None => {
                out.push('\\');
                out.push_str(&octal);
            }
        }
    }
    out
}

/// POWER_SUPPLY_POWER_NOW is µW; the older CURRENT_NOW is µA and needs the
/// voltage to become watts. Negative while discharging, as on macOS.
fn power_supply_watts(num: &impl Fn(&str) -> Option<f64>, status: Option<&str>) -> Option<f64> {
    let charging = status == Some("Charging");
    num("POWER_SUPPLY_POWER_NOW")
        .map(|w| w / MICRO_PER_UNIT)
        .or_else(|| {
            let amps = num("POWER_SUPPLY_CURRENT_NOW")? / MICRO_PER_UNIT;
            let volts = num("POWER_SUPPLY_VOLTAGE_NOW")? / MICRO_PER_UNIT;
            Some(amps * volts)
        })
        .map(|w| if charging { w.abs() } else { -w.abs() })
}

/// Capacity now against capacity when new, from whichever pair of fields this
/// driver reports.
fn battery_health(num: &impl Fn(&str) -> Option<f64>) -> Option<i64> {
    let full = num("POWER_SUPPLY_ENERGY_FULL").or_else(|| num("POWER_SUPPLY_CHARGE_FULL"))?;
    let design = num("POWER_SUPPLY_ENERGY_FULL_DESIGN")
        .or_else(|| num("POWER_SUPPLY_CHARGE_FULL_DESIGN"))?;
    (design > NO_READING).then(|| ((full / design) * FULL_SCALE_PCT).round() as i64)
}

/// `/sys/class/power_supply/BAT*/uevent`.
///
/// Laptops report either energy (µWh) or charge (µAh); both are handled, and a
/// machine that reports neither still gets its percentage from `CAPACITY`.
pub fn parse_power_supply(content: &str) -> Option<BatteryData> {
    let mut kv: HashMap<&str, &str> = HashMap::new();
    for line in content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            kv.insert(k.trim(), v.trim());
        }
    }
    if kv.get("POWER_SUPPLY_TYPE").copied() != Some("Battery") {
        return None;
    }
    let num = |k: &str| kv.get(k).and_then(|v| v.parse::<f64>().ok());
    let watts = power_supply_watts(&num, kv.get("POWER_SUPPLY_STATUS").copied());
    let status = kv.get("POWER_SUPPLY_STATUS").copied().unwrap_or("Unknown");
    let charging = status == "Charging";
    // "Not charging" is the Linux spelling of macOS's "AC attached": plugged in
    // and deliberately holding the level, which is neither charging nor
    // draining. Only "Discharging" means running on the battery.
    let on_ac = status != "Discharging";

    Some(BatteryData {
        present: kv.get("POWER_SUPPLY_PRESENT").copied() != Some("0"),
        percent: num("POWER_SUPPLY_CAPACITY").unwrap_or(NO_READING),
        charging,
        on_ac_power: on_ac,
        // The kernel does not estimate time remaining, and computing one from
        // an instantaneous draw would be a guess dressed as a measurement.
        time_remaining_min: None,
        watts,
        cycle_count: num("POWER_SUPPLY_CYCLE_COUNT")
            .map(|c| c as i64)
            .filter(|c| *c > 0),
        health_percent: battery_health(&num),
        // µ°C, and only some drivers report it at all.
        temperature_c: num("POWER_SUPPLY_TEMP").map(|t| t / DECICELSIUS_PER_CELSIUS),
    })
}

/// Killing any of these takes the session, the desktop or the machine down.
///
/// Deliberately **not** the macOS list with names swapped: this is kill-path
/// safety, so it gets its own review rather than a translation. `systemd` is
/// PID 1 and already refused by I-12; it is here as well because a container or
/// a user session can run another one.
pub const LINUX_PROTECTED_NAMES: [&str; 14] = [
    "systemd",
    "init",
    "kthreadd",
    "dbus-daemon",
    "dbus-broker",
    "systemd-journald",
    "systemd-logind",
    "systemd-udevd",
    "Xorg",
    "Xwayland",
    "gnome-shell",
    "kwin_wayland",
    "plasmashell",
    "sshd",
];
