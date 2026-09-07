//! The dashboard's own state — everything that is not a sample.

use crate::domain::scoring::SortKey;
use crate::domain::views::View;
use crate::domain::working_set::{WorkingSetCap, WORKING_SET_STEPS};

/// The full-screen modes, as one value.
///
/// The TypeScript build keeps `filterMode`, `detailPid` and `killTarget` as
/// three independent pieces of state and relies on every transition remembering
/// to clear the other two. I-26b — "at most one full-screen mode is drawn at a
/// time" — is then a rule about the render rather than a fact about the state,
/// and it had to be given its own test after the detail panel and the kill
/// confirmation were once drawn stacked, putting 93 lines into a 24-row
/// terminal.
///
/// As an enum they are exclusive by construction and the invariant needs no
/// test, because it has no counterexample to test for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Typing into the filter box.
    Filter,
    /// The detail panel for one PID.
    Detail(i32),
    /// The kill confirmation for one PID.
    Kill(i32),
}

impl Mode {
    /// Whether this mode replaces the dashboard rather than sitting under it.
    pub fn hides_dashboard(self) -> bool {
        matches!(self, Self::Detail(_) | Self::Kill(_))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub text: String,
    pub bad: bool,
    /// When it stops being drawn. Panels age themselves off the newest sample
    /// rather than a clock tick, and so does this.
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiState {
    pub view: View,
    pub sort_key: SortKey,
    pub filter: String,
    pub mode: Mode,
    /// **Keyed by PID, never by row index.** A re-sort under the cursor must
    /// not move the kill target. See I-21.
    pub selected_pid: Option<i32>,
    /// Where the window was. The truth is derived each frame by
    /// [`crate::layout::scroll::window`]; this only stops ordinary up/down
    /// movement inside the window from dragging the list around.
    pub scroll_top: usize,
    /// Index into [`WORKING_SET_STEPS`].
    pub ws_step: usize,
    /// SIGKILL needs a second, distinct press. See I-15.
    pub armed_kill: bool,
    pub toast: Option<Toast>,
    /// The input batch in which the current mode was entered.
    ///
    /// A terminal delivers a paste as one burst of key events. This is what
    /// lets a confirmation refuse a key that arrived in the *same* burst that
    /// opened it — see the note on [`crate::app::input::reduce`].
    pub mode_entered_batch: u64,
    /// The batch of the last key that reached a mode, so a second press has to
    /// come from a later burst.
    pub last_action_batch: u64,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            view: View::Overview,
            sort_key: SortKey::Cpu,
            filter: String::new(),
            mode: Mode::Normal,
            selected_pid: None,
            scroll_top: 0,
            ws_step: 0,
            armed_kill: false,
            toast: None,
            mode_entered_batch: 0,
            last_action_batch: 0,
        }
    }
}

impl UiState {
    pub fn working_set_cap(&self) -> WorkingSetCap {
        WORKING_SET_STEPS[self.ws_step.min(WORKING_SET_STEPS.len() - 1)]
    }

    /// `k` and `enter` are only bound where the screen shows which process they
    /// would act on. A hidden target is not narrated by a confirmation that
    /// names it, so the CPU, memory and disk screens — which draw no row cursor
    /// — leave both keys unbound. See I-15.
    pub fn row_actions(&self) -> bool {
        matches!(self.view, View::Overview | View::Battery)
    }

    pub fn set_mode(&mut self, mode: Mode, batch: u64) {
        self.mode = mode;
        self.mode_entered_batch = batch;
        self.armed_kill = false;
    }
}
