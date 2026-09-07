//! The parser contract, ported from `test/parse.test.ts`.
//!
//! These read the **same fixture files** as the TypeScript suite — real command
//! output captured from a real Mac, including the three locale variants. They
//! are `include_str!`ed rather than read at runtime so deleting a fixture breaks
//! the build instead of silently skipping a case.

use sysmon_core::domain::types::StartTime;
use sysmon_core::parse::battery::{parse_ioreg_battery, parse_pmset, parse_signed_int64};
use sysmon_core::parse::df::{parse_df, parse_df_all};
use sysmon_core::parse::ps::{parse_cpu_time_ms, parse_lstart, parse_ps_hot, parse_ps_static};
use sysmon_core::parse::top_power::parse_top_power;
use sysmon_core::parse::vm_stat::parse_memory;

const PS_HOT: &str = include_str!("../../../test/fixtures/ps-hot.txt");
const PS_STATIC: &str = include_str!("../../../test/fixtures/ps-static.txt");
const PS_STATIC_DE: &str = include_str!("../../../test/fixtures/ps-static-de_DE.txt");
const PS_STATIC_EN_GB: &str = include_str!("../../../test/fixtures/ps-static-en_GB.txt");
const PS_STATIC_ZH: &str = include_str!("../../../test/fixtures/ps-static-zh_CN.txt");
const VM_STAT: &str = include_str!("../../../test/fixtures/vm_stat.txt");
const SWAPUSAGE: &str = include_str!("../../../test/fixtures/swapusage.txt");
const SWAPUSAGE_DE: &str = include_str!("../../../test/fixtures/swapusage-de_DE.txt");
const DF: &str = include_str!("../../../test/fixtures/df.txt");
const PMSET_DISCHARGING: &str = include_str!("../../../test/fixtures/pmset-discharging.txt");
const PMSET_CHARGING: &str = include_str!("../../../test/fixtures/pmset-charging.txt");
const PMSET_CHARGED: &str = include_str!("../../../test/fixtures/pmset-charged.txt");
const PMSET_AC: &str = include_str!("../../../test/fixtures/pmset-ac-attached.txt");
const IOREG: &str = include_str!("../../../test/fixtures/ioreg-battery.txt");
const TOP_POWER: &str = include_str!("../../../test/fixtures/top-power.txt");

const GIB: u64 = 1024 * 1024 * 1024;
const MIB: f64 = 1024.0 * 1024.0;

// ---------------------------------------------------------------- cpu time --

#[test]
fn reads_centisecond_resolution_the_field_the_design_depends_on() {
    assert_eq!(parse_cpu_time_ms("2:36.10"), Some(156_100));
    assert_eq!(parse_cpu_time_ms("0:00.01"), Some(10));
}

/// Observed on a real machine: `ps` prints `1206:18.96`, not `20:06:18.96`.
#[test]
fn handles_minute_counts_that_never_roll_into_hours() {
    assert_eq!(
        parse_cpu_time_ms("1206:18.96"),
        Some((1206 * 60 + 18) * 1000 + 960)
    );
    assert_eq!(
        parse_cpu_time_ms("383:24.67"),
        Some((383 * 60 + 24) * 1000 + 670)
    );
}

#[test]
fn accepts_an_hours_group_defensively() {
    assert_eq!(
        parse_cpu_time_ms("1:02:03.50"),
        Some((62 * 60 + 3) * 1000 + 500)
    );
}

#[test]
fn rejects_junk_rather_than_returning_a_wrong_number() {
    assert_eq!(parse_cpu_time_ms("-"), None);
    assert_eq!(parse_cpu_time_ms(""), None);
}

/// A single-digit hundredths field is left-aligned: `.1` is 100 ms, not 10.
#[test]
fn a_single_digit_fraction_is_tenths_not_hundredths() {
    assert_eq!(parse_cpu_time_ms("0:00.1"), Some(100));
}

// ------------------------------------------------------------------ ps hot --

#[test]
fn ps_hot_parses_every_data_row() {
    let procs = parse_ps_hot(PS_HOT);
    assert!(procs.len() > 700, "only {} rows", procs.len());
}

#[test]
fn ps_hot_converts_rss_from_kilobytes_to_bytes() {
    assert!(parse_ps_hot(PS_HOT).iter().all(|p| p.rss_bytes % 1024 == 0));
}

