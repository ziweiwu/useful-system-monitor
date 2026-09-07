//! `pmset -g batt` and `ioreg -rn AppleSmartBattery -w0`.

use std::sync::LazyLock;

use regex::Regex;

use crate::domain::types::{IoregBattery, PmsetBattery};

const MINS_PER_HOUR: i64 = 60;
/// A battery field `pmset` or `ioreg` did not report.
const NO_READING: f64 = 0.0;
const FULL_SCALE_PCT: f64 = 100.0;
/// `ioreg` reports temperature in hundredths of a degree.
const CENTI_PER_UNIT: f64 = 100.0;
/// `pmset -g batt`.
///
/// macOS reports at least five states, and they are not all lowercase:
/// "discharging", "charging", "finishing charge", "charged", "AC attached".
/// The last appears whenever macOS deliberately holds the charge level
/// (optimised charging), and is neither charging nor draining. A lowercase-only
/// pattern silently fails to match it and reports "no battery".
pub fn parse_pmset(stdout: &str) -> Option<PmsetBattery> {
    static STATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+)%;\s*([A-Za-z ]+?);").expect("static regex"));
    static REMAINING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+):(\d{2})\s+remaining").expect("static regex"));
    static PRESENT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"present:\s*true").expect("static regex"));
    static AC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"drawing from 'AC Power'").expect("static regex"));

    let caps = STATE.captures(stdout)?;
    let state = caps[2].trim().to_ascii_lowercase();
    let mins = REMAINING.captures(stdout).and_then(|c| {
        let h: i64 = c[1].parse().ok()?;
        let m: i64 = c[2].parse().ok()?;
        Some(h * MINS_PER_HOUR + m)
    });

    Some(PmsetBattery {
        present: PRESENT.is_match(stdout),
        percent: caps[1].parse().unwrap_or(NO_READING),
        charging: state == "charging" || state == "finishing charge",
        on_ac_power: AC.is_match(stdout) || state != "discharging",
        // "0:00 remaining" and "(no estimate)" both mean unknown, not zero.
        time_remaining_min: mins.filter(|&m| m > 0),
    })
}

/// ioreg prints Amperage as an unsigned 64-bit integer, so a discharging
/// battery appears as e.g. 18446744073709548164 rather than -3452 mA.
/// Reinterpret it as signed, or the sign of the wattage — the whole point of
/// the field — comes out wrong.
pub fn parse_signed_int64(text: &str) -> Option<i64> {
    let t = text.trim();
    if let Ok(v) = t.parse::<i64>() {
        return Some(v);
    }
    // Above i64::MAX but within u64: the wrapped negative.
    t.parse::<u64>().map(|v| v as i64).ok()
}

/// `ioreg -rn AppleSmartBattery -w0`.
pub fn parse_ioreg_battery(stdout: &str) -> IoregBattery {
    let num = |key: &str| -> Option<i64> {
        let re = Regex::new(&format!(r#""{}"\s*=\s*(-?\d+)"#, regex::escape(key)))
            .expect("key is escaped");
        re.captures(stdout).and_then(|c| parse_signed_int64(&c[1]))
    };
    let yes_no = |key: &str| -> Option<bool> {
        let re = Regex::new(&format!(r#""{}"\s*=\s*(Yes|No)"#, regex::escape(key)))
            .expect("key is escaped");
        re.captures(stdout).map(|c| &c[1] == "Yes")
    };

    let voltage_mv = num("Voltage");
    let amperage_ma = num("InstantAmperage").or_else(|| num("Amperage"));
    let raw_max = num("AppleRawMaxCapacity");
    let design = num("DesignCapacity");
    let temp_centi_c = num("Temperature");

    IoregBattery {
        watts: match (voltage_mv, amperage_ma) {
            (Some(v), Some(a)) => Some((v as f64 * a as f64) / 1_000_000.0),
            _ => None,
        },
        cycle_count: num("CycleCount"),
        // `design` of 0 is as useless as absent, and dividing by it is worse.
        health_percent: match (raw_max, design) {
            (Some(now), Some(design)) if design != 0 => {
                Some(((now as f64 / design as f64) * FULL_SCALE_PCT).round() as i64)
            }
            _ => None,
        },
        temperature_c: temp_centi_c.map(|t| t as f64 / CENTI_PER_UNIT),
        charging: yes_no("IsCharging"),
    }
}
