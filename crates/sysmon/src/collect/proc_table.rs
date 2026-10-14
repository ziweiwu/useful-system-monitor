//! The process table, read from libproc for every process this user owns.
//!
//! # Why this module is the other one that uses `unsafe`
//!
//! `ps` is setuid root and pays per process it lists, whatever the columns:
//! 22 ms for the hot columns of ~900 processes, 42 ms for the static ones. Two
//! thirds of those processes belong to this user, and libproc describes them
//! to an unprivileged caller for microseconds each, without a spawn. Other
//! users' processes are closed to it — their CPU time, memory and command line
//! need `ps`'s privilege — so they are still read by `ps`, restricted to them.
//!
//! Every field is the one `ps` prints, from the same kernel source: CPU time
//! is `pti_total_user + pti_total_system` (what `ps` sums from the task and
//! its threads), RSS is the task's resident size, the start time is the one
//! `lstart` formats, the user is the effective UID's name, COMM is `argv[0]`
//! from the process's own argument area, and the state letter is the most
//! active thread's run state followed by the same flag letters in the same
//! order. The hot columns are read on every sample and the static ones only
//! when the caller asks — when a visible row has no name, which is when `ps
//! -A` was asked for them before. The unsafety is confined to the four calls
//! below, each into a buffer whose size is passed alongside it; everything
//! else is safe code.
#![allow(unsafe_code)]

use std::collections::{BTreeSet, HashMap};
use std::ffi::{c_int, c_void, CStr};
use std::mem::size_of;

use sysmon_core::domain::types::{ProcessMeta, RawProcess, StartTime};
use sysmon_core::parse::ps::parse_lstart;
use sysmon_core::text::sanitize_text;

/// What one pass over the process table could read, and what it leaves to `ps`.
pub struct Table {
    /// Hot columns for every process libproc could read.
    pub hot: Vec<RawProcess>,
    /// Real UIDs, other than this user's, of the processes it could not read.
    pub other_uids: BTreeSet<u32>,
    /// Processes of this user it could not read: setuid programs and zombies.
    pub own_unreadable: Vec<i32>,
}

/// Reusable state for reading the table: fixed facts about the machine, and
/// the buffer argument areas are copied into.
pub struct Reader {
    /// Mach time units to nanoseconds, as `numer / denom`.
    timebase: (u64, u64),
    /// `kern.argmax` bytes: the largest argument area the kernel will return.
    args: Vec<u8>,
}

impl Reader {
    pub fn new() -> Option<Self> {
        Some(Self {
            timebase: timebase()?,
            args: vec![0; arg_max()?],
        })
    }

    /// The hot columns of every process: read here when libproc allows it,
    /// listed for `ps` otherwise. `None` when the PID list itself cannot be
    /// had.
    pub fn read(&self) -> Option<Table> {
        let real_uid = real_uid();
        let mut table = Table {
            hot: Vec::new(),
            other_uids: BTreeSet::new(),
            own_unreadable: Vec::new(),
        };
        for pid in all_pids()? {
            let Some(info) = task_all_info(pid) else {
                self.route_unreadable(pid, real_uid, &mut table);
                continue;
            };
            table.hot.push(RawProcess {
                pid,
                ppid: i32::try_from(info.pbsd.pbi_ppid).unwrap_or(0),
                cpu_time_ms: self.cpu_time_ms(&info.ptinfo),
                rss_bytes: info.ptinfo.pti_resident_size,
            });
        }
        Some(table)
    }

    /// The static columns of `pids`, and those of them it could not read —
    /// gone in between, or an argument area closed to it — for `ps` to name.
    pub fn metas(&mut self, pids: &[i32]) -> (Vec<ProcessMeta>, Vec<i32>) {
        let mut users = HashMap::new();
        let mut metas = Vec::with_capacity(pids.len());
        let mut unread = Vec::new();
        for &pid in pids {
            let (Some(info), Some(command)) = (task_all_info(pid), self.argv0(pid)) else {
                unread.push(pid);
                continue;
            };
            let bsd = &info.pbsd;
            let user = users
                .entry(bsd.pbi_uid)
                .or_insert_with(|| user_name(bsd.pbi_uid))
                .clone();
            metas.push(ProcessMeta {
                pid,
                start_time: start_time(bsd.pbi_start_tvsec),
                command: sanitize_text(command.trim()).into_owned(),
                user: sanitize_text(&user).into_owned(),
                state: state(pid, bsd, info.ptinfo.pti_threadnum),
            });
        }
        (metas, unread)
    }