/// kernel_task (PID 0) is invisible to `ps`, which is why system CPU can never
/// be a sum of process rows. See I-6.
#[test]
fn ps_hot_includes_launchd_but_no_pid_zero_row() {
    let procs = parse_ps_hot(PS_HOT);
    assert!(procs.iter().any(|p| p.pid == 1));
    assert!(!procs.iter().any(|p| p.pid == 0));
}

// --------------------------------------------------------------- ps static --

fn data_line_count(fixture: &str) -> usize {
    fixture.lines().filter(|l| !l.trim().is_empty()).count() - 1
}

#[test]
fn ps_static_parses_every_row_without_dropping_any() {
    let metas = parse_ps_static(PS_STATIC);
    assert!(metas.len() > 700, "only {} rows", metas.len());
    assert_eq!(metas.len(), data_line_count(PS_STATIC));
}

/// The exact class of path that broke name extraction earlier.
#[test]
fn ps_static_keeps_command_paths_that_contain_spaces_intact() {
    let metas = parse_ps_static(PS_STATIC);
    if let Some(chrome) = metas.iter().find(|m| m.command.contains("Google Chrome")) {
        assert!(chrome.command.starts_with('/'));
        assert!(chrome.command.contains("Google Chrome"));
    }
    assert!(metas.iter().any(|m| m.command.contains(' ')));
}

/// `ps -o comm` is not always a path: macOS reports audio plugins as
/// "Core Audio Driver (Foo.driver)". Truncating at the first space would
/// collapse all of these into an indistinguishable "Core".
#[test]
fn ps_static_preserves_comm_values_that_are_not_paths() {
    for d in parse_ps_static(PS_STATIC)
        .iter()
        .filter(|m| m.command.starts_with("Core Audio Driver"))
    {
        assert!(d.command.contains('('), "{}", d.command);
    }
}

#[test]
fn ps_static_never_leaves_user_or_state_empty() {
    for m in parse_ps_static(PS_STATIC) {
        assert!(!m.user.is_empty());
        assert!(!m.state.is_empty());
    }
}

#[test]
fn ps_static_parses_lstart_into_a_plausible_timestamp() {
    let metas = parse_ps_static(PS_STATIC);
    let launchd = metas
        .iter()
        .find(|m| m.pid == 1)
        .expect("launchd should be present");
    let ms = launchd
        .start_time
        .epoch_ms()
        .expect("the C locale should parse");
    // 2020-01-01T00:00:00Z, the same floor the TypeScript suite uses.
    assert!(ms > 1_577_836_800_000, "{ms} is before 2020");
}

// -------------------------------------------------- I-28: locale behaviour --

/// The collector pins `LC_TIME=C`, so these should be unreachable. The parser
/// recovering the *name* from them is what stops a single unrecognised date
/// format from turning the whole table into "pid 1234" rows again.
#[test]
fn i28_still_names_every_process_under_a_foreign_locale() {
    for (locale, raw) in [
        ("de_DE", PS_STATIC_DE),
        ("zh_CN", PS_STATIC_ZH),
        ("en_GB", PS_STATIC_EN_GB),
    ] {
        let metas = parse_ps_static(raw);
        assert_eq!(metas.len(), data_line_count(raw), "{locale}: dropped rows");
        assert_eq!(
            metas
                .iter()
                .find(|m| m.pid == 1)
                .map(|m| m.command.as_str()),
            Some("/sbin/launchd"),
            "{locale}"
        );
        for m in &metas {
            assert!(!m.command.is_empty(), "{locale}");
            assert_eq!(m.user, "root", "{locale}");
            assert_eq!(m.state, "Ss", "{locale}");
        }
    }
}

#[test]
fn i28_keeps_a_spaced_command_path_intact_under_a_foreign_locale() {
    let line = "  PID STARTED USER STAT COMM\n  501 Mi. 12 Aug. 19:39:58 2026    ziweiwu          S    /Applications/Google Chrome.app/Contents/MacOS/Google Chrome\n";
    assert_eq!(
        parse_ps_static(line)[0].command,
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    );
}

