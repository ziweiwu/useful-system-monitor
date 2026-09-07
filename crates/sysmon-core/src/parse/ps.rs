//! `ps` output. Pure, so every rule is testable against `test/fixtures/`.

use std::sync::LazyLock;

use jiff::{civil, tz::TimeZone};
use regex::Regex;

use crate::domain::types::{ProcessMeta, RawProcess, StartTime};
use crate::text::sanitize_text;

/// `ps -o time` prints cumulative CPU time as `MMMM:SS.ss` — minutes, seconds
/// and hundredths. It does not roll over into hours: a machine with 20 hours of
/// CPU on one process shows `1206:18.96`, not `20:06:18`.
///
/// The centisecond field is why per-process CPU% is possible at all; whole-second
/// resolution would make short sampling windows meaningless.
const MINS_PER_HOUR: u64 = 60;
const SECS_PER_MIN: u64 = 60;
const MS_PER_SEC: u64 = 1000;
const MS_PER_CENTISECOND: u64 = 10;
/// `ps` reports RSS in kilobytes.
const BYTES_PER_KB: u64 = 1024;
/// "jan", "feb" — the shortest form any locale abbreviates a month to.
const SHORTEST_MONTH_ABBREVIATION: usize = 3;
/// A four-digit number in `lstart` is the year; a small one is the day.
const FIRST_FOUR_DIGIT_YEAR: i32 = 1000;
const LAST_FOUR_DIGIT_YEAR: i32 = 9999;
const LONGEST_MONTH_DAYS: i32 = 31;

pub fn parse_cpu_time_ms(field: &str) -> Option<u64> {
    // Defensive: accept an optional hours group in case a future format adds one.
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:(\d+):)?(\d+):(\d+)(?:\.(\d{1,2}))?$").expect("static regex")
    });
    const HOURS: usize = 1;
    const MINUTES: usize = 2;
    const SECONDS: usize = 3;
    const CENTISECONDS: usize = 4;
    let caps = RE.captures(field.trim())?;
    let hours: u64 = caps
        .get(HOURS)
        .map_or(0, |m| m.as_str().parse().unwrap_or(0));
    let minutes: u64 = caps.get(MINUTES)?.as_str().parse().ok()?;
    let seconds: u64 = caps.get(SECONDS)?.as_str().parse().ok()?;
    // "1" means 10 centiseconds, not 1 — the field is left-aligned hundredths.
    let frac: u64 = caps.get(CENTISECONDS).map_or(0, |m| {
        let s = m.as_str();
        let padded = if s.len() == 1 {
            format!("{s}0")
        } else {
            s.to_string()
        };
        padded.parse().unwrap_or(0)
    });
    Some(
        ((hours * MINS_PER_HOUR + minutes) * SECS_PER_MIN + seconds) * MS_PER_SEC
            + frac * MS_PER_CENTISECOND,
    )
}

/// `ps -Ao pid,ppid,time,rss`. RSS is in kilobytes.
pub fn parse_ps_hot(stdout: &str) -> Vec<RawProcess> {
    let mut out = Vec::new();
    for line in stdout.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // `pid ppid time rss`, in that order.
        const RSS: usize = 3;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() <= RSS {
            continue;
        }
        let (Ok(pid), Ok(ppid)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) else {
            continue;
        };
        let Some(cpu_time_ms) = parse_cpu_time_ms(parts[2]) else {
            continue;
        };
        let Ok(rss_kb) = parts[RSS].parse::<u64>() else {
            continue;
        };
        out.push(RawProcess {
            pid,
            ppid,
            cpu_time_ms,
            rss_bytes: rss_kb * BYTES_PER_KB,
        });
    }
    out
}

/// `ps -Ao pid,lstart,user,state,comm`, in the C locale.
///
/// `lstart` is five whitespace-separated tokens with a padded day
/// ("Sat Aug  1 17:46:44 2026"), and COMM is a path that may itself contain
/// spaces, so this is positional rather than a naive split.
static PS_STATIC_C: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(\d+)\s+(\S+\s+\S+\s+\d+\s+\d+:\d+:\d+\s+\d+)\s+(\S+)\s+(\S+)\s+(.*)$")
        .expect("static regex")
});

/// The same line when `lstart` did not come out in the C locale.
///
/// The collector pins `LC_TIME=C` so this should be unreachable, but a name is
/// the one column a user cannot work around — a table of "pid 1234" rows is
/// unusable, and that is what a single unrecognised date format used to produce
/// for every process on the machine. The only structure every locale shares is
/// the clock, so this anchors on the first `HH:MM:SS` and takes an optional
/// trailing year. See I-28.
static PS_STATIC_ANY_LOCALE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(\d+)\s+(\S.*?\d{1,2}:\d{2}:\d{2}(?:\s+\d{4})?)\s+(\S+)\s+(\S+)\s+(.*)$")
        .expect("static regex")
});

static MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

static LSTART_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2}):(\d{2}):(\d{2})$").expect("static regex"));