    /// A process libproc would not describe goes to `ps`: by real UID when it
    /// is another user's, by PID when it is this user's (`-U` would relist
    /// everything already read). One that has vanished goes nowhere.
    fn route_unreadable(&self, pid: i32, real_uid: u32, table: &mut Table) {
        let Some(short) =
            pid_info::<libc::proc_bsdshortinfo>(pid, libc::PROC_PIDT_SHORTBSDINFO, FIND_ZOMBIES)
        else {
            return;
        };
        if short.pbsi_ruid == real_uid {
            table.own_unreadable.push(pid);
        } else {
            table.other_uids.insert(short.pbsi_ruid);
        }
    }

    fn cpu_time_ms(&self, task: &libc::proc_taskinfo) -> u64 {
        let (numer, denom) = self.timebase;
        let ticks = u128::from(task.pti_total_user) + u128::from(task.pti_total_system);
        let nanos = ticks * u128::from(numer) / u128::from(denom);
        u64::try_from(nanos / NANOS_PER_MS).unwrap_or(u64::MAX)
    }

    /// `argv[0]`, which is what `ps -o comm` prints: the argument area holds
    /// argc, the executable path, NUL padding, then the arguments.
    fn argv0(&mut self, pid: i32) -> Option<String> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut len = self.args.len();
        // SAFETY: `args` is `len` bytes long; sysctl writes at most `len`
        // bytes into it and stores how many it wrote back into `len`.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                MIB_DEPTH,
                self.args.as_mut_ptr().cast::<c_void>(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        let area = self.args.get(size_of::<c_int>()..len)?;
        let after_path = &area[area.iter().position(|b| *b == 0)?..];
        let arg = &after_path[after_path.iter().position(|b| *b != 0)?..];
        let end = arg.iter().position(|b| *b == 0).unwrap_or(arg.len());
        Some(String::from_utf8_lossy(&arg[..end]).into_owned())
    }
}

/// The BSD and task records together, in one call rather than two.
fn task_all_info(pid: i32) -> Option<libc::proc_taskallinfo> {
    pid_info::<libc::proc_taskallinfo>(pid, libc::PROC_PIDTASKALLINFO, 0)
}

/// `proc_pidinfo` into a `T`, or `None` unless it filled the whole struct.
fn pid_info<T>(pid: i32, flavor: c_int, arg: u64) -> Option<T> {
    let size = c_int::try_from(size_of::<T>()).ok()?;
    let mut out = std::mem::MaybeUninit::<T>::zeroed();
    // SAFETY: `out` is `size` bytes; proc_pidinfo writes at most that many and
    // returns the count. Every `T` used here is a plain C struct of integers
    // and byte arrays, for which any bit pattern — all zeros included — is a
    // valid value, so it is initialised whatever was written.
    let wrote = unsafe { libc::proc_pidinfo(pid, flavor, arg, out.as_mut_ptr().cast(), size) };
    // SAFETY: as above.
    (wrote == size).then(|| unsafe { out.assume_init() })
}

fn all_pids() -> Option<Vec<i32>> {
    // SAFETY: a null buffer asks only for the count.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let mut pids = vec![0; usize::try_from(count).ok()? + PID_SLACK];
    let bytes = c_int::try_from(pids.len() * size_of::<i32>()).ok()?;
    // SAFETY: `pids` is `bytes` long, and proc_listallpids writes at most that.
    let found = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(usize::try_from(found).ok().filter(|n| *n > 0)?);
    Some(pids)
}

