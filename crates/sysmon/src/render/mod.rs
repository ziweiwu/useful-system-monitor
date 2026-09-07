//! Building the frame.
//!
//! The whole screen is a **pure function of `(data, ui_state, size)`**. Nothing
//! here reads a clock, touches a terminal or performs I/O, which buys three
//! things at once: the layout sweep is a unit test rather than a render, a
//! golden frame can be asserted without a `TestBackend`, and the fuzzer can
//! drive the app faster than a terminal could ever accept keys.
//!
//! The renderer receives budgets from `sysmon_core::layout` and does not
//! compute them. Every line it produces goes through [`writer::RowWriter`],
//! which proves in a debug build that the section fits — so an overflow fails
//! at the offending section rather than as a scrolled header three screens
//! away.

pub mod screens;
pub mod table;
pub mod widgets;
pub mod writer;

use ratatui::text::Line;
use sysmon_core::app::state::{Mode, UiState};
use sysmon_core::domain::format::{age, clock_time, duration};
use sysmon_core::domain::scoring::sort_processes;
use sysmon_core::domain::types::ProcessSample;
use sysmon_core::domain::views::{View, VIEW_ORDER};
use sysmon_core::kill::guards::process_name;
use sysmon_core::layout::overview::{plan_overview, OverviewPlan};
use sysmon_core::layout::screens::{
    content_width, too_small, view_rows, ToastRow, MIN_COLUMNS, MIN_ROWS,
};
use sysmon_core::text::width::{display_width, pad_end, truncate};
use sysmon_core::ui::theme;

use crate::render::widgets::{bold, dim, plain, span};
use crate::render::writer::RowWriter;
use crate::runtime::store::AppData;

pub const BRAND: &str = "useful-system-monitor";

/// The key legend, in two lengths.
///
/// `r` was missing from all of them — and from `--help` and the README — while
/// being the only way to force a sample before the next tier, the remedy for
/// every "data unavailable" panel, and the key a guard message already told the
/// user to press. An accelerator documented nowhere is an accelerator nobody
/// has. Adding it pushes the full legend past the 78 cells an 80-column
/// terminal has, and truncation would eat `q quit` from the end — so it comes
/// in two lengths, chosen by which one fits, never by cutting one short.
const KEYS_FULL: &str =
    "up/dn move  +/- rows  enter info  k kill  / filter  c m e sort  r refresh  q quit";
const KEYS_COMPACT: &str = "up/dn  +/-  enter  k kill  / filter  c m e sort  r refresh  q quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub columns: usize,
    pub rows: usize,
}

/// One built frame.
pub struct Frame {
    pub lines: Vec<Line<'static>>,
    /// Whether the kill confirmation is **actually on this screen**.
    ///
    /// Fed straight back into the next keypress: a confirmation that was not
    /// drawn cannot be confirmed, whatever the mode state says. This is I-15
    /// expressed mechanically, and it only exists because the frame is a value
    /// rather than a side effect.
    pub kill_modal_drawn: bool,
}

/// The rows the user is actually looking at: filtered, then sorted.
pub fn visible_rows(app: &AppData, ui: &UiState) -> Vec<ProcessSample> {
    let Some(procs) = app.processes.sample() else {
        return Vec::new();
    };
    let q = ui.filter.trim().to_lowercase();
    let list: Vec<ProcessSample> = if q.is_empty() {
        procs.visible.clone()
    } else {
        procs
            .visible
            .iter()
            .filter(|p| {
                process_name(&p.command).to_lowercase().contains(&q)
                    || p.pid.to_string().contains(&q)
            })
            .cloned()
            .collect()
    };
    sort_processes(&list, ui.sort_key)
}

