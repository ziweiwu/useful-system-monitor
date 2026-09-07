//! I-19 / I-26: the layout never exceeds the terminal it was given.
//!
//! This is the module that carried most of this app's shipped bugs, and they
//! were all the same shape: a constant counted but never checked. So the
//! centrepiece here is not a list of cases — it is one property asserted over
//! the whole size space, cheaply, with no terminal involved:
//!
//! > **a plan never uses more rows or cells than it was given.**
//!
//! The TypeScript build could only get this from `verify:layout`, which renders
//! 1,260 real frames and takes minutes. Because the plan is a pure function
//! here, the same sweep is a unit test that runs in milliseconds — and it covers
//! far more sizes than the sweep ever did.

use proptest::prelude::*;
use sysmon_core::layout::budget::fit_list;
use sysmon_core::layout::columns::{column_layout, MIN_TABLE_WIDTH};
use sysmon_core::layout::overview::plan_overview;
use sysmon_core::layout::screens::{
    content_width, too_small, view_rows, ToastRow, MIN_COLUMNS, MIN_ROWS,
};

/// The proptest strategies generate a bool; the layout takes the type.
const TOAST_ROWS: [ToastRow; 2] = [ToastRow::Hidden, ToastRow::Shown];
use sysmon_core::layout::scroll::{scrollbar_cells, window};

// ------------------------------------------------- I-26: list row budgets --

#[test]
fn i26_shows_everything_when_it_fits() {
    assert_eq!(fit_list(5, 10).shown, 5);
    assert_eq!(fit_list(5, 10).hidden, 0);
    assert_eq!(fit_list(5, 5).shown, 5);
    assert_eq!(fit_list(5, 5).hidden, 0);
}

/// 4 lines for 10 items: three rows plus "… 7 more". A list that silently stops
/// is indistinguishable from a machine with nothing more to show.
#[test]
fn i26_spends_one_line_of_the_budget_on_the_rollup_when_it_does_not() {
    let f = fit_list(10, 4);
    assert_eq!((f.shown, f.hidden), (3, 7));
    assert_eq!(f.rows_used(), 4);
}

/// A section with no budget must not be rendered at all — including its
/// roll-up, which is why callers gate on the budget rather than on `hidden`.
#[test]
fn i26_shows_nothing_at_a_budget_of_zero_or_less() {
    for budget in [0, -3] {
        let f = fit_list(10, budget);
        assert_eq!((f.shown, f.hidden), (0, 10));
        assert_eq!(f.rows_used(), 1, "the caller must not draw this at all");
    }
}

proptest! {
    #[test]
    fn i26_fit_list_never_exceeds_its_budget_and_never_loses_an_item(
        total in 0usize..500,
        budget in 1isize..200,
    ) {
        let f = fit_list(total, budget);
        prop_assert_eq!(f.shown + f.hidden, total);
        prop_assert!(f.rows_used() <= budget as usize);
    }
}

// ------------------------------------------- I-26: the overview's budget --

/// The regression: `max(3, rows - 19)` drew three rows into a budget of one on
/// a terminal shorter than 22 rows, and the frame scrolled its own header away.
///
/// Only asserted at sizes the dashboard actually draws at: below [`MIN_ROWS`]
/// it renders nothing but its own "terminal too small" message, so the fixed
/// chrome it would otherwise cost is not on screen.
#[test]
fn i26_the_overview_plan_never_exceeds_the_terminal() {
    for rows in MIN_ROWS..200usize {
        for toast in [false, true] {
            let plan = plan_overview(rows, TOAST_ROWS[usize::from(toast)]);
            assert!(
                plan.rows_used() <= rows,
                "rows={rows} toast={toast}: plan used {} of {rows}",
                plan.rows_used()
            );
        }
    }
}

/// A window of zero rows cannot contain the selection, so the table is either
/// drawn with real rows or not drawn at all.
#[test]
fn i26_the_table_is_never_drawn_with_zero_rows() {
    for rows in 0..200usize {
        for toast in [false, true] {
            let plan = plan_overview(rows, TOAST_ROWS[usize::from(toast)]);
            assert_eq!(plan.shows_table(), plan.table_rows > 0, "rows={rows}");
        }
    }
}

/// A toast costs two rows and appears exactly when a kill has just happened on
/// a short terminal. It may push the table out entirely, which is better than
/// leaving it with a header, a roll-up and no rows.
#[test]
fn i26_a_toast_may_push_the_table_out_entirely() {
    assert!(!plan_overview(MIN_ROWS, ToastRow::Shown).shows_table());
    assert!(plan_overview(MIN_ROWS, ToastRow::Hidden).shows_table());
}

/// At the documented minimum the dashboard still draws a usable table, rather
/// than the minimum also being the point at which the table is empty.
#[test]
fn i26_the_minimum_terminal_still_draws_a_table() {
    let plan = plan_overview(MIN_ROWS, ToastRow::Hidden);
    assert!(
        plan.shows_table(),
        "at {MIN_ROWS} rows the table should still draw"
    );
    assert!(plan.table_rows >= 1);
}

