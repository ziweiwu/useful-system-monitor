//! The formatters must agree with the TypeScript original, character for
//! character.
//!
//! Every row of the process table is formatted through these, so a rounding
//! difference is visible on screen and in the one-shot text output. The two
//! languages disagree by default: `Number.prototype.toFixed` rounds half **up**
//! per ECMA-262, Rust's `{:.1}` rounds half to **even**, and the difference is
//! reachable — 1280 bytes is exactly 1.25 K, rendered `1.3K` by JavaScript and
//! `1.2K` by a naive port.
//!
//! Regenerate with `npm run gen:format-corpus`.

use serde::Deserialize;
use sysmon_core::domain::format::{age, bytes, duration, minutes_to_hm, percent};

#[derive(Deserialize)]
struct Corpus {
    bytes: Vec<(u64, String)>,
    percent1: Vec<(f64, String)>,
    percent0: Vec<(f64, String)>,
    duration: Vec<(u64, String)>,
    #[serde(rename = "minutesToHm")]
    minutes_to_hm: Vec<(i64, String)>,
    age: Vec<(i64, String)>,
}

fn corpus() -> Corpus {
    serde_json::from_str(include_str!("fixtures/format-corpus.json")).expect("corpus parses")
}

#[test]
fn bytes_matches_the_typescript_oracle() {
    let c = corpus();
    assert!(c.bytes.len() > 1_000, "corpus looks thin");
    for (n, expected) in &c.bytes {
        assert_eq!(&bytes(*n), expected, "bytes({n})");
    }
}

/// The tie cases specifically, called out so a failure here reads as what it is.
#[test]
fn bytes_rounds_exact_binary_ties_the_way_javascript_does() {
    assert_eq!(bytes(1280), "1.3K"); // exactly 1.25 K — half up, not half even
    assert_eq!(bytes(1024 + 512), "1.5K");
    assert_eq!(bytes(2304), "2.3K"); // exactly 2.25 K
}

#[test]
fn percent_matches_the_typescript_oracle() {
    let c = corpus();
    for (n, expected) in &c.percent1 {
        assert_eq!(&percent(Some(*n), 1), expected, "percent({n}, 1)");
    }
    for (n, expected) in &c.percent0 {
        assert_eq!(&percent(Some(*n), 0), expected, "percent({n}, 0)");
    }
}

/// I-1: a number that was not measured is not reported as zero.
#[test]
fn percent_of_nothing_is_an_em_dash_not_zero() {
    assert_eq!(percent(None, 1), "—");
    assert_eq!(minutes_to_hm(None), "—");
}

#[test]
fn duration_matches_the_typescript_oracle() {
    for (s, expected) in &corpus().duration {
        assert_eq!(&duration(*s), expected, "duration({s})");
    }
}

#[test]
fn minutes_to_hm_matches_the_typescript_oracle() {
    for (m, expected) in &corpus().minutes_to_hm {
        assert_eq!(&minutes_to_hm(Some(*m)), expected, "minutes_to_hm({m})");
    }
}

#[test]
fn age_matches_the_typescript_oracle() {
    for (ms, expected) in &corpus().age {
        assert_eq!(&age(0, *ms), expected, "age(0, {ms})");
    }
}

/// A clock that has run backwards — an NTP step between sample and render —
/// must not print a negative age.
#[test]
fn age_never_goes_negative() {
    assert_eq!(age(5_000, 0), "0s");
}
