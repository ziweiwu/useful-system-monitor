//! All kill-safety rules, kept pure so every refusal path is unit-testable
//! without spawning anything. See I-12..I-16.

use std::collections::{HashMap, HashSet};

use crate::domain::types::{ProcessSample, StartTime};
use crate::text::sanitize_text;

/// Killing any of these either logs you out, wedges the window server, or takes
/// the whole session down. Refused outright rather than merely warned about.
pub const PROTECTED_NAMES: [&str; 12] = [
    "kernel_task",
    "launchd",
    "WindowServer",
    "loginwindow",
    "SystemUIServer",
    "opendirectoryd",
    "mds",
    "mds_stores",
    "securityd",
    "configd",
    "coreaudiod",
    "Finder",
];

/// A PID that has passed [`check_kill`].
///
/// The only constructor is private and rejects `pid <= 1`, so a signal cannot
/// be sent to one. That is not tidiness: `kill(0, …)` signals **every process
/// in the caller's process group**, and `kill(-n, …)` signals group `n`. Both
/// are reachable from an `i32` and neither is recoverable, so the type makes
/// them unwritable rather than trusting a guard to have run.
///
/// It carries no lifetime and cannot be forged, so possession of one *is* the
/// proof that the checks passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPid(i32);

impl TargetPid {
    fn new(pid: i32) -> Option<Self> {
        (pid > 1).then_some(Self(pid))
    }

    pub fn get(self) -> i32 {
        self.0
    }
}

impl std::fmt::Display for TargetPid {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    Init,
    SelfProcess,
    Ancestor,
    Protected,
    Recycled,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KillRefusal {
    pub kind: RefusalKind,
    pub message: String,
}

/// What could be learned about the target at signal time.
///
/// Deliberately three states rather than an optional number. It used to be
/// `liveStartTime?: number`, and "the caller could not find the process" and
/// "the caller did not look" were the same value — so the PID-reuse check
/// (I-16) silently did nothing in exactly the case it exists for. A recycled
/// PID belongs to a brand-new process, which has no CPU history and a small
/// RSS, so it is almost never in the working set the caller searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveIdentity {
    /// Read at signal time; compare it against the target.
    Known(StartTime),
    /// No process with this PID exists any more.
    Gone,
    /// Nothing could be read. A guard that cannot verify must refuse.
    Unverifiable,
}

/// The outcome of the guards. `Allowed` carries the proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillCheck {
    Allowed(TargetPid),
    Refused(KillRefusal),
}

impl KillCheck {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed(_))
    }

    pub fn refusal(&self) -> Option<&KillRefusal> {
        match self {
            Self::Refused(r) => Some(r),
            Self::Allowed(_) => None,
        }
    }
}

/// Basename of a full executable path, for denylist matching and display.
///
/// Must take the basename of the *whole* string: macOS paths routinely contain
/// spaces ("/Library/Application Support/…", "…/Google Chrome Helper"), so
/// splitting on whitespace first turns that path into "Application".
/// `ps -o comm` gives the executable path without arguments, so there is
/// nothing to strip.
pub fn process_name(command: &str) -> String {
    let trimmed = command.trim();
    let last = trimmed.rsplit('/').next().unwrap_or("");
    // Sanitised here as well as on ingest: this is the funnel every rendered
    // name passes through, so a collector that forgets cannot corrupt a frame.
    let base = if last.is_empty() { trimmed } else { last };
    sanitize_text(base).into_owned()
}

pub fn is_protected_name(command: &str) -> bool {
    PROTECTED_NAMES.contains(&process_name(command).as_str())
}

pub struct GuardContext {
    /// Our own PID.
    pub self_pid: i32,
    /// pid -> ppid for the current sample, used to walk the ancestor chain.
    pub parents: HashMap<i32, i32>,
}

/// Walks up from `self_pid`; true when `pid` is us or one of our ancestors.
///
/// The `seen` set is not decoration: a corrupt or racing sample can contain a
/// cycle, and this runs on the path to a destructive action.
pub fn is_self_or_ancestor(pid: i32, ctx: &GuardContext) -> bool {
    if pid == ctx.self_pid {
        return true;
    }
    let mut cur = ctx.self_pid;
    let mut seen: HashSet<i32> = HashSet::new();
    while cur > 1 && !seen.contains(&cur) {
        seen.insert(cur);
        let Some(&parent) = ctx.parents.get(&cur) else {
            return false;
        };
        if parent == pid {
            return true;
        }
        cur = parent;
    }
    false
}

