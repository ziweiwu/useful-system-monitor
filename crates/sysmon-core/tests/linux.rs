//! The Linux `/proc` parsers.
//!
//! **These have no oracle.** The shipping build is macOS-only, so the
//! differential harness that checks every other parser against a known-good
//! implementation does not exist here, and the fixtures are written from the
//! documented `proc(5)` formats rather than captured from a running machine.
//! That is the single biggest risk in adding Linux to the first release, and it
//! is why these tests assert *properties* wherever they can rather than only
//! echoing the numbers back — a property survives a fixture being wrong in a
//! way an equality check does not.

use sysmon_core::parse::linux::{
    parse_meminfo, parse_mounts, parse_power_supply, parse_proc_pid_stat,
    parse_proc_pid_status_uid, parse_proc_stat, LINUX_PROTECTED_NAMES,
};

const PROC_STAT: &str = include_str!("../../../test/fixtures/linux/proc-stat.txt");
const MEMINFO: &str = include_str!("../../../test/fixtures/linux/meminfo.txt");
const PID_STAT: &str = include_str!("../../../test/fixtures/linux/proc-pid-stat.txt");
const PID_STATUS: &str = include_str!("../../../test/fixtures/linux/proc-pid-status.txt");
const MOUNTS: &str = include_str!("../../../test/fixtures/linux/mounts.txt");
const BATTERY: &str = include_str!("../../../test/fixtures/linux/battery-uevent.txt");

// ------------------------------------------------------------- /proc/stat --

/// The aggregate `cpu` line is not a core. Counting it would report a 4-core
/// machine as having five, and the fifth would be the mean of the other four —
/// which then skews the mean. See I-6.
#[test]
fn i6_proc_stat_reads_cores_and_skips_the_aggregate_line() {
    let cores = parse_proc_stat(PROC_STAT);
    assert_eq!(
        cores.len(),
        4,
        "the aggregate line must not count as a core"
    );
    for c in &cores {
        assert!(c.user > 0.0 && c.idle > 0.0);
    }
}

/// A core waiting on disk is not doing work. Counting `iowait` as busy makes an
/// idle machine that is copying a file read as pegged.
#[test]
fn iowait_is_folded_into_idle_rather_than_counted_as_busy() {
    let c = &parse_proc_stat(PROC_STAT)[0];
    // 22200000 idle + 3100 iowait
    assert_eq!(c.idle, 22_203_100.0);
    // 0 irq + 1700 softirq
    assert_eq!(c.irq, 1_700.0);
}

#[test]
fn proc_stat_survives_junk() {
    assert!(parse_proc_stat("").is_empty());
    assert!(parse_proc_stat("cpu  1 2 3\nnonsense\n").is_empty());
}

// ---------------------------------------------------------------- meminfo --

/// I-5, and it has to hold on this platform too: the gauge measures used
/// against total, so the two must partition it exactly.
#[test]
fn i5_memory_splits_total_into_used_and_available_exactly() {
    let m = parse_meminfo(MEMINFO);
    assert_eq!(m.used_bytes + m.available_bytes, m.total_bytes);
}

/// `MemFree` alone reports a healthy Linux machine as almost full, for the same
/// reason `top`'s total-minus-free does on macOS: the page cache is reclaimable,
/// so counting it as used makes every machine look full.
#[test]
fn i5_used_is_derived_from_available_not_from_free() {
    let memory = parse_meminfo(MEMINFO);
    assert!(memory.free_bytes < memory.available_bytes);
    let pct = (memory.used_bytes as f64 / memory.total_bytes as f64) * 100.0;
    assert!(
        pct > 10.0 && pct < 97.0,
        "{pct}% is not a plausible reading"
    );
}

#[test]
fn meminfo_reads_kilobytes_as_bytes_and_computes_swap_used() {
    let m = parse_meminfo(MEMINFO);
    assert_eq!(m.total_bytes, 16_305_432 * 1024);
    assert_eq!(m.swap_total_bytes, 2_097_152 * 1024);
    assert_eq!(m.swap_used_bytes, (2_097_152 - 1_048_576) * 1024);
}

