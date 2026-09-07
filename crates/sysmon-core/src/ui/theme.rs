//! One hue per metric family, and the pure glyph arithmetic that draws with it.
//!
//! A metric's card border, sparkline, gauge fill and its process-table column
//! all share a colour, so the eye tracks CPU vs MEM vs ENERGY without reading
//! headers. Colour is always *redundant* encoding — every state is readable
//! from the text alone, so the dashboard still works under `NO_COLOR` and stays
//! greppable (I-23).
//!
//! Colours are plain RGB here rather than a renderer's type, so this crate
//! stays free of ratatui and the palette can be asserted for contrast without
//! a terminal.

/// A 24-bit colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const CPU_LOW: Rgb = Rgb(0x9e, 0xce, 0x6a);
pub const CPU_MID: Rgb = Rgb(0xe0, 0xaf, 0x68);
pub const CPU_HIGH: Rgb = Rgb(0xf7, 0x76, 0x8e);
pub const MEM: Rgb = Rgb(0x7a, 0xa2, 0xf7);
pub const DISK: Rgb = Rgb(0xbb, 0x9a, 0xf7);
pub const BATTERY: Rgb = Rgb(0xff, 0x9e, 0x64);
pub const HEADLINE: Rgb = Rgb(0xff, 0xff, 0xff);
pub const TEXT: Rgb = Rgb(0xc0, 0xca, 0xf5);
/// Measured, not chosen by eye: the old `#565f89` was 3.39:1 on black and
/// 2.76:1 on a Tokyo-Night background, below WCAG 1.4.3's 4.5:1 — and it is the
/// colour of the PID column, the sample ages and the footer legend. This is the
/// least lightening along the same hue that clears 4.5:1 on both: 5.60:1 and
/// 4.56:1.
pub const DIM: Rgb = Rgb(0x79, 0x82, 0xac);
pub const TRACK: Rgb = Rgb(0x34, 0x3a, 0x54);
/// Borders are non-text UI, so 1.4.11's 3:1 applies rather than 4.5:1. The old
/// `#454c69` measured 2.49:1 / 2.03:1; this measures 3.71:1 / 3.02:1.
pub const FRAME: Rgb = Rgb(0x5f, 0x66, 0x83);
pub const SELECTION_BG: Rgb = Rgb(0x2d, 0x34, 0x52);
pub const ROOT: Rgb = Rgb(0xe0, 0xaf, 0x68);
pub const DANGER: Rgb = Rgb(0xf7, 0x76, 0x8e);

/// Thresholds at which a reading changes colour.
const CPU_BUSY_PCT: f64 = 60.0;
const CPU_HOT_PCT: f64 = 85.0;
const DISK_FILLING_PCT: f64 = 75.0;
/// macOS starts failing writes and dropping snapshots past this.
const DISK_NEARLY_FULL_PCT: f64 = 90.0;
/// What a non-finite sample is drawn as. See I-19.
const NOT_A_READING: f64 = 0.0;
/// A sparkline with no explicit maximum is scaled as a percentage.
const FULL_SCALE_PCT: f64 = 100.0;
/// The top of the normalised 0..1 range a block height is chosen from.
const FULL_HEIGHT: f64 = 1.0;
/// Every glyph in `BLOCKS` is three UTF-8 bytes.
const MAX_UTF8_LEN_PER_BLOCK: usize = 3;

/// Green below 60%, amber to 85%, red above.
pub fn severity(pct: f64) -> Rgb {
    if pct < CPU_BUSY_PCT {
        CPU_LOW
    } else if pct < CPU_HOT_PCT {
        CPU_MID
    } else {
        CPU_HIGH
    }
}

/// A volume near capacity is a different kind of problem from a busy CPU:
/// macOS starts failing writes and dropping snapshots past 90%.
pub fn disk_severity(pct: f64) -> Rgb {
    if pct < DISK_FILLING_PCT {
        DISK
    } else if pct < DISK_NEARLY_FULL_PCT {
        CPU_MID
    } else {
        CPU_HIGH
    }
}

pub const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// I-19: a value that is not a number must still occupy its cell.
///
/// Both functions below are pure width arithmetic, and in the TypeScript
/// original NaN propagated straight through it: `'█'.repeat(NaN)` is `''` and
/// `BLOCKS[NaN]` is `undefined`, so a single bad sample silently made a bar 0
/// cells wide instead of 10 — a layout break with no error anywhere. NaN
/// reaches here from any ratio with a zero denominator (`df` reporting 0 blocks
/// for a mount), and once one lands in a history ring it also poisons the
/// maximum that scales the whole sparkline, for sixty samples.
fn finite(n: f64) -> f64 {
    if n.is_finite() {
        n
    } else {
        NOT_A_READING
    }
}

/// Unicode block sparkline. Every glyph is single-width (see `text::width`).
pub fn sparkline(values: &[f64], width: usize, max: f64) -> String {
    if width == 0 {
        return String::new();
    }
    let scale = if max.is_finite() && max > NOT_A_READING {
        max
    } else {
        FULL_SCALE_PCT
    };
    let start = values.len().saturating_sub(width);
    let slice = &values[start..];
    let pad = width - slice.len();
    let mut out = String::with_capacity(width * MAX_UTF8_LEN_PER_BLOCK);
    for _ in 0..pad {
        out.push(' ');
    }
    for v in slice {
        let height = (finite(*v) / scale).clamp(NOT_A_READING, FULL_HEIGHT);
        let idx = ((height * (BLOCKS.len() - 1) as f64).round() as usize).min(BLOCKS.len() - 1);
        out.push(BLOCKS[idx]);
    }
    out
}

/// Horizontal bar: filled cells plus a dim track remainder.
pub fn bar_cells(pct: f64, width: usize) -> (usize, usize) {
    let cells = ((finite(pct) / FULL_SCALE_PCT) * width as f64).round();
    let filled = if cells.is_finite() {
        (cells.max(NOT_A_READING) as usize).min(width)
    } else {
        0
    };
    (filled, width - filled)
}
