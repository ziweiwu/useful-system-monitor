//! `top -l 2 -o power -stats pid,power` — macOS's real per-process "Energy
//! Impact", the number Activity Monitor shows.
//!
//! Only `top` exposes it, and it costs ~1.0s of CPU and ~2.0s of wall clock per
//! sample. At a 60s interval that is ~17 ms/s, roughly 5x this app's entire
//! default budget, which is why it is opt-in behind `--energy=accurate`.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

/// `-l 2` prints two sample blocks. The first is always all zeros, because
/// energy impact is itself a rate and `top` has nothing to compare against yet.
/// Only the final block carries real numbers.
pub fn parse_top_power(stdout: &str) -> HashMap<i32, f64> {
    static ROW: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(\d+)\s+([\d.]+)\s*$").expect("static regex"));

    let mut out = HashMap::new();
    let Some(header) = stdout.rfind("PID") else {
        return out;
    };
    let Some(nl) = stdout[header..].find('\n') else {
        return out;
    };
    for line in stdout[header + nl + 1..].lines() {
        let Some(caps) = ROW.captures(line) else {
            continue;
        };
        let (Ok(pid), Ok(power)) = (caps[1].parse::<i32>(), caps[2].parse::<f64>()) else {
            continue;
        };
        out.insert(pid, power);
    }
    out
}
