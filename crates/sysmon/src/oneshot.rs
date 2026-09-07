//! One-shot, pipe-friendly output. Used when stdout is not a TTY. See I-22.

use std::io::Write;

use sysmon_core::cli::json::JsonOutput;
use sysmon_core::cli::options::Options;
use sysmon_core::domain::format::{bytes, percent, to_fixed};
use sysmon_core::domain::scoring::{sort_processes, SortKey};
use sysmon_core::domain::working_set::WorkingSetCap;
use sysmon_core::kill::guards::process_name;

use crate::collect::Collector;

/// Rows of text, before piping. Enough to answer "what is busy right now" at a
/// glance; `head` trims it further and `--json` ignores it entirely.
const TEXT_ROWS: usize = 10;

/// Both CPU sources are deltas by construction, so the first sample is only
/// ever half a measurement. See I-1.
const PRIMING_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

/// Rows `--json` reports. Enough for a consumer to rank, small enough that the
/// document stays readable.
const JSON_ROW_LIMIT: usize = 50;
const FULL_SCALE_PCT: f64 = 100.0;
/// No denominator, so no ratio — never NaN. See I-19.
const NO_RATIO: f64 = 0.0;

pub fn run(options: &Options) -> Result<(), String> {
    let mut collector = if options.mock {
        Collector::mock()?
    } else {
        Collector::new(options.accurate_energy)?
    };

    // Two samples are required: CPU% is always a delta, never a lifetime
    // average.
    collector.processes(WorkingSetCap::Top(JSON_ROW_LIMIT))?;
    collector.cpu().map_err(|e| e.to_string())?;
    std::thread::sleep(PRIMING_DELAY);

    let cpu = collector.cpu()?;
    let memory = collector.memory()?;
    let disk = collector.disk()?;
    let battery = collector.battery()?;
    let procs = collector
        .processes(WorkingSetCap::Top(JSON_ROW_LIMIT))
        .map_err(|e| e.to_string())?;

    let ranked = sort_processes(&procs.visible, SortKey::Cpu);

    let sample = Sample {
        cpu,
        memory,
        disk,
        battery,
        procs,
        ranked,
    };
    let text = if options.json {
        json_document(&sample)?
    } else {
        text_report(&sample)
    };
    let mut out = std::io::stdout().lock();
    writeln!(out, "{text}").map_err(|e| e.to_string())
}

/// One `--json`/`--once` sample, gathered before it is formatted either way.
struct Sample {
    cpu: sysmon_core::domain::types::CpuData,
    memory: sysmon_core::domain::types::MemoryData,
    disk: sysmon_core::domain::types::DiskData,
    battery: sysmon_core::domain::types::BatteryData,
    procs: crate::collect::ProcessesData,
    ranked: Vec<sysmon_core::domain::types::ProcessSample>,
}

/// JSON gets the whole working set: a consumer that wants ten rows sorted by
/// memory has jq, and guessing on its behalf is what `--top` and `--sort` were.
/// See I-25b.
fn json_document(sample: &Sample) -> Result<String, String> {
    let doc = JsonOutput {
        version: crate::VERSION.to_string(),
        cpu: (&sample.cpu).into(),
        memory: (&sample.memory).into(),
        disk: (&sample.disk).into(),
        battery: (&sample.battery).into(),
        processes: sample.ranked.iter().map(Into::into).collect(),
        others: (&sample.procs.others).into(),
        total: sample.procs.total,
        energy_accurate: sample.procs.energy_accurate,
    };
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

fn text_report(sample: &Sample) -> String {
    let mem_pct = ratio(sample.memory.used_bytes, sample.memory.total_bytes);
    let disk_pct = ratio(sample.disk.used_bytes, sample.disk.total_bytes);
    let mut lines = vec![
        format!(
            "cpu {}%  mem {}%  disk {}%  battery {}%{}",
            to_fixed(sample.cpu.system, 1),
            to_fixed(mem_pct, 1),
            to_fixed(disk_pct, 0),
            trim_float(sample.battery.percent),
            if sample.battery.charging {
                " charging"
            } else {
                ""
            }
        ),
        String::new(),
        "PID     CPU%     MEM  NAME".to_string(),
    ];
    for p in sample.ranked.iter().take(TEXT_ROWS) {
        lines.push(format!(
            "{:<7} {:>5}  {:>6}  {}",
            p.pid,
            percent(p.cpu_percent, 1),
            bytes(p.rss_bytes),
            process_name(&p.command)
        ));
    }
    lines.join("\n")
}

fn ratio(used: u64, total: u64) -> f64 {
    if total > 0 {
        (used as f64 / total as f64) * FULL_SCALE_PCT
    } else {
        NO_RATIO
    }
}

/// JavaScript prints a whole-valued number without a fractional part, and the
/// battery percentage is interpolated raw into the text line.
fn trim_float(value: f64) -> String {
    if value.fract() == NO_RATIO {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}