/// The newest sample time across every panel.
///
/// The clock and the age readouts derive from this rather than their own 1 Hz
/// timer. A second timer meant ~1.8 renders/s instead of ~0.5, and since a
/// render costs far more than any collector, the clock tick was one of the most
/// expensive things this app did.
pub fn newest_sample(app: &AppData, fallback_ms: i64) -> i64 {
    [
        app.cpu.sampled_at_ms(),
        app.memory.sampled_at_ms(),
        app.disk.sampled_at_ms(),
        app.battery.sampled_at_ms(),
        app.processes.sampled_at_ms(),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(fallback_ms)
}

/// I-26: the one size where the honest answer is "not here".
///
/// Every screen degrades — cards drop, columns drop, lists roll up — but below
/// this the table cannot carry a name and the frame cannot carry the table, so
/// it says so rather than drawing something broken. A terminal with no rows or
/// no columns is reachable transiently while a window is being dragged, and
/// there is nothing honest to draw into it — not even the complaint.
fn undrawable(size: Size) -> Option<Frame> {
    if size.rows == 0 || size.columns == 0 {
        return Some(Frame {
            lines: Vec::new(),
            kill_modal_drawn: false,
        });
    }
    if !too_small(size.columns, size.rows) {
        return None;
    }
    let mut w = RowWriter::new("too-small", size.columns, size.rows);
    let msg = format!("terminal too small: {}x{}", size.columns, size.rows);
    let need = format!("needs at least {MIN_COLUMNS}x{MIN_ROWS}");
    w.push(Line::from(span(
        truncate(&msg, size.columns.max(1)),
        theme::DANGER,
    )));
    if size.rows > 1 {
        w.push(Line::from(dim(truncate(&need, size.columns.max(1)))));
    }
    Some(Frame {
        lines: w.into_lines(),
        kill_modal_drawn: false,
    })
}

pub fn build(app: &AppData, ui: &UiState, size: Size, now_ms: i64) -> Frame {
    if let Some(frame) = undrawable(size) {
        return frame;
    }

    let width = content_width(size.columns);
    let now = newest_sample(app, now_ms);
    let toast = toast_row(ui);
    let rows = sort_and_reconcile(app, ui);
    let (content_rows, detail_rows) = row_budgets(ui, size);

    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(header(app, width, now));
    if !ui.mode.hides_dashboard() {
        out.push(tabs(ui.view, width));
    }

    // One context per row budget: the overview owns its whole content area,
    // every other screen draws inside the margins `view_rows` already removed.
    let screen = |budget: usize| screens::Screen {
        app,
        ui,
        rows: &rows,
        width,
        budget,
        now_ms: now,
    };
    let plan = plan_overview(size.rows, toast);
    let (body, kill_modal_drawn) = content(ui, &rows, &screen, (&plan, content_rows, detail_rows));
    out.extend(body);

    push_toast(&mut out, ui, width);
    seal(&mut out, size, width);

    Frame {
        lines: out,
        kill_modal_drawn,
    }
}

/// The one-line notice under the content, if there is one to show.
fn push_toast(out: &mut Vec<Line<'static>>, ui: &UiState, width: usize) {
    let Some(toast) = &ui.toast else {
        return;
    };
    out.push(Line::from(span(
        truncate(&toast.text, width),
        if toast.bad {
            theme::DANGER
        } else {
            theme::CPU_LOW
        },
    )));
}

/// What the screens may draw into: the terminal, less the chrome `build` adds
/// around them.
///
/// Computed once and handed down, so a screen never has to guess what is above
/// and below it — the class of mistake that produced every "counted but never
/// checked" bug in the original. The four detail screens and the two modes also
/// sit inside a one-row margin, top and bottom, which `view_rows` accounts for
/// and the overview does not, because the overview pays for its own margins
/// through OVERVIEW_FIXED. Getting this wrong shifts every line on four of the
/// five screens by one, which is invisible in a test and obvious in a diff.
///
/// Returns `(content_rows, detail_rows)`.
fn toast_row(ui: &UiState) -> ToastRow {
    if ui.toast.is_some() {
        ToastRow::Shown
    } else {
        ToastRow::Hidden
    }
}

fn row_budgets(ui: &UiState, size: Size) -> (usize, usize) {
    let toast = toast_row(ui);
    let chrome = 1 // header
        + usize::from(!ui.mode.hides_dashboard()) // tab strip
        + usize::from(ui.toast.is_some())
        + 1; // footer
    (
        size.rows.saturating_sub(chrome),
        view_rows(size.rows, toast),
    )
}

/// Pad to the bottom, then the footer, so the legend always sits on the last
/// line rather than floating under a short screen.
fn seal(out: &mut Vec<Line<'static>>, size: Size, width: usize) {
    while out.len() + 1 < size.rows {
        out.push(Line::from(""));
    }
    out.truncate(size.rows.saturating_sub(1));
    out.push(footer(width));
}

/// The dashboard, or whichever full-screen mode has replaced it.
///
/// Both modes replace the dashboard rather than stacking under it: drawing both
/// put 93 lines into a 24-row terminal. Returns the lines and whether the kill
/// confirmation actually reached the screen — which the input reducer reads
/// before it will let a signal through. See I-15.
fn content<'a>(
    ui: &UiState,
    rows: &[ProcessSample],
    screen: &impl Fn(usize) -> screens::Screen<'a>,
    layout: (&OverviewPlan, usize, usize),
) -> (Vec<Line<'static>>, bool) {
    let (plan, content_rows, detail_rows) = layout;
    let mut out: Vec<Line<'static>> = Vec::new();
    match ui.mode {
        Mode::Kill(pid) => {
            let Some(target) = rows.iter().find(|p| p.pid == pid) else {
                return (out, false);
            };
            out.push(Line::from(""));
            out.push(Line::from(""));
            out.extend(screens::kill_modal(target, &screen(detail_rows)));
            (out, true)
        }
        Mode::Detail(pid) => {
            if let Some(target) = rows.iter().find(|p| p.pid == pid) {
                out.push(Line::from(""));
                out.push(Line::from(""));
                out.extend(screens::detail(target, &screen(detail_rows)));
            }
            (out, false)
        }
        _ => {
            out.extend(dashboard(
                ui.view,
                screen,
                plan,
                (content_rows, detail_rows),
            ));
            (out, false)
        }
    }
}