/// Characterisation, not endorsement — this pins a **known wart** so the port
/// stays faithful and so changing it later is a deliberate act.
///
/// The two sections are independent affordability checks run in sequence, not a
/// priority order, and the result is not monotonic in terminal height:
///
/// ```text
/// rows   cards  cores  table
///   19   false  true       8
///   20   true   false      3   <- the core strip VANISHES as the terminal grows
///   21   true   false      4
///   22   true   true       3   <- and the table shrinks again when it returns
/// ```
///
/// So growing the window can make the process table smaller and can make a
/// whole section disappear. This is the same shape as the "a note's separator
/// must wait for slack" rule that `DiskView` and `BatteryView` each carry a
/// comment about having fixed — the overview never got the same treatment.
/// Recorded in `POST-CUTOVER.md`; not fixed here, because a parity port must
/// not change what the app draws.
#[test]
fn i26_the_overview_section_toggling_is_pinned_as_it_ships() {
    let at = |rows: usize| {
        let p = plan_overview(rows, ToastRow::Hidden);
        (p.cards, p.cores, p.table_rows)
    };
    assert_eq!(at(13), (false, false, 4));
    assert_eq!(at(14), (false, true, 3));
    assert_eq!(at(19), (false, true, 8));
    assert_eq!(at(20), (true, false, 3));
    assert_eq!(at(21), (true, false, 4));
    assert_eq!(at(22), (true, true, 3));
    assert_eq!(at(29), (true, true, 10));
}

proptest! {
    /// The whole size space, not the 14x9 grid the shipped sweep covers.
    #[test]
    fn i26_overview_plan_fits_any_terminal(
        rows in MIN_ROWS..300usize,
        toast in any::<bool>(),
    ) {
        prop_assert!(plan_overview(rows, TOAST_ROWS[usize::from(toast)]).rows_used() <= rows);
    }

    /// Whatever the section toggling does, the frame is always fully spent or
    /// under-spent — never over.
    #[test]
    fn i26_the_table_absorbs_whatever_the_sections_leave(rows in MIN_ROWS..300usize) {
        let p = plan_overview(rows, ToastRow::Hidden);
        // With no toast the table always draws above the minimum, and the plan
        // then accounts for every row of the terminal exactly.
        prop_assert!(p.shows_table());
        prop_assert_eq!(p.rows_used(), rows);
    }
}

// ------------------------------------------------ I-19: column budgeting --

/// The regression: the name was floored at 10 columns and everything else kept,
/// so at 72 columns the row was wider than the box it was drawn in, every row
/// wrapped, and the table cost twice its budgeted height. A floor is not a fit.
#[test]
fn i19_a_row_never_exceeds_the_width_it_was_given() {
    for width in MIN_TABLE_WIDTH..400 {
        let layout = column_layout(width);
        assert!(
            layout.row_width() <= width,
            "width={width}: row is {} cells",
            layout.row_width()
        );
    }
}

/// Columns are given up in the order they stop being worth their width: USER
/// first, then energy, and the name last — a table of bare PIDs is not a system
/// monitor.
#[test]
fn i19_columns_are_given_up_in_the_documented_order() {
    let wide = column_layout(200);
    assert!(wide.user > 0 && wide.energy);

    let mut saw_no_user = false;
    let mut saw_no_energy = false;
    for width in (MIN_TABLE_WIDTH..200).rev() {
        let l = column_layout(width);
        if l.user == 0 {
            saw_no_user = true;
        }
        if !l.energy {
            saw_no_energy = true;
            // Energy is only given up after USER has already gone.
            assert_eq!(
                l.user, 0,
                "width={width}: energy dropped while USER remained"
            );
        }
    }
    assert!(
        saw_no_user && saw_no_energy,
        "both columns should be droppable in this range"
    );
}

#[test]
fn i19_the_name_column_never_falls_below_its_floor() {
    for width in MIN_TABLE_WIDTH..400 {
        assert!(column_layout(width).name >= 10, "width={width}");
    }
}

proptest! {
    #[test]
    fn i19_row_width_fits_at_any_width(width in 0usize..500) {
        let l = column_layout(width);
        // Below MIN_TABLE_WIDTH the dashboard refuses to draw at all, so only
        // the drawable range has to fit.
        if width >= MIN_TABLE_WIDTH {
            prop_assert!(l.row_width() <= width);
        }
    }
}

// ---------------------------------------------------- screens and floors --

#[test]
fn the_minimum_size_is_derived_from_the_table_not_guessed() {
    assert_eq!(MIN_COLUMNS, MIN_TABLE_WIDTH + 2);
    assert!(too_small(MIN_COLUMNS - 1, MIN_ROWS));
    assert!(too_small(MIN_COLUMNS, MIN_ROWS - 1));
    assert!(!too_small(MIN_COLUMNS, MIN_ROWS));
}

