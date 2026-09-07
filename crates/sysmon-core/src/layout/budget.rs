//! Vertical budgeting. The horizontal equivalent lives in `layout::columns`.
//!
//! Every screen draws at least one list whose length comes from the machine
//! rather than from the design — cores, mounted volumes, top processes.
//! Rendering all of them is how a frame ends up taller than the terminal, which
//! scrolls the app's own header away. See I-26.

/// How a list was fitted into its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    /// Items to render.
    pub shown: usize,
    /// Items rolled up into a single "… N more" line. Zero when everything fits.
    pub hidden: usize,
}

impl Fit {
    /// Lines this actually occupies, roll-up included. The number a caller must
    /// subtract from its own budget.
    pub fn rows_used(self) -> usize {
        self.shown + usize::from(self.hidden > 0)
    }
}

/// How many of `total` items fit in `budget` lines.
///
/// When some must be dropped, one line of the budget is spent on the roll-up
/// that says so: a list that silently stops is indistinguishable from a machine
/// that has nothing more to show.
///
/// A budget of zero or less means the section does not fit at all; the caller
/// renders neither rows nor roll-up, which is why callers gate on the budget
/// rather than on `hidden`.
pub fn fit_list(total: usize, budget: isize) -> Fit {
    if budget <= 0 {
        return Fit {
            shown: 0,
            hidden: total,
        };
    }
    let budget = budget as usize;
    if total <= budget {
        return Fit {
            shown: total,
            hidden: 0,
        };
    }
    let shown = budget - 1;
    Fit {
        shown,
        hidden: total - shown,
    }
}
