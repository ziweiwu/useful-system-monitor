//! The process table.
//!
//! Every cell position here matches `src/ui/ProcessTable.tsx` exactly, because
//! the table is the part of the screen a user actually reads and a column in a
//! different place is a different product. The order — cursor, PID, **name**,
//! protection mark, CPU bar, CPU%, memory, energy, user, scrollbar — is not
//! obvious from the width arithmetic, which computes the *name* column last
//! while the renderer draws it second. `frame_diff` is what caught getting that
//! backwards.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use sysmon_core::domain::format::{bytes, percent, to_fixed};
use sysmon_core::domain::scoring::estimate_watts;
use sysmon_core::domain::types::{OthersRollup, ProcessSample};
use sysmon_core::kill::guards::process_name;
use sysmon_core::layout::columns::{
    ColumnLayout, CPU_BAR, CPU_NUM, CURSOR, EN_BAR, EN_NUM, MARK, MEM_NUM, PID,
};
use sysmon_core::layout::scroll::{scrollbar_cells, Window};
use sysmon_core::text::width::{pad_end, pad_start, truncate};
use sysmon_core::ui::theme;

use crate::render::widgets::{bold, color, dim, meter, plain, span};
use crate::render::writer::RowWriter;

/// Watts are only meaningful when the battery is actually moving charge.
/// Plugged in and holding, total draw is ~0 and every row would read "0.0W" — a
/// column of zeros. In that case the whole column falls back to the unitless
/// energy score, header included, so units never mix within one column.
const MIN_MEANINGFUL_WATTS: f64 = 0.5;

pub fn watts_are_meaningful(total_watts: Option<f64>) -> bool {
    total_watts.is_some_and(|w| w.abs() >= MIN_MEANINGFUL_WATTS)
}

/// Which unit the POWER/ENERGY column is carrying.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PowerUnit {
    /// Real watts, when the total draw is big enough to divide up.
    Watts,
    /// The unitless energy score, when it is not.
    Score,
}

fn power_unit(total_watts: Option<f64>) -> PowerUnit {
    if watts_are_meaningful(total_watts) {
        PowerUnit::Watts
    } else {
        PowerUnit::Score
    }
}

fn power_label(watts: Option<f64>, energy: Option<f64>, unit: PowerUnit) -> String {
    match watts {
        Some(w) if unit == PowerUnit::Watts => format!("{}W", to_fixed(w, 1)),
        _ => percent(energy, 1),
    }
}

/// What the table needs that a row does not carry.
pub struct TableContext {
    pub total_energy: f64,
    pub total_watts: Option<f64>,
    pub energy_accurate: bool,
    /// Whether `+` would still widen the working set, for the roll-up hint.
    pub can_expand: bool,
    /// A filter is on, so the roll-up reports scope rather than a remainder.
    pub filtered: bool,
}

/// Where one row sits: its columns, and what the table has decided about it.
pub struct RowPlace<'a> {
    pub cols: &'a ColumnLayout,
    pub selected: bool,
    /// `Some(true)` draws the scrollbar thumb, `Some(false)` the track, `None`
    /// no scrollbar at all.
    pub scrollbar: Option<bool>,
}

/// A CPU figure that was never measured. See I-1.
const NO_READING: f64 = 0.0;

/// Whether a row is the one the cursor is on. The cells that have to meet
/// contrast against the selection background read this rather than a bare flag.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Emphasis {
    Selected,
    Normal,
}

/// The column header.
///
/// Follows the row layout exactly, so a column dropped at a narrow width cannot
/// leave its heading behind.
pub fn header(cols: &ColumnLayout, ctx: &TableContext, writer: &mut RowWriter) {
    let unit = power_unit(ctx.total_watts);
    let mut text = String::new();
    text.push_str(&" ".repeat(CURSOR));
    text.push_str(&pad_end("PID", PID));
    text.push(' ');
    text.push_str(&pad_end("PROCESS", cols.name));
    text.push_str(&" ".repeat(MARK));
    text.push_str(&pad_start("CPU%", CPU_BAR + CPU_NUM));
    text.push_str("  ");
    text.push_str(&pad_start("MEM", MEM_NUM));
    if cols.energy {
        text.push_str("  ");
        // "est" is dropped only when the numbers are genuinely measured.
        let label = match (unit, ctx.energy_accurate) {
            (PowerUnit::Watts, true) => "POWER",
            (PowerUnit::Watts, false) => "POWER est",
            (PowerUnit::Score, true) => "ENERGY",
            (PowerUnit::Score, false) => "ENERGY est",
        };
        text.push_str(&pad_start(label, EN_BAR + 1 + EN_NUM));
    }
    if cols.user > 0 {
        text.push_str("  ");
        text.push_str(&pad_end("USER", cols.user));
    }
    writer.push(Line::from(dim(text)));
}