/// `ps`'s state column: the most active thread's run state, then flags.
fn state(pid: i32, bsd: &libc::proc_bsdinfo, threads: i32) -> String {
    let mut out = String::from(match bsd.pbi_status {
        libc::SSTOP => 'T',
        libc::SZOMB => 'Z',
        _ => RANK_LETTERS[busiest_thread_rank(pid, threads)],
    });
    let flags = bsd.pbi_flags;
    if bsd.pbi_nice < 0 {
        out.push('<');
    } else if bsd.pbi_nice > 0 {
        out.push('N');
    }
    for (flag, letter) in [
        (PROC_FLAG_TRACED, 'X'),
        (PROC_FLAG_INEXIT, 'E'),
        (PROC_FLAG_PPWAIT, 'V'),
        (PROC_FLAG_SYSTEM, 'L'),
        (PROC_FLAG_SLEADER, 's'),
    ] {
        if flags & flag != 0 {
            out.push(letter);
        }
    }
    if flags & PROC_FLAG_CONTROLT != 0 && bsd.pbi_pgid == bsd.e_tpgid {
        out.push('+');
    }
    out
}

/// The lowest `ps` rank among a process's threads: running beats
/// uninterruptible beats sleeping, and so on. See `RANK_LETTERS`.
fn busiest_thread_rank(pid: i32, threads: i32) -> usize {
    let capacity = usize::try_from(threads).unwrap_or(0) + THREAD_SLACK;
    let mut ids = vec![0u64; capacity];
    let Ok(bytes) = c_int::try_from(capacity * size_of::<u64>()) else {
        return RANK_UNKNOWN;
    };
    // SAFETY: `ids` is `bytes` long; proc_pidinfo writes at most that many
    // bytes of thread handles and returns the count.
    let wrote =
        unsafe { libc::proc_pidinfo(pid, PROC_PIDLISTTHREADS, 0, ids.as_mut_ptr().cast(), bytes) };
    let listed = usize::try_from(wrote).unwrap_or(0) / size_of::<u64>();
    let mut best = RANK_UNKNOWN;
    for id in &ids[..listed.min(capacity)] {
        let Some(t) = pid_info::<libc::proc_threadinfo>(pid, libc::PROC_PIDTHREADINFO, *id) else {
            continue;
        };
        best = best.min(thread_rank(t.pth_run_state, t.pth_sleep_time));
        if best == RANK_RUNNING {
            break;
        }
    }
    best
}

/// `ps`'s ordering of mach thread states, lowest first.
fn thread_rank(run_state: i32, sleep_secs: i32) -> usize {
    match run_state {
        libc::TH_STATE_RUNNING => RANK_RUNNING,
        libc::TH_STATE_UNINTERRUPTIBLE => RANK_UNINTERRUPTIBLE,
        libc::TH_STATE_WAITING if sleep_secs > MAX_SLEEP_SECS => RANK_IDLE,
        libc::TH_STATE_WAITING => RANK_SLEEPING,
        libc::TH_STATE_STOPPED => RANK_STOPPED,
        libc::TH_STATE_HALTED => RANK_HALTED,
        _ => RANK_UNKNOWN,
    }
}

/// The letter for each rank: running, uninterruptible, sleeping, idle,
/// stopped, halted; anything else, or no thread at all, is unknown.
const RANK_LETTERS: [char; 8] = ['?', 'R', 'U', 'S', 'I', 'T', 'H', '?'];
const RANK_RUNNING: usize = 1;
const RANK_UNINTERRUPTIBLE: usize = RANK_RUNNING + 1;
const RANK_SLEEPING: usize = RANK_UNINTERRUPTIBLE + 1;
const RANK_IDLE: usize = RANK_SLEEPING + 1;
const RANK_STOPPED: usize = RANK_IDLE + 1;
const RANK_HALTED: usize = RANK_STOPPED + 1;
const RANK_UNKNOWN: usize = RANK_LETTERS.len() - 1;
/// A waiting thread asleep longer than this is idle (`I`), not sleeping.
const MAX_SLEEP_SECS: i32 = 20;

