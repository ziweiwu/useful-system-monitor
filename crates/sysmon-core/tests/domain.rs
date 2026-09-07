//! The domain rules, ported from `test/deltas.test.ts`, `ring.test.ts`,
//! `scoring.test.ts`, `workingSet.test.ts` and `views.test.ts`.
//!
//! `fast-check`'s role is played by `proptest`; the properties themselves are
//! language-agnostic and are the point.

use proptest::prelude::*;
use sysmon_core::domain::deltas::{core_utilisation, CpuDeltaTracker, CpuTimes};
use sysmon_core::domain::ring::Ring;
use sysmon_core::domain::scoring::{compare, estimate_watts, sort_processes, SortKey};
use sysmon_core::domain::types::{ProcessSample, RawProcess, StartTime};
use sysmon_core::domain::views::{View, VIEW_ORDER};
use sysmon_core::domain::working_set::{select_working_set, WorkingSetCap, WORKING_SET_STEPS};

fn proc(pid: i32, cpu_time_ms: u64) -> RawProcess {
    RawProcess {
        pid,
        ppid: 1,
        cpu_time_ms,
        rss_bytes: 1024,
    }
}

fn sample(
    pid: i32,
    cpu_percent: Option<f64>,
    rss_bytes: u64,
    energy: Option<f64>,
) -> ProcessSample {
    ProcessSample {
        pid,
        ppid: 1,
        start_time: StartTime::Known(1),
        command: format!("/bin/proc{pid}"),
        user: "someone".into(),
        state: "S".into(),
        cpu_percent,
        rss_bytes,
        energy,
        protected: false,
    }
}

// ------------------------------------------------- I-10: history is bounded --

#[test]
fn i10_ring_never_grows_beyond_capacity() {
    let mut r = Ring::new(5);
    for i in 0..1000 {
        r.push(i as f64);
    }
    assert_eq!(r.len(), 5);
    assert_eq!(r.to_vec().len(), 5);
}

#[test]
fn i10_ring_keeps_the_most_recent_values_oldest_first() {
    let mut r = Ring::new(3);
    for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
        r.push(v);
    }
    assert_eq!(r.to_vec(), vec![3.0, 4.0, 5.0]);
    assert_eq!(r.last(), Some(5.0));
}

#[test]
fn i10_ring_reports_empty_state_without_inventing_values() {
    let r = Ring::new(3);
    assert_eq!(r.len(), 0);
    assert!(r.to_vec().is_empty());
    assert_eq!(r.last(), None);
}

#[test]
#[should_panic(expected = "capacity must be > 0")]
fn i10_ring_rejects_a_non_positive_capacity() {
    let _ = Ring::new(0);
}

proptest! {
    /// The property the memory bound rests on, over the whole space rather than
    /// the one capacity the TypeScript suite sampled.
    #[test]
    fn i10_ring_length_is_min_of_pushes_and_capacity(cap in 1usize..64, pushes in 0usize..500) {
        let mut r = Ring::new(cap);
        for i in 0..pushes {
            r.push(i as f64);
        }
        prop_assert_eq!(r.len(), pushes.min(cap));
        prop_assert_eq!(r.to_vec().len(), pushes.min(cap));
    }
}

// ------------------------------------------- I-1: CPU% is always a delta --

/// A process with 5s of accumulated CPU must not read as 0% or as its lifetime
/// average; we simply do not know its rate yet.
#[test]
fn i1_returns_nothing_on_first_observation_rather_than_zero() {
    let mut t = CpuDeltaTracker::new(1000.0);
    assert_eq!(t.update(&[proc(100, 5_000)], 1_000).get(&100), Some(&None));
}

#[test]
fn i1_computes_the_rate_from_the_second_sample_onward() {
    let mut t = CpuDeltaTracker::new(1000.0);
    t.update(&[proc(100, 1_000)], 0);
    // 500ms of CPU over 1000ms of wall clock = 50%.
    let got = t.update(&[proc(100, 1_500)], 1_000)[&100].expect("a rate");
    assert!((got - 50.0).abs() < 1e-9, "{got}");
}

/// Zero and unknown are different answers, and the column renders them
/// differently.
#[test]
fn i1_reports_zero_for_a_live_but_idle_process() {
    let mut t = CpuDeltaTracker::new(1000.0);
    t.update(&[proc(100, 1_000)], 0);
    assert_eq!(t.update(&[proc(100, 1_000)], 1_000)[&100], Some(0.0));
}

// ------------------------------ I-3: monotonicity and PID reuse --

