//! Display width, not byte or `char` count. Wide (CJK / emoji) glyphs occupy
//! two terminal cells and combining marks occupy none; using string length for
//! layout silently breaks alignment. See I-19.
//!
//! # Why this is not `unicode-width`
//!
//! Because the first release is a **parity port**, and this table is the
//! behaviour being ported. Substituting a different width oracle would change
//! what the app draws for reasons unrelated to the port, and would do it
//! everywhere at once.
//!
//! Two things worth knowing before anyone tries the substitution — both
//! measured, in `tests/width_vs_unicode_width.rs`:
//!
//! - `UnicodeWidthStr::width` **does** implement the U+FE0F rule correctly. It
//!   is the per-`char` API that cannot, since the rule is about the *previous*
//!   character. That is why [`Cells`] is a stateful walk rather than a `map`,
//!   and it is the bug the TypeScript version actually shipped — in its own
//!   code, where `truncate` measured one character at a time, so `"⚠️"` came
//!   out as 1 + 0 instead of 2 and the padded row overflowed.
//! - The two disagree on **10,530 non-control code points**, and most of that
//!   is *this table being wrong*: [`is_zero_width`] covers only U+0300–U+036F,
//!   so Hebrew points, Arabic marks and Indic matras are charged a cell each
//!   while a terminal draws them as none. Columns then pad short and drift
//!   left. It never overflows a row, which is why the layout sweep never caught
//!   it. Fixing it belongs post-cutover, applied to both implementations at
//!   once so the differential harness stays quiet.
//!
//! The port is 1:1 because JavaScript's `for (const ch of s)` and Rust's
//! `str::chars` both iterate Unicode scalar values, and it is checked against a
//! golden corpus generated from the original (`npm run gen:width-corpus`).

/// Ranges MUST stay in ascending order: [`is_wide`] early-returns on the first
/// range that starts above the code point. An out-of-order entry silently makes
/// every later range unreachable — which is how ⛔ was first measured as width
/// 1. `tests/width.rs` asserts the ordering rather than trusting it.
const WIDE_RANGES: &[(u32, u32)] = &[
    (0x1100, 0x115f),
    (0x231a, 0x231b),
    (0x2329, 0x232a),
    (0x23e9, 0x23ec),
    (0x23f0, 0x23f0),
    (0x23f3, 0x23f3),
    (0x25fd, 0x25fe),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267f, 0x267f),
    (0x2693, 0x2693),
    (0x26a1, 0x26a1),
    (0x26aa, 0x26ab),
    (0x26bd, 0x26be),
    (0x26c4, 0x26c5),
    (0x26ce, 0x26ce),
    (0x26d4, 0x26d4),
    (0x26ea, 0x26ea),
    (0x26f2, 0x26f3),
    (0x26f5, 0x26f5),
    (0x26fa, 0x26fa),
    (0x26fd, 0x26fd),
    (0x2705, 0x2705),
    (0x270a, 0x270b),
    (0x2728, 0x2728),
    (0x274c, 0x274c),
    (0x274e, 0x274e),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27b0, 0x27b0),
    (0x27bf, 0x27bf),
    (0x2b1b, 0x2b1c),
    (0x2b50, 0x2b50),
    (0x2b55, 0x2b55),
    (0x2e80, 0x303e),
    (0x3041, 0x33ff),
    (0x3400, 0x4dbf),
    (0x4e00, 0x9fff),
    (0xa000, 0xa4cf),
    (0xa960, 0xa97f),
    (0xac00, 0xd7a3),
    (0xf900, 0xfaff),
    (0xfe10, 0xfe19),
    (0xfe30, 0xfe6f),
    (0xff00, 0xff60),
    (0xffe0, 0xffe6),
    (0x1f004, 0x1f004),
    (0x1f0cf, 0x1f0cf),
    (0x1f18e, 0x1f18e),
    (0x1f191, 0x1f19a),
    (0x1f200, 0x1f320),
    (0x1f330, 0x1f335),
    (0x1f337, 0x1f37c),
    (0x1f380, 0x1f393),
    (0x1f3a0, 0x1f3ca),
    (0x1f3cf, 0x1f3d3),
    (0x1f3e0, 0x1f3f0),
    (0x1f400, 0x1f43e),
    (0x1f440, 0x1f4fc),
    (0x1f500, 0x1f53d),
    (0x1f550, 0x1f567),
    (0x1f5fb, 0x1f64f),
    (0x1f680, 0x1f6c5),
    (0x1f900, 0x1f9ff),
    (0x20000, 0x3fffd),
];

