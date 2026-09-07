//! Shared line builders. Every one returns text already fitted to its budget.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use sysmon_core::domain::format::to_fixed;
use sysmon_core::ui::theme::{self, Rgb};

/// A ratio with no denominator reads as zero, never NaN — which would propagate
/// silently through the bar arithmetic. See I-19.
const NO_RATIO: f64 = 0.0;
const FULL_SCALE_PCT: f64 = 100.0;

pub fn color(colour: Rgb) -> Color {
    Color::Rgb(colour.0, colour.1, colour.2)
}

pub fn span(text: impl Into<String>, colour: Rgb) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(color(colour)))
}

pub fn bold(text: impl Into<String>, colour: Rgb) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default()
            .fg(color(colour))
            .add_modifier(Modifier::BOLD),
    )
}

pub fn plain(text: impl Into<String>) -> Span<'static> {
    span(text, theme::TEXT)
}

pub fn dim(text: impl Into<String>) -> Span<'static> {
    span(text, theme::DIM)
}

/// A horizontal bar: filled cells in the metric's hue, remainder in a dim
/// track. Two spans, so the fill and the track keep their own colours while the
/// pair always occupies exactly `width` cells.
pub fn meter(pct: f64, width: usize, colour: Rgb) -> Vec<Span<'static>> {
    let (filled, empty) = theme::bar_cells(pct, width);
    vec![
        span("█".repeat(filled), colour),
        span("░".repeat(empty), theme::TRACK),
    ]
}

pub fn sparkline(values: &[f64], width: usize, max: f64, colour: Rgb) -> Span<'static> {
    span(theme::sparkline(values, width, max), colour)
}

/// A percentage with one decimal and a trailing sign, as the cards draw it.
pub fn pct(value: f64) -> String {
    format!("{}%", to_fixed(value, 1))
}

/// A ratio in percent, or zero when there is no denominator. See I-19.
pub fn ratio(used: u64, total: u64) -> f64 {
    if total > 0 {
        (used as f64 / total as f64) * FULL_SCALE_PCT
    } else {
        NO_RATIO
    }
}
