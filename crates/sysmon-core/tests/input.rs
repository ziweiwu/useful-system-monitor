//! The keymap contract, ported from `test/ui.test.tsx`, `selection.test.tsx`
//! and `input-batching.test.tsx`.
//!
//! Those are the twelve Ink-mounting test files the plan expects to lose in a
//! rewrite. The *scenarios* they encode are the reusable part, and they are
//! reproduced here against a pure reducer — which runs in microseconds instead
//! of hundreds of milliseconds, and needs no terminal, no timers and no mock
//! renderer to say the same thing.

use proptest::prelude::*;
use sysmon_core::app::input::{
    reconcile_mode, reconcile_selection, reduce, Effect, InputContext, Key, KeyEvent,
};
use sysmon_core::app::state::{Mode, UiState};
use sysmon_core::domain::scoring::SortKey;
use sysmon_core::domain::types::{ProcessSample, StartTime};
use sysmon_core::domain::views::View;
use sysmon_core::kill::signal::Signal;

fn sample(pid: i32) -> ProcessSample {
    ProcessSample {
        pid,
        ppid: 1,
        start_time: StartTime::Known(1_000),
        command: format!("/usr/bin/proc{pid}"),
        user: "ziweiwu".into(),
        state: "S".into(),
        cpu_percent: Some(1.0),
        rss_bytes: 1024,
        energy: Some(1.0),
        protected: false,
    }
}

fn rows() -> Vec<ProcessSample> {
    (1..=5).map(|i| sample(i * 100)).collect()
}

struct Harness {
    state: UiState,
    rows: Vec<ProcessSample>,
    batch: u64,
    too_small: bool,
    kill_allowed: bool,
    kill_modal_drawn: bool,
    effects: Vec<Effect>,
}

impl Harness {
    fn new() -> Self {
        let rows = rows();
        let state = UiState {
            selected_pid: Some(rows[0].pid),
            ..UiState::default()
        };
        Self {
            state,
            rows,
            batch: 0,
            too_small: false,
            kill_allowed: true,
            kill_modal_drawn: true,
            effects: Vec::new(),
        }
    }

    fn ctx(&self) -> InputContext<'_> {
        InputContext {
            too_small: self.too_small,
            filtered: &self.rows,
            kill_allowed: self.kill_allowed,
            kill_modal_drawn: self.kill_modal_drawn,
            now_ms: 0,
        }
    }

    /// One key, in its own burst — a deliberate human keystroke.
    fn press(&mut self, key: Key) -> &mut Self {
        self.batch += 1;
        let (s, fx) = reduce(
            &self.state,
            KeyEvent {
                key,
                batch: self.batch,
            },
            &self.ctx(),
        );
        self.state = s;
        self.effects.extend(fx);
        self
    }

    fn char_key(&mut self, key: char) -> &mut Self {
        self.press(Key::Char(key))
    }

    /// A whole burst delivered at once — a paste, or a fast typist. Every key
    /// carries the *same* batch id, which is what the guard reads.
    fn paste(&mut self, text: &str) -> &mut Self {
        self.batch += 1;
        let batch = self.batch;
        for c in text.chars() {
            let (s, fx) = reduce(
                &self.state,
                KeyEvent {
                    key: Key::Char(c),
                    batch,
                },
                &self.ctx(),
            );
            self.state = s;
            self.effects.extend(fx);
        }
        self
    }

    fn kills(&self) -> Vec<(i32, Signal)> {
        self.effects
            .iter()
            .filter_map(|e| match e {
                Effect::RequestKill { pid, signal } => Some((*pid, *signal)),
                _ => None,
            })
            .collect()
    }
}

// ------------------------------------------------ I-27: view navigation --

#[test]
fn i27_number_keys_and_arrows_reach_every_screen() {
    let mut h = Harness::new();
    h.char_key('3');
    assert_eq!(h.state.view, View::Memory);
    h.press(Key::Right);
    assert_eq!(h.state.view, View::Battery);
    h.press(Key::Left).press(Key::Left);
    assert_eq!(h.state.view, View::Cpu);
    // Wraps at both ends, so neither arrow is ever a dead key.
    h.char_key('1').press(Key::Left);
    assert_eq!(h.state.view, View::Disk);
}

// ---------------------------------------------------- I-21: selection --

#[test]
fn i21_selection_is_keyed_by_pid_and_survives_a_resort() {
    let mut h = Harness::new();
    h.press(Key::Down).press(Key::Down);
    let picked = h.state.selected_pid.expect("a selection");
    assert_eq!(picked, 300);
    // The list re-sorts under the cursor; the selection must not move.
    h.rows.reverse();
    reconcile_selection(&mut h.state, &h.rows);
    assert_eq!(h.state.selected_pid, Some(picked));
}

#[test]
fn i21_selection_falls_back_only_when_the_process_is_genuinely_gone() {
    let mut h = Harness::new();
    h.press(Key::Down);
    assert_eq!(h.state.selected_pid, Some(200));
    h.rows.retain(|p| p.pid != 200);
    reconcile_selection(&mut h.state, &h.rows);
    assert_eq!(h.state.selected_pid, Some(100));
}

