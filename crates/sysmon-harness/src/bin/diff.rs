//! The differential harness: Rust `--json` against the shipping Node build's.
//!
//! This is the instrument the whole migration rests on. It is what lets the
//! Rust build be judged against something other than an opinion, and it is the
//! reason the later phase — replacing seven shell-outs with libproc and IOKit —
//! can be a mechanical, verifiable swap rather than a second act of faith.
//!
//! # Why it compares parsed values and never bytes
//!
//! `JSON.stringify` prints a whole-valued float as `1`; `serde_json` prints
//! `1.0`. Both parse to the same number. Comparing text would report a hundred
//! differences that are not differences, and the real ones would be lost in
//! them.
//!
//! # Why the tolerances are per field rather than global
//!
//! The two binaries cannot sample the same instant, so anything time-varying
//! differs legitimately. What must *not* differ is the shape: the same machine
//! has the same total memory, the same root filesystem and the same battery,
//! and a process alive in both samples has the same name and owner in both.
//! Those are asserted exactly; the moving numbers get a tolerance sized to how
//! fast they actually move.
//!
//! Usage:
//!   cargo run -p sysmon-harness --bin diff -- [--samples N] [--node PATH] [--rust PATH]

use std::collections::HashMap;
use std::process::Command;

use serde_json::Value;

struct Config {
    samples: usize,
    node: String,
    rust: String,
}

fn parse_config() -> Config {
    let mut cfg = Config {
        samples: 3,
        node: "dist/cli.js".to_string(),
        rust: "target/debug/sysmon".to_string(),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--samples" => {
                cfg.samples = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(DEFAULT_SAMPLES);
                i += 1;
            }
            "--node" => {
                cfg.node = args.get(i + 1).cloned().unwrap_or(cfg.node);
                i += 1;
            }
            "--rust" => {
                cfg.rust = args.get(i + 1).cloned().unwrap_or(cfg.rust);
                i += 1;
            }
            other => {
                eprintln!("diff: unknown option {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    cfg
}

fn run_json(program: &str, args: &[&str]) -> Result<Value, String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{program} exited {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("{program}: bad JSON: {e}"))
}

/// One comparison and its verdict, so the report reads as a table rather than a
/// wall of prose.
struct Finding {
    field: String,
    ok: bool,
    detail: String,
}

/// How many `--json` rounds a run compares unless told otherwise.
const DEFAULT_SAMPLES: usize = 3;

// Tolerances, sized to how fast each field actually moves between two samples
// taken a second apart.
const CPU_TOLERANCE: Tolerance = Tolerance {
    max: 40.0,
    unit: "%",
};
const MEMORY_TOLERANCE: Tolerance = Tolerance {
    max: 1.5e9,
    unit: "B",
};
const DISK_TOLERANCE: Tolerance = Tolerance {
    max: 5e8,
    unit: "B",
};
const BATTERY_TOLERANCE: Tolerance = Tolerance {
    max: 2.0,
    unit: "%",
};
const PROCESS_COUNT_TOLERANCE: Tolerance = Tolerance {
    max: 60.0,
    unit: " procs",
};

/// Fewer pids in common than this and the identity checks prove nothing.
const MIN_COMMON_PIDS: usize = 5;
/// Guards the ratio against a divide-by-zero on a process reporting no RSS.
const SMALLEST_MEANINGFUL_RSS: f64 = 1.0;
/// Past this the two builds are reading different units, not the same busy machine.
const MAX_RSS_DISAGREEMENT: f64 = 0.5;
/// Mismatching pids printed before the list is truncated.
const EXAMPLES_SHOWN: usize = 3;

/// The two `--json` documents being compared, so a comparator takes one
/// argument for "both builds" rather than two positional ones that are only
/// told apart by order.
struct Pair<'a> {
    node: &'a Value,
    rust: &'a Value,
}

/// How far apart a time-varying field may legitimately be.
struct Tolerance {
    max: f64,
    unit: &'static str,
}

fn descend<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = root;
    for step in path {
        cur = cur.get(step)?;
    }
    Some(cur)
}

fn f64_at(root: &Value, path: &[&str]) -> Option<f64> {
    descend(root, path)?.as_f64()
}

/// Both sides read, or a "missing" finding naming which side lacked the field.
/// Every comparator below needs the same fallback, so it lives here once.
fn both<T: std::fmt::Debug>(
    out: &mut Vec<Finding>,
    field: String,
    read: (Option<T>, Option<T>),
) -> Option<(T, T)> {
    match read {
        (Some(x), Some(y)) => Some((x, y)),
        (x, y) => {
            out.push(Finding {
                ok: false,
                detail: format!("missing: node={x:?} rust={y:?}"),
                field,
            });
            None
        }
    }
}