/// I-16 binds a kill to the start time. A date with no Latin month cannot be
/// read back to an instant, and a plausible-looking guess would be worse than
/// admitting ignorance.
#[test]
fn i28_reports_an_unreadable_start_time_as_unknown_rather_than_inventing_one() {
    assert_eq!(
        parse_ps_static(PS_STATIC_ZH)[0].start_time,
        StartTime::Unreadable
    );
    assert!(parse_ps_static(PS_STATIC)[0]
        .start_time
        .epoch_ms()
        .is_some());
}

/// Measured against V8's `Date.parse`, which is what the TypeScript build uses:
/// it accepts the C, en_GB and de_DE forms and rejects only zh_CN. A strict
/// C-locale parser would have diverged on two of the three fixtures.
#[test]
fn lstart_matches_v8_on_every_captured_locale_form() {
    for form in [
        "Sat Aug  1 17:46:44 2026",
        "Wed Aug 12 19:39:58 2026",
        "Wed 12 Aug 19:39:58 2026",
        "Mi. 12 Aug. 19:39:58 2026",
    ] {
        assert!(
            parse_lstart(form).epoch_ms().is_some(),
            "{form} should parse"
        );
    }
    assert_eq!(
        parse_lstart("三  8月/12 19:39:58 2026"),
        StartTime::Unreadable
    );
}

/// Two unreadable start times must never compare equal — that is the whole
/// reason this is an enum and not a `0` sentinel. See I-16.
#[test]
fn i16_unknown_start_times_never_match_each_other() {
    assert!(!StartTime::Unreadable.same_process_as(StartTime::Unreadable));
    assert!(!StartTime::Known(5).same_process_as(StartTime::Unreadable));
    assert!(StartTime::Known(5).same_process_as(StartTime::Known(5)));
    assert!(!StartTime::Known(5).same_process_as(StartTime::Known(6)));
}

// ------------------------------------------------------ I-5: memory splits --

#[test]
fn i5_splits_total_into_used_and_available_exactly() {
    let mem = parse_memory(VM_STAT, SWAPUSAGE, 16 * GIB);
    assert_eq!(mem.used_bytes + mem.available_bytes, mem.total_bytes);
}

/// "free" is true `vm_stat` free and near zero on a healthy Mac; the gauge must
/// measure against "available", which adds reclaimable inactive pages.
#[test]
fn i5_keeps_free_and_available_distinct() {
    let mem = parse_memory(VM_STAT, SWAPUSAGE, 16 * GIB);
    assert!(mem.free_bytes < mem.available_bytes);
    assert!(mem.available_bytes >= mem.inactive_bytes);
}

/// The regression this pins: `free` was briefly redefined as `available`, which
/// already contains `inactive`. The breakdown listed both, so the bars summed
/// past 100% of physical RAM.
#[test]
fn i5_breakdown_rows_partition_memory_without_overlapping() {
    let mem = parse_memory(VM_STAT, SWAPUSAGE, 16 * GIB);
    let partition = mem.wired_bytes
        + mem.active_bytes
        + mem.inactive_bytes
        + mem.compressed_bytes
        + mem.free_bytes;
    let total = mem.total_bytes as f64;
    assert!(partition as f64 <= total * 1.02);
    assert!(partition as f64 > total * 0.9);
}

/// A 4K assumption would under-report memory by 4x on Apple Silicon.
#[test]
fn i5_reads_the_16k_page_size_from_the_header_rather_than_assuming_4k() {
    assert!(parse_memory(VM_STAT, SWAPUSAGE, 16 * GIB).wired_bytes > GIB);
}

/// top-style accounting (total - free) reads 99.5% on a healthy Mac, which is
/// technically true and useless as a gauge.
#[test]
fn i5_excludes_reclaimable_inactive_pages_from_used() {
    let mem = parse_memory(VM_STAT, SWAPUSAGE, 16 * GIB);
    assert_eq!(
        mem.used_bytes,
        mem.wired_bytes + mem.active_bytes + mem.compressed_bytes
    );
    let pct = (mem.used_bytes as f64 / mem.total_bytes as f64) * 100.0;
    assert!(
        pct > 10.0 && pct < 97.0,
        "{pct}% is not a plausible reading"
    );
}

