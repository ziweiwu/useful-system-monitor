//! The process table's column widths, derived from terminal width so nothing
//! ever wraps. See I-19.

pub const CURSOR: usize = 3;
pub const PID: usize = 7;
pub const MARK: usize = 1;
pub const CPU_BAR: usize = 8;
pub const CPU_NUM: usize = 6;
pub const MEM_NUM: usize = 6;
pub const EN_BAR: usize = 5;
pub const EN_NUM: usize = 6;
pub const USER: usize = 9;
pub const NAME_MAX: usize = 34;
pub const NAME_MIN: usize = 10;
/// Trailing gutter: one space plus the scrollbar cell.
pub const GUTTER: usize = 2;
/// Columns before the name that no layout gives up: cursor, PID, CPU, memory.
pub const CORE: usize = CURSOR + PID + 1 + MARK + CPU_BAR + CPU_NUM + 2 + MEM_NUM + 2;
/// The energy bar, its number, and the gap after it.
pub const ENERGY: usize = EN_BAR + 1 + EN_NUM + 2;

/// Narrowest table that can still name a process. Below this the dashboard is
/// not drawn at all — see [`crate::layout::screens::MIN_COLUMNS`].
pub const MIN_TABLE_WIDTH: usize = CORE + GUTTER + NAME_MIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnLayout {
    pub name: usize,
    /// Zero when the terminal cannot carry the USER column.
    pub user: usize,
    /// False when it cannot carry the energy bar and its number either.
    pub energy: bool,
}

impl ColumnLayout {
    /// Cells one row actually occupies. The renderer must produce exactly this,
    /// and it must never exceed the width the layout was given.
    pub fn row_width(self) -> usize {
        CORE + if self.energy { ENERGY } else { 0 } + self.user + GUTTER + self.name
    }
}

/// Widths for a table `total_width` cells wide.
///
/// The name used to be floored at 10 columns and everything else kept, which
/// meant that at 72 columns or fewer the row was simply wider than the box it
/// was drawn in: every row wrapped onto a second line carrying nothing but the
/// scrollbar glyph, the table cost twice the height that was budgeted for it,
/// and the frame ran off the bottom of the terminal. **A floor is not a fit.**
///
/// So columns are given up instead, in the order they stop being worth their
/// width. USER goes first — on a personal Mac nearly every row says the same
/// name. Energy goes second; the BATTERY screen still shows it in full. The
/// name goes last, because it is the only column here that cannot be recovered
/// from another screen, and a table of bare PIDs is not a system monitor.
pub fn column_layout(total_width: usize) -> ColumnLayout {
    for (user, energy) in [(USER, true), (0, true), (0, false)] {
        let fixed = CORE + if energy { ENERGY } else { 0 } + user + GUTTER;
        let Some(name) = total_width.checked_sub(fixed) else {
            continue;
        };
        if name >= NAME_MIN {
            return ColumnLayout {
                name: name.min(NAME_MAX),
                user,
                energy,
            };
        }
    }
    // Narrower than MIN_TABLE_WIDTH; the app refuses to draw the dashboard at
    // all rather than let this overflow.
    ColumnLayout {
        name: NAME_MIN,
        user: 0,
        energy: false,
    }
}
