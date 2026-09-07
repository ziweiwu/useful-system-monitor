//! Collectors: everything that reads from the machine.
//!
//! Each platform has its own implementation producing the same shapes, so the
//! renderer, the runtime and the kill path never learn which one they are
//! talking to. That seam is what lets the native-API phase — libproc and IOKit
//! instead of seven shelled-out commands — be a swap rather than a rewrite.

use std::collections::HashMap;

use sysmon_core::domain::types::{OthersRollup, ProcessSample, StartTime};

#[cfg(target_os = "macos")]
pub mod cpu;
#[cfg(target_os = "macos")]
pub mod darwin;
#[cfg(target_os = "macos")]
pub mod exec;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod mock;

/// The platform's collector. Both expose the same methods.
#[cfg(target_os = "macos")]
pub use darwin::DarwinCollector as Platform;
#[cfg(target_os = "linux")]
pub use linux::LinuxCollector as Platform;

use sysmon_core::domain::types::{BatteryData, CpuData, DiskData, HostInfo, MemoryData};
use sysmon_core::domain::working_set::WorkingSetCap;

/// Where the numbers come from.
///
/// An enum rather than a trait object: there are exactly two, the choice is
/// made once at startup, and dispatching statically keeps the collector threads
/// free of an allocation and a vtable on a path whose entire justification is
/// that it is cheap.
pub enum Collector {
    Platform(Box<Platform>),
    Mock(Box<mock::MockCollector>),
}

macro_rules! dispatch {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Collector::Platform(c) => c.$method($($arg),*),
            Collector::Mock(c) => c.$method($($arg),*),
        }
    };
}

impl Collector {
    pub fn new(accurate_energy: bool) -> Result<Self, String> {
        Ok(Self::Platform(Box::new(Platform::new(accurate_energy)?)))
    }

    pub fn mock() -> Result<Self, String> {
        Ok(Self::Mock(Box::new(mock::MockCollector::new(false)?)))
    }

    /// Both real collectors surface their errors as strings by this point; the
    /// platform ones carry the richer `CommandError` internally, where the
    /// distinction between "ran and said no" and "never answered" still
    /// matters. See I-16.
    pub fn host(&self) -> HostInfo {
        dispatch!(self, host)
    }

    pub fn cpu(&mut self) -> Result<CpuData, String> {
        dispatch!(self, cpu)
    }

    pub fn memory(&self) -> Result<MemoryData, String> {
        match self {
            Collector::Platform(c) => c.memory().map_err(|e| e.to_string()),
            Collector::Mock(c) => c.memory(),
        }
    }

    pub fn disk(&self) -> Result<DiskData, String> {
        dispatch!(self, disk)
    }

    pub fn battery(&self) -> Result<BatteryData, String> {
        match self {
            Collector::Platform(c) => c.battery().map_err(|e| e.to_string()),
            Collector::Mock(c) => c.battery(),
        }
    }

    pub fn processes(&mut self, cap: WorkingSetCap) -> Result<ProcessesData, String> {
        match self {
            Collector::Platform(c) => c.processes(cap).map_err(|e| e.to_string()),
            Collector::Mock(c) => c.processes(cap),
        }
    }

    pub fn identity(&self, pid: i32) -> Result<Identity, String> {
        match self {
            Collector::Platform(c) => c.identity(pid).map_err(|e| e.to_string()),
            Collector::Mock(c) => c.identity(pid),
        }
    }

    pub fn command_line(&self, pid: i32) -> Option<String> {
        dispatch!(self, command_line, pid)
    }
}

/// Everything one `processes()` call produces.
///
/// Some of this is reached only by the dashboard, which does not exist yet: the
/// kill path and its inputs landed first on purpose, because every past bug
/// there was a distinction collapsed under time pressure. See the milestone
/// order in the plan.
#[allow(dead_code)]
pub struct ProcessesData {
    pub total: usize,
    pub visible: Vec<ProcessSample>,
    pub others: OthersRollup,
    /// pid -> ppid for **every** process, not just the working set. The
    /// ancestor guard (I-13) walks up from our own PID, and those ancestors are
    /// usually idle shells that never make the top 50; building this from
    /// `visible` alone silently breaks the guard.
    pub parents: HashMap<i32, i32>,
    /// True when `energy` carries macOS's measured Energy Impact rather than
    /// the CPU-time estimate. The column is one unit or the other, never a
    /// blend — see the note in `build`.
    pub energy_accurate: bool,
}

/// What the process itself can be identified as, right now. See I-16.
#[allow(dead_code)]
pub enum Identity {
    Known(StartTime),
    Gone,
}
