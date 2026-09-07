//! The sampling runtime: five collectors, five threads, one channel.
//!
//! # Why threads and not an async runtime
//!
//! Every collector here blocks on a pipe read from a child process. That is six
//! mostly-sleeping OS threads. Tokio would add a work-stealing scheduler and,
//! through `tokio::process`, a SIGCHLD reaper of its own — so an extra thread
//! anyway — in exchange for `.await` syntax on operations that are inherently
//! blocking. In an app whose entire pitch is under 1% of a core, that is a loss.
//!
//! Threads also buy something async does not: **each collector's mutable state
//! lives on its own thread's stack with no lock at all.** The CPU delta
//! tracker, the process name cache and the energy staleness counters are never
//! shared, so there is nothing to contend on and nothing to get wrong.
//!
//! # What each guarantee costs here
//!
//! - **I-8, at most one run in flight** stops being a guard and becomes a fact:
//!   a collector thread *is* its own run, and cannot start a second one.
//! - **An overrun skips rather than queues.** Each loop advances its deadline
//!   with `while next <= now { next += interval }`, so a 30s stall on a 10s
//!   tier misses two ticks and keeps its original phase.
//! - **I-11, a failure is isolated**, because a collector only ever publishes
//!   into its own panel. A *panic* is caught too, and becomes a panel error
//!   rather than a silently dead thread.
//! - **Nothing blocks the render.** The main thread never spawns a process and
//!   never calls a collector; it waits on the channel.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use sysmon_core::domain::types::{BatteryData, CpuData, DiskData, HostInfo, MemoryData, Panel};
use sysmon_core::domain::working_set::WorkingSetCap;
use sysmon_core::kill::guards::{GuardContext, LiveIdentity, TargetPid};
use sysmon_core::kill::signal::{
    send_signal, KillErrno, KillOutcome, Killer, Signal, SignalRequest,
};

use crate::collect::{Collector, Identity, ProcessesData};

/// The default tiers.
///
/// Not 1s, even though the CPU counters are free to read: "free to collect" is
/// not "free to display". Every CPU tick triggers a render, and a render costs
/// an order of magnitude more than every collector combined. Measured
/// self-cost of the TypeScript build by tier: 1s → 2.14%, 2s → 1.92%,
/// 5s → 1.14%, **10s → 0.85%** of one core. 10s is chosen because battery life
/// beats gauge smoothness for a tool that lives in a background pane.
#[derive(Debug, Clone, Copy)]
pub struct Tiers {
    pub cpu: Duration,
    pub memory: Duration,
    pub processes: Duration,
    pub battery: Duration,
    pub disk: Duration,
}

impl Default for Tiers {
    fn default() -> Self {
        Self {
            cpu: Duration::from_secs(10),
            memory: Duration::from_secs(10),
            processes: Duration::from_secs(10),
            battery: BATTERY_TIER,
            disk: DISK_TIER,
        }
    }
}

impl Tiers {
    /// `--interval` moves the three fast tiers together. The CPU tier drives the
    /// render rate, which dominates cost, so the flag has to move it too or it
    /// cannot buy responsiveness.
    pub fn with_interval(secs: f64) -> Self {
        let d = Duration::from_secs_f64(secs);
        Self {
            cpu: d,
            memory: d,
            processes: d,
            ..Self::default()
        }
    }
}

/// How soon after launch the delta-based collectors take their second sample.
///
/// Long enough that `ps`'s centisecond CPU-time column still quantises finely
/// (~1.4% per process), short enough that the first real numbers are on screen
/// before the user has finished reading the header. Both CPU sources are deltas
/// by construction, so the first run is only ever half a measurement — without
/// this the app opened with every CPU% reading "—" for a whole tier, which is
/// exactly when the user is looking at it. See I-29.
pub const PRIMING_DELAY: Duration = Duration::from_millis(700);

/// Battery state moves in minutes, not seconds, and `pmset` + `ioreg` are the
/// two most expensive spawns here.
const BATTERY_TIER: Duration = Duration::from_secs(60);
/// Disk usage barely moves at all, and `df` walks every mount.
const DISK_TIER: Duration = Duration::from_secs(300);

/// A sample, or the reason there is not one.
pub enum Msg {
    Host(Box<HostInfo>),
    Cpu(Box<Panel<CpuData>>),
    Memory(Box<Panel<MemoryData>>),
    Disk(Box<Panel<DiskData>>),
    Battery(Box<Panel<BatteryData>>),
    Processes(Box<Panel<ProcessesData>>),
    CommandLine(Option<String>),
    /// The outcome of a kill the user asked for.
    Killed {
        text: String,
        bad: bool,
    },
}

