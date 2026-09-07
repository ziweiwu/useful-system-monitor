//! The kill contract, ported from `test/guards.test.ts` and `signal.test.ts`.
//!
//! Every past bug on this path was **a distinction being collapsed** — "gone"
//! read as "could not check", an unreadable start time read as the epoch, an
//! empty parent map read as "no ancestors". A port is exactly where distinctions
//! collapse, so these land before any UI exists and the types are built so the
//! collapses cannot be written.

use std::collections::HashMap;

use proptest::prelude::*;
use sysmon_core::domain::types::{ProcessSample, StartTime};
use sysmon_core::kill::guards::{
    check_kill, is_self_or_ancestor, process_name, GuardContext, LiveIdentity, RefusalKind,
    TargetPid,
};
use sysmon_core::kill::signal::{
    send_signal, KillErrno, KillOutcome, Killer, Signal, SignalRequest,
};

/// Records every call, so "no signal was sent" is an assertion rather than a
/// hope. The whole point of the refusal paths is that this stays empty.
#[derive(Default)]
struct SpyKiller {
    calls: Vec<(i32, Signal)>,
    result: Option<KillErrno>,
}

impl SpyKiller {
    fn failing(errno: KillErrno) -> Self {
        Self {
            calls: Vec::new(),
            result: Some(errno),
        }
    }
}