/// Laying out narrower than the renderer only wastes space; laying out wider
/// corrupts the frame. See I-19.
#[test]
fn i19_content_is_never_laid_out_wider_than_the_terminal() {
    for columns in 1..300usize {
        assert!(content_width(columns) <= columns, "columns={columns}");
    }
}

proptest! {
    #[test]
    fn i26_view_rows_never_exceeds_the_terminal(rows in 10usize..300, toast in any::<bool>()) {
        // The floor of 4 is deliberate and only reachable below MIN_ROWS, where
        // the dashboard is not drawn at all.
        let v = view_rows(rows, TOAST_ROWS[usize::from(toast)]);
        prop_assert!(v <= rows || rows < MIN_ROWS);
    }
}

// ------------------------------- I-26: the window always holds the cursor --

#[test]
fn i26_the_window_always_contains_the_selection() {
    for total in [0usize, 1, 5, 50, 500] {
        for table_rows in [0usize, 1, 3, 10, 40] {
            for scroll_top in [0usize, 3, 400] {
                assert_selection_visible(total, table_rows, scroll_top);
            }
        }
    }
}

/// Every selection worth trying at one table size: none, the first row, the
/// last, and the middle.
fn assert_selection_visible(total: usize, table_rows: usize, scroll_top: usize) {
    for sel in [
        None,
        Some(0),
        Some(total.saturating_sub(1)),
        Some(total / 2),
    ] {
        let w = window(sel.filter(|s| *s < total), total, table_rows, scroll_top);
        if let Some(sel) = sel.filter(|s| *s < total) {
            assert!(
                w.contains(sel),
                "total={total} rows={table_rows} top={scroll_top} sel={sel} -> {w:?}"
            );
        }
    }
}

/// The window is at least one row even when the table is not drawn — a zero-row
/// window has no offset that contains anything, and in the TypeScript build
/// that was a fixpoint that React reported as "Maximum update depth exceeded".
#[test]
fn i26_the_window_is_never_zero_rows() {
    assert_eq!(window(Some(0), 10, 0, 0).rows.get(), 1);
}

proptest! {
    #[test]
    fn i26_window_holds_the_selection_and_stays_in_range(
        total in 0usize..500,
        table_rows in 0usize..60,
        scroll_top in 0usize..600,
        sel_raw in 0usize..500,
    ) {
        let sel = (total > 0).then(|| sel_raw % total);
        let w = window(sel, total, table_rows, scroll_top);
        if let Some(sel) = sel {
            prop_assert!(w.contains(sel), "{w:?} does not contain {sel} of {total}");
        }
        // The window never starts past the end of a non-empty list.
        if total > 0 {
            prop_assert!(w.offset < total);
        }
    }
}

// ------------------------------------------------------------- scrollbar --

fn thumb(cells: &[bool]) -> Option<(usize, usize)> {
    let start = cells.iter().position(|c| *c)?;
    let mut end = start;
    while cells.get(end) == Some(&true) {
        end += 1;
    }
    assert!(
        cells[end..].iter().all(|c| !c),
        "the thumb must be one contiguous run"
    );
    Some((start, end - start))
}

#[test]
fn i26_the_scrollbar_renders_nothing_when_the_whole_list_fits() {
    assert!(scrollbar_cells(10, 20, 0).is_empty());
    assert!(scrollbar_cells(20, 20, 0).is_empty());
    assert!(scrollbar_cells(0, 20, 0).is_empty());
}

#[test]
fn i26_the_thumb_pins_to_each_end_of_the_track() {
    assert_eq!(thumb(&scrollbar_cells(100, 10, 0)).expect("a thumb").0, 0);
    let (start, size) = thumb(&scrollbar_cells(100, 10, 90)).expect("a thumb");
    assert_eq!(start + size, 10);
}

proptest! {
    #[test]
    fn i26_the_thumb_stays_inside_the_track(
        total in 1usize..5000,
        window_size in 1usize..200,
        raw_offset in 0usize..5000,
    ) {
        let offset = raw_offset.min(total.saturating_sub(window_size));
        let cells = scrollbar_cells(total, window_size, offset);
        if total <= window_size {
            prop_assert!(cells.is_empty());
            return Ok(());
        }
        prop_assert_eq!(cells.len(), window_size);
        let (start, size) = thumb(&cells).expect("a thumb always exists");
        prop_assert!(size >= 1);
        prop_assert!(start + size <= window_size);
    }

    /// A thumb that jumps backwards as you scroll down reads as a broken widget.
    #[test]
    fn i26_the_thumb_never_moves_backwards_as_the_offset_grows(
        total in 21usize..2000,
        window_size in 2usize..20,
    ) {
        let mut prev = 0usize;
        for offset in 0..=(total - window_size) {
            let (start, _) = thumb(&scrollbar_cells(total, window_size, offset)).expect("a thumb");
            prop_assert!(start >= prev, "offset {offset}: {start} < {prev}");
            prev = start;
        }
    }
}
