//! Command-line parsing, kept free of I/O so it can be tested directly and so
//! importing it never starts anything.
//!
//! Two rules keep this surface small:
//!
//! Anything the tool does not understand is an error naming the cause and the
//! remedy, never something to ignore (I-24). Ignoring them is how `--jsonn`
//! silently produced text for a script that asked for JSON, and how
//! `--sort bogus` returned rows in no order at all.
//!
//! And an option has to buy something the terminal cannot. Row counts and
//! ordering are `head` and `jq`'s job, so `--json` hands over the whole working
//! set and stays out of the way. What is left either changes what is measured
//! (`--energy`), how often (`--interval`), or where it comes from (`--mock`).

/// Refresh floor. Below a second the render cost dominates the sample, and a
/// negative value used to reach the timer, which clamps to 1 ms and spins the
/// render loop at ~1000 Hz.
pub const MIN_INTERVAL_SEC: f64 = 1.0;

/// Refresh ceiling: the largest delay a timer can actually hold.
///
/// There was deliberately no ceiling — a very lazy background pane is a
/// legitimate thing to want — but that reasoning stops at the 32-bit boundary.
/// The timer takes a signed 32-bit millisecond count, so anything past
/// 2^31-1 ms (24.8 days) is silently clamped to **1 ms**: `--interval 3000000`
/// measured 265 fires in 300 ms. That is the same ~1000 Hz render spin the
/// lower bound exists to prevent, reached from the other end.
pub const MAX_INTERVAL_SEC: f64 = 2_147_483.0;

/// With nothing set this is the dashboard: no JSON, no mock, default tiers,
/// the estimated energy column. Every field's default is its "off" value, so
/// the derive says it as clearly as a hand-written impl would.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    pub accurate_energy: bool,
    pub mock: bool,
    pub json: bool,
    pub interval: Option<f64>,
    pub help: bool,
    pub version: bool,
}