/// One `lstart` field to an epoch, or [`StartTime::Unreadable`].
///
/// # Why this is a token scan and not a `strptime`
///
/// The TypeScript original calls `Date.parse`, whose behaviour on non-ISO input
/// is implementation-defined — and V8's is *lenient*. Measured on the captured
/// fixtures, it accepts all of:
///
/// ```text
/// Sat Aug  1 17:46:44 2026     C           month first, padded day
/// Wed 12 Aug 19:39:58 2026     en_GB       day first
/// Mi. 12 Aug. 19:39:58 2026    de_DE       localised weekday, abbreviated month
/// ```
///
/// and rejects only `三  8月/12 19:39:58 2026` (zh_CN), which carries no Latin
/// month at all. A strict C-locale `strptime` would therefore have diverged from
/// the shipping build on two of the three locale fixtures — quietly, since the
/// collector pins `LC_TIME=C` and never produces them live.
///
/// So this reproduces what V8 actually does: find an English month token, a day,
/// a clock and a year, in any order. Anything without a recognisable month is
/// unreadable, which is the honest answer — inventing a plausible instant for
/// the field a kill is bound to (I-16) would be worse than admitting ignorance.
pub fn parse_lstart(field: &str) -> StartTime {
    parse_lstart_in(field, &TimeZone::system())
}

/// [`parse_lstart`] against an explicit zone.
///
/// Split out so the rule is a pure function that a test can pin exactly against
/// V8's `Date.parse`, instead of only checking that the answer looks plausible.
/// The machine's zone is an input, not an ambient fact.
/// The fields `lstart` scatters across its five tokens, in whatever order and
/// spelling the locale used.
#[derive(Default)]
struct LstartFields {
    month: Option<u8>,
    day: Option<i8>,
    year: Option<i16>,
    hms: Option<(i8, i8, i8)>,
}

/// One `lstart` token, folded into whichever field it turns out to be.
fn absorb_token(fields: &mut LstartFields, token: &str) {
    const SECS: usize = 3;
    if let Some(caps) = LSTART_TIME.captures(token) {
        if fields.hms.is_none() {
            fields.hms = Some((
                caps[1].parse().unwrap_or(0),
                caps[2].parse().unwrap_or(0),
                caps[SECS].parse().unwrap_or(0),
            ));
        }
        return;
    }
    // Trailing punctuation is the de_DE case: "Aug." and "Mi.".
    let word = token.trim_end_matches('.').to_ascii_lowercase();
    if fields.month.is_none() && word.len() >= SHORTEST_MONTH_ABBREVIATION {
        if let Some(i) = MONTHS.iter().position(|m| word.starts_with(m)) {
            fields.month = Some(i as u8 + 1);
            return;
        }
    }
    if let Ok(n) = token.parse::<i32>() {
        if (FIRST_FOUR_DIGIT_YEAR..=LAST_FOUR_DIGIT_YEAR).contains(&n) {
            fields.year.get_or_insert(n as i16);
        } else if (1..=LONGEST_MONTH_DAYS).contains(&n) {
            fields.day.get_or_insert(n as i8);
        }
    }
}

pub fn parse_lstart_in(field: &str, zone: &TimeZone) -> StartTime {
    let mut fields = LstartFields::default();
    for token in field.split_whitespace() {
        absorb_token(&mut fields, token);
    }

    let (Some(month), Some(day), Some(year), Some((h, m, s))) =
        (fields.month, fields.day, fields.year, fields.hms)
    else {
        return StartTime::Unreadable;
    };
    let Ok(dt) = civil::DateTime::new(year, month as i8, day, h, m, s, 0) else {
        return StartTime::Unreadable;
    };
    // `ps` prints local time, so the conversion needs a zone — the same thing
    // `Date.parse` does for a string carrying no offset.
    match dt.to_zoned(zone.clone()) {
        Ok(zoned) => StartTime::Known(zoned.timestamp().as_millisecond()),
        Err(_) => StartTime::Unreadable,
    }
}

/// `ps -Ao pid,lstart,user,state,comm`.
pub fn parse_ps_static(stdout: &str) -> Vec<ProcessMeta> {
    let mut out = Vec::new();
    for raw in stdout.lines().skip(1) {
        if raw.trim().is_empty() {
            continue;
        }
        let Some(caps) = PS_STATIC_C
            .captures(raw)
            .or_else(|| PS_STATIC_ANY_LOCALE.captures(raw))
        else {
            continue;
        };
        let Ok(pid) = caps[1].parse::<i32>() else {
            continue;
        };
        // Sanitised on ingest so every consumer — the table, --json, the detail
        // panel — gets a string that is safe to print. `ps` escapes control
        // bytes itself today; this does not depend on that.
        const USER: usize = 3;
        const STATE: usize = 4;
        const COMMAND: usize = 5;
        out.push(ProcessMeta {
            pid,
            start_time: parse_lstart(&caps[2]),
            user: sanitize_text(&caps[USER]).into_owned(),
            state: sanitize_text(&caps[STATE]).into_owned(),
            command: sanitize_text(caps[COMMAND].trim()).into_owned(),
        });
    }
    out
}