/// What the main thread can ask a collector to do out of band.
pub enum Cmd {
    /// Sample now, ahead of the tier — and **without** disturbing its phase.
    RefreshNow,
    SetCap(WorkingSetCap),
    Shutdown,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Turn a sample result into a panel, catching a panic as an error rather than
/// letting it take the thread down silently.
fn panel<T>(f: impl FnOnce() -> Result<T, String>) -> Panel<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(data)) => Panel::Ok {
            data,
            sampled_at_ms: now_ms(),
        },
        Ok(Err(message)) => Panel::Err {
            message,
            sampled_at_ms: now_ms(),
        },
        Err(_) => Panel::Err {
            message: "collector panicked".to_string(),
            sampled_at_ms: now_ms(),
        },
    }
}

/// Whether a collector takes an extra sample shortly after launch.
///
/// Only the delta-based ones need it: their first run is half a measurement by
/// construction. See I-29.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Priming {
    /// Sample again `PRIMING_DELAY` after launch.
    Early,
    /// A first sample is already a whole measurement, so there is nothing to
    /// prime.
    OnTierOnly,
}

/// A tier's schedule: when the next sample is due, and whether the priming run
/// is still owed.
///
/// The priming run is an extra sample, not a change of phase — the tier
/// deadline is untouched by it.
struct Schedule {
    interval: Duration,
    next: Instant,
    start: Instant,
    primed: bool,
}

impl Schedule {
    fn new(interval: Duration, priming: Priming) -> Self {
        let start = Instant::now();
        Self {
            interval,
            next: start + interval,
            start,
            primed: priming == Priming::OnTierOnly,
        }
    }

    fn deadline(&self) -> Instant {
        if !self.primed && self.start + PRIMING_DELAY < self.next {
            self.start + PRIMING_DELAY
        } else {
            self.next
        }
    }

    fn wait(&self) -> Duration {
        self.deadline().saturating_duration_since(Instant::now())
    }

    /// Advance past a deadline that has just fired.
    ///
    /// Skip, do not queue: an overrun misses ticks and keeps its original phase.
    fn advance(&mut self) {
        if self.deadline() != self.next {
            self.primed = true;
            return;
        }
        let now = Instant::now();
        while self.next <= now {
            self.next += self.interval;
        }
    }
}

/// The tier fired: take the sample and move to the next deadline.
fn on_tick(sample: &mut impl FnMut(), schedule: &mut Schedule) {
    sample();
    schedule.advance();
}

