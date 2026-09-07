//! Scripted data, for working on the interface without touching the machine.
//!
//! `--mock` exists so the dashboard can be developed, screenshotted and fuzzed
//! on any machine, in any state, reproducibly — including the states a real Mac
//! will not hold still in: a disk at 94%, a battery discharging at 40 W, a
//! process list churning.
//!
//! # Why the arithmetic here is integer and seeded
//!
//! The TypeScript mock uses `Math.sin`. V8's `sin` and the platform libm's can
//! differ by one unit in the last place, which is enough to flip a `toFixed(1)`
//! at a rounding boundary — so a frame-for-frame comparison between the two
//! builds would show differences that are neither build's fault. Everything
//! below is integer or exactly-representable, and driven by a seeded LCG, so
//! the same tick produces the same bytes on any machine and in either language.

use std::collections::HashMap;

use sysmon_core::domain::types::{
    BatteryData, CpuData, DiskData, HostInfo, MemoryData, ProcessSample, StartTime, VolumeUsage,
};
use sysmon_core::domain::working_set::{select_working_set, WorkingSetCap};
use sysmon_core::kill::guards::is_protected_name;

use crate::collect::{Identity, ProcessesData};

/// Names chosen to exercise the table: spaces, a protected process, an emoji
/// with the variation selector, and a CJK path.
const NAMES: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome Helper (Renderer)",
    "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer",
    "/Applications/Slack.app/Contents/MacOS/Slack",
    "/usr/libexec/logd",
    "/Applications/⚠️ Warning App.app/Contents/MacOS/⚠️ Warning App",
    "/Users/x/日本語/とても長いパス/アプリケーション.app/Contents/MacOS/アプリ",
    "/opt/homebrew/bin/node",
    "/usr/sbin/mds_stores",
    "/Applications/Firefox.app/Contents/MacOS/plugin-container",
];

/// A triangle wave: `period` ticks climbing from `low` to `high` and back,
/// started `phase` ticks in. Grouped into a shape because four positional
/// numbers at a call site say nothing about which is which.
#[derive(Clone, Copy)]
struct Wave {
    period: u64,
    low: f64,
    high: f64,
    phase: u64,
}

/// The top of the normalised climb, before it is scaled into `low..high`.
const PEAK: f64 = 1.0;

// ---- the mock machine, fixed so a scripted run looks the same everywhere ----
const MOCK_CORE_COUNT: u64 = 10;
/// Fans the per-core curves out so they do not all peak on the same tick.
const CORE_PHASE_STEP: u64 = 3;
/// Roughly the user/system split a busy Mac shows.
const USER_SHARE: f64 = 0.7;
const SYS_SHARE: f64 = 0.3;
/// Load average is derived from CPU so the three figures stay plausibly ordered.
const LOAD_1MIN_DIVISOR: f64 = 12.0;
const LOAD_5MIN_DIVISOR: f64 = 14.0;
const LOAD_15MIN_DIVISOR: f64 = 16.0;

/// How the mock splits "used" across the memory buckets. Arbitrary, but fixed,
/// so the breakdown bars always add up the same way.
const WIRED_SHARE: u64 = 4;
const ACTIVE_SHARE: u64 = 3;
const INACTIVE_SHARE: u64 = 8;
const COMPRESSED_SHARE: u64 = 6;

/// Leaves the boot volume comfortably under the "nearly full" threshold, so the
/// warning on the mock's network volume is the only one on screen.
const DISK_USED_FRACTION: f64 = 0.36;

// ---- a battery with every optional field populated ----
/// Ticks between charging and draining, so both states are reachable by waiting.
const CHARGE_FLIP_TICKS: u64 = 30;
const MINUTES_REMAINING: i64 = 143;
const CHARGE_WATTS: f64 = 32.5;
const DRAW_WATTS: f64 = -12.4;
const CYCLE_COUNT: i64 = 203;
const HEALTH_PCT: i64 = 91;
const TEMPERATURE_C: f64 = 30.2;

