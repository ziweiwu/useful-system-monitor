//! I-19 / I-26, against the **renderer** rather than the plan.
//!
//! `verify:layout` in the TypeScript build sweeps 1,260 real frames through a
//! terminal and takes minutes. Because the frame here is a pure function of
//! `(data, ui, size)`, the same sweep — over far more sizes, every screen, both
//! full-screen modes, with and without a toast — is an ordinary test that runs
//! in milliseconds.
//!
//! Two properties, and they are the ones every past layout bug broke:
//!
//! > **no frame has more lines than the terminal has rows**
//! > **no line is wider than the terminal is columns**
//!
//! The `RowWriter` debug assertions fire first and name the offending section;
//! these catch anything that gets past them.

use std::collections::HashMap;

use sysmon::render::{self, Size};
use sysmon::runtime::store::AppData;
use sysmon_core::app::state::{Mode, Toast, UiState};
use sysmon_core::domain::types::{
    BatteryData, CpuData, DiskData, HostInfo, MemoryData, OthersRollup, Panel, ProcessSample,
    StartTime, VolumeUsage,
};
use sysmon_core::domain::views::{View, VIEW_ORDER};
use sysmon_core::layout::screens::{MIN_COLUMNS, MIN_ROWS};
use sysmon_core::text::width::display_width;

const NOW: i64 = 1_800_000_000_000;

fn process(pid: i32, name: &str) -> ProcessSample {
    ProcessSample {
        pid,
        ppid: 1,
        start_time: StartTime::Known(NOW - 60_000),
        command: format!("/Applications/{name}.app/Contents/MacOS/{name}"),
        user: "ziweiwu".into(),
        state: "S".into(),
        cpu_percent: Some((pid % 97) as f64),
        rss_bytes: (pid as u64) * 1_048_576,
        energy: Some((pid % 41) as f64),
        protected: pid % 13 == 0,
    }
}

/// Deliberately awkward data: a CJK name, an emoji with the variation selector
/// that once overflowed rows, and a very long path.
fn awkward() -> Vec<ProcessSample> {
    vec![
        ProcessSample {
            command: "/Applications/⚠️ Warning App.app/Contents/MacOS/⚠️ Warning App".into(),
            ..process(4242, "warn")
        },
        ProcessSample {
            command: "/Users/x/日本語/とても長いパス/アプリケーション.app/Contents/MacOS/アプリ"
                .into(),
            ..process(4243, "cjk")
        },
        ProcessSample {
            command: "/bin/malware\rSafari".into(),
            ..process(4244, "spoof")
        },
    ]
}

/// Whether the fixture carries samples, or is the just-launched state where
/// every panel is still empty.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Populated {
    Yes,
    No,
}

fn fixture(populated: Populated) -> AppData {
    let mut app = AppData {
        host: Some(HostInfo {
            model: "Apple M1 Pro".into(),
            cores: 10,
            perf_cores: 8,
            eff_cores: 2,
            total_mem_bytes: 17_179_869_184,
            uptime_sec: 264_000,
        }),
        ..AppData::default()
    };
    if populated == Populated::No {
        return app;
    }
    app.cpu = Panel::Ok {
        data: CpuData {
            per_core: (0..10).map(|i| (i * 11) as f64).collect(),
            system: 42.5,
            user_percent: 30.0,
            sys_percent: 12.5,
            load_avg: [4.1, 4.7, 5.2],
        },
        sampled_at_ms: NOW - 3_000,
    };
    app.memory = Panel::Ok {
        data: MemoryData {
            total_bytes: 17_179_869_184,
            used_bytes: 12_884_901_888,
            free_bytes: 93_732_864,
            available_bytes: 4_294_967_296,
            wired_bytes: 3_716_710_400,
            active_bytes: 3_761_422_336,
            inactive_bytes: 3_750_412_288,
            compressed_bytes: 5_406_769_152,
            swap_total_bytes: 2_147_483_648,
            swap_used_bytes: 1_610_612_736,
        },
        sampled_at_ms: NOW - 3_000,
    };
    app.disk = Panel::Ok {
        data: DiskData {
            mount: "/".into(),
            total_bytes: 994_662_584_320,
            used_bytes: 362_546_425_856,
            free_bytes: 632_116_158_464,
            volumes: vec![
                VolumeUsage {
                    mount: "/".into(),
                    device: "/dev/disk3s5".into(),
                    total_bytes: 994_662_584_320,
                    used_bytes: 362_546_425_856,
                    free_bytes: 632_116_158_464,
                    network: false,
                },
                VolumeUsage {
                    mount: "/Volumes/a-very-long-network-share-name".into(),
                    device: "//user@host/share".into(),
                    total_bytes: 8_000_000_000_000,
                    used_bytes: 7_800_000_000_000,
                    free_bytes: 200_000_000_000,
                    network: true,
                },
            ],
        },
        sampled_at_ms: NOW - 100_000,
    };
    app.battery = Panel::Ok {
        data: BatteryData {
            present: true,
            percent: 87.0,
            charging: false,
            on_ac_power: false,
            time_remaining_min: Some(143),
            watts: Some(-12.4),
            cycle_count: Some(203),
            health_percent: Some(91),
            temperature_c: Some(30.2),
        },
        sampled_at_ms: NOW - 20_000,
    };

    let mut visible: Vec<ProcessSample> = (1..=60)
        .map(|i| process(i * 7, &format!("App{i}")))
        .collect();
    visible.extend(awkward());
    let parents: HashMap<i32, i32> = visible.iter().map(|p| (p.pid, 1)).collect();
    app.processes = Panel::Ok {
        data: sysmon::collect::ProcessesData {
            total: 800,
            visible,
            others: OthersRollup {
                count: 740,
                cpu_percent: 22.5,
                rss_bytes: 9_000_000_000,
                energy: 30.0,
            },
            parents,
            energy_accurate: false,
        },
        sampled_at_ms: NOW - 3_000,
    };
    for r in [
        &mut app.histories.cpu,
        &mut app.histories.memory,
        &mut app.histories.disk,
    ] {
        for i in 0..60 {
            r.push((i * 3 % 100) as f64);
        }
    }
    app.command_line =
        Some("/Applications/App1.app/Contents/MacOS/App1 --a-very-long-flag=1".into());
    app.record_processes();
    app
}