/// `Number("")` is 0 in JavaScript, not NaN.
const EMPTY_STRING_IS_ZERO: f64 = 0.0;
/// JavaScript's `Number(string)`, which the shipping build's validation depends
/// on and which no Rust parser reproduces on its own.
///
/// The differences that matter here, all reachable from a command line:
///
/// | input | JS `Number()` | Rust `parse::<f64>()` |
/// |---|---|---|
/// | `""`, `"  "` | `0` | error |
/// | `"0x10"` | `16` | error |
/// | `"inf"`, `"nan"` | `NaN` | `inf`, `NaN` |
/// | `"Infinity"` | `∞` | error |
///
/// The `inf`/`nan` row is the dangerous one in the other direction: Rust
/// *accepts* spellings JavaScript rejects, so `--interval inf` would have
/// parsed rather than being reported as the mistake it is.
///
/// Returns `NaN` for anything unparseable, exactly as `Number()` does.
pub fn js_number(text: &str) -> f64 {
    // JS trims whitespace and line terminators, and the BOM.
    let trimmed = text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if trimmed.is_empty() {
        return EMPTY_STRING_IS_ZERO;
    }
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if let Some(value) = radix_literal(trimmed) {
        return value;
    }
    // Rust accepts "inf", "infinity" and "nan" case-insensitively; JavaScript
    // accepts none of them, and only the exact "Infinity" handled above.
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("inf") || lower.contains("nan") {
        return f64::NAN;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

/// `0x`, `0o` and `0b` literals, or `None` if this is not one.
///
/// Radix prefixes take no sign in JavaScript. Accumulated in f64 rather than an
/// integer, so a very long literal saturates the way JavaScript's does instead
/// of overflowing.
fn radix_literal(trimmed: &str) -> Option<f64> {
    const HEX: u32 = 16;
    const OCTAL: u32 = 8;
    const BINARY: u32 = 2;
    let (prefix, radix) = [
        ("0x", HEX),
        ("0X", HEX),
        ("0o", OCTAL),
        ("0O", OCTAL),
        ("0b", BINARY),
        ("0B", BINARY),
    ]
    .into_iter()
    .find(|(prefix, _)| trimmed.starts_with(prefix))?;
    let rest = trimmed.strip_prefix(prefix)?;
    if rest.is_empty() {
        return Some(f64::NAN);
    }
    let mut acc = 0f64;
    for c in rest.chars() {
        match c.to_digit(radix) {
            Some(d) => acc = acc * radix as f64 + d as f64,
            None => return Some(f64::NAN),
        }
    }
    Some(acc)
}

/// The parsed options, or the cause phrased for a terminal. The caller adds the
/// pointer to `--help`.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseResult {
    Ok(Box<Options>),
    Err(String),
}

/// `--interval`, validated against the range the tiers can actually honour.
fn set_interval(options: &mut Options, raw: &str) -> Option<String> {
    let n = js_number(raw);
    if !n.is_finite() || n < MIN_INTERVAL_SEC || n > MAX_INTERVAL_SEC {
        return Some(format!(
            "--interval needs a number of seconds between {} and {} (got \"{raw}\")",
            MIN_INTERVAL_SEC as i64, MAX_INTERVAL_SEC as i64
        ));
    }
    options.interval = Some(n);
    None
}

/// The message for an argument that is not an option this tool takes.
fn unknown(arg: &str) -> String {
    if arg.starts_with('-') {
        format!("unknown option \"{arg}\"")
    } else {
        format!("unexpected argument \"{arg}\" — this tool takes options only")
    }
}

/// One option and its value, folded into `o`. Returns the error message, if any.
fn apply_option(options: &mut Options, name: &str, value: Option<String>) -> Option<String> {
    match name {
        "--mock" => options.mock = true,
        "--json" => options.json = true,
        "--help" | "-h" => options.help = true,
        "--version" => options.version = true,
        "--interval" => {
            let raw = value?;
            return set_interval(options, &raw);
        }
        "--energy" => {
            let Some(raw) = value else {
                return Some(format!("{name} needs a value: accurate"));
            };
            if raw != "accurate" {
                return Some(format!(
                    "--energy only accepts \"accurate\" (got \"{raw}\")"
                ));
            }
            options.accurate_energy = true;
        }
        _ => return Some(unknown(name)),
    }
    None
}

/// `--name=value` and `--name value` are the same option; split once here so
/// each case sees one shape.
fn split_option(arg: &str) -> (&str, Option<String>) {
    let eq = if arg.starts_with("--") {
        arg.find('=')
    } else {
        None
    };
    match eq {
        Some(pos) if pos > 0 => (&arg[..pos], Some(arg[pos + 1..].to_string())),
        _ => (arg, None),
    }
}

pub fn parse_args<S: AsRef<str>>(argv: &[S]) -> ParseResult {
    let mut options = Options::default();
    let mut i = 0usize;

    while i < argv.len() {
        let arg = argv[i].as_ref();
        let (name, inline) = split_option(arg);

        /* The option's value, or `None` when none was supplied. Consumes the
        next argument only when it is a value rather than another option — a
        leading `-` counts as a value when it parses as a number, so
        `--interval -5` reports the real problem instead of "missing value". */
        let wants_value = matches!(name, "--interval" | "--energy");
        let value = if wants_value {
            inline.or_else(|| next_value(argv, i).inspect(|_| i += 1))
        } else {
            None
        };
        if wants_value && value.is_none() && name == "--interval" {
            return ParseResult::Err(format!("{name} needs a value"));
        }

        if let Some(message) = apply_option(&mut options, name, value) {
            return ParseResult::Err(message);
        }
        i += 1;
    }

    ParseResult::Ok(Box::new(options))
}

/// The next argument, if it is a value rather than another option.
fn next_value<S: AsRef<str>>(argv: &[S], i: usize) -> Option<String> {
    let next = argv.get(i + 1)?.as_ref();
    if next.starts_with('-') && !js_number(next).is_finite() {
        return None;
    }
    Some(next.to_string())
}
