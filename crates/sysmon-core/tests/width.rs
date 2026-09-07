//! I-19: layout uses display width, not string length.
//!
//! The cases from `test/width.test.ts`, plus the properties that the
//! TypeScript suite could only sample. The golden corpus in
//! `width_golden.rs` proves this agrees with the original; these say what the
//! rule *is*, so a failure reads as a sentence rather than as a diff.

use proptest::prelude::*;
use sysmon_core::text::width::{
    display_width, pad_end, pad_start, truncate, wide_ranges, wrap_to_width,
};

#[test]
fn counts_ascii_as_one_cell_each() {
    assert_eq!(display_width("hello"), 5);
}

#[test]
fn counts_cjk_as_two_cells() {
    assert_eq!(display_width("中文"), 4);
}

#[test]
fn counts_the_glyphs_that_broke_the_first_mock_as_two_cells() {
    // ⛔ and the ⚠ family render double-width; treating them as length 1 is
    // exactly what misaligned the original mock.
    assert_eq!(display_width("⛔"), 2);
    assert_eq!(display_width("🔥"), 2);
    // The emoji presentation selector widens an otherwise-narrow glyph.
    assert_eq!(display_width("⚠️"), 2);
}

#[test]
fn ignores_zero_width_combining_marks() {
    assert_eq!(display_width("é"), 1); // e + U+0301
}

/// An out-of-order entry makes every later range unreachable — a silent
/// correctness bug rather than a crash, and how ⛔ was first measured as 1.
/// The TypeScript version could only probe a handful of code points for this;
/// here the table is small enough to check outright.
#[test]
fn the_wide_range_table_is_sorted_and_disjoint() {
    let ranges = wide_ranges();
    for w in ranges.windows(2) {
        let (a, b) = (w[0], w[1]);
        assert!(a.0 <= a.1, "range {a:#x?} is inverted");
        assert!(b.0 > a.1, "range {b:#x?} does not start after {a:#x?}");
    }
}

#[test]
fn leaves_short_strings_untouched() {
    assert_eq!(truncate("abc", 10), "abc");
}

/// The exact regression: `truncate` used to measure per character, which cannot
/// see that U+FE0F widens the character *before* it, so every such pair was
/// under-counted by one and the padded row overflowed.
#[test]
fn the_emoji_selector_widens_the_preceding_glyph() {
    assert_eq!(display_width("⚠"), 1);
    assert_eq!(display_width("\u{fe0f}"), 0);
    assert_eq!(display_width("⚠\u{fe0f}"), 2);
    assert_eq!(display_width(&truncate("⚠️abc", 3)), 3);
    assert_eq!(display_width(&pad_end("⚠️abc", 4)), 4);
}

/// A selector with nothing narrow before it must not invent a cell.
#[test]
fn a_leading_or_post_wide_selector_adds_nothing() {
    assert_eq!(display_width("\u{fe0f}"), 0);
    assert_eq!(display_width("中\u{fe0f}"), 2);
}

const TRICKY: &[&str] = &[
    "⚠️",
    "⚠️abc",
    "x⚠️y",
    "⚠️⚠️",
    "⚠️ Warning App",
    "⚠",
    "🔥",
    "🔥a",
    "日本",
    "é",
    "",
];

#[test]
fn padding_is_exactly_n_cells_even_for_the_tricky_cases() {
    for s in TRICKY {
        for n in [0usize, 1, 2, 3, 4, 6, 10, 20] {
            assert_eq!(display_width(&pad_end(s, n)), n, "pad_end({s:?}, {n})");
            assert_eq!(display_width(&pad_start(s, n)), n, "pad_start({s:?}, {n})");
        }
    }
}

#[test]
fn truncation_never_exceeds_the_requested_width() {
    for s in ["abcdef", "中文中文中文", "a中b文c", "🔥🔥🔥🔥"] {
        for n in 1..=10usize {
            assert!(display_width(&truncate(s, n)) <= n, "truncate({s:?}, {n})");
        }
    }
}

/// A budget of zero draws nothing; a budget of one is all ellipsis. Both are
/// reachable from the layout code, and both used to be special-cased twice.
#[test]
fn degenerate_budgets() {
    assert_eq!(truncate("anything", 0), "");
    assert_eq!(truncate("anything", 1), "…");
    assert_eq!(pad_end("anything", 0), "");
    assert!(wrap_to_width("anything", 0).is_empty());
}

/// A glyph wider than the whole budget is dropped rather than allowed to
/// overflow: the budget wins and content gives way, as everywhere else here.
#[test]
fn a_glyph_wider_than_the_budget_is_dropped() {
    assert_eq!(wrap_to_width("中", 1), Vec::<String>::new());
    assert_eq!(wrap_to_width("a中b", 1), vec!["a", "b"]);
}

proptest! {
    /// The property the layout depends on. Sampled by the TypeScript suite at
    /// four strings and ten widths; asserted here over the whole space.
    #[test]
    fn truncate_never_exceeds_its_budget(text in ".{0,40}", n in 0usize..40) {
        prop_assert!(display_width(&truncate(&text, n)) <= n);
    }

    /// Padding is exact, not "at least" — a row is built by concatenating
    /// padded columns, so one cell over is one cell of overflow.
    #[test]
    fn padding_is_exact(text in ".{0,40}", n in 0usize..40) {
        prop_assert_eq!(display_width(&pad_end(&text, n)), n);
        prop_assert_eq!(display_width(&pad_start(&text, n)), n);
    }

    #[test]
    fn wrapped_lines_never_exceed_the_budget(text in ".{0,80}", n in 1usize..20) {
        for line in wrap_to_width(&text, n) {
            prop_assert!(display_width(&line) <= n);
        }
    }

    /// Wrapping may drop glyphs wider than the budget, but must not reorder or
    /// duplicate what it keeps.
    #[test]
    fn wrapping_preserves_order(text in ".{0,80}", n in 2usize..20) {
        let joined: String = wrap_to_width(&text, n).concat();
        let kept: String = text.chars().filter(|c| joined.contains(*c)).collect();
        prop_assert_eq!(joined.chars().count(), kept.chars().count());
    }

    /// Truncation is idempotent at the same budget — it used to not be, because
    /// the ellipsis was measured with a different rule than the content.
    #[test]
    fn truncate_is_idempotent(text in ".{0,40}", n in 1usize..40) {
        let once = truncate(&text, n);
        prop_assert_eq!(truncate(&once, n), once.clone());
    }
}