// ---- the mock process table ----
/// Enough rows that the working-set caps and the rollup both have work to do.
const MOCK_PROCESS_COUNT: usize = 120;
const FIRST_MOCK_PID: i32 = 400;
const PID_STRIDE: i32 = 7;
/// Spreads the per-process curves so the table reorders as it runs.
const PROCESS_PERIOD_SPREAD: u64 = 11;
/// Every Nth mock process runs as root, so both users appear in the table.
const ROOT_EVERY: i32 = 5;
/// I-1: every Nth row reports no sample, so the "—" path is on screen rather
/// than only in a test.
const UNMEASURED_EVERY: i32 = 37;
const MIN_RSS_MB: u64 = 8;
const RSS_SPREAD_MB: u32 = 900;

pub struct MockCollector {
    tick: u64,
    rng: u32,
    /// PIDs the user has "closed", so a kill visibly does something.
    killed: Vec<i32>,
}

impl MockCollector {
    pub fn new(_accurate_energy: bool) -> Result<Self, String> {
        Ok(Self {
            tick: 0,
            rng: 0x5eed,
            killed: Vec::new(),
        })
    }

    /// A small LCG — the same one the corpus generators use, so a scripted run
    /// is reproducible across machines and languages.
    fn next(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.rng
    }

    /// A value that moves smoothly between `low` and `high` — a triangle wave,
    /// not a sine, so it is exact in binary at every tick.
    fn wave(&self, shape: Wave) -> f64 {
        let position = (self.tick + shape.phase) % shape.period;
        let half = shape.period / 2;
        let climbing = position < half;
        let up = if climbing {
            position as f64 / half as f64
        } else {
            PEAK - (position - half) as f64 / half as f64
        };
        shape.low + (shape.high - shape.low) * up
    }

    pub fn host(&self) -> HostInfo {
        HostInfo {
            model: "Mock Silicon M-1".into(),
            cores: 10,
            perf_cores: 8,
            eff_cores: 2,
            total_mem_bytes: 17_179_869_184,
            uptime_sec: 264_000 + self.tick * 10,
        }
    }

    pub fn cpu(&mut self) -> Result<CpuData, String> {
        self.tick += 1;
        let system = self.wave(Wave {
            period: 24,
            low: 8.0,
            high: 82.0,
            phase: 0,
        });
        let cores = 0..MOCK_CORE_COUNT;
        let per_core = cores
            .map(|core| {
                self.wave(Wave {
                    period: 18 + core,
                    low: 2.0,
                    high: 98.0,
                    phase: core * CORE_PHASE_STEP,
                })
            })
            .collect::<Vec<_>>();
        Ok(CpuData {
            per_core,
            system,
            user_percent: system * USER_SHARE,
            sys_percent: system * SYS_SHARE,
            load_avg: [
                system / LOAD_1MIN_DIVISOR,
                system / LOAD_5MIN_DIVISOR,
                system / LOAD_15MIN_DIVISOR,
            ],
        })
    }

    pub fn memory(&self) -> Result<MemoryData, String> {
        let total = 17_179_869_184u64;
        let pressure = self.wave(Wave {
            period: 30,
            low: 0.55,
            high: 0.88,
            phase: 5,
        });
        let used = (total as f64 * pressure) as u64;
        Ok(MemoryData {
            total_bytes: total,
            used_bytes: used,
            free_bytes: 93_732_864,
            available_bytes: total - used,
            wired_bytes: used / WIRED_SHARE,
            active_bytes: used / ACTIVE_SHARE,
            inactive_bytes: total / INACTIVE_SHARE,
            compressed_bytes: used / COMPRESSED_SHARE,
            swap_total_bytes: 2_147_483_648,
            swap_used_bytes: (2_147_483_648f64
                * self.wave(Wave {
                    period: 40,
                    low: 0.1,
                    high: 0.8,
                    phase: 11,
                })) as u64,
        })
    }