const BRAND: &str = "useful-system-monitor";

/// The single entry point every kill must pass through.
///
/// `live` is the identity observed at signal time. Binding the target to
/// `(pid, start_time)` is what stops a kill landing on an unrelated process
/// that inherited a recycled PID between selection and confirmation. See I-16.
/// Pass `None` to run only the static checks — that is what the confirmation
/// panel renders, before the user has committed to anything.
pub fn check_kill(
    target: &ProcessSample,
    ctx: &GuardContext,
    live: Option<LiveIdentity>,
) -> KillCheck {
    // Order matters: every static reason to refuse is checked before the live
    // identity, so the confirmation panel and the signal path agree on which
    // reason a user is shown.
    let pid = match static_checks(target, ctx) {
        Ok(pid) => pid,
        Err(refusal) => return KillCheck::Refused(refusal),
    };
    if let Some(live) = live {
        if let Some(refusal) = identity_check(target, live) {
            return KillCheck::Refused(refusal);
        }
    }
    KillCheck::Allowed(pid)
}

fn refusal(kind: RefusalKind, message: String) -> KillRefusal {
    KillRefusal { kind, message }
}

/// Everything that can be decided from the sample alone.
fn static_checks(target: &ProcessSample, ctx: &GuardContext) -> Result<TargetPid, KillRefusal> {
    /*
     * I-13 depends on the parent map, so an empty one is not "no ancestors" —
     * it is "no answer". The map comes from the process sample, which goes away
     * whenever that collector errors (a `ps` timeout on a loaded machine is
     * enough), and the confirmation panel stays open across that. Refusing is
     * the only safe reading: the alternative silently drops the guard that
     * stops a user killing the shell they are sitting in.
     */
    if ctx.parents.is_empty() {
        return Err(refusal(
            RefusalKind::Unverifiable,
            format!(
                "No current process list, so this cannot be checked against the ancestors of {BRAND}. Press r to refresh."
            ),
        ));
    }

    let Some(pid) = TargetPid::new(target.pid) else {
        return Err(refusal(
            RefusalKind::Init,
            format!(
                "PID {} is the init process — killing it would halt the system.",
                target.pid
            ),
        ));
    };

    if let Some(refused) = lineage_checks(target, ctx) {
        return Err(refused);
    }
    Ok(pid)
}

/// The three "this would take your session with it" refusals. See I-13, I-14.
fn lineage_checks(target: &ProcessSample, ctx: &GuardContext) -> Option<KillRefusal> {
    if target.pid == ctx.self_pid {
        return Some(refusal(
            RefusalKind::SelfProcess,
            format!("That is {BRAND} itself. Press q to quit instead."),
        ));
    }
    if is_self_or_ancestor(target.pid, ctx) {
        return Some(refusal(
            RefusalKind::Ancestor,
            format!(
                "PID {} is a parent of {BRAND} — killing it would take this session down.",
                target.pid
            ),
        ));
    }
    if is_protected_name(&target.command) {
        return Some(refusal(
            RefusalKind::Protected,
            format!(
                "{} is a critical system process — killing it would log you out or wedge the UI.",
                process_name(&target.command)
            ),
        ));
    }
    None
}

/// The identity read at signal time, against the one that was selected.
fn identity_check(target: &ProcessSample, live: LiveIdentity) -> Option<KillRefusal> {
    match live {
        LiveIdentity::Gone => Some(refusal(
            RefusalKind::Recycled,
            format!("PID {} has already exited — nothing to signal.", target.pid),
        )),
        LiveIdentity::Unverifiable => Some(refusal(
            RefusalKind::Unverifiable,
            format!(
                "Could not confirm that PID {} is still {}. Refusing rather than signalling a process it cannot identify.",
                target.pid,
                process_name(&target.command)
            ),
        )),
        // An unreadable start time on either side is unknown, not a value: two
        // unreadable processes must never compare equal. `same_process_as` is
        // the only comparison that gets this right.
        LiveIdentity::Known(live_start) => (!live_start.same_process_as(target.start_time)).then(
            || {
                refusal(
                    RefusalKind::Recycled,
                    format!(
                        "PID {} is no longer the process you selected — it has been reused. Aborted.",
                        target.pid
                    ),
                )
            },
        ),
    }
}
