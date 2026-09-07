//! The process table's scrolling window and its scrollbar.

use std::num::NonZeroUsize;

use crate::domain::format::js_round;

/// A window into the process list.
///
/// `rows` is [`NonZeroUsize`] deliberately. "The window contains the selection"
/// has **no solution** for a window of zero rows, and in the TypeScript build
/// that was a fixpoint rather than merely an empty list: the computed offset
/// was written back to the state it was computed from, so at zero rows it
/// alternated between `sel` and `sel + 1` on every render and React reported
/// "Maximum update depth exceeded" — an infinite render loop, not a layout
/// glitch. Making the degenerate case unrepresentable removes the bug rather
/// than guarding against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub offset: usize,
    pub rows: NonZeroUsize,
}

impl Window {
    /// Whether `index` is inside the window.
    pub fn contains(&self, index: usize) -> bool {
        index >= self.offset && index < self.offset + self.rows.get()
    }
}

/// Where the table's window sits, given the selection and the last offset.
///
/// Derived rather than stored, so a re-sort, a filter change, a resize or a `+`
/// expansion can never leave the cursor stranded off-screen for a frame.
/// `scroll_top` only remembers where the window *was*, so ordinary up/down
/// movement inside the window does not drag the list around. See I-26.
pub fn window(
    sel_index: Option<usize>,
    total: usize,
    table_rows: usize,
    scroll_top: usize,
) -> Window {
    // At least one row even when the table is not drawn — see the type note.
    let rows = NonZeroUsize::new(table_rows.max(1)).expect("max(1) is never zero");
    let max = total.saturating_sub(rows.get());
    let cur = scroll_top.min(max);

    let Some(sel) = sel_index else {
        return Window { offset: cur, rows };
    };
    if sel < cur {
        return Window { offset: sel, rows };
    }
    if sel >= cur + rows.get() {
        return Window {
            offset: (sel + 1).saturating_sub(rows.get()),
            rows,
        };
    }
    Window { offset: cur, rows }
}

/// Per-row scrollbar glyphs for a window of `window_size` rows starting at
/// `offset` within a list of `total`.
///
/// Empty when everything fits, so a short list renders no gutter content at all
/// rather than a full-height thumb that cannot move.
pub fn scrollbar_cells(total: usize, window_size: usize, offset: usize) -> Vec<bool> {
    if window_size == 0 || total <= window_size {
        return Vec::new();
    }
    let thumb = (js_round((window_size * window_size) as f64 / total as f64) as usize).max(1);
    let max_offset = total - window_size;
    let travel = window_size.saturating_sub(thumb);
    let top = if max_offset > 0 {
        js_round((offset as f64 / max_offset as f64) * travel as f64) as usize
    } else {
        0
    };
    // Clamped so a rounded thumb can never hang off the end of the track.
    let start = top.min(travel);
    (0..window_size)
        .map(|i| i >= start && i < start + thumb)
        .collect()
}