    pub fn disk(&self) -> Result<DiskData, String> {
        let total = 994_662_584_320u64;
        let used = (total as f64 * DISK_USED_FRACTION) as u64;
        Ok(DiskData {
            mount: "/".into(),
            total_bytes: total,
            used_bytes: used,
            free_bytes: total - used,
            volumes: vec![
                VolumeUsage {
                    mount: "/".into(),
                    device: "/dev/disk3s5".into(),
                    total_bytes: total,
                    used_bytes: used,
                    free_bytes: total - used,
                    network: false,
                },
                // Deliberately past 90%, so the "above 90% full" note is on a
                // screen someone can actually look at.
                VolumeUsage {
                    mount: "/Volumes/media".into(),
                    device: "//user@nas/media".into(),
                    total_bytes: 8_000_000_000_000,
                    used_bytes: 7_600_000_000_000,
                    free_bytes: 400_000_000_000,
                    network: true,
                },
            ],
        })
    }

    pub fn battery(&self) -> Result<BatteryData, String> {
        let percent = self
            .wave(Wave {
                period: 60,
                low: 20.0,
                high: 100.0,
                phase: 0,
            })
            .round();
        let charging = (self.tick / CHARGE_FLIP_TICKS) % 2 == 0;
        Ok(BatteryData {
            present: true,
            percent,
            charging,
            on_ac_power: charging,
            time_remaining_min: (!charging).then_some(MINUTES_REMAINING),
            watts: Some(if charging { CHARGE_WATTS } else { DRAW_WATTS }),
            cycle_count: Some(CYCLE_COUNT),
            health_percent: Some(HEALTH_PCT),
            temperature_c: Some(TEMPERATURE_C),
        })
    }

    pub fn identity(&self, pid: i32) -> Result<Identity, String> {
        if self.killed.contains(&pid) {
            return Ok(Identity::Gone);
        }
        Ok(Identity::Known(StartTime::Known(1_700_000_000_000)))
    }

    pub fn command_line(&self, pid: i32) -> Option<String> {
        Some(format!(
            "{} --mock --pid={pid}",
            NAMES[(pid as usize) % NAMES.len()]
        ))
    }

    /// A kill in mock mode removes the process, so the action visibly does
    /// something without signalling anything real.
    pub fn simulate_kill(&mut self, pid: i32) {
        self.killed.push(pid);
    }

    /// One mock process. `index` drives everything about it, so the table is a
    /// pure function of the tick and stays identical across machines.
    fn mock_process(&mut self, index: i32) -> ProcessSample {
        let command = NAMES[(index as usize) % NAMES.len()].to_string();
        let cpu = self.wave(Wave {
            period: 20 + (index as u64 % PROCESS_PERIOD_SPREAD),
            low: 0.0,
            high: 60.0,
            phase: index as u64,
        });
        ProcessSample {
            pid: FIRST_MOCK_PID + index * PID_STRIDE,
            ppid: if index == 0 { 1 } else { FIRST_MOCK_PID },
            start_time: StartTime::Known(1_700_000_000_000),
            user: if index % ROOT_EVERY == 0 {
                "root".into()
            } else {
                "ziweiwu".into()
            },
            state: "S".into(),
            // I-1: a couple of rows have never been measured, so the "—" path is
            // on screen rather than only in a test.
            cpu_percent: (index % UNMEASURED_EVERY != 0).then_some(cpu),
            rss_bytes: (MIN_RSS_MB + (self.next() % RSS_SPREAD_MB) as u64) * 1_048_576,
            energy: (index % UNMEASURED_EVERY != 0).then_some(cpu),
            protected: is_protected_name(&command),
            command,
        }
    }

    pub fn processes(&mut self, cap: WorkingSetCap) -> Result<ProcessesData, String> {
        let mut all: Vec<ProcessSample> = Vec::with_capacity(MOCK_PROCESS_COUNT);
        for i in 0..MOCK_PROCESS_COUNT as i32 {
            if self.killed.contains(&(FIRST_MOCK_PID + i * PID_STRIDE)) {
                continue;
            }
            all.push(self.mock_process(i));
        }
        let total = all.len();
        let ws = select_working_set(&all, cap);
        let parents: HashMap<i32, i32> = all.iter().map(|p| (p.pid, p.ppid)).collect();
        Ok(ProcessesData {
            total,
            visible: ws.visible,
            others: ws.others,
            parents,
            energy_accurate: false,
        })
    }
}
