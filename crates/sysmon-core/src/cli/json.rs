//! The `--json` shape.
//!
//! **This module is the parity contract.** `--json` is a public interface — the
//! README shows `jq` pipelines against it — so the field names, their nesting
//! and their null-vs-absent behaviour are all promised to scripts that already
//! exist. Everything else in the port can be reasoned about; this has to match.
//!
//! One difference is unavoidable and deliberate: JavaScript's `JSON.stringify`
//! prints a whole-valued float as `1`, while `serde_json` prints `1.0`. Both
//! parse to the same number, which is why the differential harness compares
//! parsed values with per-field tolerances and never compares bytes.

/// The 1, 5 and 15 minute load averages.
const LOAD_WINDOWS: usize = 3;

use serde::{Deserialize, Serialize};

use crate::domain::types::{
    BatteryData, CpuData, DiskData, MemoryData, OthersRollup, ProcessSample, VolumeUsage,
};
use crate::kill::guards::process_name;

/// Only the three CPU fields a script can use. `perCore` is included because it
/// is the one thing `jq` cannot derive from the others.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CpuJson {
    pub system: f64,
    pub per_core: Vec<f64>,
    pub load_avg: [f64; LOAD_WINDOWS],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryJson {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub available_bytes: u64,
    pub wired_bytes: u64,
    pub active_bytes: u64,
    pub inactive_bytes: u64,
    pub compressed_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VolumeJson {
    pub mount: String,
    pub device: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiskJson {
    pub mount: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub volumes: Vec<VolumeJson>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatteryJson {
    pub present: bool,
    pub percent: f64,
    pub charging: bool,
    pub on_ac_power: bool,
    pub time_remaining_min: Option<i64>,
    pub watts: Option<f64>,
    pub cycle_count: Option<i64>,
    pub health_percent: Option<i64>,
    pub temperature_c: Option<f64>,
}

/// A row. `name` is the basename and `command` the full path, because a script
/// filtering on "Google Chrome Helper" needs the first and a script that has to
/// tell two of them apart needs the second.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessJson {
    pub pid: i32,
    pub name: String,
    pub command: String,
    pub user: String,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub energy: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OthersJson {
    pub count: usize,
    pub cpu_percent: f64,
    pub rss_bytes: u64,
    pub energy: f64,
}

/// The whole document.
///
/// `version` is first and deliberate: it is how a consumer tells which shape it
/// is reading (I-25).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JsonOutput {
    pub version: String,
    pub cpu: CpuJson,
    pub memory: MemoryJson,
    pub disk: DiskJson,
    pub battery: BatteryJson,
    /// The **whole** working set, not a slice: a consumer that wants ten rows
    /// sorted by memory has `jq`, and guessing on its behalf is what `--top`
    /// and `--sort` were. See I-25b.
    pub processes: Vec<ProcessJson>,
    pub others: OthersJson,
    pub total: usize,
    pub energy_accurate: bool,
    /// Whether these numbers are scripted.
    ///
    /// The dashboard says so in its header, for the reason AGENTS.md gives: a
    /// screen of invented numbers that looks exactly like one of real numbers
    /// is a trap, and it outlives the terminal it was captured in. A consumer
    /// piping `--json` had no equivalent, so a script pointed at `--mock` by
    /// accident could not tell.
    pub mock: bool,
}

impl From<&CpuData> for CpuJson {
    fn from(cpu: &CpuData) -> Self {
        Self {
            system: cpu.system,
            per_core: cpu.per_core.clone(),
            load_avg: cpu.load_avg,
        }
    }
}

impl From<&MemoryData> for MemoryJson {
    fn from(memory: &MemoryData) -> Self {
        Self {
            total_bytes: memory.total_bytes,
            used_bytes: memory.used_bytes,
            free_bytes: memory.free_bytes,
            available_bytes: memory.available_bytes,
            wired_bytes: memory.wired_bytes,
            active_bytes: memory.active_bytes,
            inactive_bytes: memory.inactive_bytes,
            compressed_bytes: memory.compressed_bytes,
            swap_total_bytes: memory.swap_total_bytes,
            swap_used_bytes: memory.swap_used_bytes,
        }
    }
}

impl From<&VolumeUsage> for VolumeJson {
    fn from(volume: &VolumeUsage) -> Self {
        Self {
            mount: volume.mount.clone(),
            device: volume.device.clone(),
            total_bytes: volume.total_bytes,
            used_bytes: volume.used_bytes,
            free_bytes: volume.free_bytes,
            network: volume.network,
        }
    }
}

impl From<&DiskData> for DiskJson {
    fn from(disk: &DiskData) -> Self {
        Self {
            mount: disk.mount.clone(),
            total_bytes: disk.total_bytes,
            used_bytes: disk.used_bytes,
            free_bytes: disk.free_bytes,
            volumes: disk.volumes.iter().map(Into::into).collect(),
        }
    }
}

impl From<&BatteryData> for BatteryJson {
    fn from(battery: &BatteryData) -> Self {
        Self {
            present: battery.present,
            percent: battery.percent,
            charging: battery.charging,
            on_ac_power: battery.on_ac_power,
            time_remaining_min: battery.time_remaining_min,
            watts: battery.watts,
            cycle_count: battery.cycle_count,
            health_percent: battery.health_percent,
            temperature_c: battery.temperature_c,
        }
    }
}

impl From<&ProcessSample> for ProcessJson {
    fn from(sample: &ProcessSample) -> Self {
        Self {
            pid: sample.pid,
            name: process_name(&sample.command),
            command: sample.command.clone(),
            user: sample.user.clone(),
            cpu_percent: sample.cpu_percent,
            rss_bytes: sample.rss_bytes,
            energy: sample.energy,
        }
    }
}

impl From<&OthersRollup> for OthersJson {
    fn from(rollup: &OthersRollup) -> Self {
        Self {
            count: rollup.count,
            cpu_percent: rollup.cpu_percent,
            rss_bytes: rollup.rss_bytes,
            energy: rollup.energy,
        }
    }
}