/// The start time through the parser `lstart` goes through, so a kill's
/// identity check (I-16), which re-reads `lstart` from `ps`, compares like
/// with like — even in the hour a DST fall-back repeats.
fn start_time(epoch_secs: u64) -> StartTime {
    let Ok(secs) = libc::time_t::try_from(epoch_secs) else {
        return StartTime::Unreadable;
    };
    // SAFETY: an all-zero `tm` is valid, and localtime_r only writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are to live, correctly typed locals.
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return StartTime::Unreadable;
    }
    let month = usize::try_from(tm.tm_mon)
        .ok()
        .and_then(|m| MONTHS.get(m))
        .copied()
        .unwrap_or("?");
    parse_lstart(&format!(
        "{month} {} {:02}:{:02}:{:02} {}",
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        tm.tm_year + TM_YEAR_BASE
    ))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const TM_YEAR_BASE: c_int = 1900;

/// The name `ps` prints for a UID: the account name, or the number if none.
fn user_name(uid: u32) -> String {
    let mut buf = vec![0 as libc::c_char; PASSWD_BUF];
    // SAFETY: an all-zero `passwd` is valid; getpwuid_r only writes into it.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is to a live local, and `buf.len()` is its size.
    let rc = unsafe { libc::getpwuid_r(uid, &mut entry, buf.as_mut_ptr(), buf.len(), &mut found) };
    if rc != 0 || found.is_null() || entry.pw_name.is_null() {
        return uid.to_string();
    }
    // SAFETY: on success pw_name points at a NUL-terminated string in `buf`.
    unsafe { CStr::from_ptr(entry.pw_name) }
        .to_string_lossy()
        .into_owned()
}

/// Mach time units to nanoseconds: 125/3 on Apple Silicon, 1/1 on Intel.
///
/// Deprecated in the libc crate in favour of `mach2`, not in the OS; `cpu.rs`
/// makes the same call for the same reason.
#[allow(deprecated)]
fn timebase() -> Option<(u64, u64)> {
    let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: `info` is a valid, writable `mach_timebase_info`.
    if unsafe { libc::mach_timebase_info(&mut info) } != 0 || info.denom == 0 {
        return None;
    }
    Some((u64::from(info.numer), u64::from(info.denom)))
}

fn real_uid() -> u32 {
    // SAFETY: getuid cannot fail and has no preconditions.
    unsafe { libc::getuid() }
}

fn arg_max() -> Option<usize> {
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut value: c_int = 0;
    let mut len = size_of::<c_int>();
    // SAFETY: `value` is a writable c_int and `len` is its size.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut value as *mut c_int).cast::<c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0).then(|| usize::try_from(value).ok()).flatten()
}

/// `PROC_PIDLISTTHREADS`, which the libc crate does not export.
const PROC_PIDLISTTHREADS: c_int = 6;
/// Ask `PROC_PIDT_SHORTBSDINFO` to answer for zombies too, as `ps` lists them.
const FIND_ZOMBIES: u64 = 1;
/// The `proc_bsdinfo.pbi_flags` bits behind `ps`'s flag letters.
const PROC_FLAG_SYSTEM: u32 = 0x1;
const PROC_FLAG_TRACED: u32 = 0x2;
const PROC_FLAG_INEXIT: u32 = 0x4;
const PROC_FLAG_PPWAIT: u32 = 0x8;
const PROC_FLAG_SLEADER: u32 = 0x20;
const PROC_FLAG_CONTROLT: u32 = 0x80;
/// `{CTL_KERN, KERN_PROCARGS2, pid}`.
const MIB_DEPTH: u32 = 3;
/// Room for processes started between counting and listing.
const PID_SLACK: usize = 64;
/// Room for threads started between counting and listing.
const THREAD_SLACK: usize = 16;
const NANOS_PER_MS: u128 = 1_000_000;
/// Comfortably above `sysconf(_SC_GETPW_R_SIZE_MAX)` on macOS.
const PASSWD_BUF: usize = 4096;
