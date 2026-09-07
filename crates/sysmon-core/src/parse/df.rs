//! `df -k`. Blocks are 1024 bytes.

use std::sync::LazyLock;

use regex::Regex;

use crate::domain::types::{RootDisk, VolumeUsage};

/// The eight fixed numeric/percent columns, then the mount point as the
/// remainder. `map auto_home` and friends have a space in the device column and
/// so fail this outright, which is the intended outcome.
static DF_ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\S+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)%\s+(\d+)\s+(\d+)\s+(\S+)\s+(.*)$")
        .expect("static regex")
});

/// Capture groups in the `df -k` line pattern.
const BLOCKS: usize = 2;
const AVAILABLE: usize = 4;
const MOUNT: usize = 9;
/// `df -k` reports 1 KiB blocks.
const BYTES_PER_BLOCK: u64 = 1024;
/// One mount point's row.
///
/// Usage is computed as total minus available, **not** df's own "Used" column.
/// On APFS, `/` is a sealed read-only system snapshot whose Used column reads
/// 12 G on a machine whose shared container actually holds 285 G. Reporting the
/// Used column shows a 926 G disk as 1% full.
pub fn parse_df(stdout: &str, mount: &str) -> Option<RootDisk> {
    for line in stdout.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let Some(caps) = DF_ROW.captures(line) else {
            continue;
        };
        if caps[MOUNT].trim() != mount {
            continue;
        }
        let total_bytes = caps[BLOCKS].parse::<u64>().ok()? * BYTES_PER_BLOCK;
        let free_bytes = caps[AVAILABLE].parse::<u64>().ok()? * BYTES_PER_BLOCK;
        return Some(RootDisk {
            mount: mount.to_string(),
            total_bytes,
            used_bytes: total_bytes.saturating_sub(free_bytes),
            free_bytes,
        });
    }
    None
}

struct Row {
    device: String,
    mount: String,
    total_bytes: u64,
    free_bytes: u64,
}

/// Every storage row in the output, before grouping.
fn df_rows(stdout: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for line in stdout.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let Some(caps) = DF_ROW.captures(line) else {
            continue;
        };
        let device = caps[1].to_string();
        let mount = caps[MOUNT].trim().to_string();
        let total_bytes = caps[BLOCKS].parse::<u64>().unwrap_or(0) * BYTES_PER_BLOCK;
        // Zero-block and devfs-style entries are not storage.
        if total_bytes == 0 || device == "devfs" || device == "devfs." {
            continue;
        }
        rows.push(Row {
            device,
            mount,
            total_bytes,
            free_bytes: caps[AVAILABLE].parse::<u64>().unwrap_or(0) * BYTES_PER_BLOCK,
        });
    }
    rows
}

/// Group by APFS container, so one pool reports once.
///
/// Anything that is not a `/dev/diskN` device (network shares, disk images) is
/// its own group. Insertion order is kept, so the result is deterministic
/// before sorting.
fn by_container(rows: Vec<Row>) -> Vec<Vec<Row>> {
    static CONTAINER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^/dev/(disk\d+)").expect("static regex"));
    let mut keys: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<Row>> = Vec::new();
    for r in rows {
        let key = CONTAINER
            .captures(&r.device)
            .map_or_else(|| r.device.clone(), |c| c[1].to_string());
        match keys.iter().position(|k| *k == key) {
            Some(i) => groups[i].push(r),
            None => {
                keys.push(key);
                groups.push(vec![r]);
            }
        }
    }
    groups
}

/// The volume that stands for one container, or `None` if it is a macOS
/// internal that should not be listed.
///
/// The outermost mount represents the pool: `/` if the container holds it,
/// otherwise the shortest path, which is the one a user recognises. Containers
/// surfacing only under `/System/Volumes` are internals (Preboot, Update/SFR,
/// the firmware volumes); `/` is never excluded here because it is picked as
/// the representative above.
fn representative(group: &[Row]) -> Option<VolumeUsage> {
    static NFS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^/]+:/").expect("static regex"));
    let rep = group
        .iter()
        .find(|r| r.mount == "/")
        .or_else(|| group.iter().min_by_key(|r| r.mount.len()))?;
    if rep.mount.starts_with("/System/Volumes/") {
        return None;
    }
    Some(VolumeUsage {
        mount: rep.mount.clone(),
        device: rep.device.clone(),
        total_bytes: rep.total_bytes,
        used_bytes: rep.total_bytes.saturating_sub(rep.free_bytes),
        free_bytes: rep.free_bytes,
        // SMB/AFP shares are `//user@host/share`; NFS is `host:/export`.
        network: rep.device.starts_with("//") || NFS.is_match(&rep.device),
    })
}

/// Every user-facing volume in `df -k` output.
///
/// Two kinds of noise have to go, or the panel lies about how much storage the
/// machine has:
///
/// 1. **APFS container siblings.** `/`, `/System/Volumes/Data`,
///    `/System/Volumes/VM` and `/System/Volumes/Preboot` are separate mounts
///    sharing one pool, so df reports the *same* total and available blocks for
///    each. Listing them verbatim shows a 926 G disk four times. They are
///    grouped by container (`/dev/disk3s5` -> `disk3`) and reported once, at
///    their outermost mount.
/// 2. **Pseudo and firmware filesystems**: devfs, `map auto_home` (zero blocks),
///    and the iSCPreboot/xarts/Hardware volumes, which are macOS internals no
///    user can act on.
pub fn parse_df_all(stdout: &str) -> Vec<VolumeUsage> {
    let groups = by_container(df_rows(stdout));
    let mut out: Vec<VolumeUsage> = groups.iter().filter_map(|g| representative(g)).collect();
    // Root first, then alphabetically, so the list is stable across samples.
    out.sort_by(|a, b| match (a.mount.as_str(), b.mount.as_str()) {
        ("/", "/") => std::cmp::Ordering::Equal,
        ("/", _) => std::cmp::Ordering::Less,
        (_, "/") => std::cmp::Ordering::Greater,
        _ => a.mount.cmp(&b.mount),
    });
    out
}