#[test]
fn i5_parses_swap_totals() {
    let mem = parse_memory(VM_STAT, SWAPUSAGE, 16 * GIB);
    assert!(mem.swap_total_bytes > 0);
    assert!(mem.swap_used_bytes > 0);
    assert!(mem.swap_used_bytes <= mem.swap_total_bytes);
}

/// A comma-decimal locale prints the swap total with a comma. The old pattern
/// stopped at it and reported a machine with 1 GB of swap as having none.
/// See I-28.
#[test]
fn i28_reads_swap_written_with_a_comma_decimal_separator() {
    let mem = parse_memory(VM_STAT, SWAPUSAGE_DE, 16 * GIB);
    assert_eq!(mem.swap_total_bytes, 1024 * 1024 * 1024);
    assert!((mem.swap_used_bytes as f64 - 80.31 * MIB).abs() < 1.0);
}

// -------------------------------------------------------------------- disk --

#[test]
fn df_finds_the_root_filesystem() {
    let disk = parse_df(DF, "/").expect("root should be present");
    assert!(disk.total_bytes > 100 * GIB);
}

#[test]
fn df_returns_none_for_a_mount_that_is_not_present() {
    assert!(parse_df(DF, "/nope").is_none());
}

/// df's Used column for `/` is the read-only system snapshot (~12 G). Using it
/// would show a 926 G disk as ~1% full.
#[test]
fn df_reports_apfs_container_usage_not_the_sealed_snapshot() {
    let root = parse_df(DF, "/").expect("root should be present");
    let pct = (root.used_bytes as f64 / root.total_bytes as f64) * 100.0;
    assert!(pct > 10.0, "{pct}% suggests the sealed snapshot was read");
    assert_eq!(root.used_bytes, root.total_bytes - root.free_bytes);
}

#[test]
fn df_agrees_with_the_data_volume_which_shares_the_container() {
    let root = parse_df(DF, "/").expect("root should be present");
    if let Some(data) = parse_df(DF, "/System/Volumes/Data") {
        assert_eq!(data.free_bytes, root.free_bytes);
        assert_eq!(data.used_bytes, root.used_bytes);
    }
}

/// The fixture has several `/dev/disk3*` rows all sharing 926 G. Listing them
/// would imply ~5.5 TB of storage.
#[test]
fn df_all_reports_one_apfs_container_once_not_once_per_sibling_mount() {
    let mounts: Vec<String> = parse_df_all(DF).into_iter().map(|v| v.mount).collect();
    assert_eq!(mounts.iter().filter(|m| *m == "/").count(), 1);
    assert!(!mounts.iter().any(|m| m == "/System/Volumes/Data"));
    assert!(!mounts.iter().any(|m| m == "/System/Volumes/VM"));
}

#[test]
fn df_all_keeps_the_root_volume_and_every_user_visible_mount() {
    let mounts: Vec<String> = parse_df_all(DF).into_iter().map(|v| v.mount).collect();
    assert_eq!(mounts, vec!["/", "/Volumes/data", "/Volumes/docker"]);
}

#[test]
fn df_all_drops_pseudo_and_firmware_filesystems() {
    let mounts: Vec<String> = parse_df_all(DF).into_iter().map(|v| v.mount).collect();
    assert!(!mounts.iter().any(|m| m == "/dev")); // devfs
    assert!(!mounts.iter().any(|m| m == "/System/Volumes/Data/home")); // map auto_home
    assert!(!mounts.iter().any(|m| m.starts_with("/System/Volumes/")));
}

#[test]
fn df_all_flags_network_shares() {
    let vols = parse_df_all(DF);
    assert!(
        vols.iter()
            .find(|v| v.mount == "/Volumes/docker")
            .expect("docker")
            .network
    );
    assert!(!vols.iter().find(|v| v.mount == "/").expect("root").network);
}

#[test]
fn df_all_computes_usage_as_total_minus_available_matching_parse_df() {
    let vols = parse_df_all(DF);
    let root = vols.iter().find(|v| v.mount == "/").expect("root");
    assert_eq!(root.used_bytes, parse_df(DF, "/").expect("root").used_bytes);
    assert_eq!(root.used_bytes, root.total_bytes - root.free_bytes);
}

#[test]
fn df_all_survives_output_with_no_parsable_rows() {
    assert!(parse_df_all("Filesystem 1024-blocks Used Available Capacity\n").is_empty());
    assert!(parse_df_all("").is_empty());
}