/// PID 100 was recycled: the new process has far less accumulated CPU, and
/// diffing would yield a large negative rate.
#[test]
fn i3_discards_the_delta_when_cumulative_cpu_time_goes_backwards() {
    let mut t = CpuDeltaTracker::new(1000.0);
    t.update(&[proc(100, 60_000)], 0);
    assert_eq!(t.update(&[proc(100, 5)], 1_000)[&100], None);
}

/// The signal anything else caching per-PID has to hear about — a name cache
/// keyed only on "do I have this pid" would keep showing the dead process.
#[test]
fn i3_flags_the_recycled_pid_so_other_caches_can_evict_it() {
    let mut t = CpuDeltaTracker::new(1000.0);
    t.update(&[proc(100, 60_000)], 0);
    t.update(&[proc(100, 5)], 1_000);
    assert!(t.recycled().contains(&100));
    // And clears it once the next sample is clean.
    t.update(&[proc(100, 505)], 2_000);
    assert!(t.recycled().is_empty());
}

#[test]
fn i3_recovers_on_the_sample_after_a_reuse_event() {
    let mut t = CpuDeltaTracker::new(1000.0);
    t.update(&[proc(100, 60_000)], 0);
    t.update(&[proc(100, 5)], 1_000);
    let got = t.update(&[proc(100, 505)], 2_000)[&100].expect("a rate");
    assert!((got - 50.0).abs() < 1e-9, "{got}");
}

#[test]
fn i10_drops_exited_processes_so_the_map_cannot_grow_without_bound() {
    let mut t = CpuDeltaTracker::new(1000.0);
    t.update(&[proc(1, 0), proc(2, 0), proc(3, 0)], 0);
    assert_eq!(t.tracked_count(), 3);
    t.update(&[proc(1, 0)], 1_000);
    assert_eq!(t.tracked_count(), 1);
}

/// A clock that steps backwards between samples must not produce a rate.
#[test]
fn i2_a_non_positive_window_yields_nothing_rather_than_a_wild_rate() {
    let mut t = CpuDeltaTracker::new(1000.0);
    t.update(&[proc(1, 0)], 5_000);
    assert_eq!(t.update(&[proc(1, 1_000)], 4_000)[&1], None);
    let mut t2 = CpuDeltaTracker::new(1000.0);
    t2.update(&[proc(1, 0)], 1_000);
    assert_eq!(t2.update(&[proc(1, 1_000)], 1_000)[&1], None);
}

proptest! {
    /// I-2: every sampled pid is present in the result, and any rate it carries
    /// is inside the physical ceiling — however far apart the two counter
    /// readings are, and however short the window between them.
    #[test]
    fn i2_cpu_percent_stays_inside_its_physical_bounds(
        first in 0u64..10_000_000,
        second in 0u64..10_000_000,
        window_ms in 1i64..60_000,
    ) {
        let mut t = CpuDeltaTracker::new(1000.0);
        t.update(&[proc(1, first)], 0);
        let out = t.update(&[proc(1, second)], window_ms);
        let v = out.get(&1).expect("every sampled pid must be present");
        match v {
            None => prop_assert!(
                second < first,
                "only a backwards counter may yield nothing"
            ),
            Some(v) => prop_assert!(*v >= 0.0 && *v <= 1000.0, "{v}"),
        }
    }
}

// -------------------------------------- I-2 / I-6: per-core utilisation --

fn times(user: f64, sys: f64, idle: f64) -> CpuTimes {
    CpuTimes {
        user,
        nice: 0.0,
        sys,
        idle,
        irq: 0.0,
    }
}

#[test]
fn i6_derives_utilisation_from_idle_share() {
    let got = core_utilisation(&[times(0.0, 0.0, 0.0)], &[times(30.0, 20.0, 50.0)])[0];
    assert!((got - 50.0).abs() < 1e-9, "{got}");
}

#[test]
fn i2_reports_zero_rather_than_nan_when_no_time_has_passed() {
    let same = [times(10.0, 10.0, 10.0)];
    assert_eq!(core_utilisation(&same, &same)[0], 0.0);
}

/// A shorter `prev` than `cur` — a core appearing mid-run — must not panic.
#[test]
fn i2_a_missing_previous_core_reads_as_zero_not_a_crash() {
    assert_eq!(core_utilisation(&[], &[times(1.0, 1.0, 1.0)]), vec![0.0]);
}

proptest! {
    #[test]
    fn i2_core_utilisation_clamps_into_0_100(
        before in prop::array::uniform3(0f64..100_000.0),
        after in prop::array::uniform3(0f64..100_000.0),
    ) {
        let prev = [times(before[0], before[1], before[2])];
        let cur = [times(after[0], after[1], after[2])];
        let v = core_utilisation(&prev, &cur)[0];
        prop_assert!((0.0..=100.0).contains(&v), "{v}");
    }
}