/// A missing field is zero, not a panic: `/proc/meminfo` varies by kernel
/// version and by whether zswap is compiled in.
#[test]
fn meminfo_treats_a_missing_field_as_zero() {
    let m = parse_meminfo("MemTotal:  1024 kB\nMemAvailable:  512 kB\n");
    assert_eq!(m.total_bytes, 1024 * 1024);
    assert_eq!(m.compressed_bytes, 0);
    assert_eq!(m.used_bytes + m.available_bytes, m.total_bytes);
}

// ------------------------------------------------------- /proc/[pid]/stat --

/// **The parse everyone gets wrong.** Field 2 is the executable name in
/// parentheses, and it may contain spaces *and* parentheses — `(Web Content
/// (tab))` is a real Firefox process. Splitting on whitespace shifts every
/// later field by an unpredictable amount and yields plausible nonsense rather
/// than an error, so the name is delimited by the **last** `)`.
#[test]
fn a_process_name_containing_spaces_and_parens_does_not_shift_every_field() {
    let (raw, meta) = parse_proc_pid_stat(PID_STAT, 100, 4096, 1_754_079_004_000)
        .expect("the fixture should parse");
    assert_eq!(meta.command, "Web Content (tab)");
    assert_eq!(raw.pid, 4242);
    assert_eq!(raw.ppid, 1337);
    assert_eq!(meta.state, "S");
    // utime 45678 + stime 12345 ticks at 100 Hz = 580.23s
    assert_eq!(raw.cpu_time_ms, (45_678 + 12_345) * 1000 / 100);
    // rss is in pages, not bytes or kB.
    assert_eq!(raw.rss_bytes, 65_536 * 4096);
}

#[test]
fn a_start_time_is_an_instant_not_a_tick_count() {
    let boot = 1_754_079_004_000i64;
    let (_, meta) = parse_proc_pid_stat(PID_STAT, 100, 4096, boot).expect("parses");
    // starttime 987654 ticks at 100 Hz = 9876.54s after boot.
    assert_eq!(meta.start_time.epoch_ms(), Some(boot + 9_876_540));
}

#[test]
fn a_truncated_or_empty_stat_line_is_refused_rather_than_guessed() {
    assert!(parse_proc_pid_stat("", 100, 4096, 0).is_none());
    assert!(parse_proc_pid_stat("4242 (short) S 1", 100, 4096, 0).is_none());
    assert!(parse_proc_pid_stat("no parens here", 100, 4096, 0).is_none());
}

/// Only the *real* uid: a setuid binary's effective uid is not who is running
/// it, and the table answers "whose process is this".
#[test]
fn the_owner_is_the_real_uid() {
    assert_eq!(parse_proc_pid_status_uid(PID_STATUS), Some(1000));
    assert_eq!(parse_proc_pid_status_uid("Name:\tx\n"), None);
}

// ----------------------------------------------------------- /proc/mounts --

/// A list that shows `proc`, `sysfs` and eleven `tmpfs` entries alongside the
/// disk is not a disk panel — the same judgement `parse_df_all` makes on macOS.
#[test]
fn pseudo_filesystems_are_dropped() {
    let mounts = parse_mounts(MOUNTS);
    let paths: Vec<&str> = mounts.iter().map(|m| m.mount.as_str()).collect();
    assert_eq!(
        paths,
        vec!["/", "/boot/efi", "/mnt/backup", "/mnt/data", "/mnt/media"]
    );
    assert!(!paths
        .iter()
        .any(|p| p.starts_with("/sys") || p.starts_with("/proc")));
    assert!(!paths.contains(&"/run"), "tmpfs is not storage");
}

#[test]
fn network_filesystems_are_flagged() {
    let mounts = parse_mounts(MOUNTS);
    let net: Vec<&str> = mounts
        .iter()
        .filter(|m| m.network)
        .map(|m| m.mount.as_str())
        .collect();
    assert_eq!(net, vec!["/mnt/backup", "/mnt/media"]);
    assert!(
        !mounts
            .iter()
            .find(|m| m.mount == "/")
            .expect("root")
            .network
    );
}