/// One collector's loop.
///
/// `sample` is called on the tier, on an explicit refresh, and once more
/// `PRIMING_DELAY` after launch when `priming` asks for it.
fn spawn_tier<F>(
    name: &'static str,
    interval: Duration,
    priming: Priming,
    commands: Receiver<Cmd>,
    mut sample: F,
) -> std::thread::JoinHandle<()>
where
    F: FnMut() + Send + 'static,
{
    std::thread::Builder::new()
        .name(format!("sysmon-{name}"))
        .spawn(move || {
            sample();
            let mut schedule = Schedule::new(interval, priming);
            loop {
                match commands.recv_timeout(schedule.wait()) {
                    Ok(Cmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
                    // Out of band, so the phase is deliberately not moved.
                    Ok(Cmd::RefreshNow) => sample(),
                    Ok(Cmd::SetCap(_)) => unreachable!("only the process tier takes a cap"),
                    Err(RecvTimeoutError::Timeout) => on_tick(&mut sample, &mut schedule),
                }
            }
        })
        .expect("spawning a collector thread")
}

pub struct Handles {
    pub cpu: Sender<Cmd>,
    pub memory: Sender<Cmd>,
    pub disk: Sender<Cmd>,
    pub battery: Sender<Cmd>,
    pub processes: Sender<Cmd>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Handles {
    /// Ask every collector to sample now. `r` in the keymap.
    pub fn refresh_all(&self) {
        for tx in [
            &self.cpu,
            &self.memory,
            &self.disk,
            &self.battery,
            &self.processes,
        ] {
            let _ = tx.send(Cmd::RefreshNow);
        }
    }

    /// A new cap is only visible after a resample, and waiting out a 10s tier
    /// for a keypress reads as a dead key. **Processes only**: rebuilding every
    /// tier would reset all five phases and re-run the CPU collector out of
    /// band, shortening one delta window and printing a bogus reading. See I-4.
    pub fn set_cap(&self, cap: WorkingSetCap) {
        let _ = self.processes.send(Cmd::SetCap(cap));
        let _ = self.processes.send(Cmd::RefreshNow);
    }

    pub fn shutdown(self) {
        for tx in [
            &self.cpu,
            &self.memory,
            &self.disk,
            &self.battery,
            &self.processes,
        ] {
            let _ = tx.send(Cmd::Shutdown);
        }
        for t in self.threads {
            let _ = t.join();
        }
    }
}

/// Start every collector. Each gets its own collector instance, so no two
/// threads share a lock and a slow one cannot stall a fast one.
pub fn start(outbox: Sender<Msg>, tiers: Tiers, sources: Sources) -> Result<Handles, String> {
    let Sources {
        accurate_energy,
        collectors,
        initial_cap,
    } = sources;
    let source = |accurate: bool| -> Result<Collector, String> {
        match collectors {
            CollectorKind::Mock => Collector::mock(),
            CollectorKind::Live => Collector::new(accurate),
        }
    };
    let mut threads = Vec::new();
    spawn_host(&outbox)?;

    let (cpu_tx, mem_tx) = spawn_fast_tiers(&outbox, &tiers, &source, &mut threads)?;
    let (disk_tx, batt_tx) = spawn_slow_tiers(&outbox, &tiers, &source, &mut threads)?;

    let (proc_tx, commands) = std::sync::mpsc::channel::<Cmd>();
    threads.push(spawn_processes(
        outbox,
        tiers.processes,
        source(accurate_energy)?,
        (commands, initial_cap),
    )?);

    Ok(Handles {
        cpu: cpu_tx,
        memory: mem_tx,
        disk: disk_tx,
        battery: batt_tx,
        processes: proc_tx,
        threads,
    })
}

/// The process tier's loop, which unlike the others can be told a new cap.
fn process_loop(
    outbox: &Sender<Msg>,
    collector: &mut Collector,
    interval: Duration,
    inbox: (Receiver<Cmd>, WorkingSetCap),
) {
    let (commands, initial_cap) = inbox;
    let mut cap = initial_cap;
    let take = |c: &mut Collector, cap: WorkingSetCap| {
        let p = panel(|| c.processes(cap));
        let _ = outbox.send(Msg::Processes(Box::new(p)));
    };
    take(collector, cap);
    let mut schedule = Schedule::new(interval, Priming::Early);
    loop {
        match commands.recv_timeout(schedule.wait()) {
            Ok(Cmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Cmd::SetCap(v)) => cap = v,
            Ok(Cmd::RefreshNow) => take(collector, cap),
            Err(RecvTimeoutError::Timeout) => on_tick(&mut || take(collector, cap), &mut schedule),
        }
    }
}

/// CPU and memory: the two tiers the `--interval` flag moves.
fn spawn_fast_tiers(
    outbox: &Sender<Msg>,
    tiers: &Tiers,
    source: &impl Fn(bool) -> Result<Collector, String>,
    threads: &mut Vec<std::thread::JoinHandle<()>>,
) -> Result<(Sender<Cmd>, Sender<Cmd>), String> {
    let (cpu_tx, commands) = std::sync::mpsc::channel();
    let out = outbox.clone();
    let mut c = source(false)?;
    threads.push(spawn_tier(
        "cpu",
        tiers.cpu,
        // CPU% is a delta, so the first sample is only half a measurement.
        Priming::Early,
        commands,
        move || {
            let p = panel(|| c.cpu());
            let _ = out.send(Msg::Cpu(Box::new(p)));
        },
    ));

    let (mem_tx, commands) = std::sync::mpsc::channel();
    let out = outbox.clone();
    let c = source(false)?;
    threads.push(spawn_tier(
        "memory",
        tiers.memory,
        Priming::OnTierOnly,
        commands,
        move || {
            let p = panel(|| c.memory());
            let _ = out.send(Msg::Memory(Box::new(p)));
        },
    ));
    Ok((cpu_tx, mem_tx))
}

/// Disk and battery: the two tiers that move slowly and cost the most to read.
fn spawn_slow_tiers(
    outbox: &Sender<Msg>,
    tiers: &Tiers,
    source: &impl Fn(bool) -> Result<Collector, String>,
    threads: &mut Vec<std::thread::JoinHandle<()>>,
) -> Result<(Sender<Cmd>, Sender<Cmd>), String> {
    let (disk_tx, commands) = std::sync::mpsc::channel();
    let out = outbox.clone();
    let c = source(false)?;
    threads.push(spawn_tier(
        "disk",
        tiers.disk,
        Priming::OnTierOnly,
        commands,
        move || {
            let p = panel(|| c.disk());
            let _ = out.send(Msg::Disk(Box::new(p)));
        },
    ));

    let (batt_tx, commands) = std::sync::mpsc::channel();
    let out = outbox.clone();
    let c = source(false)?;
    threads.push(spawn_tier(
        "battery",
        tiers.battery,
        Priming::OnTierOnly,
        commands,
        move || {
            let p = panel(|| c.battery());
            let _ = out.send(Msg::Battery(Box::new(p)));
        },
    ));
    Ok((disk_tx, batt_tx))
}

/// Where the samples come from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CollectorKind {
    Live,
    /// Scripted data, and a killer that signals nothing. See `MockKiller`.
    Mock,
}

/// What `start` needs beyond the tiers and the channel.
pub struct Sources {
    pub accurate_energy: bool,
    pub collectors: CollectorKind,
    pub initial_cap: WorkingSetCap,
}

/// Host facts are read once and never resampled, so this thread is not a tier.
fn spawn_host(outbox: &Sender<Msg>) -> Result<(), String> {
    let tx = outbox.clone();
    std::thread::Builder::new()
        .name("sysmon-host".into())
        .spawn(move || {
            if let Ok(c) = Collector::new(false) {
                let _ = tx.send(Msg::Host(Box::new(c.host())));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// The process tier is the one that takes a cap, so it owns its loop.
fn spawn_processes(
    outbox: Sender<Msg>,
    interval: Duration,
    mut collector: Collector,
    inbox: (Receiver<Cmd>, WorkingSetCap),
) -> Result<std::thread::JoinHandle<()>, String> {
    let (commands, initial_cap) = inbox;
    std::thread::Builder::new()
        .name("sysmon-processes".into())
        .spawn(move || {
            process_loop(&outbox, &mut collector, interval, (commands, initial_cap));
        })
        .map_err(|e| e.to_string())
}

/// `kill(2)`, as the one implementation that actually signals.
///
/// Takes [`TargetPid`], so it cannot be handed 0 — which signals the caller's
/// whole process group — or a negative, which signals another group.
struct RealKiller;

impl Killer for RealKiller {
    fn kill(&mut self, pid: TargetPid, signal: Signal) -> Result<(), KillErrno> {
        let sig = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        // SAFETY: `kill(2)` with a pid the type system has already proven is
        // greater than 1, so no process-group signal is representable here.
        #[allow(unsafe_code)]
        let rc = unsafe { libc::kill(pid.get(), sig) };
        if rc == 0 {
            return Ok(());
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Err(KillErrno::Esrch),
            Some(libc::EPERM) => Err(KillErrno::Eperm),
            _ => Err(KillErrno::Other(
                std::io::Error::last_os_error().to_string(),
            )),
        }
    }
}

/// A killer that signals nothing, for `--mock`.
///
/// This is not a convenience. The scripted PIDs are ordinary small integers,
/// and on a real machine some of them **are** real processes — so a mock mode
/// that reached the real `kill(2)` would signal a bystander chosen at random.
/// The mode advertised as safe to try has to actually be safe to try.
struct MockKiller;

impl Killer for MockKiller {
    fn kill(&mut self, _pid: TargetPid, _signal: Signal) -> Result<(), KillErrno> {
        Ok(())
    }
}

/// What the user asked to be signalled, and the parent map the ancestor guard
/// walks. See I-13.
pub struct KillRequest {
    pub target: sysmon_core::domain::types::ProcessSample,
    pub signal: Signal,
    pub parents: std::collections::HashMap<i32, i32>,
}

/// The target's identity, read now rather than taken from the last sample.
///
/// "Could not check" is not "gone". Conflating them told users a very much
/// alive process had already exited. See I-16.
fn read_identity(pid: i32) -> LiveIdentity {
    match Collector::new(false).and_then(|c| c.identity(pid).map_err(|e| e.to_string())) {
        Ok(Identity::Known(start)) => LiveIdentity::Known(start),
        Ok(Identity::Gone) => LiveIdentity::Gone,
        Err(_) => LiveIdentity::Unverifiable,
    }
}

/// Read the target's identity **at signal time** and then signal it, off the
/// main thread so a slow `ps` cannot stall the render.
///
/// The identity is re-read rather than taken from the last sample because the
/// whole point is that it is newer than the sample: a PID recycled between
/// selection and confirmation belongs to a different program now. See I-16.
pub fn spawn_kill(outbox: Sender<Msg>, request: KillRequest, collectors: CollectorKind) {
    let KillRequest {
        target,
        signal,
        parents,
    } = request;
    std::thread::Builder::new()
        .name("sysmon-kill".into())
        .spawn(move || {
            let live = read_identity(target.pid);
            let ctx = GuardContext {
                self_pid: std::process::id() as i32,
                parents,
            };
            let name = sysmon_core::kill::guards::process_name(&target.command);
            let request = SignalRequest {
                target: &target,
                signal,
                ctx: &ctx,
                live,
            };
            let outcome = match collectors {
                CollectorKind::Mock => send_signal(request, &mut MockKiller),
                CollectorKind::Live => send_signal(request, &mut RealKiller),
            };
            let (text, bad) = match outcome {
                KillOutcome::Sent { note } => (
                    note.unwrap_or_else(|| format!("asked {name} to close")),
                    false,
                ),
                KillOutcome::Refused(r) => (r.message, true),
                KillOutcome::Failed(e) => (e, true),
            };
            let _ = outbox.send(Msg::Killed { text, bad });
        })
        .ok();
}
