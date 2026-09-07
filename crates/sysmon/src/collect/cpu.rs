//! Per-core CPU tick counters.
//!
//! # Why this module is the one that uses `unsafe`
//!
//! Node got these for free from `os.cpus()`. Rust's standard library has no
//! equivalent, and macOS exposes them only through mach's `host_processor_info`
//! — there is no file to read and no command that prints them cheaply. `top`
//! would cost ~1s of CPU per sample, which is five times this app's entire
//! budget, and the whole point of this collector is that it is free (I-6, and
//! the tier notes: CPU can be sampled often precisely because it spawns
//! nothing).
//!
//! So the workspace sets `unsafe_code = "deny"` rather than `forbid`, and this
//! module opts out for about twenty lines. The unsafety is confined to one
//! call and one slice construction, both immediately below; everything above
//! and below this file is safe code operating on a plain `Vec<CpuTimes>`.
#![allow(unsafe_code)]

use sysmon_core::domain::deltas::CpuTimes;

/// Reads every core's cumulative tick counters.
///
/// The counters are monotonic and cumulative, which is what makes utilisation a
/// delta rather than an instantaneous reading — see I-1: the first sample can
/// only ever be half a measurement.
///
/// macOS reports four states (user, system, idle, nice) and has no separate
/// interrupt bucket, so `irq` is always zero. Node's `os.cpus()` reports the
/// same zero on this platform, so the arithmetic is unchanged.
pub fn per_core_times() -> Result<Vec<CpuTimes>, String> {
    let mut count: libc::natural_t = 0;
    let mut info: *mut libc::processor_cpu_load_info_t = std::ptr::null_mut();
    let mut info_count: libc::mach_msg_type_number_t = 0;

    // SAFETY: `mach_host_self()` returns a send right that is valid for the
    // life of the process. `host_processor_info` writes the out-parameters only
    // on success (checked below), and is the documented way to obtain
    // PROCESSOR_CPU_LOAD_INFO. Deprecated in the libc crate but not in the OS,
    // and it remains what Activity Monitor and `top` themselves use.
    let result = unsafe {
        #[allow(deprecated)]
        let host = libc::mach_host_self();
        libc::host_processor_info(
            host,
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut count,
            std::ptr::addr_of_mut!(info).cast(),
            &mut info_count,
        )
    };
    if result != libc::KERN_SUCCESS {
        return Err(format!(
            "host_processor_info failed with kern error {result}"
        ));
    }
    if info.is_null() {
        return Err("host_processor_info returned no data".to_string());
    }

    // SAFETY: on success the kernel guarantees `count` structs at `info`, each
    // holding CPU_STATE_MAX ticks. The slice is read and copied out before the
    // allocation is released below, and never escapes this function.
    let out = unsafe { read_tick_counters(info, count) };
    // SAFETY: the kernel allocated this with vm_allocate on our behalf, and the
    // caller owns it. `info_count` is in units of natural_t, as returned above.
    unsafe { release_tick_counters(info, info_count) };

    Ok(out)
}

/// # Safety
///
/// `counters` must point at `count` `[natural_t; CPU_STATE_MAX]` structs, as
/// `host_processor_info` guarantees on success.
#[allow(unsafe_code)]
unsafe fn read_tick_counters(
    counters: *mut libc::processor_cpu_load_info_t,
    count: libc::natural_t,
) -> Vec<CpuTimes> {
    let ticks = std::slice::from_raw_parts(
        counters as *const [libc::natural_t; libc::CPU_STATE_MAX as usize],
        count as usize,
    );
    ticks
        .iter()
        .map(|t| CpuTimes {
            user: t[libc::CPU_STATE_USER as usize] as f64,
            nice: t[libc::CPU_STATE_NICE as usize] as f64,
            sys: t[libc::CPU_STATE_SYSTEM as usize] as f64,
            idle: t[libc::CPU_STATE_IDLE as usize] as f64,
            // macOS has no separate interrupt bucket; `os.cpus()` reports 0 here
            // too, so the delta arithmetic is unchanged.
            irq: NO_INTERRUPT_BUCKET,
        })
        .collect()
}

/// # Safety
///
/// `counters` must be the allocation `host_processor_info` handed out, and
/// `len` its length in `natural_t` units.
#[allow(unsafe_code)]
unsafe fn release_tick_counters(
    counters: *mut libc::processor_cpu_load_info_t,
    len: libc::mach_msg_type_number_t,
) {
    #[allow(deprecated)]
    let task = libc::mach_task_self();
    libc::vm_deallocate(
        task,
        counters as libc::vm_address_t,
        len as usize * std::mem::size_of::<libc::natural_t>(),
    );
}

/// macOS has no separate interrupt bucket, and `os.cpus()` reports zero here
/// too, so the delta arithmetic is unchanged by it.
const NO_INTERRUPT_BUCKET: f64 = 0.0;
/// A figure `getloadavg` could not supply.
const NO_READING: f64 = 0.0;
/// `getloadavg` reports the 1, 5 and 15 minute figures.
const LOAD_WINDOWS: usize = 3;

/// The 1, 5 and 15 minute figures, in that order.
pub fn load_average() -> [f64; LOAD_WINDOWS] {
    let mut avg = [NO_READING; LOAD_WINDOWS];
    // SAFETY: `getloadavg` writes at most the number of elements it is told the
    // buffer holds, and the buffer is a local array of exactly that size.
    let written = unsafe { libc::getloadavg(avg.as_mut_ptr(), LOAD_WINDOWS as libc::c_int) };
    if written < 0 {
        return [NO_READING; LOAD_WINDOWS];
    }
    avg
}