/// A bind mount and a btrfs subvolume both report the same device with the same
/// totals — the Linux shape of the APFS-container problem. Listing them
/// verbatim multiplies the machine's apparent storage.
#[test]
fn one_device_is_reported_once_at_its_outermost_mount() {
    let content =
        "/dev/sda2 /home ext4 rw 0 0\n/dev/sda2 / ext4 rw 0 0\n/dev/sda2 /var/lib ext4 rw 0 0\n";
    let mounts = parse_mounts(content);
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].mount, "/", "root always represents its device");
}

/// `/proc/mounts` escapes space, tab, newline and backslash as octal.
#[test]
fn an_octal_escaped_mount_point_is_decoded() {
    let mounts = parse_mounts("/dev/sdb1 /mnt/My\\040Disk ext4 rw 0 0\n");
    assert_eq!(mounts[0].mount, "/mnt/My Disk");
}

#[test]
fn mounts_survives_junk() {
    assert!(parse_mounts("").is_empty());
    assert!(parse_mounts("garbage\n").is_empty());
}

// ------------------------------------------------------------ the battery --

#[test]
fn a_discharging_battery_reads_negative_watts() {
    let b = parse_power_supply(BATTERY).expect("a battery");
    assert!(b.present);
    assert_eq!(b.percent, 87.0);
    assert!(!b.charging);
    assert!(!b.on_ac_power);
    assert_eq!(b.watts, Some(-12.4));
    assert_eq!(b.cycle_count, Some(203));
    // 48230000 / 53000000 = 91%
    assert_eq!(b.health_percent, Some(91));
}

/// "Not charging" is the Linux spelling of macOS's "AC attached": plugged in
/// and deliberately holding the level, which is neither charging nor draining.
/// Only "Discharging" means running on the battery.
#[test]
fn not_charging_is_on_ac_and_not_charging() {
    let text = BATTERY.replace("STATUS=Discharging", "STATUS=Not charging");
    let b = parse_power_supply(&text).expect("a battery");
    assert!(!b.charging);
    assert!(b.on_ac_power);

    let text = BATTERY.replace("STATUS=Discharging", "STATUS=Charging");
    let b = parse_power_supply(&text).expect("a battery");
    assert!(b.charging);
    assert!(b.on_ac_power);
    assert!(b.watts.expect("watts") > 0.0, "charging draws positive");
}

/// Older drivers report charge in µAh and current in µA rather than power in
/// µW, and need the voltage to become watts.
#[test]
fn a_current_only_driver_still_yields_watts() {
    let text = BATTERY.replace(
        "POWER_SUPPLY_POWER_NOW=12400000",
        "POWER_SUPPLY_CURRENT_NOW=1048000",
    );
    let b = parse_power_supply(&text).expect("a battery");
    let w = b.watts.expect("watts");
    // 1.048 A × 11.823 V ≈ 12.39 W, discharging.
    assert!((w + 12.39).abs() < 0.05, "{w}");
}

/// A desktop has no battery, and the AC adapter is not one either — reading it
/// as a battery at 0% would draw an empty gauge on a machine with mains power.
#[test]
fn a_mains_adapter_is_not_a_battery() {
    let adapter = "POWER_SUPPLY_NAME=AC\nPOWER_SUPPLY_TYPE=Mains\nPOWER_SUPPLY_ONLINE=1\n";
    assert!(parse_power_supply(adapter).is_none());
    assert!(parse_power_supply("").is_none());
}

/// I-14 on this platform is its own list, reviewed rather than translated.
#[test]
fn i14_the_protected_list_is_linux_specific_and_not_a_translation() {
    for name in ["systemd", "Xorg", "gnome-shell", "dbus-daemon"] {
        assert!(
            LINUX_PROTECTED_NAMES.contains(&name),
            "{name} should be protected"
        );
    }
    for name in ["WindowServer", "launchd", "Finder"] {
        assert!(
            !LINUX_PROTECTED_NAMES.contains(&name),
            "{name} is a macOS process"
        );
    }
}