/// The screen the current tab draws, with the row budget that tab is owed.
fn dashboard<'a>(
    view: View,
    screen: &impl Fn(usize) -> screens::Screen<'a>,
    plan: &OverviewPlan,
    budgets: (usize, usize),
) -> Vec<Line<'static>> {
    let (content_rows, detail_rows) = budgets;
    if view == View::Overview {
        return screens::overview(&screen(content_rows), plan);
    }
    // Every detail screen sits under a one-row margin the overview pays for
    // itself, so they open with a blank the overview does not.
    let mut out = vec![Line::from("")];
    out.extend(match view {
        View::Cpu => screens::cpu(&screen(detail_rows)),
        View::Memory => screens::memory(&screen(detail_rows)),
        View::Battery => screens::battery(&screen(detail_rows)),
        View::Disk => screens::disk(&screen(detail_rows)),
        View::Overview => unreachable!("handled above"),
    });
    out
}

fn sort_and_reconcile(app: &AppData, ui: &UiState) -> Vec<ProcessSample> {
    visible_rows(app, ui)
}

fn header(app: &AppData, width: usize, now_ms: i64) -> Line<'static> {
    let clock = clock_time(now_ms);
    let host = app.host.as_ref();
    let right = clock.clone();
    /* host() is async, so the first frame has no hardware info yet. Say that
    plainly rather than show a placeholder "unknown · 1 cores", which is a
    visible falsehood even for one frame. */
    let hardware = match host {
        Some(h) if h.cores > 0 && h.model != "unknown" => {
            format!(
                "{} · {} cores · up {}",
                h.model,
                h.cores,
                duration(h.uptime_sec)
            )
        }
        _ => "detecting hardware…".to_string(),
    };
    let left = format!(
        "{BRAND} {hardware}{}",
        if app.mock { "  [MOCK DATA]" } else { "" }
    );
    let gap = width.saturating_sub(display_width(&right));
    Line::from(vec![
        bold(pad_end(&truncate(&left, gap), gap), theme::HEADLINE),
        dim(right),
    ])
}

/// The tab strip, in two lengths for the same reason the footer legend is.
///
/// `wrap="truncate"` alone meant that at 50 columns the fifth screen rendered
/// as a bare `5` with the closing bracket and the arrow hint cut off, so the
/// strip stopped saying what the arrows did at exactly the size where the
/// numbers were the only way to navigate. See I-27.
fn tabs(current: View, width: usize) -> Line<'static> {
    let full = build_tabs(current, TabDensity::Full);
    let line = if display_width(&text_of(&full)) <= width {
        full
    } else {
        build_tabs(current, TabDensity::KeysOnly)
    };
    let used = display_width(&text_of(&line));
    let mut spans = line;
    if used < width {
        spans.push(plain(" ".repeat(width - used)));
    }
    Line::from(spans)
}

fn text_of(spans: &[ratatui::text::Span<'static>]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

/// One tab's text.
///
/// The active tab always carries its name; only the others lose theirs. The
/// leading and trailing spaces are part of each inactive tab rather than a
/// separator, which is why two of them sit between adjacent tabs and only one
/// beside the active one.
fn tab_text(view: View, active: View, density: TabDensity) -> String {
    if view == active {
        format!("[{} {}]", view.key(), view.label())
    } else if density == TabDensity::KeysOnly {
        format!(" {} ", view.key())
    } else {
        format!(" {} {} ", view.key(), view.label())
    }
}

/// How much room the tab strip has for its own labels.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TabDensity {
    /// Each inactive tab shows its key and its name.
    Full,
    /// Only the active tab keeps its name.
    KeysOnly,
}

const HINT: &str = "   ←/→ or 1-5";
const SHORT_HINT: &str = "  ←/→";

fn build_tabs(current: View, density: TabDensity) -> Vec<ratatui::text::Span<'static>> {
    /// One hue per data family, matching each screen's cards and columns.
    fn tab_colour(view: View) -> theme::Rgb {
        match view {
            View::Overview => theme::HEADLINE,
            View::Cpu => theme::CPU_MID,
            View::Memory => theme::MEM,
            View::Battery => theme::BATTERY,
            View::Disk => theme::DISK,
        }
    }
    let mut out = Vec::with_capacity(VIEW_ORDER.len() + 1);
    for v in VIEW_ORDER {
        let text = tab_text(v, current, density);
        // I-23: the active tab is marked by brackets as well as by colour and
        // weight, so it survives NO_COLOR and a plain-text capture.
        out.push(if v == current {
            bold(text, tab_colour(v))
        } else {
            dim(text)
        });
    }
    out.push(dim(if density == TabDensity::KeysOnly {
        SHORT_HINT
    } else {
        HINT
    }));
    out
}

fn footer(width: usize) -> Line<'static> {
    let keys = if display_width(KEYS_FULL) <= width {
        KEYS_FULL
    } else {
        KEYS_COMPACT
    };
    Line::from(dim(truncate(keys, width)))
}

/// "3s" style panel age, or an em dash before the first sample. Panels are
/// per-tier, so each shows its own. See I-4.
pub fn panel_age<T>(p: &sysmon_core::domain::types::Panel<T>, now_ms: i64) -> String {
    match p.sampled_at_ms() {
        Some(at) => age(at, now_ms),
        None => "—".to_string(),
    }
}