/// Every state the frame can be in, so the sweep covers modes as well as sizes.
fn states(rows: &[ProcessSample]) -> Vec<UiState> {
    let pid = rows.first().map(|p| p.pid).unwrap_or(7);
    let mut out = Vec::new();
    for view in VIEW_ORDER {
        for toast in [None, Some(long_toast())] {
            out.push(with_toast(view, pid, toast));
        }
    }
    for mode in [Mode::Detail(pid), Mode::Kill(pid), Mode::Filter] {
        out.push(UiState {
            mode,
            selected_pid: Some(pid),
            ..UiState::default()
        });
    }
    out.push(UiState {
        mode: Mode::Kill(pid),
        selected_pid: Some(pid),
        armed_kill: true,
        ..UiState::default()
    });
    // A filter that matches nothing, so every screen has to cope with no rows.
    out.push(UiState {
        view: View::Overview,
        filter: "zzzznomatch".into(),
        selected_pid: Some(pid),
        ..UiState::default()
    });
    out
}

/// The notice that costs two rows, long enough to need truncating.
fn long_toast() -> Toast {
    Toast {
        text: "asked App1 to close — a message long enough to need truncating".into(),
        bad: false,
        expires_at_ms: NOW + 4_000,
    }
}

/// One view, with or without the notice.
fn with_toast(view: View, pid: i32, toast: Option<Toast>) -> UiState {
    UiState {
        view,
        selected_pid: Some(pid),
        toast,
        ..UiState::default()
    }
}