// --------------------------------------- I-20: sort is stable and total --

#[test]
fn i20_breaks_ties_by_pid_so_equal_rows_cannot_swap_between_frames() {
    let procs = vec![
        sample(30, Some(10.0), 0, None),
        sample(10, Some(10.0), 0, None),
        sample(20, Some(10.0), 0, None),
    ];
    let once: Vec<i32> = sort_processes(&procs, SortKey::Cpu)
        .iter()
        .map(|p| p.pid)
        .collect();
    let mut reversed = procs.clone();
    reversed.reverse();
    let twice: Vec<i32> = sort_processes(&reversed, SortKey::Cpu)
        .iter()
        .map(|p| p.pid)
        .collect();
    assert_eq!(once, vec![10, 20, 30]);
    assert_eq!(twice, once);
}

#[test]
fn i20_orders_descending_by_the_chosen_key() {
    let procs = vec![
        sample(1, Some(5.0), 900, None),
        sample(2, Some(50.0), 100, None),
    ];
    let by_cpu: Vec<i32> = sort_processes(&procs, SortKey::Cpu)
        .iter()
        .map(|p| p.pid)
        .collect();
    let by_mem: Vec<i32> = sort_processes(&procs, SortKey::Mem)
        .iter()
        .map(|p| p.pid)
        .collect();
    assert_eq!(by_cpu, vec![2, 1]);
    assert_eq!(by_mem, vec![1, 2]);
}

/// I-1 again, at the sort: a process whose CPU% is not yet known must not
/// outrank one measured at 0%.
#[test]
fn i20_sorts_unknown_cpu_below_every_known_value() {
    let procs = vec![sample(1, None, 0, None), sample(2, Some(0.0), 0, None)];
    let order: Vec<i32> = sort_processes(&procs, SortKey::Cpu)
        .iter()
        .map(|p| p.pid)
        .collect();
    assert_eq!(order, vec![2, 1]);
}

#[test]
fn i20_is_a_total_order_equal_only_for_identical_pids() {
    let a = sample(1, Some(7.0), 0, None);
    let b = sample(2, Some(7.0), 0, None);
    assert_ne!(compare(&a, &b, SortKey::Cpu), std::cmp::Ordering::Equal);
    assert_eq!(compare(&a, &a, SortKey::Cpu), std::cmp::Ordering::Equal);
}

proptest! {
    /// Sorting the same multiset in any order must give the same sequence —
    /// the property that stops the table jittering.
    #[test]
    fn i20_sort_is_order_independent(mut pids in prop::collection::vec(1i32..200, 1..40)) {
        pids.sort_unstable();
        pids.dedup();
        let procs: Vec<ProcessSample> =
            pids.iter().map(|p| sample(*p, Some((p % 7) as f64), (p % 5) as u64, None)).collect();
        let forward: Vec<i32> =
            sort_processes(&procs, SortKey::Cpu).iter().map(|p| p.pid).collect();
        let mut shuffled = procs.clone();
        shuffled.reverse();
        let backward: Vec<i32> =
            sort_processes(&shuffled, SortKey::Cpu).iter().map(|p| p.pid).collect();
        prop_assert_eq!(forward, backward);
    }
}

#[test]
fn watts_split_total_draw_in_proportion_to_energy_share() {
    let got = estimate_watts(Some(25.0), 100.0, Some(-20.0)).expect("watts");
    assert!((got - 5.0).abs() < 1e-9, "{got}");
}

#[test]
fn watts_return_nothing_rather_than_guessing_when_inputs_are_missing() {
    assert_eq!(estimate_watts(None, 100.0, Some(-20.0)), None);
    assert_eq!(estimate_watts(Some(25.0), 100.0, None), None);
    assert_eq!(estimate_watts(Some(25.0), 0.0, Some(-20.0)), None);
}

// ------------------------------------------ C-7 / C-9: the working set --

fn many() -> Vec<ProcessSample> {
    (0..400)
        .map(|i| {
            sample(
                i + 1,
                Some(i as f64),
                ((400 - i) as u64) * 1024,
                Some(i as f64),
            )
        })
        .collect()
}

#[test]
fn c7_keeps_the_union_within_the_budget() {
    let ws = select_working_set(&many(), WorkingSetCap::Top(50));
    assert!(ws.visible.len() <= 50, "{} rows", ws.visible.len());
}