#[test]
fn selection_movement_clamps_at_both_ends() {
    let mut h = Harness::new();
    for _ in 0..20 {
        h.press(Key::Down);
    }
    assert_eq!(h.state.selected_pid, Some(500));
    h.press(Key::PageUp);
    assert_eq!(h.state.selected_pid, Some(100));
}

// -------------------------------------------------------- filter mode --

/// The bug this pins: `/` set filter mode and the characters after it in the
/// same chunk still saw `false`, falling through to the main keymap — pasting
/// "chrome" silently re-sorted by energy (the `e`), and pasting "book" opened
/// the kill confirmation (the `k`).
#[test]
fn a_pasted_filter_is_filter_text_not_a_burst_of_commands() {
    let mut typed = Harness::new();
    typed.char_key('/');
    for c in "chrome".chars() {
        typed.char_key(c);
    }

    let mut pasted = Harness::new();
    pasted.char_key('/').paste("chrome");

    assert_eq!(typed.state.filter, "chrome");
    assert_eq!(pasted.state.filter, "chrome");
    // The `e` must not have reached the sort key, nor the `k` a kill.
    assert_eq!(pasted.state.sort_key, SortKey::Cpu);
    assert_eq!(pasted.state.mode, Mode::Filter);

    let mut book = Harness::new();
    book.char_key('/').paste("book");
    assert_eq!(book.state.filter, "book");
    assert!(
        book.kills().is_empty(),
        "a pasted filter must not open a kill"
    );
}

#[test]
fn filter_mode_backspaces_commits_and_clears() {
    let mut h = Harness::new();
    h.char_key('/').paste("abc").press(Key::Backspace);
    assert_eq!(h.state.filter, "ab");
    h.press(Key::Enter);
    assert_eq!(h.state.mode, Mode::Normal);
    assert_eq!(h.state.filter, "ab", "enter commits the text");
    h.char_key('/').press(Key::Escape);
    assert_eq!(h.state.filter, "", "escape clears it");
}

// ------------------------------------------------- I-15: the kill path --

#[test]
fn i15_sigkill_needs_a_second_distinct_press() {
    let mut h = Harness::new();
    h.char_key('k');
    assert_eq!(h.state.mode, Mode::Kill(100));
    h.char_key('k');
    assert!(h.state.armed_kill, "the first press arms");
    assert!(h.kills().is_empty(), "and sends nothing");
    h.char_key('k');
    assert_eq!(h.kills(), vec![(100, Signal::Kill)]);
}

#[test]
fn i15_sigterm_takes_one_press_once_the_modal_has_settled() {
    let mut h = Harness::new();
    h.char_key('k').char_key('t');
    assert_eq!(h.kills(), vec![(100, Signal::Term)]);
}

/// **The hazard Rust introduces.** Ink coalesced a burst into one string, so a
/// pasted "kk" matched no binding. crossterm delivers discrete events, so the
/// second-press rule has to be stated rather than inherited.
#[test]
fn i15_a_pasted_burst_can_never_both_open_and_confirm_a_kill() {
    for burst in ["kk", "kt", "kkk", "kkkkkk", "ktk"] {
        let mut h = Harness::new();
        h.paste(burst);
        assert!(
            h.kills().is_empty(),
            "{burst:?} sent {:?} from a single burst",
            h.kills()
        );
    }
}

/// Signal keys are inert on a refused target; escape is the only way out.
#[test]
fn i15_signal_keys_do_nothing_on_a_refused_target() {
    let mut h = Harness::new();
    h.kill_allowed = false;
    h.char_key('k');
    assert_eq!(h.state.mode, Mode::Kill(100));
    h.char_key('t').char_key('k').char_key('k');
    assert!(h.kills().is_empty());
    h.press(Key::Escape);
    assert_eq!(h.state.mode, Mode::Normal);
}

/// I-15 made mechanical: a confirmation that is not on the screen cannot be
/// confirmed, whatever the terminal size or the mode state says.
#[test]
fn i15_a_confirmation_that_was_not_drawn_cannot_be_confirmed() {
    let mut h = Harness::new();
    h.char_key('k');
    h.kill_modal_drawn = false;
    h.char_key('t').char_key('k').char_key('k');
    assert!(
        h.kills().is_empty(),
        "signalled against an undrawn confirmation"
    );
}

/// The 49-column bug: the mode is still *entered* at a size where nothing is
/// drawn, so the keymap has to be inert apart from the two ways out.
#[test]
fn i15_a_terminal_too_small_to_draw_binds_only_quit_and_escape() {
    let mut h = Harness::new();
    h.too_small = true;
    h.char_key('k').char_key('t').char_key('k').char_key('k');
    assert_eq!(
        h.state.mode,
        Mode::Normal,
        "no mode may be entered at this size"
    );
    assert!(h.kills().is_empty());
    assert!(h.effects.is_empty());
    h.char_key('q');
    assert_eq!(h.effects, vec![Effect::Quit]);
}

