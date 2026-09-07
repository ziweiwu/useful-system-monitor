//! The keymap, as a pure function.
//!
//! Nothing here does I/O. Everything a key can cause leaves as an [`Effect`]
//! for the caller to perform, which is what lets the fuzzer drive millions of
//! events per second with no terminal and no processes anywhere near it.

use crate::app::state::{Mode, Toast, UiState};
use crate::domain::scoring::SortKey;
use crate::domain::types::ProcessSample;
use crate::domain::views::View;
use crate::domain::working_set::WORKING_SET_STEPS;
use crate::kill::signal::Signal;

/// One key, normalised away from any particular terminal library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    CtrlC,
}

/// A key, and which input burst it arrived in.
///
/// The batch is not bookkeeping: it is the whole anti-paste guard. See
/// [`reduce`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub batch: u64,
}

/// Something the caller must do. The reducer never does it itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Quit,
    /// Force a sample of every collector now, ahead of its tier.
    RefreshAll,
    /// Only the process collector — see the note on the working-set keys.
    RefreshProcesses,
    /// Read the target's identity **at signal time** and then signal it. The
    /// reducer deliberately does not decide whether this is allowed; that is
    /// `send_signal`'s job, and it is the only place that can be sure. See I-16.
    RequestKill {
        pid: i32,
        signal: Signal,
    },
    /// The detail panel's argv line, fetched on demand.
    FetchCommandLine(i32),
}

/// What the reducer needs to know about the frame that is currently on screen.
pub struct InputContext<'a> {
    /// Below the minimum size the app draws nothing but its own complaint.
    pub too_small: bool,
    /// The rows as the user sees them: filtered and sorted.
    pub filtered: &'a [ProcessSample],
    /// Whether the guards would allow the current kill target. Signal keys are
    /// inert on a refused target; escape is the only way out.
    pub kill_allowed: bool,
    /// Whether the **last drawn frame** actually contained the kill
    /// confirmation. See the fourth guard in [`reduce`].
    pub kill_modal_drawn: bool,
    /// Wall clock, for toast expiry.
    pub now_ms: i64,
}

