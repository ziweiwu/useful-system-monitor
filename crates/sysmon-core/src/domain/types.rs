//! The shared data model. Every field the UI renders comes from here, and every
//! collector produces one of these shapes. See INVARIANTS.md.

/// A process start time, or the admission that it could not be read.
///
/// The TypeScript original uses `0` as the "unknown" sentinel, and the kill
/// guard has to remember to check for it — `startTime === 0 || target.startTime
/// === 0 || live.startTime !== target.startTime`. Two unreadable timestamps
/// must never compare equal, because I-16 binds a kill to this field: treating
/// "I could not read it" as a value is how you signal the wrong process.
///
/// As an enum the check cannot be forgotten. [`Self::same_process_as`] is the
/// only way to compare two of these, and it answers `false` when either side is
/// unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartTime {
    /// Milliseconds since the Unix epoch.
    Known(i64),
    /// `ps` answered, but not in a format that could be read back to an
    /// instant — a localised month, say. See I-28.
    Unreadable,
}

impl StartTime {
    /// The epoch, when there is one.
    pub fn epoch_ms(self) -> Option<i64> {
        match self {
            Self::Known(ms) => Some(ms),
            Self::Unreadable => None,
        }
    }

    /// Whether two observations are of the same process instance.
    ///
    /// Deliberately not `PartialEq`: `Unreadable == Unreadable` is `true` and
    /// would be exactly the wrong answer here.
    pub fn same_process_as(self, other: Self) -> bool {
        match (self, other) {
            (Self::Known(a), Self::Known(b)) => a == b,
            _ => false,
        }
    }

    /// The wire form the TypeScript build uses: `0` for unknown.
    ///
    /// Only for parity with the existing `--json` shape and the fixtures. Never
    /// use it to make a decision — that is what [`Self::same_process_as`] and
    /// [`Self::epoch_ms`] are for.
    pub fn as_legacy_number(self) -> i64 {
        self.epoch_ms().unwrap_or(0)
    }
}

/// Cheap per-tick columns: `ps -Ao pid,ppid,time,rss`. See C-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProcess {
    pub pid: i32,
    pub ppid: i32,
    /// Cumulative CPU time in ms. Monotonic per (pid, start time). See I-3.
    pub cpu_time_ms: u64,
    pub rss_bytes: u64,
}

/// Static columns, fetched only when the PID set changes. See C-1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessMeta {
    pub pid: i32,
    pub start_time: StartTime,
    pub command: String,
    pub user: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSample {
    pub pid: i32,
    pub ppid: i32,
    pub start_time: StartTime,
    pub command: String,
    pub user: String,
    pub state: String,
    /// `None` on first observation — never `0.0`. See I-1.
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    /// Energy-impact proxy, or macOS Energy Impact under `--energy=accurate`.
    pub energy: Option<f64>,
    /// True when this process must never be signalled. See I-12..I-14.
    pub protected: bool,
}

/// Everything outside the working set, summed, so the table reconciles with the
/// cards. See C-9.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OthersRollup {
    pub count: usize,
    pub cpu_percent: f64,
    pub rss_bytes: u64,
    pub energy: f64,
}

/// The 1, 5 and 15 minute load averages.
pub const LOAD_WINDOWS: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct CpuData {
    /// Per-core utilisation, each 0..=100. See I-2.
    pub per_core: Vec<f64>,
    /// Mean across cores, 0..=100. Never a sum of process rows. See I-6.
    pub system: f64,
    pub user_percent: f64,
    pub sys_percent: f64,
    pub load_avg: [f64; LOAD_WINDOWS],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryData {
    pub total_bytes: u64,
    /// Wired + active + compressed: the genuinely non-reclaimable portion.
    pub used_bytes: u64,
    /// True `vm_stat` free (+ speculative). Small by design on macOS.
    pub free_bytes: u64,
    /// Everything the system can hand out on demand. This is `total - used`,
    /// and it is what the gauge measures against — NOT `free_bytes`, which is
    /// near zero on a healthy Mac. I-5: `used + available == total`.
    pub available_bytes: u64,
    pub wired_bytes: u64,
    pub active_bytes: u64,
    pub inactive_bytes: u64,
    pub compressed_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

/// One user-facing filesystem.
///
/// "User-facing" is doing real work: `df` lists every APFS sibling in a
/// container with the *same* total and available blocks, so listing rows
/// verbatim shows one 926 G disk four times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeUsage {
    pub mount: String,
    /// The df "Filesystem" column: a device node, or a network share URL.
    pub device: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    /// Not backed by a local device — an SMB/AFP/NFS share.
    pub network: bool,
}

/// The root volume, plus every other user-facing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskData {
    pub mount: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub volumes: Vec<VolumeUsage>,
}

/// The four fields `parse_df` can fill from one mount's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootDisk {
    pub mount: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatteryData {
    pub present: bool,
    pub percent: f64,
    pub charging: bool,
    /// On mains power. Distinct from `charging`: macOS reports "AC attached;
    /// not charging" whenever it is deliberately holding the charge level,
    /// which is neither charging nor draining.
    pub on_ac_power: bool,
    pub time_remaining_min: Option<i64>,
    /// Instantaneous power. Negative = discharging.
    pub watts: Option<f64>,
    pub cycle_count: Option<i64>,
    pub health_percent: Option<i64>,
    pub temperature_c: Option<f64>,
}

/// What `pmset -g batt` alone can answer. The `ioreg` detail is layered on top.
#[derive(Debug, Clone, PartialEq)]
pub struct PmsetBattery {
    pub present: bool,
    pub percent: f64,
    pub charging: bool,
    pub on_ac_power: bool,
    pub time_remaining_min: Option<i64>,
}

/// The `ioreg` detail fields, each independently absent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IoregBattery {
    pub watts: Option<f64>,
    pub cycle_count: Option<i64>,
    pub health_percent: Option<i64>,
    pub temperature_c: Option<f64>,
    /// Only overrides `pmset` when ioreg actually reported it.
    pub charging: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub model: String,
    pub cores: usize,
    pub perf_cores: usize,
    pub eff_cores: usize,
    pub total_mem_bytes: u64,
    pub uptime_sec: u64,
}

/// Per-panel state. A failing collector degrades only its own panel (I-11), and
/// panels sit on different tiers, so each carries its own sample time (I-4).
///
/// The error case deliberately replaces the data rather than annotating it,
/// which is what the shipping build does. Keeping the last-good value alongside
/// the error is better UX and is written down in `POST-CUTOVER.md`; doing it
/// during the port would widen the differential diff for a reason unrelated to
/// the port.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Panel<T> {
    /// Nothing has been sampled yet. The panel says "sampling…" rather than
    /// drawing a zero, for the same reason CPU% is `None` on first sight: a
    /// number that has not been measured is not reported. See I-1.
    #[default]
    Pending,
    Ok {
        data: T,
        sampled_at_ms: i64,
    },
    Err {
        message: String,
        sampled_at_ms: i64,
    },
}

impl<T> Panel<T> {
    /// The sample, if this panel has one.
    pub fn sample(&self) -> Option<&T> {
        match self {
            Self::Ok { data, .. } => Some(data),
            _ => None,
        }
    }

    pub fn sampled_at_ms(&self) -> Option<i64> {
        match self {
            Self::Ok { sampled_at_ms, .. } | Self::Err { sampled_at_ms, .. } => {
                Some(*sampled_at_ms)
            }
            Self::Pending => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Err { message, .. } => Some(message),
            _ => None,
        }
    }
}