/// Escape at a too-small size clears a mode inherited from a resize, so growing
/// the terminal back cannot reveal a confirmation the user forgot arming.
#[test]
fn i15_escape_clears_a_mode_inherited_from_a_resize() {
    let mut h = Harness::new();
    h.char_key('k');
    assert_eq!(h.state.mode, Mode::Kill(100));
    h.too_small = true;
    h.press(Key::Escape);
    assert_eq!(h.state.mode, Mode::Normal);
    assert!(!h.state.armed_kill);
}

/// I-26b, and here it needs no test of the render at all: the modes are one
/// enum, so two cannot be open at once.
#[test]
fn i26b_at_most_one_full_screen_mode_is_ever_open() {
    let mut h = Harness::new();
    h.press(Key::Enter);
    assert_eq!(h.state.mode, Mode::Detail(100));
    h.char_key('k');
    assert_eq!(
        h.state.mode,
        Mode::Kill(100),
        "opening the kill closes the detail"
    );
    h.press(Key::Escape);
    assert_eq!(h.state.mode, Mode::Normal);
}

/// `k` and `enter` are bound only where the screen shows which process they
/// would act on. See I-15.
#[test]
fn i15_row_actions_are_unbound_on_screens_with_no_row_cursor() {
    for (view_key, view) in [('2', View::Cpu), ('3', View::Memory), ('5', View::Disk)] {
        let mut h = Harness::new();
        h.char_key(view_key);
        assert_eq!(h.state.view, view);
        h.char_key('k');
        assert_eq!(
            h.state.mode,
            Mode::Normal,
            "{view:?} should not open a kill"
        );
        h.press(Key::Enter);
        assert_eq!(
            h.state.mode,
            Mode::Normal,
            "{view:?} should not open a detail"
        );
    }
    // Overview and battery do show the cursor, so both are bound there.
    for view_key in ['1', '4'] {
        let mut h = Harness::new();
        h.char_key(view_key).char_key('k');
        assert!(matches!(h.state.mode, Mode::Kill(_)));
    }
}

#[test]
fn a_process_that_exits_closes_its_open_mode() {
    let mut h = Harness::new();
    h.char_key('k');
    assert_eq!(h.state.mode, Mode::Kill(100));
    h.rows.retain(|p| p.pid != 100);
    reconcile_mode(&mut h.state, &h.rows);
    assert_eq!(h.state.mode, Mode::Normal);
}

// ------------------------------------------------------- the main keymap --

#[test]
fn sort_keys_refresh_and_the_working_set_steps() {
    let mut h = Harness::new();
    h.char_key('m');
    assert_eq!(h.state.sort_key, SortKey::Mem);
    h.char_key('e');
    assert_eq!(h.state.sort_key, SortKey::Energy);
    h.char_key('c');
    assert_eq!(h.state.sort_key, SortKey::Cpu);

    h.effects.clear();
    h.char_key('r');
    assert_eq!(h.effects, vec![Effect::RefreshAll]);

    // I-9c: widening only disturbs the process collector; rebuilding every tier
    // would shorten a delta window and print a bogus CPU reading.
    h.effects.clear();
    h.char_key('+').char_key('=');
    assert_eq!(h.state.ws_step, 2);
    assert_eq!(
        h.effects,
        vec![Effect::RefreshProcesses, Effect::RefreshProcesses]
    );
    h.char_key('-').char_key('_').char_key('-');
    assert_eq!(h.state.ws_step, 0, "and clamps at the bottom");
}

proptest! {
    /// The safety property behind every case above, over arbitrary input: a
    /// signal is never requested from the same burst that opened the
    /// confirmation, and never at all when the confirmation was not drawn.
    #[test]
    fn no_single_burst_ever_produces_a_kill(
        keys in prop::collection::vec(
            prop::sample::select(vec!['k', 't', 'q', 'c', 'm', 'e', '/', '+', '-', '1', '4']),
            1..24,
        ),
        drawn in any::<bool>(),
    ) {
        let mut h = Harness::new();
        h.kill_modal_drawn = drawn;
        let burst: String = keys.into_iter().collect();
        h.paste(&burst);
        prop_assert!(h.kills().is_empty(), "{burst:?} produced {:?}", h.kills());
    }

    /// And with the confirmation never drawn, no sequence of *deliberate*
    /// keystrokes can signal either.
    #[test]
    fn an_undrawn_confirmation_never_signals(
        keys in prop::collection::vec(
            prop::sample::select(vec!['k', 't', 'q', '1', '4']),
            1..30,
        ),
    ) {
        let mut h = Harness::new();
        h.kill_modal_drawn = false;
        for c in keys {
            h.char_key(c);
        }
        prop_assert!(h.kills().is_empty());
    }

    /// The reducer is total: no key sequence panics, and the mode is always one
    /// of the four.
    #[test]
    fn the_reducer_is_total(
        keys in prop::collection::vec(any::<char>(), 0..40),
        too_small in any::<bool>(),
    ) {
        let mut h = Harness::new();
        h.too_small = too_small;
        for c in keys {
            h.char_key(c);
        }
        reconcile_selection(&mut h.state, &h.rows);
        reconcile_mode(&mut h.state, &h.rows);
        prop_assert!(h.state.ws_step < 4);
    }
}
