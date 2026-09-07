//! The overview screen's row budget, spent in priority order.

/// Everything on screen no matter how short the terminal is: the app header,
/// the tab strip, the status line, the footer, and their margins.
///
/// The cards and the core strip are deliberately **not** in it, because at some
/// height they stop being affordable and the process table — the only part that
/// answers "what is using up my Mac" — has to win.
use crate::layout::screens::ToastRow;

const OVERVIEW_FIXED: usize = 7;
/// The table's own column header and its "… N others" roll-up.
const TABLE_CHROME: usize = 2;
const CARDS_ROWS: usize = 8;
/// The core strip plus the blank line above it.
const CORES_ROWS: usize = 2;
/// Below this the table costs more in headers than it returns in rows.
const MIN_TABLE_ROWS: usize = 3;
/// What the overview will draw, and how many rows each part gets.
///
/// This is data, not a side effect: the renderer is handed one of these and may
/// not do arithmetic of its own. That is what makes [`Self::rows_used`]
/// meaningful, and it is why the whole size sweep can run as a unit test with
/// no terminal anywhere near it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverviewPlan {
    pub toast: ToastRow,
    pub cards: bool,
    pub cores: bool,
    /// Data rows in the process table. **Never zero while the table is drawn**
    /// — see [`crate::layout::scroll::window`] for why that would be
    /// unsatisfiable rather than merely empty.
    pub table_rows: usize,
}

impl OverviewPlan {
    pub fn shows_table(&self) -> bool {
        self.table_rows > 0
    }

    /// Rows this plan occupies. The single number that says how tall the frame
    /// is, and the one the sweep asserts against the terminal height.
    pub fn rows_used(&self) -> usize {
        OVERVIEW_FIXED
            + self.toast.rows()
            + if self.cards { CARDS_ROWS } else { 0 }
            + if self.cores { CORES_ROWS } else { 0 }
            + if self.shows_table() {
                TABLE_CHROME + self.table_rows
            } else {
                0
            }
    }
}

/// Decide the overview's layout for a terminal `rows` tall.
///
/// This used to be one constant, 19, with `max(3, rows - 19)` under it. A floor
/// is not a fit: on a terminal shorter than 22 rows the table drew three rows
/// into a budget of one and the frame scrolled its own header away. Dropping a
/// section is the only way to actually get shorter. See I-26.
pub fn plan_overview(rows: usize, toast: ToastRow) -> OverviewPlan {
    let mut spare = (rows as isize) - (OVERVIEW_FIXED as isize) - toast.rows() as isize;

    let floor = (TABLE_CHROME + MIN_TABLE_ROWS) as isize;

    /* Cards go before the core strip: the strip is ten glyphs of the same
    number the CPU card already shows, and the CPU screen draws it per core
    in full. */
    let cards = spare - (CARDS_ROWS as isize) >= floor;
    if cards {
        spare -= CARDS_ROWS as isize;
    }
    let cores = spare - (CORES_ROWS as isize) >= floor;
    if cores {
        spare -= CORES_ROWS as isize;
    }

    /*
     * The table's two chrome lines are charged only when it is drawn, so that a
     * toast — which costs two rows and appears exactly when a kill has just
     * happened on a short terminal — can push the table out entirely instead of
     * leaving it with a header, a roll-up and no rows.
     */
    // The TypeScript reads `spare >= TABLE_CHROME + 1`; same thing, said the
    // way clippy prefers. At least one data row, or no table at all.
    let table_rows = if spare > TABLE_CHROME as isize {
        (spare - TABLE_CHROME as isize) as usize
    } else {
        0
    };

    OverviewPlan {
        toast,
        cards,
        cores,
        table_rows,
    }
}