fn check(app: &AppData, ui: &UiState, size: Size, label: &str) {
    let frame = render::build(app, ui, size, NOW);
    assert!(
        frame.lines.len() <= size.rows,
        "{label} {}x{}: {} lines in {} rows",
        size.columns,
        size.rows,
        frame.lines.len(),
        size.rows
    );
    for (i, line) in frame.lines.iter().enumerate() {
        let w: usize = line.spans.iter().map(|s| display_width(&s.content)).sum();
        assert!(
            w <= size.columns,
            "{label} {}x{}: line {i} is {w} cells\n  {:?}",
            size.columns,
            size.rows,
            line.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn i19_i26_no_frame_ever_overflows_its_terminal() {
    let populated = fixture(Populated::Yes);
    let rows = render::visible_rows(&populated, &UiState::default());
    let empty = fixture(Populated::No);

    let mut frames = 0usize;
    for columns in [
        20, 40, 49, 50, 51, 55, 60, 62, 70, 72, 79, 80, 81, 88, 100, 104, 120, 140, 200, 300,
    ] {
        for rows_n in [
            0, 1, 5, 9, 10, 11, 13, 14, 19, 20, 21, 22, 24, 30, 40, 60, 100,
        ] {
            let size = Size {
                columns,
                rows: rows_n,
            };
            for ui in states(&rows) {
                check(&populated, &ui, size, "populated");
                check(&empty, &ui, size, "pending");
                frames += 2;
            }
        }
    }
    assert!(frames > 4_000, "only {frames} frames swept");
}

/// The size the whole app is designed around, and the two edges either side of
/// the floor where it stops drawing.
#[test]
fn i26_the_minimum_and_the_refusal_below_it_are_both_drawn() {
    let app = fixture(Populated::Yes);
    let ui = UiState::default();

    let at_min = render::build(
        &app,
        &ui,
        Size {
            columns: MIN_COLUMNS,
            rows: MIN_ROWS,
        },
        NOW,
    );
    let text: String = at_min
        .lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect();
    assert!(
        text.contains("useful-system-monitor"),
        "the minimum size should still draw the app"
    );

    let below = render::build(
        &app,
        &ui,
        Size {
            columns: MIN_COLUMNS - 1,
            rows: MIN_ROWS,
        },
        NOW,
    );
    let text: String = below
        .lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect();
    assert!(
        text.contains("too small"),
        "below the floor it must say so: {text:?}"
    );
    assert!(!below.kill_modal_drawn);
}

/// I-15, mechanically: the flag the keymap reads is true only when the
/// confirmation is genuinely in the frame.
#[test]
fn i15_kill_modal_drawn_tracks_the_frame_and_not_the_mode() {
    let app = fixture(Populated::Yes);
    let rows = render::visible_rows(&app, &UiState::default());
    let pid = rows[0].pid;
    let ui = UiState {
        mode: Mode::Kill(pid),
        selected_pid: Some(pid),
        ..UiState::default()
    };

    let big = render::build(
        &app,
        &ui,
        Size {
            columns: 100,
            rows: 32,
        },
        NOW,
    );
    assert!(
        big.kill_modal_drawn,
        "a confirmation on a big terminal is drawn"
    );

    // The same mode, at a size where nothing is drawn at all.
    let tiny = render::build(
        &app,
        &ui,
        Size {
            columns: 49,
            rows: 32,
        },
        NOW,
    );
    assert!(
        !tiny.kill_modal_drawn,
        "nothing is drawn at 49 columns, so nothing is confirmable"
    );

    // And the same mode against a process that has since left the working set.
    let gone = UiState {
        mode: Mode::Kill(999_999),
        ..ui.clone()
    };
    let frame = render::build(
        &app,
        &gone,
        Size {
            columns: 100,
            rows: 32,
        },
        NOW,
    );
    assert!(!frame.kill_modal_drawn);
}

/// A confirmation that does not name its target is not a confirmation: the
/// user has no way to tell which process they are about to close. See I-15.
#[test]
fn i15_the_confirmation_names_the_process() {
    let app = fixture(Populated::Yes);
    let rows = render::visible_rows(&app, &UiState::default());
    let target = &rows[0];
    let ui = UiState {
        mode: Mode::Kill(target.pid),
        selected_pid: Some(target.pid),
        ..UiState::default()
    };
    let frame = render::build(
        &app,
        &ui,
        Size {
            columns: 100,
            rows: 32,
        },
        NOW,
    );
    let text: String = frame
        .lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect();
    let name = sysmon_core::kill::guards::process_name(&target.command);
    assert!(
        text.contains(&name),
        "the confirmation should name {name:?}"
    );
    assert!(text.contains(&target.pid.to_string()));
}

/// A hostile process name must not be able to move the cursor. See I-19.
#[test]
fn i19_a_control_byte_in_a_name_never_reaches_the_screen() {
    let app = fixture(Populated::Yes);
    let ui = UiState::default();
    let frame = render::build(
        &app,
        &ui,
        Size {
            columns: 100,
            rows: 40,
        },
        NOW,
    );
    for line in &frame.lines {
        for s in &line.spans {
            assert!(
                !s.content.chars().any(|c| c.is_control()),
                "a control byte reached the frame: {:?}",
                s.content
            );
        }
    }
}

/// Every panel can fail independently, and a failure must degrade only its own
/// panel rather than the frame. See I-11.
#[test]
fn i11_a_failed_collector_degrades_only_its_own_panel() {
    let mut app = fixture(Populated::Yes);
    app.disk = Panel::Err {
        message: "/bin/df timed out after 5s".into(),
        sampled_at_ms: NOW,
    };
    for view in VIEW_ORDER {
        let ui = UiState {
            view,
            ..UiState::default()
        };
        let frame = render::build(
            &app,
            &ui,
            Size {
                columns: 100,
                rows: 32,
            },
            NOW,
        );
        let text: String = frame
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(
            text.contains("useful-system-monitor"),
            "{view:?} lost its header"
        );
        if view == View::Disk {
            // I-24: the message names the cause *and* the remedy, in the
            // wording the shipping build uses.
            assert!(text.contains("timed out"), "{view:?} should say why");
            assert!(
                text.contains("press r to retry"),
                "{view:?} should say what to do"
            );
        }
    }
}