/// One row.
///
/// The cursor, the protection mark and the selection are all readable without
/// colour — `>` and `!` — so the table survives `NO_COLOR` and stays greppable.
/// See I-23.
pub fn row(sample: &ProcessSample, place: &RowPlace, ctx: &TableContext, writer: &mut RowWriter) {
    let RowPlace {
        cols,
        selected,
        scrollbar,
    } = *place;
    let emphasis = if selected {
        Emphasis::Selected
    } else {
        Emphasis::Normal
    };
    let p = sample;
    let cpu = p.cpu_percent.unwrap_or(NO_READING);
    let unit = power_unit(ctx.total_watts);
    let watts = estimate_watts(p.energy, ctx.total_energy, ctx.total_watts);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(16);
    spans.extend(identity_cells(p, cols.name, emphasis));
    spans.extend(cpu_cells(p, cpu));
    spans.push(plain("  ".to_string()));
    spans.push(span(pad_start(&bytes(p.rss_bytes), MEM_NUM), theme::MEM));

    if cols.energy {
        spans.extend(energy_cells(p, watts, unit));
    }
    if cols.user > 0 {
        spans.push(user_cell(p, cols.user, emphasis));
    }

    spans.push(plain(" ".to_string()));
    spans.push(match scrollbar {
        Some(true) => span("█", theme::MEM),
        Some(false) => span("│", theme::TRACK),
        None => plain(" ".to_string()),
    });
    writer.push(Line::from(spans));
}

/// The cursor, PID, process name and protection mark.
///
/// Every other cell brightens on the selected row; the PID did not, so it sat
/// at 1.97:1 on the selection background — on the very row the user is about to
/// press enter or k against.
fn identity_cells(
    sample: &ProcessSample,
    name_width: usize,
    emphasis: Emphasis,
) -> Vec<Span<'static>> {
    let selected = emphasis == Emphasis::Selected;
    let p = sample;
    let name_style = if selected {
        Style::default()
            .fg(color(theme::HEADLINE))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(color(theme::TEXT))
    };
    vec![
        if selected {
            bold(" > ".to_string(), theme::MEM)
        } else {
            bold("   ".to_string(), theme::DIM)
        },
        span(
            pad_end(&p.pid.to_string(), PID),
            if selected { theme::TEXT } else { theme::DIM },
        ),
        plain(" ".to_string()),
        Span::styled(pad_end(&process_name(&p.command), name_width), name_style),
        // Protection is marked with a glyph, not colour alone. See I-23.
        span(if p.protected { "!" } else { " " }, theme::DANGER),
    ]
}

/// The CPU bar and its figure. A row that was never measured dims the number
/// rather than printing a zero. See I-1.
fn cpu_cells(sample: &ProcessSample, cpu: f64) -> Vec<Span<'static>> {
    let mut out = meter(cpu, CPU_BAR, theme::severity(cpu));
    out.push(Span::styled(
        pad_start(&percent(sample.cpu_percent, 1), CPU_NUM),
        Style::default()
            .fg(color(if sample.cpu_percent.is_none() {
                theme::DIM
            } else {
                theme::HEADLINE
            }))
            .add_modifier(Modifier::BOLD),
    ));
    out
}

/// The energy bar and its figure.
fn energy_cells(sample: &ProcessSample, watts: Option<f64>, unit: PowerUnit) -> Vec<Span<'static>> {
    let mut out = vec![plain("  ".to_string())];
    out.extend(meter(
        sample.energy.unwrap_or(NO_READING),
        EN_BAR,
        theme::BATTERY,
    ));
    out.push(plain(" ".to_string()));
    out.push(span(
        pad_start(&power_label(watts, sample.energy, unit), EN_NUM),
        theme::BATTERY,
    ));
    out
}

/// The USER cell.
///
/// Selection-aware for the same reason as the PID cell: dim is 3.25:1 against
/// the selection background, below 4.5:1.
fn user_cell(sample: &ProcessSample, width: usize, emphasis: Emphasis) -> Span<'static> {
    let p = sample;
    let colour = if p.user == "root" {
        theme::ROOT
    } else if emphasis == Emphasis::Selected {
        theme::TEXT
    } else {
        theme::DIM
    };
    span(pad_end(&truncate(&p.user, width), width), colour)
}

/// The "… N others" line.
///
/// The roll-up exists so the table reconciles with the CPU and MEM cards:
/// everything on screen, plus `others`, is the whole machine. A filter breaks
/// that — `others` is the tail of the *working-set cap*, which has nothing to do
/// with the search — so under a filter it reports the **scope** instead.
/// Filtering for "chrome" and reading "… 774 others" beneath two matches
/// implies 774 more Chrome processes.
pub fn rollup(
    others: &OthersRollup,
    cols: &ColumnLayout,
    ctx: &TableContext,
    writer: &mut RowWriter,
) {
    if others.count == 0 {
        return;
    }
    if ctx.filtered {
        let text = format!(
            "{}… {} outside this view were not searched{}",
            " ".repeat(CURSOR),
            others.count,
            if ctx.can_expand { "  (+ widen)" } else { "" }
        );
        writer.push(Line::from(dim(truncate(&text, writer.width()))));
        return;
    }

    let unit = power_unit(ctx.total_watts);
    let others_watts = estimate_watts(Some(others.energy), ctx.total_energy, ctx.total_watts);
    let label = if ctx.can_expand {
        format!("… {} others  (+ show more)", others.count)
    } else {
        format!("… {} others", others.count)
    };
    let text = rollup_text(others, cols, &label, (others_watts, unit));
    writer.push(Line::from(dim(text)));
}

