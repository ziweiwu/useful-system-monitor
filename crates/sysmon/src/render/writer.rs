//! The second line of defence for I-19 and I-26.
//!
//! `layout` decides how many rows and cells each section gets; this is what
//! makes a section that overruns its allowance fail *at the section*, instead
//! of surfacing three screens away as a scrolled header.
//!
//! Every line reaching a widget has already been truncated by our own
//! `text::width`, and `Paragraph` is used with wrapping **off**. That is not an
//! optimisation: ratatui's `Wrap` re-measures with `unicode-width`, which
//! disagrees with `cells()` on emoji and the U+FE0F rule, so letting it wrap
//! would quietly reintroduce a second width authority — the exact bug
//! `text::width` exists to prevent.

use ratatui::text::{Line, Span};
use sysmon_core::text::width::{display_width, truncate};

/// Collects the lines of one section against the budget it was given.
pub struct RowWriter {
    width: usize,
    rows: usize,
    lines: Vec<Line<'static>>,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    label: &'static str,
}

impl RowWriter {
    pub fn new(label: &'static str, width: usize, rows: usize) -> Self {
        Self {
            width,
            rows,
            lines: Vec::new(),
            label,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn remaining(&self) -> usize {
        self.rows.saturating_sub(self.lines.len())
    }

    /// Fits a line to the width, truncating the tail.
    ///
    /// This is what `wrap="truncate"` did for every `Text` in the original, and
    /// it is the same rule `text::truncate` applies to a single string —
    /// applied across a line built from several styled spans, so the ellipsis
    /// lands in whichever span runs over. A caller that cares *where* the cut
    /// falls still pre-fits its own text; this is the backstop that makes an
    /// unfitted one impossible rather than merely unlikely.
    fn fit(&self, line: Line<'static>) -> Line<'static> {
        let total: usize = line.spans.iter().map(|s| display_width(&s.content)).sum();
        if total <= self.width {
            return line;
        }
        let mut used = 0usize;
        let mut out: Vec<Span<'static>> = Vec::with_capacity(line.spans.len());
        for s in line.spans {
            let w = display_width(&s.content);
            if used + w <= self.width {
                used += w;
                out.push(s);
                continue;
            }
            let room = self.width - used;
            if room > 0 {
                let style = s.style;
                out.push(Span::styled(truncate(&s.content, room), style));
            }
            break;
        }
        Line::from(out)
    }

    /// Adds a line — if there is a row left for it.
    ///
    /// **The budget wins and content gives way**, which is the same rule
    /// `text::width` applies horizontally: a line that will not fit is dropped
    /// rather than allowed to overflow. Because every screen emits its most
    /// important lines first, that is also a priority order — at a four-row
    /// budget the disk screen keeps its headline and loses its volume list,
    /// rather than running five lines into four and scrolling the header away.
    ///
    /// Width is a different matter and is never negotiable: a line wider than
    /// its budget is a bug in the caller, because every string reaching here
    /// should already have been fitted by `text::width`. So that one asserts.
    pub fn push(&mut self, line: Line<'static>) {
        if self.lines.len() < self.rows {
            let fitted = self.fit(line);
            #[cfg(debug_assertions)]
            {
                let w: usize = fitted.spans.iter().map(|s| display_width(&s.content)).sum();
                debug_assert!(w <= self.width, "{}: fit() left {w} cells", self.label);
            }
            self.lines.push(fitted);
        }
    }

    pub fn blank(&mut self) {
        self.push(Line::from(""));
    }

    pub fn into_lines(self) -> Vec<Line<'static>> {
        self.lines
    }
}
