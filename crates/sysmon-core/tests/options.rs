//! I-24: unknown or unusable options are rejected, never ignored.
//!
//! Ported from `test/options.test.ts`, plus an exact parity corpus. The corpus
//! is not ceremony: `--interval`'s validation runs its argument through
//! JavaScript's `Number()`, whose behaviour no Rust parser reproduces —
//! `Number("")` is 0, `Number("0x10")` is 16, and `Number("inf")` is NaN while
//! Rust's `parse::<f64>()` returns infinity. Every one of those is reachable
//! from a shell, and the last would have turned a rejected argument into an
//! accepted one.
//!
//! Regenerate with `npm run gen:options-corpus`.

use serde::Deserialize;
use sysmon_core::cli::options::{
    js_number, parse_args, Options, ParseResult, MAX_INTERVAL_SEC, MIN_INTERVAL_SEC,
};

fn ok(argv: &[&str]) -> Options {
    match parse_args(argv) {
        ParseResult::Ok(o) => *o,
        ParseResult::Err(e) => panic!("expected success, got: {e}"),
    }
}

fn err(argv: &[&str]) -> String {
    match parse_args(argv) {
        ParseResult::Err(e) => e,
        ParseResult::Ok(o) => panic!("expected an error, got: {o:?}"),
    }
}

/// A typo'd `--json` used to be ignored, so a script asking for JSON silently
/// got text and failed somewhere further downstream.
#[test]
fn i24_names_the_option_it_does_not_know() {
    assert!(err(&["--jsonn"]).contains("unknown option \"--jsonn\""));
    assert!(err(&["-x"]).contains("unknown option \"-x\""));
}

#[test]
fn i24_rejects_positional_arguments_which_this_tool_has_none_of() {
    assert!(err(&["report.txt"]).contains("unexpected argument"));
}

#[test]
fn i24_rejects_an_unusable_interval_and_quotes_what_it_was_given() {
    // -5 reached the timer, which clamps to 1ms; 0 and 0.5 are below the floor;
    // abc is not a number at all.
    for value in ["-5", "0", "0.5", "abc"] {
        assert!(err(&["--interval", value]).contains(value), "{value}");
    }
}

#[test]
fn i24_accepts_a_lazy_interval_because_a_quiet_pane_is_a_fair_thing_to_want() {
    assert_eq!(ok(&["--interval", "7200"]).interval, Some(7200.0));
    assert_eq!(ok(&["--interval", "1"]).interval, Some(MIN_INTERVAL_SEC));
}

/// The timer takes a signed 32-bit millisecond count, so a delay past 2^31-1 ms
/// is silently clamped to 1 ms — `--interval 3000000` measured 265 fires in
/// 300 ms, the same spin the lower bound exists to prevent.
#[test]
fn i24_rejects_an_interval_a_timer_cannot_hold() {
    assert!(err(&["--interval", "2147484"]).contains("between"));
    assert!(matches!(
        parse_args(&["--interval", "3000000"]),
        ParseResult::Err(_)
    ));
    let at_ceiling = ok(&["--interval", "2147483"]).interval.expect("accepted");
    assert!(at_ceiling * 1000.0 <= 2_147_483_647.0);
    assert_eq!(at_ceiling, MAX_INTERVAL_SEC);
}

#[test]
fn i24_rejects_an_energy_mode_it_does_not_have() {
    assert!(err(&["--energy", "fast"]).contains("accurate"));
    assert!(err(&["--energy=fast"]).contains("accurate"));
}

#[test]
fn i24_says_which_option_is_missing_its_value() {
    assert_eq!(err(&["--interval"]), "--interval needs a value");
}

#[test]
fn i24_does_not_swallow_the_next_option_as_a_value() {
    assert_eq!(err(&["--interval", "--json"]), "--interval needs a value");
}

#[test]
fn defaults_to_the_dashboard_with_nothing_set() {
    assert_eq!(ok(&[]), Options::default());
}