/// The roll-up line, aligned to the same columns as a row.
fn rollup_text(
    others: &OthersRollup,
    cols: &ColumnLayout,
    label: &str,
    power: (Option<f64>, PowerUnit),
) -> String {
    let (others_watts, unit) = power;
    let mut text = String::new();
    text.push_str(&" ".repeat(CURSOR));
    text.push_str(&pad_end("", PID));
    text.push(' ');
    text.push_str(&pad_end(&truncate(label, cols.name), cols.name));
    text.push_str(&" ".repeat(MARK));
    text.push_str(&pad_start(
        &to_fixed(others.cpu_percent, 1),
        CPU_BAR + CPU_NUM,
    ));
    text.push_str("  ");
    text.push_str(&pad_start(&bytes(others.rss_bytes), MEM_NUM));
    if cols.energy {
        text.push_str("  ");
        text.push_str(&pad_start(
            &power_label(others_watts, Some(others.energy), unit),
            EN_BAR + 1 + EN_NUM,
        ));
    }
    text
}

/// The whole table body.
/// Which rows to draw, and which of them the user has selected.
pub struct Body<'a> {
    pub rows: &'a [ProcessSample],
    pub window: Window,
    pub cols: &'a ColumnLayout,
    pub selected_pid: Option<i32>,
}

pub fn body(body: &Body, ctx: &TableContext, writer: &mut RowWriter) {
    let Body {
        rows,
        window,
        cols,
        selected_pid,
    } = *body;
    // Clamped here as well as by the caller: the window shrinks on a resize,
    // and a stale offset would otherwise render past the end.
    let start = window
        .offset
        .min(rows.len().saturating_sub(window.rows.get()));
    let cells = scrollbar_cells(rows.len(), window.rows.get(), start);
    for (i, p) in rows.iter().skip(start).take(window.rows.get()).enumerate() {
        let place = RowPlace {
            cols,
            selected: Some(p.pid) == selected_pid,
            scrollbar: (!cells.is_empty()).then(|| cells.get(i).copied().unwrap_or(false)),
        };
        row(p, &place, ctx, writer);
    }
}

/// What the line above the table says.
///
/// The widest variable string on the screen, and the overview budgets exactly
/// one row for it. The range is spelled out — `1-13 of 37` — because "13 of 37"
/// does not say *which* thirteen, and the window moves.
pub struct Status<'a> {
    /// First visible row, zero-based.
    pub offset: usize,
    /// Rows the window holds.
    pub window: usize,
    /// Rows after filtering.
    pub matched: usize,
    /// Processes on the machine.
    pub total: usize,
    pub cap_label: &'a str,
    pub cost_note: &'a str,
    pub sort: &'a str,
    pub filter: &'a str,
    pub filter_mode: bool,
    /// Nothing sampled yet.
    pub pending: bool,
    /// Each panel's own sample age, because panels sit on different tiers and a
    /// single "last updated" would be a lie about four of them. See I-4.
    pub ages: &'a str,
}

pub fn status(s: &Status<'_>, writer: &mut RowWriter) {
    let head = if s.pending {
        "sampling…".to_string()
    } else if s.matched == 0 {
        "no matches".to_string()
    } else {
        format!(
            "{}-{} of {} · top {} of {}{}",
            s.offset + 1,
            (s.offset + s.window).min(s.matched),
            s.matched,
            s.cap_label,
            s.total,
            s.cost_note
        )
    };
    let filter = if s.filter_mode {
        format!("{}▏", if s.filter.is_empty() { "…" } else { s.filter })
    } else if s.filter.is_empty() {
        "(none)".to_string()
    } else {
        s.filter.to_string()
    };
    let text = format!("{head} · sort {} · filter {filter}      {}", s.sort, s.ages);
    writer.push(Line::from(dim(truncate(&text, writer.width()))));
}

/// A panel that has nothing to show yet, or could not be read.
pub fn unavailable(label: &str, message: Option<&str>, writer: &mut RowWriter) {
    let text = match message {
        // I-24: cause and remedy, not a bare "error".
        Some(m) => format!("{label} unavailable — {m}. Press r to retry."),
        None => format!("{label} sampling…"),
    };
    writer.push(Line::from(span(
        truncate(&text, writer.width()),
        theme::DIM,
    )));
}

/// The heading a detail screen puts above its own top-N list.
pub fn section(title: &str, writer: &mut RowWriter) {
    writer.push(Line::from(bold(
        truncate(title, writer.width()),
        theme::HEADLINE,
    )));
}