fn exact_num(out: &mut Vec<Finding>, pair: &Pair, path: &[&str]) {
    let field = path.join(".");
    let read = (f64_at(pair.node, path), f64_at(pair.rust, path));
    if let Some((x, y)) = both(out, field.clone(), read) {
        out.push(Finding {
            ok: x == y,
            detail: format!("node={x} rust={y}"),
            field,
        });
    }
}

fn within(out: &mut Vec<Finding>, pair: &Pair, path: &[&str], tol: Tolerance) {
    let field = path.join(".");
    let read = (f64_at(pair.node, path), f64_at(pair.rust, path));
    let Some((x, y)) = both(out, field.clone(), read) else {
        return;
    };
    let gap = (x - y).abs();
    let (max, unit) = (tol.max, tol.unit);
    out.push(Finding {
        ok: gap <= max,
        detail: format!("node={x:.3} rust={y:.3} Δ={gap:.3}{unit} (tol {max}{unit})"),
        field,
    });
}

/// Booleans need their own comparison: `as_f64()` returns `None` for them, so
/// comparing one as a number reports "missing" rather than comparing it.
fn exact_bool(out: &mut Vec<Finding>, pair: &Pair, path: &[&str]) {
    let field = path.join(".");
    let read = (
        descend(pair.node, path).and_then(Value::as_bool),
        descend(pair.rust, path).and_then(Value::as_bool),
    );
    if let Some((x, y)) = both(out, field.clone(), read) {
        out.push(Finding {
            ok: x == y,
            detail: format!("node={x} rust={y}"),
            field,
        });
    }
}

fn exact_str(out: &mut Vec<Finding>, pair: &Pair, path: &[&str]) {
    let field = path.join(".");
    let read = (
        descend(pair.node, path).and_then(Value::as_str),
        descend(pair.rust, path).and_then(Value::as_str),
    );
    if let Some((x, y)) = both(out, field.clone(), read) {
        out.push(Finding {
            ok: x == y,
            detail: format!("node={x:?} rust={y:?}"),
            field,
        });
    }
}

/// Rows keyed by pid, for the identity comparison.
fn processes(root: &Value) -> HashMap<i64, &Value> {
    root.get("processes")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| r.get("pid")?.as_i64().map(|p| (p, r)))
                .collect()
        })
        .unwrap_or_default()
}

/// The same machine, so these cannot legitimately differ.
fn compare_structural(out: &mut Vec<Finding>, pair: &Pair) {
    exact_num(out, pair, &["memory", "totalBytes"]);
    exact_num(out, pair, &["disk", "totalBytes"]);
    exact_str(out, pair, &["disk", "mount"]);
    exact_bool(out, pair, &["battery", "present"]);
    exact_bool(out, pair, &["battery", "charging"]);
    exact_bool(out, pair, &["battery", "onAcPower"]);
    // I-1b: the energy column carries one unit or the other, never a blend. If
    // the two builds disagree here the whole column means different things.
    exact_bool(out, pair, &["energyAccurate"]);
    exact_num(out, pair, &["battery", "cycleCount"]);

    // The version string is the shape marker a consumer reads (I-25). Both
    // builds report their own version, so only its presence is comparable.
    for (label, doc) in [("node", pair.node), ("rust", pair.rust)] {
        let ok = doc
            .get("version")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
        out.push(Finding {
            field: format!("version ({label})"),
            ok,
            detail: format!("{:?}", doc.get("version")),
        });
    }

    // Core count is fixed hardware; per-core *values* move, the length does not.
    let core_count = |doc: &Value| {
        descend(doc, &["cpu", "perCore"])
            .and_then(Value::as_array)
            .map(Vec::len)
    };
    let (n_node, n_rust) = (core_count(pair.node), core_count(pair.rust));
    out.push(Finding {
        field: "cpu.perCore.length".into(),
        ok: n_node.is_some() && n_node == n_rust,
        detail: format!("node={n_node:?} rust={n_rust:?}"),
    });
}

/// Time-varying, with tolerances sized to how fast each actually moves.
fn compare_drifting(out: &mut Vec<Finding>, pair: &Pair) {
    within(out, pair, &["cpu", "system"], CPU_TOLERANCE);
    within(out, pair, &["memory", "usedBytes"], MEMORY_TOLERANCE);
    within(out, pair, &["disk", "usedBytes"], DISK_TOLERANCE);
    within(out, pair, &["battery", "percent"], BATTERY_TOLERANCE);
    within(out, pair, &["total"], PROCESS_COUNT_TOLERANCE);
}

/// A process alive in both samples must be the *same* process in both. This is
/// the check that would catch a parser reading the wrong column, which no
/// tolerance on a number ever would.
fn compare_identity(out: &mut Vec<Finding>, pair: &Pair) {
    let (pn, pr) = (processes(pair.node), processes(pair.rust));
    let common: Vec<i64> = pn.keys().filter(|p| pr.contains_key(p)).copied().collect();
    out.push(Finding {
        field: "processes.overlap".into(),
        ok: common.len() >= MIN_COMMON_PIDS,
        detail: format!(
            "{} pids in both (node {}, rust {})",
            common.len(),
            pn.len(),
            pr.len()
        ),
    });

    compare_names(out, &pn, &pr, &common);
    compare_rss(out, &pn, &pr, &common);
}