/// pid 1 is the largest by RSS and the smallest by CPU. A CPU-only cut would
/// hide it, which is exactly the leak you want to find.
#[test]
fn c7_keeps_memory_hogs_that_use_no_cpu() {
    let ws = select_working_set(&many(), WorkingSetCap::Top(50));
    assert!(ws.visible.iter().any(|p| p.pid == 1));
    assert!(ws.visible.iter().any(|p| p.pid == 400));
}

#[test]
fn c9_rolls_the_tail_up_so_totals_still_reconcile() {
    let all = many();
    let ws = select_working_set(&all, WorkingSetCap::Top(50));
    let visible_cpu: f64 = ws.visible.iter().filter_map(|p| p.cpu_percent).sum();
    let expected: f64 = all.iter().filter_map(|p| p.cpu_percent).sum();
    assert!((visible_cpu + ws.others.cpu_percent - expected).abs() < 1e-6);
    assert_eq!(ws.visible.len() + ws.others.count, all.len());
}

#[test]
fn c7_passes_everything_through_when_under_the_limit() {
    let few: Vec<ProcessSample> = many().into_iter().take(10).collect();
    let ws = select_working_set(&few, WorkingSetCap::Top(50));
    assert_eq!(ws.visible.len(), 10);
    assert_eq!(ws.others.count, 0);
}

#[test]
fn c7_all_materialises_every_process_and_rolls_up_nothing() {
    let all = many();
    let ws = select_working_set(&all, WorkingSetCap::All);
    assert_eq!(ws.visible.len(), all.len());
    assert_eq!(ws.others.count, 0);
}

/// I-9c: only the unbounded step admits to costing anything.
#[test]
fn i9c_only_the_all_step_carries_a_cost_note() {
    assert_eq!(WORKING_SET_STEPS[0].label(), "50");
    assert_eq!(WORKING_SET_STEPS[0].cost_note(), "");
    assert_eq!(WorkingSetCap::All.label(), "all");
    assert!(WorkingSetCap::All.cost_note().contains("cpu"));
}

proptest! {
    /// C-9 as a property: the table plus the roll-up is the whole machine, at
    /// every cap and every population size.
    #[test]
    fn c9_every_process_is_visible_or_rolled_up_exactly_once(
        n in 0usize..300,
        cap in 1usize..120,
    ) {
        let pids = 0..n as i32;
        let all: Vec<ProcessSample> = pids
            .map(|i| sample(i + 1, Some((i % 13) as f64), (i % 17) as u64 * 1024, None))
            .collect();
        let ws = select_working_set(&all, WorkingSetCap::Top(cap));
        prop_assert_eq!(ws.visible.len() + ws.others.count, all.len());
        prop_assert!(ws.visible.len() <= all.len().max(cap));
    }
}

// ------------------------------------------- I-27: view navigation --

#[test]
fn i27_steps_forward_and_backward_through_the_strip() {
    assert_eq!(View::Overview.step(1), View::Cpu);
    assert_eq!(View::Cpu.step(1), View::Memory);
    assert_eq!(View::Memory.step(-1), View::Cpu);
}

/// A tab strip whose arrow key silently does nothing at the last tab reads as a
/// broken key, not as a boundary.
#[test]
fn i27_wraps_at_both_ends_so_neither_arrow_is_ever_a_dead_key() {
    assert_eq!(View::Disk.step(1), View::Overview);
    assert_eq!(View::Overview.step(-1), View::Disk);
}

#[test]
fn i27_returns_to_the_same_view_after_a_full_lap_either_way() {
    for v in VIEW_ORDER {
        let mut f = v;
        let mut b = v;
        for _ in 0..VIEW_ORDER.len() {
            f = f.step(1);
            b = b.step(-1);
        }
        assert_eq!(f, v);
        assert_eq!(b, v);
    }
}

/// The tab strip prints `key()`; the keymap reads `from_key`. If these
/// disagreed, the label on screen would open a different screen.
#[test]
fn i27_keeps_the_number_keys_and_the_strip_order_in_sync() {
    for v in VIEW_ORDER {
        assert_eq!(View::from_key(v.key()), Some(v));
    }
    let keys: Vec<char> = VIEW_ORDER.iter().map(|v| v.key()).collect();
    assert_eq!(keys, vec!['1', '2', '3', '4', '5']);
    assert_eq!(View::from_key('0'), None);
    assert_eq!(View::from_key('6'), None);
    assert_eq!(View::from_key('x'), None);
}

#[test]
fn i27_labels_every_view() {
    for v in VIEW_ORDER {
        assert!(!v.label().is_empty());
        assert!(
            v.label().chars().all(|c| c.is_ascii_uppercase()),
            "{}",
            v.label()
        );
    }
}
