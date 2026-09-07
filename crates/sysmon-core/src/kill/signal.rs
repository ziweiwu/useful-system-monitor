//! The only place a signal is sent. See I-15, I-17.

use crate::domain::types::ProcessSample;
use crate::kill::guards::{
    check_kill, GuardContext, KillCheck, KillRefusal, LiveIdentity, TargetPid,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
}

impl Signal {
    pub fn name(self) -> &'static str {
        match self {
            Self::Term => "SIGTERM",
            Self::Kill => "SIGKILL",
        }
    }
}

/// What `kill(2)` reported.
///
/// Kept as three cases rather than a string, because two of them are *answers*
/// and only the third is a failure. Flattening them into one error type is how
/// a live process came to be reported as already exited. The platform mapping
/// lives in the binary crate; this crate stays free of libc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillErrno {
    /// No such process.
    Esrch,
    /// Not permitted.
    Eperm,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillOutcome {
    /// The signal was sent, or the process was already gone — both are success.
    Sent {
        note: Option<String>,
    },
    Refused(KillRefusal),
    Failed(String),
}

/// Sends a signal to a PID that has already passed the guards.
///
/// Taking [`TargetPid`] rather than `i32` is the point: an implementation
/// cannot be handed 0 (which signals the whole process group) or a negative
/// (which signals another group), because those values have no `TargetPid`.
pub trait Killer {
    fn kill(&mut self, pid: TargetPid, signal: Signal) -> Result<(), KillErrno>;
}

impl<F> Killer for F
where
    F: FnMut(TargetPid, Signal) -> Result<(), KillErrno>,
{
    fn kill(&mut self, pid: TargetPid, signal: Signal) -> Result<(), KillErrno> {
        self(pid, signal)
    }
}

const BRAND: &str = "useful-system-monitor";

/// Sends a signal, but only after [`check_kill`] passes.
///
/// `live` is required and has no "not checked" value on purpose: this is the
/// only place a signal is actually sent, so it is the one place where skipping
/// the PID-reuse check has consequences. A caller with nothing to offer passes
/// [`LiveIdentity::Unverifiable`] and is refused. See I-16.
/// One signal, and everything the guards need to decide whether it may be sent.
pub struct SignalRequest<'a> {
    pub target: &'a ProcessSample,
    pub signal: Signal,
    pub ctx: &'a GuardContext,
    /// The identity read at signal time. No "not checked" value on purpose —
    /// see the note above.
    pub live: LiveIdentity,
}

pub fn send_signal(request: SignalRequest, killer: &mut impl Killer) -> KillOutcome {
    let SignalRequest {
        target,
        signal,
        ctx,
        live,
    } = request;
    let pid = match check_kill(target, ctx, Some(live)) {
        KillCheck::Refused(r) => return KillOutcome::Refused(r),
        KillCheck::Allowed(pid) => pid,
    };

    match killer.kill(pid, signal) {
        Ok(()) => KillOutcome::Sent { note: None },
        // I-17: the process already exited. That is the outcome we wanted.
        Err(KillErrno::Esrch) => {
            KillOutcome::Sent { note: Some("Process had already exited.".to_string()) }
        }
        // I-17: surface the remedy, never silently escalate to sudo.
        Err(KillErrno::Eperm) => KillOutcome::Failed(format!(
            "Not permitted to signal PID {pid}. It belongs to another user — rerun {BRAND} with sudo if you are sure."
        )),
        Err(KillErrno::Other(msg)) => KillOutcome::Failed(msg),
    }
}