/// Apply one key.
///
/// # The hazard this handles, which Rust makes *worse*
///
/// Ink hands `useInput` a whole burst as one string, so a pasted `"kk"` arrives
/// as `input === "kk"`, which matches no binding and does nothing. The
/// TypeScript build's second-press rule was therefore protected **by accident**
/// — by its input layer coalescing, not by any rule it states.
///
/// crossterm delivers discrete key events. A correct synchronous reducer would
/// therefore see two `k`s, arm SIGKILL and fire it, from one paste. So the
/// protection has to be made explicit, and it is, in three layers:
///
/// 1. Bracketed paste, handled by the caller: pasted text arrives as a distinct
///    event and is only accepted in filter mode.
/// 2. **Batch identity** — a confirm key is refused when it arrived in the same
///    burst that opened the mode. Timing-free, so the fuzzer can drive it
///    deterministically with no sleeps.
/// 3. **The plan-gated confirm** — the caller passes `kill_modal_drawn`, which
///    is true only if the confirmation was in the *last frame actually drawn*.
///    This is I-15 ("a kill is confirmed by name, on screen") expressed
///    mechanically rather than by remembering to check the terminal size, and
///    it subsumes the 49-column bug, the drag-narrow-then-widen case, and any
///    size class nobody has thought of yet.
pub fn reduce(state: &UiState, ev: KeyEvent, ctx: &InputContext<'_>) -> (UiState, Vec<Effect>) {
    let mut s = state.clone();
    let mut fx = Vec::new();

    // Toasts age off the newest sample rather than their own timer, for the
    // same reason the clock does: a render costs more than any collector.
    if s.toast
        .as_ref()
        .is_some_and(|t| ctx.now_ms >= t.expires_at_ms)
    {
        s.toast = None;
    }

    /*
     * At this size the app draws nothing but its own size complaint, so every
     * key would act on a screen that is not there.
     *
     * `k` was the one that mattered: the kill confirmation is a *mode*, and a
     * mode that is not rendered is still entered. On a 49-column terminal `k`
     * then `t` sent SIGTERM with the process name, the "unsaved work will be
     * lost" warning and the whole confirmation unrendered. The layout sweep
     * drove exactly this path and passed, because it only asserted that the
     * frame did not overflow.
     *
     * So the only bindings left are the two that get you out: quit, and an
     * escape that clears any mode inherited from a resize, so growing the
     * terminal back cannot reveal a confirmation the user has forgotten arming.
     */
    if ctx.too_small {
        match ev.key {
            Key::Char('q') | Key::CtrlC => fx.push(Effect::Quit),
            Key::Escape => s.set_mode(Mode::Normal, ev.batch),
            _ => {}
        }
        return (s, fx);
    }

    match s.mode {
        Mode::Filter => {
            match ev.key {
                Key::Escape => {
                    s.set_mode(Mode::Normal, ev.batch);
                    s.filter.clear();
                }
                Key::Enter => s.set_mode(Mode::Normal, ev.batch),
                Key::Backspace => {
                    s.filter.pop();
                }
                // Printable characters only; a control key is not filter text.
                Key::Char(c) if !c.is_control() => s.filter.push(c),
                _ => {}
            }
            return (s, fx);
        }

        Mode::Kill(pid) => {
            if ev.key == Key::Escape {
                s.set_mode(Mode::Normal, ev.batch);
                return (s, fx);
            }
            // Signal keys are inert on a refused target, and inert on a
            // confirmation that is not actually on screen.
            if !ctx.kill_allowed || !ctx.kill_modal_drawn {
                return (s, fx);
            }
            // Layer 2: the key must come from a later burst than the one that
            // opened the mode.
            let same_burst = ev.batch <= s.mode_entered_batch;
            if same_burst {
                return (s, fx);
            }
            match ev.key {
                Key::Char('t') => {
                    s.last_action_batch = ev.batch;
                    fx.push(Effect::RequestKill {
                        pid,
                        signal: Signal::Term,
                    });
                }
                Key::Char('k') => {
                    if s.armed_kill && ev.batch > s.last_action_batch {
                        // I-15: the second, distinct press.
                        fx.push(Effect::RequestKill {
                            pid,
                            signal: Signal::Kill,
                        });
                    } else {
                        s.armed_kill = true;
                        s.last_action_batch = ev.batch;
                    }
                }
                _ => {}
            }
            return (s, fx);
        }

        Mode::Detail(pid) => {
            match ev.key {
                Key::Escape | Key::Enter => s.set_mode(Mode::Normal, ev.batch),
                Key::Char('k') => {
                    if ctx.filtered.iter().any(|p| p.pid == pid) {
                        s.set_mode(Mode::Kill(pid), ev.batch);
                    }
                }
                Key::Char('q') | Key::CtrlC => fx.push(Effect::Quit),
                _ => {}
            }
            return (s, fx);
        }

        Mode::Normal => {}
    }

    match ev.key {
        Key::Char('q') | Key::CtrlC => fx.push(Effect::Quit),
        // Left/right walk the tab strip and wrap at both ends (I-27). They are
        // free to take: up/down own the row cursor, and nothing on any screen
        // is horizontally scrollable.
        Key::Left => s.view = s.view.step(-1),
        Key::Right => s.view = s.view.step(1),
        Key::Up => move_selection(&mut s, ctx.filtered, -1),
        Key::Down => move_selection(&mut s, ctx.filtered, 1),
        Key::PageUp => move_selection(&mut s, ctx.filtered, -10),
        Key::PageDown => move_selection(&mut s, ctx.filtered, 10),
        Key::Enter => {
            if s.row_actions() {
                if let Some(pid) = s.selected_pid {
                    s.set_mode(Mode::Detail(pid), ev.batch);
                    fx.push(Effect::FetchCommandLine(pid));
                }
            }
        }
        Key::Char(c) => match c {
            // Arrows move; `k` is reserved for kill. Binding it to vim-up as
            // well would make the single most destructive action ambiguous.
            'k' => {
                /* Only where the screen shows which process this would act on. */
                if s.row_actions() {
                    if let Some(pid) = s
                        .selected_pid
                        .filter(|pid| ctx.filtered.iter().any(|p| p.pid == *pid))
                    {
                        s.set_mode(Mode::Kill(pid), ev.batch);
                    }
                }
            }
            'c' => s.sort_key = SortKey::Cpu,
            'm' => s.sort_key = SortKey::Mem,
            'e' => s.sort_key = SortKey::Energy,
            'r' => fx.push(Effect::RefreshAll),
            '/' => s.set_mode(Mode::Filter, ev.batch),
            // `=` is the same physical key as `+`, so expanding does not need
            // shift. A new cap is only visible after a resample, and waiting out
            // a 10s tier for a keypress reads as a dead key — but only the
            // process collector is disturbed, because rebuilding every tier
            // would shorten a delta window and print a bogus CPU reading.
            '+' | '=' => {
                if s.ws_step + 1 < WORKING_SET_STEPS.len() {
                    s.ws_step += 1;
                    fx.push(Effect::RefreshProcesses);
                }
            }
            '-' | '_' => {
                if s.ws_step > 0 {
                    s.ws_step -= 1;
                    fx.push(Effect::RefreshProcesses);
                }
            }
            digit if digit.is_ascii_digit() => {
                if let Some(v) = View::from_key(digit) {
                    s.view = v;
                }
            }
            _ => {}
        },
        _ => {}
    }

    (s, fx)
}

