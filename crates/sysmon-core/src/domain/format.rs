//! Human-readable formatting. Units are dimmed separately in the UI.

use jiff::{tz::TimeZone, Timestamp};

/// Zero as a float, where the comparison is about sign or a floor rather than
/// arithmetic.
const ZERO: f64 = 0.0;
/// The tie `js_round` breaks upwards.
const HALF: f64 = 0.5;
const MS_PER_SEC: f64 = 1000.0;
const SECS_PER_MIN: u64 = 60;
const SECS_PER_HOUR: u64 = 3600;
const SECS_PER_DAY: u64 = 86_400;
const MINS_PER_HOUR: i64 = 60;
/// Binary units: this is a byte count, not an SI one.
const BYTES_PER_UNIT: u64 = 1024;
/// Above this a tenth is noise, so it is dropped to keep the column narrow.
const THREE_FIGURE_CUTOFF: f64 = 100.0;

/// JavaScript's `Number.prototype.toFixed`, which is **not** what Rust's
/// `{:.N}` does.
///
/// Rust rounds half to even; ECMA-262 says to pick the integer `n` minimising
/// `|n / 10^f - x|` and, on a tie, **the larger n** — round half up. The two
/// disagree on values that are exact ties in binary, and those are reachable
/// here: 1280 bytes is exactly 1.25 K, which JavaScript renders `1.3K` and
/// `{:.1}` renders `1.2K`.
///
/// Working on the decimal expansion rather than multiplying by `10^f` avoids
/// introducing a second rounding error: `0.35 * 10` is `3.4999999999999996`,
/// and deciding the tie from that would give the wrong answer for a value that
/// is not actually a tie. Rust prints the *exact* decimal expansion of a double
/// at a given precision, so asking for extra places and rounding the string is
/// faithful to the stored value.
///
/// Only non-negative input is used here, so "larger n" and "away from zero"
/// coincide.
/// Adds one to a decimal expansion held as digits, carrying leftwards and
/// growing the number if the carry runs off the front.
fn increment_decimal(digits_vec: &mut Vec<u8>) {
    const HIGHEST_DIGIT: u8 = 9;
    let mut i = digits_vec.len();
    loop {
        if i == 0 {
            digits_vec.insert(0, 1);
            return;
        }
        i -= 1;
        if digits_vec[i] == HIGHEST_DIGIT {
            digits_vec[i] = 0;
        } else {
            digits_vec[i] += 1;
            return;
        }
    }
}

pub fn to_fixed(x: f64, digits: usize) -> String {
    if !x.is_finite() {
        // Matches JS, which prints "NaN" / "Infinity" from toFixed.
        return format!("{x}");
    }
    let negative = x < ZERO;
    let v = x.abs();

    // Enough extra places to decide any tie the expansion actually contains.
    const GUARD: usize = 20;
    let exact = format!("{:.*}", digits + GUARD, v);
    let (int_part, frac_part) = exact.split_once('.').unwrap_or((exact.as_str(), ""));

    let kept = &frac_part[..digits];
    let rest = &frac_part[digits..];

    // Round half up: a remainder of exactly 5000… is a tie and goes up.
    let round_up = matches!(rest.chars().next(), Some(c) if c >= '5');

    let mut digits_vec: Vec<u8> = int_part
        .bytes()
        .chain(kept.bytes())
        .map(|b| b - b'0')
        .collect();
    if round_up {
        increment_decimal(&mut digits_vec);
    }

    let mut out = render_decimal(&digits_vec, digits);
    if negative {
        out.insert(0, '-');
    }
    out
}

/// Digits back to text, with the point put back `digits` places from the end.
fn render_decimal(digits_vec: &[u8], digits: usize) -> String {
    let s: String = digits_vec.iter().map(|d| (d + b'0') as char).collect();
    let split = s.len() - digits;
    let mut out = String::from(&s[..split]);
    if digits > 0 {
        out.push('.');
        out.push_str(&s[split..]);
    }
    out
}

/// `Math.round`, which is `floor(x + 0.5)` — not Rust's half-away-from-zero.
///
/// Public because the scrollbar thumb is positioned with it too, and the two
/// must round the same way or the thumb lands a row off the selection.
pub fn js_round(x: f64) -> f64 {
    (x + HALF).floor()
}

/// Bytes with a unit suffix: `512B`, `1.2K`, `926G`.
///
/// The negative and non-finite guards the TypeScript version carries are
/// unrepresentable here, because a byte count is a `u64`.
pub fn bytes(n: u64) -> String {
    if n < BYTES_PER_UNIT {
        return format!("{n}B");
    }
    const UNITS: [&str; 4] = ["K", "M", "G", "T"];
    let step = BYTES_PER_UNIT as f64;
    let mut v = n as f64 / step;
    let mut i = 0;
    while v >= step && i < UNITS.len() - 1 {
        v /= step;
        i += 1;
    }
    // Three significant figures: past 100 the tenth is noise, and dropping it
    // keeps the column one cell narrower.
    if v >= THREE_FIGURE_CUTOFF {
        format!("{}{}", js_round(v), UNITS[i])
    } else {
        format!("{}{}", to_fixed(v, 1), UNITS[i])
    }
}

/// A percentage, or an em dash when it was never measured. See I-1 — a number
/// that was not measured is not reported as zero.
pub fn percent(n: Option<f64>, digits: usize) -> String {
    match n {
        None => "—".to_string(),
        Some(v) => to_fixed(v, digits),
    }
}

pub fn duration(sec: u64) -> String {
    let days = sec / SECS_PER_DAY;
    let hours = (sec % SECS_PER_DAY) / SECS_PER_HOUR;
    let minutes = (sec % SECS_PER_HOUR) / SECS_PER_MIN;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

pub fn minutes_to_hm(min: Option<i64>) -> String {
    match min {
        None => "—".to_string(),
        Some(min) => format!("{}:{:02}", min / MINS_PER_HOUR, min % MINS_PER_HOUR),
    }
}

/// Wall-clock `HH:MM:SS` in the machine's zone.
pub fn clock_time(epoch_ms: i64) -> String {
    clock_time_in(epoch_ms, &TimeZone::system())
}

/// [`clock_time`] against an explicit zone, so it is a pure function a test can
/// pin without depending on where the machine is.
pub fn clock_time_in(epoch_ms: i64, zone: &TimeZone) -> String {
    let Ok(ts) = Timestamp::from_millisecond(epoch_ms) else {
        return "--:--:--".to_string();
    };
    let local = ts.to_zoned(zone.clone());
    format!(
        "{:02}:{:02}:{:02}",
        local.hour(),
        local.minute(),
        local.second()
    )
}

/// "3s ago" style sample age. Panels are per-tier, so age is visible. See I-4.
pub fn age(sampled_at_ms: i64, now_ms: i64) -> String {
    let seconds = js_round(((now_ms - sampled_at_ms) as f64) / MS_PER_SEC).max(ZERO) as i64;
    if seconds < SECS_PER_MIN as i64 {
        format!("{seconds}s")
    } else {
        format!("{}m", seconds / SECS_PER_MIN as i64)
    }
}
