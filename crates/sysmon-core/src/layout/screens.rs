//! Sizes shared across the full-screen views, and the floor below which the
//! dashboard is not drawn at all.

use crate::layout::columns::MIN_TABLE_WIDTH;

/// Whether a one-line notice is on screen. Both budgets have to agree about it,
/// so it is a type rather than a bare flag threaded through two signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastRow {
    Shown,
    Hidden,
}

impl ToastRow {
    /// Rows the notice costs: the line itself plus the blank above it.
    pub fn rows(self) -> usize {
        match self {
            Self::Shown => 2,
            Self::Hidden => 0,
        }
    }
}

/// The header, the tab strip, the blank above and below the content, and the
/// footer.
const VIEW_CHROME_ROWS: usize = 5;
/// However short the terminal, a view is handed at least this much, because
/// below it there is nothing left to roll up.
const MIN_VIEW_ROWS: usize = 4;

/// The smallest terminal the dashboard will draw into.
///
/// Width: the narrowest process table that can still fit a name, plus the two
/// columns kept as a margin. Height: the overview's fixed chrome with both the
/// cards and the core strip given up, plus the table's chrome and a row of
/// headroom — so the minimum is not also the point at which the table is empty.
pub const MIN_COLUMNS: usize = MIN_TABLE_WIDTH + 2;
pub const MIN_ROWS: usize = 10;

/// Below this the app says the terminal is too small rather than drawing a
/// broken frame. Decided once, because `useInput` has to know too: a key
/// handler that acts on a screen the user cannot see is the whole reason this
/// matters. See I-15, I-26.
pub fn too_small(columns: usize, rows: usize) -> bool {
    columns < MIN_COLUMNS || rows < MIN_ROWS
}

/// Layout width for the content, never wider than the frame actually drawn.
///
/// This was `max(60, columns - 2)`, which on a terminal narrower than 62 laid
/// the whole app out wider than the terminal. Laying out narrower than the
/// renderer only wastes space; laying out wider corrupts the frame. See I-19.
pub fn content_width(columns: usize) -> usize {
    columns.saturating_sub(2).max(1)
}

/// Rows a full-screen view may draw into: everything except the header, the tab
/// strip, the blank line above and below the content, and the footer.
///
/// The four detail screens all render at least one list that grows with the
/// machine (cores, volumes, top processes), so each is handed this budget and
/// rolls up whatever does not fit. Without it a 16-core Mac overflowed the CPU
/// screen by seven lines on an 80x24 terminal. See I-26.
pub fn view_rows(rows: usize, toast: ToastRow) -> usize {
    (rows as isize - VIEW_CHROME_ROWS as isize - toast.rows() as isize).max(MIN_VIEW_ROWS as isize)
        as usize
}