/// I-21: selection is keyed by PID, so a re-sort cannot move the kill target.
fn move_selection(ui: &mut UiState, filtered: &[ProcessSample], delta: i32) {
    if filtered.is_empty() {
        return;
    }
    let idx = ui
        .selected_pid
        .and_then(|pid| filtered.iter().position(|p| p.pid == pid))
        .unwrap_or(0) as i32;
    let next = (idx + delta).clamp(0, filtered.len() as i32 - 1) as usize;
    ui.selected_pid = Some(filtered[next].pid);
}

/// Keep the selection on a row that still exists, without moving it when the
/// list merely re-sorts underneath. Called once per frame, before layout.
pub fn reconcile_selection(ui: &mut UiState, filtered: &[ProcessSample]) {
    if filtered.is_empty() {
        return;
    }
    let gone = ui
        .selected_pid
        .is_none_or(|pid| !filtered.iter().any(|p| p.pid == pid));
    if gone {
        ui.selected_pid = Some(filtered[0].pid);
    }
}

/// The process being detailed or confirmed can exit, or drop out of the working
/// set, while its mode is open. Falling back to the table beats rendering an
/// empty screen.
pub fn reconcile_mode(ui: &mut UiState, filtered: &[ProcessSample]) {
    let pid = match ui.mode {
        Mode::Detail(pid) | Mode::Kill(pid) => pid,
        _ => return,
    };
    if !filtered.iter().any(|p| p.pid == pid) {
        ui.mode = Mode::Normal;
        ui.armed_kill = false;
    }
}

/// A message under the header, for four seconds.
/// How long a notice stays on screen. Long enough to read a kill result,
/// short enough that it is gone before the next one.
const TOAST_LIFETIME_MS: i64 = 4_000;

/// How a notice reads. Not a severity scale — the UI only distinguishes "this
/// went wrong" from "this happened", and each gets its own colour. See I-23.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Bad,
}

pub fn toast(ui: &mut UiState, text: impl Into<String>, tone: Tone, now_ms: i64) {
    ui.toast = Some(Toast {
        text: text.into(),
        bad: tone == Tone::Bad,
        expires_at_ms: now_ms + TOAST_LIFETIME_MS,
    });
}
