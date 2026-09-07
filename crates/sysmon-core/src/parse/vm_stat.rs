//! `vm_stat` + `sysctl vm.swapusage`.

use std::sync::LazyLock;

use regex::Regex;

use crate::domain::types::MemoryData;

/// What `vm_stat` did not tell us. 4 KiB is the macOS page size everywhere this
/// ships.
const FALLBACK_PAGE_SIZE: u64 = 4096;
/// `vm.swapusage` suffixes are binary.
const KB: f64 = 1024.0;
const BYTES: f64 = 1.0;
/// A figure that would not parse.
const NO_READING: f64 = 0.0;
/// `os.freemem()`'s equivalent is deliberately not used: it reported 170 MB on a
/// machine with over a gigabyte genuinely available, because macOS counts
/// compressed and purgeable pages differently than that number does.
/// What counts as "used".
///
/// `top` reports total-minus-free, which on a healthy Mac reads 99.5% — true,
/// but useless as a gauge, because macOS deliberately leaves almost nothing
/// free and reclaims inactive pages on demand.
///
/// Activity Monitor's "Memory Used" is wired + app memory + compressed, which
/// measured 76.6% at the same instant. That is the number that actually moves
/// when you close something, so it is the one on the gauge. Inactive is
/// reported separately as reclaimable.
fn used_bytes(wired: u64, active: u64, compressed: u64) -> u64 {
    wired + active + compressed
}

/// Bytes behind one `vm_stat` label, using the page size the header declared.
fn page_bytes(vm_stat: &str, page_size: u64, label: &str) -> u64 {
    let re = Regex::new(&format!(r"(?m)^{}:\s+(\d+)\.", regex::escape(label)))
        .expect("label is escaped");
    re.captures(vm_stat)
        .and_then(|c| c[1].parse::<u64>().ok())
        .map_or(0, |n| n * page_size)
}

pub fn parse_memory(vm_stat: &str, swap_usage: &str, total_bytes: u64) -> MemoryData {
    static PAGE_SIZE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"page size of (\d+) bytes").expect("static regex"));

    let page_size: u64 = PAGE_SIZE
        .captures(vm_stat)
        .and_then(|c| c[1].parse().ok())
        .unwrap_or(FALLBACK_PAGE_SIZE);
    let pages = |label: &str| page_bytes(vm_stat, page_size, label);

    let free = pages("Pages free") + pages("Pages speculative");
    let active = pages("Pages active");
    let inactive = pages("Pages inactive");
    let wired = pages("Pages wired down");
    // "occupied by compressor" is the compressed footprint. "stored in
    // compressor" is the pre-compression size of the same data and would
    // massively over-count.
    let compressed = pages("Pages occupied by compressor");

    let used_clamped = used_bytes(wired, active, compressed).min(total_bytes);

    MemoryData {
        total_bytes,
        used_bytes: used_clamped,
        // True free, kept distinct from available: listing both `inactive` and
        // an "available" figure that already contains inactive would
        // double-count it in the breakdown.
        free_bytes: free,
        // I-5, and the reason this is derived rather than read: the two must
        // agree exactly, and two independent reads of a live machine will not.
        available_bytes: total_bytes.saturating_sub(used_clamped),
        wired_bytes: wired,
        active_bytes: active,
        inactive_bytes: inactive,
        compressed_bytes: compressed,
        swap_total_bytes: swap_num(swap_usage, "total"),
        swap_used_bytes: swap_num(swap_usage, "used"),
    }
}

/// `sysctl vm.swapusage` formats through `LC_NUMERIC`, so a comma-decimal
/// locale prints "total = 1024,00M". The collector pins `LC_NUMERIC=C`, but a
/// comma is accepted here too: the previous pattern stopped at the separator and
/// reported a machine with 1 GB of swap as having none. See I-28.
fn swap_num(swap_usage: &str, label: &str) -> u64 {
    let re = Regex::new(&format!(
        r"(?i){}\s*=\s*(\d+(?:[.,]\d+)?)([KMG])",
        regex::escape(label)
    ))
    .expect("label is escaped");
    let Some(caps) = re.captures(swap_usage) else {
        return 0;
    };
    let mult: f64 = match caps[2].to_ascii_uppercase().as_str() {
        "K" => KB,
        "M" => KB * KB,
        "G" => KB * KB * KB,
        _ => BYTES,
    };
    let n: f64 = caps[1].replace(',', ".").parse().unwrap_or(NO_READING);
    if n.is_finite() {
        (n * mult) as u64
    } else {
        0
    }
}