/// The name and user a pid reports must agree between the two builds.
fn compare_names(
    out: &mut Vec<Finding>,
    from_node: &HashMap<i64, &Value>,
    from_rust: &HashMap<i64, &Value>,
    common: &[i64],
) {
    let mut name_mismatch = Vec::new();
    let mut user_mismatch = 0usize;
    for pid in common {
        let (left, right) = (from_node[pid], from_rust[pid]);
        if left.get("name") != right.get("name") {
            name_mismatch.push(format!(
                "pid {pid}: node={:?} rust={:?}",
                left.get("name"),
                right.get("name")
            ));
        }
        if left.get("user") != right.get("user") {
            user_mismatch += 1;
        }
    }
    out.push(Finding {
        field: "processes.name".into(),
        ok: name_mismatch.is_empty(),
        detail: if name_mismatch.is_empty() {
            format!("all {} common pids agree", common.len())
        } else {
            name_mismatch.join("; ")
        },
    });
    out.push(Finding {
        field: "processes.user".into(),
        ok: user_mismatch == 0,
        detail: format!("{user_mismatch} of {} disagree", common.len()),
    });
}

/// RSS moves, but not by orders of magnitude between two samples a second
/// apart. A 2x disagreement means a unit error, not a busy machine.
fn compare_rss(
    out: &mut Vec<Finding>,
    from_node: &HashMap<i64, &Value>,
    from_rust: &HashMap<i64, &Value>,
    common: &[i64],
) {
    let mut rss_bad = Vec::new();
    for pid in common {
        let (Some(x), Some(y)) = (
            from_node[pid].get("rssBytes").and_then(Value::as_f64),
            from_rust[pid].get("rssBytes").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let larger = x.max(y).max(SMALLEST_MEANINGFUL_RSS);
        if (x - y).abs() / larger > MAX_RSS_DISAGREEMENT {
            rss_bad.push(format!("pid {pid}: node={x} rust={y}"));
        }
    }
    out.push(Finding {
        field: "processes.rssBytes".into(),
        ok: rss_bad.len() * 10 <= common.len(),
        detail: if rss_bad.is_empty() {
            format!("all {} common pids within 50%", common.len())
        } else {
            format!(
                "{} of {} differ by >50%: {}",
                rss_bad.len(),
                common.len(),
                rss_bad
                    .iter()
                    .take(EXAMPLES_SHOWN)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        },
    });
}

fn compare(node: &Value, rust: &Value) -> Vec<Finding> {
    let pair = Pair { node, rust };
    let mut out = Vec::new();
    compare_structural(&mut out, &pair);
    compare_drifting(&mut out, &pair);
    compare_identity(&mut out, &pair);
    out
}

/// One interleaved round. The order alternates, so a systematic drift in one
/// direction cannot masquerade as agreement.
fn run_round(cfg: &Config, round: usize) -> Option<Vec<Finding>> {
    let (node, rust) = if round % 2 == 1 {
        let n = run_json("node", &[&cfg.node, "--json"]);
        let r = run_json(&cfg.rust, &["--json"]);
        (n, r)
    } else {
        let r = run_json(&cfg.rust, &["--json"]);
        let n = run_json("node", &[&cfg.node, "--json"]);
        (n, r)
    };
    match (node, rust) {
        (Ok(n), Ok(r)) => Some(compare(&n, &r)),
        (n, r) => {
            for e in [n.err(), r.err()].into_iter().flatten() {
                eprintln!("round {round}: {e}");
            }
            None
        }
    }
}

/// Prints a round and returns how many of its checks failed. The first round
/// prints every check; later ones print only what failed.
fn report_round(findings: &[Finding], round: usize) -> usize {
    let bad = findings.iter().filter(|f| !f.ok).count();
    println!("round {round}: {} checks, {bad} failed", findings.len());
    let verbose = round == 1;
    for finding in findings.iter().filter(|f| verbose || !f.ok) {
        println!(
            "  {} {:<24} {}",
            if finding.ok { "ok  " } else { "FAIL" },
            finding.field,
            finding.detail
        );
    }
    bad
}

fn main() -> std::process::ExitCode {
    let cfg = parse_config();
    println!(
        "node: {}\nrust: {}\nsamples: {}\n",
        cfg.node, cfg.rust, cfg.samples
    );

    let mut failures = 0usize;
    for round in 1..=cfg.samples {
        let Some(findings) = run_round(&cfg, round) else {
            return std::process::ExitCode::from(1);
        };
        failures += report_round(&findings, round);
    }

    println!(
        "\ndifferential: {}",
        if failures == 0 { "PASS" } else { "FAIL" }
    );
    if failures == 0 {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(1)
    }
}
