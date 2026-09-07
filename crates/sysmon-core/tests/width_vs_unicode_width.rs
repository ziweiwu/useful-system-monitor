//! Where this crate differs from `unicode-width`, measured rather than assumed.
//!
//! The obvious move when porting `src/core/width.ts` is to delete the range
//! table and call `unicode-width` instead. This file exists so that stays a
//! decision: it pins the size and shape of the gap, so a change to either side
//! fails the build instead of quietly moving the layout.
//!
//! Two findings, both of which corrected an assumption carried into the port:
//!
//! 1. **`unicode-width` handles the U+FE0F rule at the string level.** The
//!    porting note originally claimed no crate could express "the selector
//!    widens the *previous* character". That is true of the per-`char` API and
//!    false of `UnicodeWidthStr::width`, which gets `"⚠\u{FE0F}"` right. What
//!    the finding really shows is why *our* implementation must stay a stateful
//!    walk rather than a per-character map — which is the bug the TypeScript
//!    version actually shipped, in its own code, not in a dependency.
//!
//! 2. **Most of the remaining disagreement is ours, and it is a real bug.**
//!    `is_zero_width` covers only U+0300–U+036F, the Latin combining
//!    diacriticals. Hebrew points, Arabic marks and Indic matras are counted as
//!    one cell each while a terminal draws them as zero, so a name or path in
//!    those scripts measures wider than it renders — columns pad short and
//!    drift left. It does not overflow a row, which is why the layout sweep
//!    never caught it.
//!
//! (2) is **deliberately not fixed here.** The first Rust release is a parity
//! port, and a fix would widen the differential diff against the shipping Node
//! build for reasons unrelated to the port. It belongs in `POST-CUTOVER.md`,
//! applied to both implementations at once.

use sysmon_core::text::width::display_width;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Why `Cells` is a stateful iterator and not a `map` over characters.
#[test]
fn the_selector_rule_needs_a_stateful_walk_not_a_per_char_map() {
    for s in ["⚠\u{fe0f}", "⚠\u{fe0f}abc", "x⚠\u{fe0f}y"] {
        let per_char: usize = s.chars().map(|c| c.width().unwrap_or(0)).sum();
        assert_eq!(
            display_width(s),
            s.width(),
            "we should match the str-level API on {s:?}"
        );
        assert!(
            per_char < display_width(s),
            "{s:?}: a per-character sum ({per_char}) should undercount the \
             retroactive selector rule ({})",
            display_width(s)
        );
    }
}

/// Control characters: we charge one cell for most of C0, `unicode-width`
/// charges none. Held apart from the table comparison below because the cause
/// is different — `is_zero_width` deliberately exempts NUL, and `sanitize_text`
/// removes these long before they reach a width calculation anyway.
#[test]
fn control_characters_are_a_known_and_separate_divergence() {
    let controls = (0u32..0x20).chain(std::iter::once(0x7f)).chain(0x80..0xa0);
    let differing = controls
        .filter_map(char::from_u32)
        .filter(|c| display_width(&c.to_string()) != c.to_string().width())
        .count();
    assert_eq!(differing, EXPECTED_CONTROL_DIVERGENCE);
}

/// The golden number. It moving means a table changed — allowed, but only on
/// purpose and with this line updated in the same commit.
#[test]
fn the_table_disagreement_is_the_recorded_size() {
    let mut disagreeing = 0usize;
    let mut first: Vec<String> = Vec::new();
    for cp in 0u32..0x30000 {
        let Some(ch) = char::from_u32(cp) else {
            continue;
        };
        let is_control = cp < 0x20 || cp == 0x7f || (0x80..=0xa0).contains(&cp);
        if is_control {
            continue;
        }
        let s = ch.to_string();
        if display_width(&s) != s.width() {
            disagreeing += 1;
            if first.len() < 8 {
                first.push(format!("U+{cp:04X}"));
            }
        }
    }
    assert_eq!(
        disagreeing, EXPECTED_TABLE_DIVERGENCE,
        "non-control code points where we differ from unicode-width changed \
         ({disagreeing} now, {EXPECTED_TABLE_DIVERGENCE} recorded); first few {first:?}. \
         If you edited WIDE_RANGES or is_zero_width on purpose, update this constant."
    );
}

/// Measured against **unicode-width 0.2.0**, which `Cargo.toml` pins exactly
/// for this reason: the number is a property of that version's tables as much
/// as of ours, and a patch bump moves it. It did — 10,978 on the version
/// resolved earlier in this port, 10,530 on 0.2.0 — which is the assertion
/// doing its job rather than a regression.
///
/// Dominated by combining marks outside U+0300–U+036F that we count as one cell
/// and a terminal draws as zero. See the module note: a parity-preserving bug,
/// fixed post-cutover.
const EXPECTED_TABLE_DIVERGENCE: usize = 10_530;

/// Most of C0 plus DEL and C1.
const EXPECTED_CONTROL_DIVERGENCE: usize = 32;