/// Exposed so a test can assert the table is sorted and non-overlapping.
#[doc(hidden)]
pub fn wide_ranges() -> &'static [(u32, u32)] {
    WIDE_RANGES
}

const VARIATION_SELECTOR_16: u32 = 0xfe0f;

fn is_wide(code_point: u32) -> bool {
    for &(lo, hi) in WIDE_RANGES {
        if code_point < lo {
            return false;
        }
        if code_point <= hi {
            return true;
        }
    }
    false
}

fn is_zero_width(code_point: u32) -> bool {
    // Combining marks, ZWJ/ZWNJ, variation selectors, control chars.
    //
    // Note `code_point < 0x20 && code_point != 0x00`: NUL is deliberately *not* zero-width here
    // and falls through to one cell. Ported as-is rather than tidied — the
    // golden corpus pins it, so changing it is a decision, not a cleanup.
    (0x0300..=0x036f).contains(&code_point)
        || (0x200b..=0x200f).contains(&code_point)
        || (0xfe00..=0xfe0f).contains(&code_point)
        || code_point == 0x20e3
        || (code_point < 0x20 && code_point != 0x00)
        || code_point == 0x7f
}

/// Each character of `s` paired with the cells it adds, left to right.
///
/// The single definition of how wide anything is — every truncate, pad and wrap
/// below consumes this one iterator, so the rule cannot disagree with itself.
///
/// U+FE0F requests emoji presentation, which renders the *preceding* narrow
/// character in two cells. It is charged one cell of its own so the pair totals
/// two, which is why this has to be a stateful walk rather than a per-character
/// map.
pub struct Cells<'a> {
    inner: core::str::Chars<'a>,
    prev_width: usize,
}

impl Iterator for Cells<'_> {
    type Item = (char, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let ch = self.inner.next()?;
        let code_point = ch as u32;

        if code_point == VARIATION_SELECTOR_16 {
            return Some(if self.prev_width == 1 {
                self.prev_width = 2;
                (ch, 1)
            } else {
                (ch, 0)
            });
        }
        if is_zero_width(code_point) {
            return Some((ch, 0));
        }
        self.prev_width = if is_wide(code_point) { 2 } else { 1 };
        Some((ch, self.prev_width))
    }
}

/// See [`Cells`].
pub fn cells(text: &str) -> Cells<'_> {
    Cells {
        inner: text.chars(),
        prev_width: 0,
    }
}

/// Terminal cells occupied by `s`.
pub fn display_width(text: &str) -> usize {
    cells(text).map(|(_, w)| w).sum()
}

/// Truncate to at most `max` cells, appending an ellipsis when cut.
pub fn truncate(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if display_width(text) <= max {
        return text.to_string();
    }
    if max == 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for (ch, cw) in cells(text) {
        if w + cw > max - 1 {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

/// Pad to exactly `n` cells (truncating if needed).
pub fn pad_end(text: &str, n: usize) -> String {
    let t = truncate(text, n);
    let pad = n.saturating_sub(display_width(&t));
    let mut out = t;
    out.extend(core::iter::repeat_n(' ', pad));
    out
}

/// Pad to exactly `n` cells, right-aligned (truncating if needed).
pub fn pad_start(text: &str, n: usize) -> String {
    let t = truncate(text, n);
    let pad = n.saturating_sub(display_width(&t));
    let mut out = String::with_capacity(pad + t.len());
    out.extend(core::iter::repeat_n(' ', pad));
    out.push_str(&t);
    out
}

/// Break `s` into lines of at most `max` cells each, measured the same way as
/// everything else here.
///
/// The detail panel used to do this by slicing UTF-16 units: a 66-unit CJK path
/// measures 98 cells, so the whole thing was packed into one "line" and the
/// renderer truncated two thirds of it away — on the one panel whose stated job
/// is telling two identically-named helpers apart.
///
/// No returned line ever exceeds `max`. As everywhere else in this module the
/// budget wins and content gives way, so a single glyph wider than the whole
/// budget is dropped rather than allowed to overflow.
pub fn wrap_to_width(text: &str, max: usize) -> Vec<String> {
    if max == 0 {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut w = 0usize;
    for (ch, cw) in cells(text) {
        if cw > max {
            continue;
        }
        // A wide glyph that would straddle the edge starts the next line.
        if w + cw > max {
            out.push(core::mem::take(&mut line));
            w = 0;
        }
        line.push(ch);
        w += cw;
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}
