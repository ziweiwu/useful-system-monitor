//! I-19: the Rust width implementation must agree with the TypeScript original
//! on every case in the golden corpus.
//!
//! `src/core/width.ts` is a tested oracle rather than a generic Unicode table —
//! it disagrees with `unicode-width` in several ranges and implements the U+FE0F
//! retroactive-widening rule a per-character crate cannot express. Porting it by
//! eye is exactly how that rule gets lost, so the port is checked against
//! behaviour recorded from the original.
//!
//! Regenerate with `npm run gen:width-corpus`, and only when the TypeScript
//! behaviour changes deliberately. A diff in this file is a behaviour change,
//! not a refactor.

use serde::Deserialize;
use sysmon_core::text::sanitize_text;
use sysmon_core::text::width::{display_width, pad_end, pad_start, truncate, wrap_to_width};

#[derive(Deserialize)]
struct Case {
    s: String,
    w: usize,
    t: Vec<String>,
    pe: Vec<String>,
    ps: Vec<String>,
    wr: Vec<Vec<String>>,
    sa: String,
}

#[derive(Deserialize)]
struct Corpus {
    ns: Vec<usize>,
    #[serde(rename = "wrapNs")]
    wrap_ns: Vec<usize>,
    cases: Vec<Case>,
}

fn corpus() -> Corpus {
    let raw = include_str!("fixtures/width-corpus.json");
    serde_json::from_str(raw).expect("width-corpus.json should parse")
}

/// Rendered so a failure names the offending code points rather than printing
/// an invisible character and leaving you to guess which one it was.
fn escape(text: &str) -> String {
    text.chars()
        .map(|c| format!("U+{:04X}", c as u32))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn i19_display_width_matches_the_typescript_oracle() {
    let c = corpus();
    assert!(
        c.cases.len() > 1_000,
        "corpus looks truncated: {} cases",
        c.cases.len()
    );
    for case in &c.cases {
        assert_eq!(
            display_width(&case.s),
            case.w,
            "display_width({:?}) [{}]",
            case.s,
            escape(&case.s)
        );
    }
}

#[test]
fn i19_truncate_matches_the_typescript_oracle() {
    let c = corpus();
    for case in &c.cases {
        for (i, &n) in c.ns.iter().enumerate() {
            assert_eq!(
                truncate(&case.s, n),
                case.t[i],
                "truncate({:?}, {}) [{}]",
                case.s,
                n,
                escape(&case.s)
            );
        }
    }
}

#[test]
fn i19_padding_matches_the_typescript_oracle() {
    let c = corpus();
    for case in &c.cases {
        for (i, &n) in c.ns.iter().enumerate() {
            assert_eq!(
                pad_end(&case.s, n),
                case.pe[i],
                "pad_end({:?}, {})",
                case.s,
                n
            );
            assert_eq!(
                pad_start(&case.s, n),
                case.ps[i],
                "pad_start({:?}, {})",
                case.s,
                n
            );
        }
    }
}

#[test]
fn i19_wrapping_matches_the_typescript_oracle() {
    let c = corpus();
    for case in &c.cases {
        for (i, &n) in c.wrap_ns.iter().enumerate() {
            assert_eq!(
                wrap_to_width(&case.s, n),
                case.wr[i],
                "wrap_to_width({:?}, {}) [{}]",
                case.s,
                n,
                escape(&case.s)
            );
        }
    }
}

#[test]
fn sanitize_matches_the_typescript_oracle() {
    for case in &corpus().cases {
        assert_eq!(
            sanitize_text(&case.s),
            case.sa,
            "sanitize_text({:?})",
            case.s
        );
    }
}
