//! I-28 / I-16: `parse_lstart` must agree with V8's `Date.parse`, exactly.
//!
//! `Date.parse` is implementation-defined for the formats `ps -o lstart`
//! produces, and V8's implementation is lenient in ways no specification
//! describes: it accepts the C, en_GB and de_DE forms and rejects only zh_CN,
//! which carries no Latin month. Reimplementing that from first principles
//! would have quietly diverged on two of the three locale fixtures, so it is
//! recorded from the real thing instead.
//!
//! This matters beyond tidiness: the start time is the field a kill is bound to
//! (I-16). A parser that reads one more or one fewer format than the shipping
//! build makes a different set of kills refusable.
//!
//! Regenerate with `npm run gen:lstart-corpus`.

use jiff::tz::TimeZone;
use serde::Deserialize;
use sysmon_core::domain::types::StartTime;
use sysmon_core::parse::ps::parse_lstart_in;

#[derive(Deserialize)]
struct Case {
    field: String,
    #[serde(rename = "epochMs")]
    epoch_ms: Option<i64>,
}

#[derive(Deserialize)]
struct Corpus {
    tz: String,
    cases: Vec<Case>,
}

#[test]
fn i28_lstart_matches_v8_date_parse_on_every_captured_field() {
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/lstart-corpus.json")).expect("corpus parses");
    let tz = TimeZone::get(&corpus.tz).expect("the recorded zone should resolve");

    assert!(
        corpus.cases.len() > 300,
        "corpus looks thin: {}",
        corpus.cases.len()
    );
    // Both outcomes must be represented, or this proves only one direction.
    assert!(
        corpus.cases.iter().any(|c| c.epoch_ms.is_none()),
        "no unparseable case recorded"
    );
    assert!(
        corpus.cases.iter().any(|c| c.epoch_ms.is_some()),
        "no parseable case recorded"
    );

    for case in &corpus.cases {
        let ours = parse_lstart_in(&case.field, &tz);
        match case.epoch_ms {
            Some(expected) => assert_eq!(
                ours,
                StartTime::Known(expected),
                "V8 read {:?} as {expected}",
                case.field
            ),
            None => assert_eq!(
                ours,
                StartTime::Unreadable,
                "V8 could not read {:?}, so neither should we",
                case.field
            ),
        }
    }
}
