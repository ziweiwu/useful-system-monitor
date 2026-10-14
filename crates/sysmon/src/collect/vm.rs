//! Memory and swap, read from the kernel instead of from `vm_stat` and `sysctl`.
//!
//! # Why this module uses `unsafe`
//!
//! The memory tier runs at the `--interval`, and every run spawned three
//! processes — `vm_stat`, `sysctl vm.swapusage`, `sysctl hw.memsize` — to print
//! counters this process can read itself: once a second at `--interval 1`.
//! These are the calls those commands make (`host_statistics64` with
//! `HOST_VM_INFO64` and `host_page_size`; `vm.swapusage` and `hw.memsize`), so
//! every figure is the counter they print, unrounded: `sysctl` formats swap to
//! hundredths of a megabyte. The unsafety is the four calls below, each into a
//! correctly sized local.
#![allow(unsafe_code)]

use std::ffi::{c_void, CStr};
use std::mem::size_of;

use sysmon_core::domain::types::MemoryData;

/// One reading of every figure the memory panel shows, or `None` if any of
/// the calls failed — the caller then falls back to the commands.
///
/// The arithmetic is `parse_memory`'s, on the same counters: `vm_stat`'s
/// "Pages free" is `free_count - speculative_count`, and `parse_memory` adds
/// speculative back, so free is `free_count`.
pub fn read() -> Option<MemoryData> {
    let vm = vm_statistics()?;
    let page = page_size()?;
    let swap = swap_usage()?;
    let total_bytes = sysctl_value::<u64>(c"hw.memsize")?;
    let bytes = |pages: u32| u64::from(pages) * page;

    let wired = bytes(vm.wire_count);
    let active = bytes(vm.active_count);
    // "Occupied by compressor", the compressed footprint, as in `parse_memory`.
    let compressed = bytes(vm.compressor_page_count);
    let used = (wired + active + compressed).min(total_bytes);
    Some(MemoryData {
        total_bytes,
        used_bytes: used,
        free_bytes: bytes(vm.free_count),
        // I-5: derived, so the two agree exactly.
        available_bytes: total_bytes.saturating_sub(used),
        wired_bytes: wired,
        active_bytes: active,
        inactive_bytes: bytes(vm.inactive_count),
        compressed_bytes: compressed,
        swap_total_bytes: swap.xsu_total,
        swap_used_bytes: swap.xsu_used,
    })
}

#[allow(deprecated)] // in the libc crate, in favour of `mach2`; not in the OS
fn vm_statistics() -> Option<libc::vm_statistics64> {
    // SAFETY: an all-zero `vm_statistics64` is a valid value of a struct of
    // integers.
    let mut vm: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: `mach_host_self()` is valid for the life of the process, and
    // `count` tells the kernel how many words `vm` has room for.
    let rc = unsafe {
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            std::ptr::addr_of_mut!(vm).cast(),
            &mut count,
        )
    };
    (rc == libc::KERN_SUCCESS).then_some(vm)
}

/// The kernel's page size, which is what `vm_stat` multiplies its counts by.
#[allow(deprecated)] // as above
fn page_size() -> Option<u64> {
    let mut size: libc::vm_size_t = 0;
    // SAFETY: as above, and `size` is a writable `vm_size_t`.
    let rc = unsafe { host_page_size(libc::mach_host_self(), &mut size) };
    (rc == libc::KERN_SUCCESS && size > 0).then(|| u64::try_from(size).ok())?
}

fn swap_usage() -> Option<libc::xsw_usage> {
    sysctl_value::<libc::xsw_usage>(c"vm.swapusage")
}

/// A fixed-size sysctl, or `None` unless it filled exactly a `T`.
fn sysctl_value<T>(name: &CStr) -> Option<T> {
    let mut out = std::mem::MaybeUninit::<T>::zeroed();
    let mut len = size_of::<T>();
    // SAFETY: `out` is `len` bytes; sysctlbyname writes at most that and
    // stores how many it wrote. Every `T` here is plain integers, valid for
    // any bit pattern, so it is initialised whatever was written.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            out.as_mut_ptr().cast::<c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    // SAFETY: as above.
    (rc == 0 && len == size_of::<T>()).then(|| unsafe { out.assume_init() })
}

extern "C" {
    /// Not exported by the libc crate.
    fn host_page_size(host: libc::mach_port_t, size: *mut libc::vm_size_t) -> libc::kern_return_t;
}