// ----------------------------------------------------------------- battery --

#[test]
fn pmset_reads_a_discharging_battery() {
    let b = parse_pmset(PMSET_DISCHARGING).expect("a battery");
    assert_eq!(b.percent, 48.0);
    assert!(!b.charging);
    assert_eq!(b.time_remaining_min, Some(43));
    assert!(b.present);
    assert!(!b.on_ac_power);
}

#[test]
fn pmset_reads_a_charging_battery() {
    let b = parse_pmset(PMSET_CHARGING).expect("a battery");
    assert!(b.charging);
    assert!(b.percent > 0.0);
}

/// Optimised charging holds the level; a lowercase-only pattern misses the
/// capital "AC" and reports "no battery reported by pmset".
#[test]
fn pmset_reads_ac_attached_the_state_that_is_neither() {
    let b = parse_pmset(PMSET_AC).expect("a battery");
    assert!(b.percent > 0.0);
    assert!(!b.charging);
    assert!(b.on_ac_power);
}

#[test]
fn pmset_reads_a_fully_charged_battery() {
    let b = parse_pmset(PMSET_CHARGED).expect("a battery");
    assert_eq!(b.percent, 100.0);
    assert!(!b.charging);
    assert!(b.on_ac_power);
    // "0:00 remaining" means unknown, not "zero minutes left".
    assert_eq!(b.time_remaining_min, None);
}

#[test]
fn pmset_handles_every_state_macos_reports() {
    for st in [
        "discharging",
        "charging",
        "finishing charge",
        "charged",
        "AC attached",
    ] {
        let out = parse_pmset(&format!(
            "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t55%; {st}; present: true\n"
        ));
        assert_eq!(out.expect("{st} should parse").percent, 55.0, "{st}");
    }
}

/// Mac mini, Studio, Pro, iMac and every CI runner report no battery. Treating
/// this as an error would show a broken panel on a machine working perfectly.
#[test]
fn pmset_none_means_desktop_mac_which_is_not_an_error() {
    for out in ["Now drawing from AC Power\n", "", "No adapter attached.\n"] {
        assert!(parse_pmset(out).is_none());
    }
}

/// A discharging battery reports -3452 mA as 18446744073709548164.
#[test]
fn signed_int64_reinterprets_the_wrap_as_a_negative_current() {
    assert_eq!(parse_signed_int64("18446744073709548164"), Some(-3452));
    assert_eq!(parse_signed_int64("2566"), Some(2566));
    assert_eq!(parse_signed_int64("abc"), None);
}

#[test]
fn ioreg_derives_wattage_cycles_health_and_temperature() {
    let b = parse_ioreg_battery(IOREG);
    assert!(b.watts.expect("watts").abs() < 200.0);
    assert!(b.cycle_count.expect("cycles") > 0);
    let health = b.health_percent.expect("health");
    assert!((50..=100).contains(&health), "health {health}");
    let celsius = b.temperature_c.expect("temperature");
    assert!(celsius > 0.0 && celsius < 80.0, "temperature {celsius}");
}

#[test]
fn ioreg_signs_wattage_negative_while_discharging() {
    let discharging = IOREG
        .lines()
        .map(|l| {
            if l.contains("\"InstantAmperage\" =") || l.contains("\"Amperage\" =") {
                let key = if l.contains("InstantAmperage") {
                    "InstantAmperage"
                } else {
                    "Amperage"
                };
                format!("    \"{key}\" = 18446744073709548164")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(parse_ioreg_battery(&discharging).watts.expect("watts") < 0.0);
}

// -------------------------------------------------------------- top -power --

/// `-l 2` prints two blocks and the first is always zeros, because Energy
/// Impact is itself a rate. Reading the first block reports an idle machine.
#[test]
fn top_power_reads_the_second_sample_block_not_the_first() {
    let m = parse_top_power(TOP_POWER);
    assert!(!m.is_empty(), "no rows parsed");
    assert!(
        m.values().any(|&v| v > 0.0),
        "every value is zero — the first block was read"
    );
}

#[test]
fn top_power_survives_output_with_no_header() {
    assert!(parse_top_power("").is_empty());
    assert!(parse_top_power("nothing useful here\n").is_empty());
}