#[test]
fn accepts_both_name_value_and_name_equals_value() {
    assert_eq!(ok(&["--interval", "2"]).interval, Some(2.0));
    assert_eq!(ok(&["--interval=2"]).interval, Some(2.0));
    assert!(ok(&["--energy", "accurate"]).accurate_energy);
    assert!(ok(&["--energy=accurate"]).accurate_energy);
}

#[test]
fn takes_the_flags_with_no_value() {
    assert!(ok(&["--mock"]).mock);
    assert!(ok(&["--json"]).json);
    assert!(ok(&["-h"]).help);
    assert!(ok(&["--help"]).help);
    assert!(ok(&["--version"]).version);
}

/// Row counts and ordering belong to `head` and `jq`, so there is nothing here
/// to configure them with. Guard the list so it does not creep back. See I-25b.
#[test]
fn i25b_keeps_the_option_surface_to_what_a_terminal_cannot_already_do() {
    for gone in ["--top", "--sort", "-v", "-V"] {
        assert!(err(&[gone]).contains("unknown option"), "{gone}");
    }
}

#[test]
fn combines_options_in_any_order() {
    let o = ok(&["--json", "--mock", "--interval=3"]);
    assert!(o.json && o.mock);
    assert_eq!(o.interval, Some(3.0));
}

/// The table in `js_number`'s documentation, asserted.
#[test]
fn js_number_matches_javascript_where_rust_would_not() {
    assert_eq!(js_number(""), 0.0);
    assert_eq!(js_number("   "), 0.0);
    assert_eq!(js_number("0x10"), 16.0);
    assert_eq!(js_number("0b101"), 5.0);
    assert_eq!(js_number("0o17"), 15.0);
    assert_eq!(js_number("Infinity"), f64::INFINITY);
    // Rust would accept all three of these; JavaScript accepts none.
    assert!(js_number("inf").is_nan());
    assert!(js_number("infinity").is_nan());
    assert!(js_number("nan").is_nan());
    assert!(js_number("1_000").is_nan());
    assert_eq!(js_number("  3  "), 3.0);
    assert_eq!(js_number("1e3"), 1000.0);
}

// ------------------------------------------------------------ the corpus --

#[derive(Deserialize)]
struct CorpusOptions {
    #[serde(rename = "accurateEnergy")]
    accurate_energy: bool,
    mock: bool,
    json: bool,
    interval: Option<f64>,
    help: bool,
    version: bool,
}

#[derive(Deserialize)]
struct Case {
    argv: Vec<String>,
    ok: bool,
    options: Option<CorpusOptions>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

#[test]
fn i24_parsing_matches_the_typescript_oracle_argv_for_argv() {
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/options-corpus.json")).expect("corpus parses");
    assert!(corpus.cases.len() > 100, "corpus looks thin");
    // Both outcomes must be represented, or this proves only one direction.
    assert!(corpus.cases.iter().any(|c| c.ok));
    assert!(corpus.cases.iter().any(|c| !c.ok));

    for case in &corpus.cases {
        match (case.ok, parse_args(&case.argv)) {
            (true, ParseResult::Ok(got)) => {
                let want = case.options.as_ref().expect("an accepted case has options");
                assert_eq!(got.accurate_energy, want.accurate_energy, "{:?}", case.argv);
                assert_eq!(got.mock, want.mock, "{:?}", case.argv);
                assert_eq!(got.json, want.json, "{:?}", case.argv);
                assert_eq!(got.interval, want.interval, "{:?}", case.argv);
                assert_eq!(got.help, want.help, "{:?}", case.argv);
                assert_eq!(got.version, want.version, "{:?}", case.argv);
            }
            (false, ParseResult::Err(got)) => {
                assert_eq!(
                    &got,
                    case.error.as_ref().expect("a rejected case has an error"),
                    "{:?}",
                    case.argv
                );
            }
            (true, ParseResult::Err(e)) => {
                panic!("{:?} should have been accepted, got error: {e}", case.argv)
            }
            (false, ParseResult::Ok(o)) => {
                panic!("{:?} should have been rejected, got: {o:?}", case.argv)
            }
        }
    }
}