impl Killer for SpyKiller {
    fn kill(&mut self, pid: TargetPid, signal: Signal) -> Result<(), KillErrno> {
        self.calls.push((pid.get(), signal));
        match &self.result {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

fn sample(pid: i32) -> ProcessSample {
    ProcessSample {
        pid,
        ppid: 1,
        start_time: StartTime::Known(1_000),
        command: format!("/usr/bin/proc{pid}"),
        user: "ziweiwu".into(),
        state: "S".into(),
        cpu_percent: Some(0.0),
        rss_bytes: 1024,
        energy: Some(0.0),
        protected: false,
    }
}

fn named(pid: i32, command: &str) -> ProcessSample {
    ProcessSample {
        command: command.to_string(),
        ..sample(pid)
    }
}

/// A non-empty parent map by default: an empty one means "no process sample"
/// and is refused outright, which would mask every other rule under test.
fn ctx(self_pid: i32, parents: &[(i32, i32)]) -> GuardContext {
    let map: HashMap<i32, i32> = if parents.is_empty() {
        [(self_pid, 1)].into()
    } else {
        parents.iter().copied().collect()
    };
    GuardContext {
        self_pid,
        parents: map,
    }
}

fn refusal_kind(
    target: &ProcessSample,
    ctx: &GuardContext,
    live: Option<LiveIdentity>,
) -> RefusalKind {
    check_kill(target, ctx, live)
        .refusal()
        .expect("should have refused")
        .kind
}

// ------------------------------------------------------------ process name --

/// The bug this pins: splitting on whitespace first turned
/// "/Library/Application Support/…" into "Application".
#[test]
fn process_name_handles_macos_paths_containing_spaces() {
    assert_eq!(
        process_name("/Library/Application Support/Logitech/logioptionsplus_agent"),
        "logioptionsplus_agent"
    );
    assert_eq!(
        process_name("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome Helper"),
        "Google Chrome Helper"
    );
}

/// A hostile name reaches this funnel even if a collector forgot to sanitise.
#[test]
fn process_name_neutralises_control_bytes() {
    assert_eq!(process_name("/bin/malware\rSafari"), "malware·Safari");
}

// -------------------------------------------- I-12: never signal PID <= 1 --

#[test]
fn i12_refuses_pid_zero_and_one() {
    for pid in [0, 1] {
        assert_eq!(
            refusal_kind(&sample(pid), &ctx(999, &[]), None),
            RefusalKind::Init,
            "pid {pid}"
        );
    }
}

/// `kill(0, …)` signals the caller's whole process group and `kill(-n, …)`
/// signals group `n`. Neither has a `TargetPid`, so neither can be written.
#[test]
fn i12_no_target_pid_exists_for_a_group_signal() {
    for pid in [i32::MIN, -1, 0, 1] {
        assert!(
            !check_kill(&sample(pid), &ctx(999, &[]), None).is_allowed(),
            "pid {pid} must never be allowed"
        );
    }
}

// ------------------------------- I-13: never signal ourselves or an ancestor --

#[test]
fn i13_refuses_our_own_pid() {
    assert_eq!(
        refusal_kind(&sample(500), &ctx(500, &[]), None),
        RefusalKind::SelfProcess
    );
}

#[test]
fn i13_refuses_a_direct_parent() {
    assert_eq!(
        refusal_kind(&sample(400), &ctx(500, &[(500, 400)]), None),
        RefusalKind::Ancestor
    );
}

#[test]
fn i13_refuses_a_grandparent_walking_the_whole_chain() {
    assert_eq!(
        refusal_kind(&sample(300), &ctx(500, &[(500, 400), (400, 300)]), None),
        RefusalKind::Ancestor
    );
}

#[test]
fn i13_allows_an_unrelated_process() {
    assert!(check_kill(&sample(700), &ctx(500, &[(500, 400)]), None).is_allowed());
}

/// A racing or corrupt sample can contain a cycle, and this runs on the path to
/// a destructive action.
#[test]
fn i13_terminates_on_a_cyclic_parent_map_instead_of_hanging() {
    assert!(!is_self_or_ancestor(
        999,
        &ctx(500, &[(500, 400), (400, 500)])
    ));
}

/// The parent map comes from the process sample, which is gone whenever that
/// collector errors — a `ps` timeout on a loaded machine is enough. The
/// confirmation panel stays open across that, so the ancestor walk silently
/// returned false for everything and the rule that stops you killing your own
/// shell stopped applying.
#[test]
fn i13_fails_closed_when_there_is_no_process_sample() {
    let empty = GuardContext {
        self_pid: 999,
        parents: HashMap::new(),
    };
    assert_eq!(
        refusal_kind(&sample(700), &empty, None),
        RefusalKind::Unverifiable
    );

    let mut spy = SpyKiller::default();
    let out = send_signal(
        SignalRequest {
            target: &sample(700),
            signal: Signal::Kill,
            ctx: &empty,
            live: LiveIdentity::Known(StartTime::Known(1_000)),
        },
        &mut spy,
    );
    assert!(matches!(out, KillOutcome::Refused(_)));
    assert!(spy.calls.is_empty(), "a refusal must emit no signal");
}

// ------------------------- I-14: critical system processes are refused --

#[test]
fn i14_refuses_critical_system_processes() {
    for command in [
        "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer",
        "/sbin/launchd",
        "/System/Library/CoreServices/loginwindow.app/Contents/MacOS/loginwindow",
    ] {
        assert_eq!(
            refusal_kind(&named(616, command), &ctx(999, &[]), None),
            RefusalKind::Protected,
            "{command}"
        );
    }
}

#[test]
fn i14_explains_the_consequence_rather_than_just_saying_no() {
    let c = ctx(999, &[]);
    let check = check_kill(&named(616, "/usr/bin/WindowServer"), &c, None);
    let msg = &check.refusal().expect("refused").message;
    let lower = msg.to_lowercase();
    assert!(
        lower.contains("log you out") || lower.contains("wedge"),
        "{msg}"
    );
}

// ----------------------------------------- I-16: PID reuse aborts the kill --

#[test]
fn i16_refuses_when_the_live_start_time_no_longer_matches() {
    assert_eq!(
        refusal_kind(
            &sample(700),
            &ctx(999, &[]),
            Some(LiveIdentity::Known(StartTime::Known(2_000)))
        ),
        RefusalKind::Recycled
    );
}

#[test]
fn i16_allows_when_the_start_time_still_matches() {
    assert!(check_kill(
        &sample(700),
        &ctx(999, &[]),
        Some(LiveIdentity::Known(StartTime::Known(1_000)))
    )
    .is_allowed());
}

#[test]
fn i16_sends_no_signal_at_all_when_the_pid_was_recycled() {
    let mut spy = SpyKiller::default();
    let out = send_signal(
        SignalRequest {
            target: &sample(700),
            signal: Signal::Kill,
            ctx: &ctx(999, &[]),
            live: LiveIdentity::Known(StartTime::Known(2_000)),
        },
        &mut spy,
    );
    assert!(matches!(out, KillOutcome::Refused(_)));
    assert!(spy.calls.is_empty());
}

/// The hole this closes: "the caller looked and found nothing" and "the caller
/// did not look" used to be the same value, and the check was skipped for both.
/// A recycled PID belongs to a brand-new process with no CPU history and a small
/// RSS, so it is almost never in the working set the caller searched — the guard
/// did nothing in exactly the case it exists for.
#[test]
fn i16_refuses_rather_than_signals_when_the_identity_could_not_be_read() {
    let mut spy = SpyKiller::default();
    let out = send_signal(
        SignalRequest {
            target: &sample(700),
            signal: Signal::Kill,
            ctx: &ctx(999, &[]),
            live: LiveIdentity::Unverifiable,
        },
        &mut spy,
    );
    match out {
        KillOutcome::Refused(r) => assert_eq!(r.kind, RefusalKind::Unverifiable),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(spy.calls.is_empty());
}

/// "Gone" and "could not check" must not be spelled the same way: one told users
/// a very much alive process had already exited.
#[test]
fn i16_sends_nothing_when_the_process_has_already_exited() {
    let mut spy = SpyKiller::default();
    let out = send_signal(
        SignalRequest {
            target: &sample(700),
            signal: Signal::Term,
            ctx: &ctx(999, &[]),
            live: LiveIdentity::Gone,
        },
        &mut spy,
    );
    match out {
        KillOutcome::Refused(r) => assert_eq!(r.kind, RefusalKind::Recycled),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(spy.calls.is_empty());
}

/// An unreadable date is unknown, not the epoch. Treating it as a value would
/// make every unreadable process compare equal to every other one.
#[test]
fn i16_treats_an_unknown_start_time_as_unverifiable_not_as_epoch() {
    let unreadable = ProcessSample {
        start_time: StartTime::Unreadable,
        ..sample(700)
    };
    let mut spy = SpyKiller::default();
    let out = send_signal(
        SignalRequest {
            target: &unreadable,
            signal: Signal::Kill,
            ctx: &ctx(999, &[]),
            live: LiveIdentity::Known(StartTime::Unreadable),
        },
        &mut spy,
    );
    assert!(matches!(out, KillOutcome::Refused(_)));
    assert!(spy.calls.is_empty());
    // And in each direction independently.
    assert!(!check_kill(
        &unreadable,
        &ctx(999, &[]),
        Some(LiveIdentity::Known(StartTime::Known(1_000)))
    )
    .is_allowed());
    assert!(!check_kill(
        &sample(700),
        &ctx(999, &[]),
        Some(LiveIdentity::Known(StartTime::Unreadable))
    )
    .is_allowed());
}

// ------------------------------------------- I-15 / I-17: signal delivery --

#[test]
fn i15_sends_the_requested_signal_when_allowed() {
    let mut spy = SpyKiller::default();
    let out = send_signal(
        SignalRequest {
            target: &sample(700),
            signal: Signal::Term,
            ctx: &ctx(999, &[]),
            live: LiveIdentity::Known(StartTime::Known(1_000)),
        },
        &mut spy,
    );
    assert!(matches!(out, KillOutcome::Sent { .. }));
    assert_eq!(spy.calls, vec![(700, Signal::Term)]);
}

/// I-17: the process already exited. That is the outcome we wanted.
#[test]
fn i17_treats_esrch_as_success() {
    let mut spy = SpyKiller::failing(KillErrno::Esrch);
    let out = send_signal(
        SignalRequest {
            target: &sample(700),
            signal: Signal::Term,
            ctx: &ctx(999, &[]),
            live: LiveIdentity::Known(StartTime::Known(1_000)),
        },
        &mut spy,
    );
    match out {
        KillOutcome::Sent { note } => assert!(note.expect("a note").contains("already exited")),
        other => panic!("expected success, got {other:?}"),
    }
}

#[test]
fn i17_surfaces_eperm_with_a_remedy_and_never_escalates_on_its_own() {
    let mut spy = SpyKiller::failing(KillErrno::Eperm);
    let out = send_signal(
        SignalRequest {
            target: &sample(700),
            signal: Signal::Kill,
            ctx: &ctx(999, &[]),
            live: LiveIdentity::Known(StartTime::Known(1_000)),
        },
        &mut spy,
    );
    match out {
        KillOutcome::Failed(msg) => assert!(msg.contains("sudo"), "{msg}"),
        other => panic!("expected a failure, got {other:?}"),
    }
    assert_eq!(
        spy.calls.len(),
        1,
        "exactly one attempt, never a retry as root"
    );
}

#[test]
fn i17_reports_unexpected_errors_rather_than_swallowing_them() {
    let mut spy = SpyKiller::failing(KillErrno::Other("boom".into()));
    let out = send_signal(
        SignalRequest {
            target: &sample(700),
            signal: Signal::Term,
            ctx: &ctx(999, &[]),
            live: LiveIdentity::Known(StartTime::Known(1_000)),
        },
        &mut spy,
    );
    assert_eq!(out, KillOutcome::Failed("boom".into()));
}

// --------------------------------------- every refusal path emits nothing --

#[test]
fn every_refusal_path_emits_no_signal() {
    let cases: Vec<(&str, ProcessSample)> = vec![
        ("init", sample(1)),
        ("self", sample(999)),
        ("protected", named(616, "/usr/bin/WindowServer")),
    ];
    for (label, target) in cases {
        let mut spy = SpyKiller::default();
        let out = send_signal(
            SignalRequest {
                target: &target,
                signal: Signal::Term,
                ctx: &ctx(999, &[]),
                live: LiveIdentity::Known(target.start_time),
            },
            &mut spy,
        );
        assert!(matches!(out, KillOutcome::Refused(_)), "{label}");
        assert!(spy.calls.is_empty(), "{label} emitted a signal");
    }
}

proptest! {
    /// The invariant behind all of the above, over the whole input space: a
    /// refusal never reaches the syscall, and an allowed kill always does.
    #[test]
    fn a_refusal_never_reaches_the_syscall(
        pids in (-5i32..1000, 1i32..1000, 1i32..1000),
        matching in any::<bool>(),
        protected in any::<bool>(),
    ) {
        let (pid, self_pid, parent) = pids;
        let command = if protected { "/usr/bin/WindowServer".to_string() }
                      else { format!("/usr/bin/proc{pid}") };
        let target = ProcessSample { command, ..sample(pid) };
        let live = LiveIdentity::Known(StartTime::Known(if matching { 1_000 } else { 2_000 }));
        let c = ctx(self_pid, &[(self_pid, parent)]);

        let mut spy = SpyKiller::default();
        let out = send_signal(
            SignalRequest {
                target: &target,
                signal: Signal::Term,
                ctx: &c,
                live,
            },
            &mut spy,
        );
        match out {
            KillOutcome::Refused(_) => prop_assert!(spy.calls.is_empty()),
            KillOutcome::Sent { .. } => {
                prop_assert_eq!(spy.calls.len(), 1);
                // Whatever else was true, the syscall saw a safe pid.
                prop_assert!(spy.calls[0].0 > 1);
            }
            KillOutcome::Failed(_) => prop_assert_eq!(spy.calls.len(), 1),
        }
    }
}
