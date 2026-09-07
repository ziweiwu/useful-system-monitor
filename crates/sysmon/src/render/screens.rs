//! The five screens, plus the two full-screen modes.
//!
//! Every one of these draws at least one list whose length comes from the
//! machine — cores, mounted volumes, top processes — so every one is handed a
//! row budget and rolls up whatever does not fit. Without that a 16-core Mac
//! overflowed the CPU screen by seven lines on an 80x24 terminal. See I-26.

use ratatui::text::Line;
use sysmon_core::app::state::UiState;
use sysmon_core::domain::format::{bytes, minutes_to_hm, percent, to_fixed};
use sysmon_core::domain::scoring::{estimate_watts, sort_processes, SortKey};
use sysmon_core::domain::types::{BatteryData, MemoryData, ProcessSample, VolumeUsage};
use sysmon_core::kill::guards::{check_kill, process_name, GuardContext, KillCheck};
use sysmon_core::layout::budget::fit_list;
use sysmon_core::layout::columns::column_layout;
use sysmon_core::layout::overview::OverviewPlan;
use sysmon_core::layout::scroll::window;
use sysmon_core::text::width::{display_width, pad_end, pad_start, truncate, wrap_to_width};
use sysmon_core::ui::theme;

use crate::render::table;
use crate::render::widgets::{bold, dim, meter, plain, ratio, span, sparkline};
use crate::render::writer::RowWriter;
use crate::render::{panel_age, BRAND};
use crate::runtime::store::AppData;

/// Everything a screen draws from.
///
/// Grouped rather than threaded one argument at a time: the overview alone
/// wanted seven, and every screen takes some subset of the same six. Each
/// screen destructures out only the fields it uses, so an unused one is a
/// compile error rather than a silently ignored argument.
pub struct Screen<'a> {
    pub app: &'a AppData,
    pub ui: &'a UiState,
    pub rows: &'a [ProcessSample],
    pub width: usize,
    /// Rows this screen may draw into. The overview is handed the whole content
    /// area; the detail screens are handed what `view_rows` leaves after margins.
    pub budget: usize,
    pub now_ms: i64,
}

/// A gauge card, as a column of lines. The overview lays four of these side by
/// side; here each is built independently so the widths cannot disagree.
struct Card {
    title: &'static str,
    headline: String,
    detail: [(String, String); 2],
    pct: f64,
    colour: theme::Rgb,
    history: Vec<f64>,
}

const CARD_ROWS: usize = 8;
/// Below this a card cannot carry its own title, so it is dropped instead.
const MIN_CARD: usize = 12;
/// The overview lays out this many gauge cards side by side.
const CARD_COUNT: usize = 4;
/// Two border cells and two of padding, per Ink's `borderStyle` + `paddingX={1}`.
const CARD_CHROME_COLS: usize = 4;
/// Narrower than this and the headline truncates to nothing, so the card floors here.
const MIN_CARD_INNER: usize = 6;
/// Gauges are drawn against a full-scale percentage, never against the series max.
const FULL_SCALE_PCT: f64 = 100.0;
/// What a gauge reads when its collector has no sample yet.
const NO_READING_PCT: f64 = 0.0;
/// Swap above this earns the colour-free `!` marker. See I-23.
const SWAP_PRESSURE_PCT: f64 = 70.0;
/// Below this the wattage is noise, so the card shows AC state instead.
const MIN_REPORTABLE_WATTS: f64 = 0.5;

// ---- column widths, in cells ----
/// The PID column on the CPU and memory "top processes" lists.
const PID_COL: usize = 8;
/// The CPU-percent column beside each process on the CPU screen.
const CPU_PCT_COL: usize = 8;
/// The resident-size column beside each process on the memory screen.
const RSS_COL: usize = 8;
/// The `P0`/`E3` label in front of each per-core bar.
const CORE_LABEL_COL: usize = 4;
/// The percentage printed after each per-core bar.
const CORE_PCT_COL: usize = 6;
/// The `wired`/`active`/… labels in the memory breakdown.
const BREAKDOWN_LABEL_COL: usize = 12;
/// The byte figure printed after each memory-breakdown bar.
const BREAKDOWN_VALUE_COL: usize = 8;
/// The used-percentage column on the disk screen.
const VOLUME_PCT_COL: usize = 5;
/// The trailing "N free" column on the disk screen.
const VOLUME_FREE_COL: usize = 12;

// ---- the core strip ----
/// Held back so the strip's own label never eats into the core cells.
const CORE_STRIP_LABEL_COLS: usize = 7;
/// Held back for the `  E ` divider between performance and efficiency cores.
const CORE_GROUP_GAP_COLS: usize = 6;
/// Two block cells and a space, per core.
const CORE_CELL_COLS: usize = 3;

// ---- detail-screen geometry ----
/// Columns the CPU/memory bar gives up to the figures printed beside it.
const DETAIL_BAR_RESERVE: usize = 34;
const DETAIL_BAR_MIN: usize = 10;
const DETAIL_BAR_MAX: usize = 36;
/// Columns the process-name field gives up to the figures beside it.
const DETAIL_NAME_RESERVE: usize = 30;
const DETAIL_NAME_MIN: usize = 12;
const DETAIL_NAME_MAX: usize = 34;
/// A history ring holds 60 samples, so a wider sparkline would only pad.
const SPARK_MAX_COLS: usize = 60;
/// Most processes a detail screen's top list will show, however much room there is.
const MAX_TOP_ROWS: usize = 6;
/// Physical memory is partitioned into this many non-overlapping buckets.
const MEMORY_BREAKDOWN_ROWS: usize = 5;
/// Swap this full of its own total is "heavy", and says so in words.
const SWAP_HEAVY_FRACTION: f64 = 0.7;

// ---- the disk screen ----
/// A volume at or above this is called out as full.
const VOLUME_FULL_PCT: f64 = 90.0;
const MOUNT_COL_MIN: usize = 6;
const MOUNT_COL_MAX: usize = 24;
/// Columns the volume bar gives up to the mount name and the three figures.
const DISK_BAR_RESERVE: usize = 36;
const DISK_BAR_MIN: usize = 8;
const DISK_BAR_MAX: usize = 30;

// ---- the battery screen ----
/// Processes ranked by energy that the battery screen will consider drawing.
const ENERGY_RANK_LIMIT: usize = 8;
/// Energy attributed to a process that reported none.
const NO_ENERGY: f64 = 0.0;
/// Watts below which the charge/draw wording is noise rather than a reading.
const IDLE_WATTS: f64 = 0.0;
/// A per-process energy share under this is not worth an advice line.
const MIN_ADVICE_ENERGY: f64 = 0.2;
const BATTERY_NAME_RESERVE: usize = 44;
const BATTERY_NAME_MIN: usize = 12;
const BATTERY_NAME_MAX: usize = 30;
const BATTERY_BAR_MIN: usize = 6;
const BATTERY_BAR_MAX: usize = 20;
/// Columns the charge gauge gives up to the figures printed beside it.
const BATTERY_GAUGE_RESERVE: usize = 20;
const BATTERY_GAUGE_MAX: usize = 40;

// ---- the process detail screen ----
/// A PATH block costs a blank, a title and at least one line of path.
const PATH_BLOCK_ROWS: usize = 3;
/// Rows the PATH block must leave for whatever follows it.
const PATH_BLOCK_TAIL: usize = 6;
/// A COMMAND or history block costs a blank, a title and two lines.
const DETAIL_BLOCK_ROWS: usize = 4;
/// Columns the per-process sparklines give up to their labels.
const DETAIL_SPARK_RESERVE: usize = 12;
const DETAIL_SPARK_MAX: usize = 40;

/// One card, as its eight lines.
///
/// Matches the Ink `Box borderStyle="round" paddingX={1}`: a border, then rows
/// of `│ ` + content + ` │`, so the content area is `width - 4` — two border
/// cells and two of padding. Getting `inner` wrong shifts every card in the row
/// and is invisible until you diff the frames.
fn card_lines(card: &Card, width: usize) -> Vec<Vec<ratatui::text::Span<'static>>> {
    let inner = width.saturating_sub(CARD_CHROME_COLS).max(MIN_CARD_INNER);
    let bar_w = inner;
    let mut out = Vec::with_capacity(CARD_ROWS);
    let edge = |left: &str, right: &str| {
        vec![span(
            format!("{left}{}{right}", "─".repeat(inner + 2)),
            theme::FRAME,
        )]
    };
    let wrap = |content: Vec<ratatui::text::Span<'static>>| {
        let mut v = vec![span("│ ".to_string(), theme::FRAME)];
        v.extend(content);
        v.push(span(" │".to_string(), theme::FRAME));
        v
    };

    out.push(edge("╭", "╮"));
    out.push(wrap(vec![dim(pad_end(card.title, inner))]));
    out.push(wrap(vec![sparkline(
        &card.history,
        inner,
        FULL_SCALE_PCT,
        card.colour,
    )]));
    // `justifyContent="center"` on a one-child Box: the headline is centred in
    // the content area, with the odd cell going to the right.
    let hw = display_width(&card.headline).min(inner);
    let left = (inner - hw) / 2;
    out.push(wrap(vec![
        plain(" ".repeat(left)),
        bold(truncate(&card.headline, hw), theme::HEADLINE),
        plain(" ".repeat(inner - hw - left)),
    ]));
    let mut barline = Vec::new();
    barline.extend(meter(card.pct, bar_w, card.colour));
    out.push(wrap(barline));
    for (label, value) in &card.detail {
        let text = if value.is_empty() {
            label.clone()
        } else {
            format!("{label} {value}")
        };
        out.push(wrap(vec![dim(pad_end(&truncate(&text, inner), inner))]));
    }
    out.push(edge("╰", "╯"));
    out
}

/// What a gauge shows in place of a reading it does not have.
fn dash() -> String {
    "—".to_string()
}

/// An empty detail row, for a card that has nothing to put in one.
fn blank_detail() -> (String, String) {
    (String::new(), String::new())
}

/// The headline, the two detail rows and the gauge percentage, in the order a
/// `Card` wants them. Each `*_card` builds one of these from its own sample.
type CardFacts = (String, [(String, String); 2], f64);

fn cpu_card(app: &AppData) -> Card {
    let (headline, detail, pct) = match app.cpu.sample() {
        Some(cpu) => (
            format!("{}%", to_fixed(cpu.system, 1)),
            [
                (
                    format!("user {}%", to_fixed(cpu.user_percent, 0)),
                    format!("sys {}%", to_fixed(cpu.sys_percent, 0)),
                ),
                (
                    "load".to_string(),
                    format!(
                        "{} {}",
                        to_fixed(cpu.load_avg[0], 1),
                        to_fixed(cpu.load_avg[1], 1)
                    ),
                ),
            ],
            cpu.system,
        ),
        None => (dash(), [blank_detail(), blank_detail()], NO_READING_PCT),
    };
    Card {
        title: "CPU",
        headline,
        detail,
        pct,
        colour: theme::severity(pct),
        history: app.histories.cpu.to_vec(),
    }
}

/// The `!` beside swap is the colour-free marker that it is under real
/// pressure, so the warning survives NO_COLOR. See I-23.
fn memory_facts(memory: &MemoryData) -> CardFacts {
    let used_pct = ratio(memory.used_bytes, memory.total_bytes);
    let swap_pct = ratio(memory.swap_used_bytes, memory.swap_total_bytes);
    let pressure = if swap_pct > SWAP_PRESSURE_PCT {
        "  !"
    } else {
        ""
    };
    (
        format!("{}%", to_fixed(used_pct, 1)),
        [
            (
                bytes(memory.used_bytes),
                format!("/ {}", bytes(memory.total_bytes)),
            ),
            (
                "swap".to_string(),
                format!("{}{pressure}", bytes(memory.swap_used_bytes)),
            ),
        ],
        used_pct,
    )
}

fn memory_card(app: &AppData) -> Card {
    let (headline, detail, pct) = match app.memory.sample() {
        Some(memory) => memory_facts(memory),
        None => (dash(), [blank_detail(), blank_detail()], NO_READING_PCT),
    };
    Card {
        title: "MEM",
        headline,
        detail,
        pct,
        colour: theme::MEM,
        history: app.histories.memory.to_vec(),
    }
}

fn disk_card(app: &AppData) -> Card {
    let (headline, detail, pct) = match app.disk.sample() {
        Some(disk) => {
            let used_pct = ratio(disk.used_bytes, disk.total_bytes);
            (
                format!("{}%", to_fixed(used_pct, 0)),
                [
                    (
                        bytes(disk.used_bytes),
                        format!("/ {}", bytes(disk.total_bytes)),
                    ),
                    ("free".to_string(), bytes(disk.free_bytes)),
                ],
                used_pct,
            )
        }
        None => (dash(), [blank_detail(), blank_detail()], NO_READING_PCT),
    };
    Card {
        title: "DISK",
        headline,
        detail,
        pct,
        colour: theme::disk_severity(pct),
        history: app.histories.disk.to_vec(),
    }
}

/// The charge line for a battery that is actually present.
///
/// `^` charging, `=` holding, `v` draining: the state is readable without
/// colour (I-23).
fn battery_facts(battery: &BatteryData) -> CardFacts {
    let flow = if battery.charging {
        "^"
    } else if battery.on_ac_power {
        "="
    } else {
        "v"
    };
    let power = match battery.watts {
        Some(watts) if watts.abs() >= MIN_REPORTABLE_WATTS => {
            format!("{}W", to_fixed(watts, 1))
        }
        _ if battery.on_ac_power => "on AC".to_string(),
        _ => dash(),
    };
    let health = match (battery.cycle_count, battery.temperature_c) {
        (Some(cycles), Some(celsius)) => {
            (format!("{cycles}cy"), format!("{}C", to_fixed(celsius, 1)))
        }
        _ => blank_detail(),
    };
    (
        format!("{}% {}", to_fixed(battery.percent, 0), flow),
        [
            (
                power,
                battery
                    .time_remaining_min
                    .map(|minutes| minutes_to_hm(Some(minutes)))
                    .unwrap_or_default(),
            ),
            health,
        ],
        battery.percent,
    )
}

fn battery_card(app: &AppData) -> Card {
    let (headline, detail, pct) = match app.battery.sample() {
        Some(battery) if battery.present => battery_facts(battery),
        // No battery is a fact, not a gap: every desktop Mac reports none.
        Some(_) => (
            dash(),
            [
                ("no battery".to_string(), String::new()),
                ("on AC".to_string(), String::new()),
            ],
            NO_READING_PCT,
        ),
        None => (dash(), [blank_detail(), blank_detail()], NO_READING_PCT),
    };
    Card {
        title: "BATT",
        headline,
        detail,
        pct,
        colour: theme::BATTERY,
        history: app.histories.battery.to_vec(),
    }
}

/// The four cards, most to least worth the width. `fit` takes from the front,
/// so battery and disk go first — both have a screen of their own, and CPU and
/// memory are what the overview is for.
fn cards(app: &AppData) -> Vec<Card> {
    let mut out = Vec::with_capacity(CARD_COUNT);
    out.push(cpu_card(app));
    out.push(memory_card(app));
    out.push(disk_card(app));
    out.push(battery_card(app));
    out
}

pub fn overview(screen: &Screen, plan: &OverviewPlan) -> Vec<Line<'static>> {
    let Screen {
        app,
        ui,
        rows,
        width,
        budget,
        now_ms,
    } = *screen;
    let mut w = RowWriter::new("overview", width, budget);
    // The tab strip and the cards are separated by a row, which is part of what
    // OVERVIEW_FIXED pays for.
    w.blank();

    if plan.cards {
        let all = cards(app);
        // Cards are dropped from the tail — BATT first, then DISK — because the
        // gap is only ever at the right-hand end of the row.
        let gap = 1usize;
        let fit = ((width + gap) / (MIN_CARD + gap)).clamp(1, CARD_COUNT);
        let shown = &all[..fit.min(all.len())];
        let each = (width - gap * (shown.len() - 1)) / shown.len();
        let built: Vec<_> = shown.iter().map(|c| card_lines(c, each)).collect();
        for r in 0..CARD_ROWS {
            let mut spans = Vec::new();
            for (i, c) in built.iter().enumerate() {
                if i > 0 {
                    spans.push(plain(" ".to_string()));
                }
                spans.extend(c[r].clone());
            }
            w.push(Line::from(spans));
        }
    }

    if plan.cores {
        w.blank();
        core_strip(app, &mut w);
    }

    if plan.shows_table() {
        // The status block sits under a one-row margin, which is part of what
        // OVERVIEW_FIXED pays for.
        w.blank();
        let ages = format!(
            "cpu {} · proc {} · batt {}",
            panel_age(&app.cpu, now_ms),
            panel_age(&app.processes, now_ms),
            panel_age(&app.battery, now_ms)
        );
        let cap = ui.working_set_cap();
        let total = app.processes.sample().map_or(0, |p| p.total);
        let sel = ui
            .selected_pid
            .and_then(|pid| rows.iter().position(|p| p.pid == pid));
        let win = window(sel, rows.len(), plan.table_rows, ui.scroll_top);
        table::status(
            &table::Status {
                offset: win.offset,
                window: win.rows.get(),
                matched: rows.len(),
                total,
                cap_label: &cap.label(),
                cost_note: cap.cost_note(),
                sort: sort_label(ui.sort_key),
                filter: &ui.filter,
                filter_mode: ui.mode == sysmon_core::app::state::Mode::Filter,
                pending: app.processes.sample().is_none(),
                ages: &ages,
            },
            &mut w,
        );

        let cols = column_layout(width);
        match app.processes.sample() {
            Some(procs) => {
                let total_energy: f64 =
                    rows.iter().filter_map(|p| p.energy).sum::<f64>() + procs.others.energy;
                let ctx = table::TableContext {
                    total_energy,
                    total_watts: app.battery.sample().and_then(|b| b.watts),
                    energy_accurate: procs.energy_accurate,
                    can_expand: ui.ws_step + 1
                        < sysmon_core::domain::working_set::WORKING_SET_STEPS.len(),
                    filtered: !ui.filter.trim().is_empty(),
                };
                let body = table::Body {
                    rows,
                    window: win,
                    cols: &cols,
                    selected_pid: ui.selected_pid,
                };
                table::header(&cols, &ctx, &mut w);
                table::body(&body, &ctx, &mut w);
                table::rollup(&procs.others, &cols, &ctx, &mut w);
            }
            None => table::unavailable("processes", app.processes.error(), &mut w),
        }
    }
    w.into_lines()
}

fn sort_label(k: SortKey) -> &'static str {
    match k {
        SortKey::Cpu => "cpu",
        SortKey::Mem => "memory",
        SortKey::Energy => "energy",
    }
}

/// One line of two-cell blocks, performance cores then efficiency cores.
/// Overflow is counted rather than wrapped: a second line would cost a table
/// row, and the CPU screen draws every core in full anyway.
fn core_strip(app: &AppData, writer: &mut RowWriter) {
    let Some(cpu) = app.cpu.sample() else {
        writer.push(Line::from(dim("CORES  sampling…")));
        return;
    };
    let perf = app
        .host
        .as_ref()
        .map_or(cpu.per_core.len(), |h| h.perf_cores);
    let capacity = writer
        .width()
        .saturating_sub(CORE_STRIP_LABEL_COLS + CORE_GROUP_GAP_COLS)
        / CORE_CELL_COLS;
    let mut spans = vec![dim("CORES  ".to_string())];
    let mut drawn = 0usize;
    for (i, v) in cpu.per_core.iter().enumerate() {
        if drawn >= capacity {
            break;
        }
        if i == 0 {
            spans.push(dim("P ".to_string()));
        } else if i == perf && perf < cpu.per_core.len() {
            spans.push(dim("  E ".to_string()));
        }
        let idx = ((v / FULL_SCALE_PCT * (theme::BLOCKS.len() - 1) as f64).round() as usize)
            .min(theme::BLOCKS.len() - 1);
        spans.push(span(
            format!("{}{} ", theme::BLOCKS[idx], theme::BLOCKS[idx]),
            theme::severity(*v),
        ));
        drawn += 1;
    }
    if drawn < cpu.per_core.len() {
        spans.push(dim(format!("+{}", cpu.per_core.len() - drawn)));
    }
    writer.push(Line::from(spans));
}

/// Widths shared by the two "bar plus number" detail screens.
fn detail_widths(width: usize) -> (usize, usize) {
    let bar_w = (width.saturating_sub(DETAIL_BAR_RESERVE)).clamp(DETAIL_BAR_MIN, DETAIL_BAR_MAX);
    let name_w =
        (width.saturating_sub(DETAIL_NAME_RESERVE)).clamp(DETAIL_NAME_MIN, DETAIL_NAME_MAX);
    (bar_w, name_w)
}

/// The sparkline under a detail headline is capped, not stretched: a 200-cell
/// history of a 60-sample ring would be 140 cells of padding.
fn detail_spark(width: usize) -> usize {
    width.saturating_sub(CARD_CHROME_COLS).min(SPARK_MAX_COLS)
}

pub fn cpu(screen: &Screen) -> Vec<Line<'static>> {
    let Screen {
        app,
        rows,
        width,
        budget,
        ..
    } = *screen;
    let mut w = RowWriter::new("cpu", width, budget);
    let Some(c) = app.cpu.sample() else {
        w.push(Line::from(dim("CPU data unavailable — press r to retry.")));
        return w.into_lines();
    };
    let (bar_w, name_w) = detail_widths(width);

    /*
     * Row budget. Fixed: the headline, the sparkline and its trailing blank.
     * The TOP CPU block costs a blank line and a title on top of its rows.
     *
     * The process list gives way first — per-core bars are what this screen is
     * for — and only if the cores still do not fit do they roll up. A 16-core
     * Mac on an 80x24 terminal used to overrun it by seven lines.
     */
    const CHROME: usize = 3;
    let top_list = sort_processes(rows, SortKey::Cpu);
    let top_n = budget
        .saturating_sub(CHROME + c.per_core.len() + 2)
        .min(MAX_TOP_ROWS)
        .min(top_list.len());
    let core_budget =
        budget as isize - CHROME as isize - if top_n > 0 { 2 + top_n as isize } else { 0 };
    let cores = fit_list(c.per_core.len(), core_budget);

    /* I-19: load averages are three unbounded numbers, so this line grows with
    the machine. Unwrapped it ran to 53 cells at 50 columns and cost the
    screen a second row that the budget had not allowed for. */
    w.push(Line::from(vec![
        bold(format!("{}%", to_fixed(c.system, 1)), theme::HEADLINE),
        dim(format!(
            "  user {}%  sys {}%  load {}",
            to_fixed(c.user_percent, 1),
            to_fixed(c.sys_percent, 1),
            c.load_avg
                .iter()
                .map(|n| to_fixed(*n, 2))
                .collect::<Vec<_>>()
                .join("  ")
        )),
    ]));
    w.push(Line::from(sparkline(
        &app.histories.cpu.to_vec(),
        detail_spark(width),
        FULL_SCALE_PCT,
        theme::severity(c.system),
    )));
    w.blank();

    let perf = app.host.as_ref().map_or(c.per_core.len(), |h| h.perf_cores);
    for (i, v) in c.per_core.iter().take(cores.shown).enumerate() {
        let label = if i < perf {
            format!("P{i}")
        } else {
            format!("E{}", i - perf)
        };
        let mut spans = vec![dim(pad_end(&label, CORE_LABEL_COL))];
        spans.extend(meter(*v, bar_w, theme::severity(*v)));
        spans.push(span(
            pad_start(&format!("{}%", to_fixed(*v, 0)), CORE_PCT_COL),
            theme::HEADLINE,
        ));
        w.push(Line::from(spans));
    }
    if core_budget > 0 && cores.hidden > 0 {
        w.push(Line::from(dim(format!(
            "… {} more cores — taller terminal to see them",
            cores.hidden
        ))));
    }

    if top_n > 0 {
        w.blank();
        w.push(Line::from(bold("TOP CPU", theme::CPU_MID)));
        for p in top_list.iter().take(top_n) {
            w.push(Line::from(vec![
                dim(pad_end(&p.pid.to_string(), PID_COL)),
                plain(pad_end(&process_name(&p.command), name_w)),
                bold(
                    pad_start(&format!("{}%", percent(p.cpu_percent, 1)), CPU_PCT_COL),
                    theme::severity(p.cpu_percent.unwrap_or(NO_READING_PCT)),
                ),
            ]));
        }
    }
    w.into_lines()
}

pub fn memory(screen: &Screen) -> Vec<Line<'static>> {
    let Screen {
        app,
        rows,
        width,
        budget,
        ..
    } = *screen;
    let mut w = RowWriter::new("memory", width, budget);
    let Some(m) = app.memory.sample() else {
        w.push(Line::from(dim(
            "Memory data unavailable — press r to retry.",
        )));
        return w.into_lines();
    };
    let (bar_w, name_w) = detail_widths(width);

    // These partition physical memory: they must not overlap, which is why
    // `available` (free + inactive) is shown below rather than in this list.
    let breakdown_rows: [(&str, u64); MEMORY_BREAKDOWN_ROWS] = [
        ("wired", m.wired_bytes),
        ("active", m.active_bytes),
        ("inactive", m.inactive_bytes),
        ("compressed", m.compressed_bytes),
        ("free", m.free_bytes),
    ];

    /*
     * Row budget, spent in priority order. See I-26.
     *
     * The chrome used to be counted but never checked: the headline, the five
     * breakdown bars and the two summary lines were drawn whatever the budget
     * said, so on a 14-row terminal this screen ran two lines past the bottom.
     * Only the TOP MEMORY list ever gave way.
     *
     * The breakdown is what the screen is for, so the summary goes first, then
     * the breakdown rolls up, then the process list takes what is left.
     */
    const HEAD: usize = 3;
    const SUMMARY: usize = 3;
    const TOP_CHROME: usize = 2;
    let mut spare = budget.saturating_sub(HEAD);
    let show_summary = spare >= breakdown_rows.len() + SUMMARY;
    if show_summary {
        spare -= SUMMARY;
    }
    let breakdown = fit_list(breakdown_rows.len(), spare as isize);
    spare = spare.saturating_sub(breakdown.rows_used());
    let top_list = sort_processes(rows, SortKey::Mem);
    let top_n = spare
        .saturating_sub(TOP_CHROME)
        .min(MAX_TOP_ROWS)
        .min(top_list.len());

    w.push(Line::from(vec![
        bold(bytes(m.used_bytes), theme::HEADLINE),
        dim(format!(" / {} used", bytes(m.total_bytes))),
    ]));
    w.push(Line::from(sparkline(
        &app.histories.memory.to_vec(),
        detail_spark(width),
        FULL_SCALE_PCT,
        theme::MEM,
    )));
    w.blank();

    for (label, v) in breakdown_rows.iter().take(breakdown.shown) {
        let mut spans = vec![dim(pad_end(label, BREAKDOWN_LABEL_COL))];
        spans.extend(meter(ratio(*v, m.total_bytes), bar_w, theme::MEM));
        spans.push(plain(pad_start(&bytes(*v), BREAKDOWN_VALUE_COL)));
        w.push(Line::from(spans));
    }
    if breakdown.hidden > 0 {
        w.push(Line::from(dim(format!(
            "… {} more — taller terminal to see them",
            breakdown.hidden
        ))));
    }

    if show_summary {
        w.blank();
        w.push(Line::from(dim(format!(
            "available {}   (free + reclaimable inactive)",
            bytes(m.available_bytes)
        ))));
        let heavy = m.swap_used_bytes as f64 > m.swap_total_bytes as f64 * SWAP_HEAVY_FRACTION;
        w.push(Line::from(span(
            format!(
                "swap {} / {}{}",
                bytes(m.swap_used_bytes),
                bytes(m.swap_total_bytes),
                if heavy {
                    "   ! heavy swap pressure"
                } else {
                    ""
                }
            ),
            if heavy { theme::CPU_HIGH } else { theme::DIM },
        )));
    }

    if top_n > 0 {
        w.blank();
        w.push(Line::from(bold("TOP MEMORY", theme::MEM)));
        for p in top_list.iter().take(top_n) {
            w.push(Line::from(vec![
                dim(pad_end(&p.pid.to_string(), PID_COL)),
                plain(pad_end(&process_name(&p.command), name_w)),
                bold(pad_start(&bytes(p.rss_bytes), RSS_COL), theme::MEM),
            ]));
        }
    }
    w.into_lines()
}

pub fn disk(screen: &Screen) -> Vec<Line<'static>> {
    let Screen {
        app,
        width,
        budget,
        now_ms,
        ..
    } = *screen;
    let mut w = RowWriter::new("disk", width, budget);
    let Some(d) = app.disk.sample() else {
        let msg = match app.disk.error() {
            Some(m) => format!("Disk data unavailable: {m} — press r to retry."),
            None => "Disk data unavailable — press r to retry.".to_string(),
        };
        w.push(Line::from(dim(truncate(&msg, width))));
        return w.into_lines();
    };

    let root_pct = ratio(d.used_bytes, d.total_bytes);
    /* Older samples predate the per-volume field, and a collector is free to
    report none, so the root volume is synthesised rather than assumed. This
    is also what keeps the panel useful on a single-filesystem machine. */
    let synthesised;
    let volumes: &[VolumeUsage] = if d.volumes.is_empty() {
        synthesised = vec![VolumeUsage {
            mount: d.mount.clone(),
            device: String::new(),
            total_bytes: d.total_bytes,
            used_bytes: d.used_bytes,
            free_bytes: d.free_bytes,
            network: false,
        }];
        &synthesised
    } else {
        &d.volumes
    };

    let vpct = |v: &VolumeUsage| ratio(v.used_bytes, v.total_bytes);
    /* Columns are budgeted from the widest mount actually present, so a lone
    `/` does not reserve 20 blank columns, and a deep path is truncated
    rather than allowed to wrap (I-19). */
    let full: Vec<&VolumeUsage> = volumes
        .iter()
        .filter(|v| vpct(v) >= VOLUME_FULL_PCT)
        .collect();
    let any_net = volumes.iter().any(|v| v.network);
    let widest = volumes
        .iter()
        .map(|v| v.mount.chars().count())
        .max()
        .unwrap_or(MOUNT_COL_MIN);
    let mount_w = widest.clamp(MOUNT_COL_MIN, MOUNT_COL_MAX);
    /* 4 columns for the `net` marker, the rest for the three numeric columns. */
    let bar_w = width
        .saturating_sub(mount_w + DISK_BAR_RESERVE)
        .clamp(DISK_BAR_MIN, DISK_BAR_MAX);

    /*
     * Row budget, spent in priority order. See I-26.
     *
     * The two notes below the list were subtracted from the budget but drawn
     * unconditionally, so on a short terminal they were charged for and printed
     * anyway — at 10 rows this screen drew eight lines into a budget of five.
     *
     * A note costs **one** row, and its blank separator is bought separately.
     * It used to cost two, switched on the moment it became affordable — and a
     * two-row section can never switch on without the list beneath it losing a
     * row, because it arrives one row later than the row that paid for it. So
     * growing the terminal by one *shrank* the list. The arithmetic only works
     * at a cost of one, so the separator waits until there is slack for it.
     */
    const HEAD: usize = 3;
    const LIST_CHROME: usize = 1;
    const NOTE: usize = 1;
    let affords = |spare: usize| spare as isize - NOTE as isize - LIST_CHROME as isize >= 1;
    let mut spare = budget.saturating_sub(HEAD);
    let show_net = any_net && affords(spare);
    if show_net {
        spare -= NOTE;
    }
    let show_full = !full.is_empty() && affords(spare);
    if show_full {
        spare -= NOTE;
    }
    let list_budget = spare as isize - LIST_CHROME as isize;
    let show_list = list_budget > 0;
    let fit = fit_list(volumes.len(), list_budget);
    /* The blank lines above the notes, only once the list needs no more rows. */
    let notes = usize::from(show_net) + usize::from(show_full);
    let note_gap = list_budget - volumes.len() as isize >= notes as isize;

    // I-19: mount paths and the sample age both grow this line.
    w.push(Line::from(vec![
        bold(bytes(d.used_bytes), theme::HEADLINE),
        dim(format!(
            " / {} used on {}   ",
            bytes(d.total_bytes),
            d.mount
        )),
        span(
            format!("{} free", bytes(d.free_bytes)),
            theme::disk_severity(root_pct),
        ),
        dim(format!("   sampled {}", panel_age(&app.disk, now_ms))),
    ]));
    w.push(Line::from(sparkline(
        &app.histories.disk.to_vec(),
        detail_spark(width),
        FULL_SCALE_PCT,
        theme::DISK,
    )));
    w.blank();

    if show_list {
        w.push(Line::from(bold("VOLUMES", theme::DISK)));
    }
    for v in volumes.iter().take(fit.shown) {
        let pct = vpct(v);
        let mut spans = vec![plain(pad_end(&truncate(&v.mount, mount_w), mount_w + 1))];
        spans.extend(meter(pct, bar_w, theme::disk_severity(pct)));
        spans.push(span(
            pad_start(&format!("{}%", to_fixed(pct, 0)), VOLUME_PCT_COL),
            theme::HEADLINE,
        ));
        spans.push(dim(pad_start(
            &format!("{} free", bytes(v.free_bytes)),
            VOLUME_FREE_COL,
        )));
        spans.push(dim(pad_start(&format!("of {}", bytes(v.total_bytes)), 10)));
        if v.network {
            spans.push(dim("  net".to_string()));
        }
        w.push(Line::from(spans));
    }
    if list_budget > 0 && fit.hidden > 0 {
        w.push(Line::from(dim(format!(
            "… {} more volumes — taller terminal to see them",
            fit.hidden
        ))));
    }

    if show_net {
        if note_gap {
            w.blank();
        }
        // Worth saying: a stalled share is the usual reason this panel is the
        // slowest one to sample.
        w.push(Line::from(dim(
            "net = network share, measured by the remote host",
        )));
    }
    if show_full {
        if note_gap {
            w.blank();
        }
        // I-19: one truncated line. Wrapping this pushed the footer off a
        // 24-row terminal whenever two volumes were full at once.
        w.push(Line::from(span(
            truncate(
                &format!(
                    "! {} above 90% — writes and snapshots start to fail",
                    full.iter()
                        .map(|v| v.mount.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                width,
            ),
            theme::CPU_HIGH,
        )));
    }
    w.into_lines()
}

pub fn battery(screen: &Screen) -> Vec<Line<'static>> {
    let Screen {
        app,
        ui,
        rows,
        width,
        budget,
        ..
    } = *screen;
    let mut w = RowWriter::new("battery", width, budget);
    let Some(b) = app.battery.sample() else {
        w.push(Line::from(dim(
            "Battery data unavailable — press r to retry.",
        )));
        return w.into_lines();
    };
    if !b.present {
        // A fact, not a gap: every desktop Mac reports none.
        w.push(Line::from(dim(truncate(
            "This Mac has no battery — it runs on AC power, so there is nothing to attribute energy against.",
            width,
        ))));
        return w.into_lines();
    }

    let ranked: Vec<ProcessSample> = sort_processes(rows, SortKey::Energy)
        .into_iter()
        .take(ENERGY_RANK_LIMIT)
        .collect();
    let total_energy: f64 = rows.iter().filter_map(|p| p.energy).sum::<f64>()
        + app
            .processes
            .sample()
            .map_or(NO_ENERGY, |d| d.others.energy);
    let name_w = width
        .saturating_sub(BATTERY_NAME_RESERVE)
        .clamp(BATTERY_NAME_MIN, BATTERY_NAME_MAX);
    let show_watts = crate::render::table::watts_are_meaningful(b.watts);

    /*
     * I-19: the energy bar takes what is left, rather than a fixed 20 columns.
     * At 50 columns the row came to 51 cells against a 48-cell box, so every
     * consumer wrapped onto a second line and the list silently cost twice the
     * height it was budgeted — the same floor-instead-of-fit mistake the
     * process table's name column used to make.
     */
    const CURSOR_W: usize = 3;
    const PCT_W: usize = 7;
    let watts_w = if show_watts { 10 } else { 0 };
    let bar_w = width
        .saturating_sub(CURSOR_W + name_w + PCT_W + watts_w)
        .clamp(BATTERY_BAR_MIN, BATTERY_BAR_MAX);

    /*
     * Row budget, spent in priority order. See I-26.
     *
     * HEAD is the headline, the blank + gauge, the sparkline and the health
     * line. It used to include the list's own title in a constant that was
     * counted but never checked, so on a short terminal the whole screen was
     * drawn regardless and ran off the bottom. Now the advice goes first, then
     * the list rolls up, then the list drops entirely.
     *
     * The advice costs **one** row; its blank separator is bought separately,
     * for the same reason the disk notes are — a two-row section cannot switch
     * on without the list beneath it losing a row.
     */
    const HEAD: usize = 5;
    const LIST_CHROME: usize = 2;
    const NOTE: usize = 1;
    let advice = match (ranked.first(), show_watts, b.watts) {
        (Some(top), true, Some(_)) => estimate_watts(top.energy, total_energy, b.watts),
        _ => None,
    };
    let mut spare = budget.saturating_sub(HEAD);
    let show_advice = advice.is_some_and(|share| share > MIN_ADVICE_ENERGY)
        && spare as isize - NOTE as isize - LIST_CHROME as isize >= 1;
    if show_advice {
        spare -= NOTE;
    }
    let list_budget = spare as isize - LIST_CHROME as isize;
    let show_list = list_budget > 0;
    let fit = fit_list(ranked.len(), list_budget);
    let note_gap = show_advice && list_budget - ranked.len() as isize >= 1;

    w.push(Line::from(vec![
        bold("BATTERY  ".to_string(), theme::BATTERY),
        bold(format!("{}%", to_fixed(b.percent, 0)), theme::HEADLINE),
        dim(format!(
            "  {}{}",
            if b.charging {
                "charging"
            } else if b.on_ac_power {
                "on AC, holding charge"
            } else {
                "discharging"
            },
            match b.time_remaining_min {
                Some(m) => format!("  ~{} remaining", minutes_to_hm(Some(m))),
                None => String::new(),
            }
        )),
    ]));
    w.blank();
    let gauge_w = width
        .saturating_sub(BATTERY_GAUGE_RESERVE)
        .min(BATTERY_GAUGE_MAX);
    let mut gauge = Vec::new();
    gauge.extend(meter(b.percent, gauge_w, theme::BATTERY));
    gauge.push(plain(format!(
        "  {}",
        match b.watts {
            Some(v) if show_watts => format!(
                "{} W {}",
                to_fixed(v, 1),
                if v > IDLE_WATTS { "charging" } else { "draw" }
            ),
            _ => "no net draw".to_string(),
        }
    )));
    w.push(Line::from(gauge));
    w.push(Line::from(vec![
        sparkline(
            &app.histories.battery.to_vec(),
            gauge_w,
            FULL_SCALE_PCT,
            theme::BATTERY,
        ),
        dim("  charge history".to_string()),
    ]));
    w.push(Line::from(dim(format!(
        "{} cycles · {}% health · {}",
        b.cycle_count.map_or("—".to_string(), |c| c.to_string()),
        b.health_percent.map_or("—".to_string(), |h| h.to_string()),
        b.temperature_c
            .map_or("—".to_string(), |t| format!("{}°C", to_fixed(t, 1)))
    ))));

    if show_list {
        w.blank();
        w.push(Line::from(vec![
            bold("TOP ENERGY CONSUMERS", theme::BATTERY),
            dim("   (estimated from CPU time)".to_string()),
        ]));
    }
    for p in ranked.iter().take(fit.shown) {
        let watts = estimate_watts(p.energy, total_energy, b.watts);
        let selected = Some(p.pid) == ui.selected_pid;
        let mut spans = vec![if selected {
            bold(" > ".to_string(), theme::MEM)
        } else {
            bold("   ".to_string(), theme::DIM)
        }];
        spans.push(plain(pad_end(&process_name(&p.command), name_w)));
        spans.extend(meter(p.energy.unwrap_or(NO_ENERGY), bar_w, theme::BATTERY));
        spans.push(span(
            pad_start(&percent(p.energy, 1), PCT_W),
            theme::BATTERY,
        ));
        if let (true, Some(v)) = (show_watts, watts) {
            spans.push(dim(pad_start(&format!("~{} W", to_fixed(v, 1)), watts_w)));
        }
        w.push(Line::from(spans));
    }
    if list_budget > 0 && fit.hidden > 0 {
        w.push(Line::from(dim(format!(
            "   … {} more — taller terminal to see them",
            fit.hidden
        ))));
    }

    if let (true, Some(top), Some(a), Some(watts)) = (show_advice, ranked.first(), advice, b.watts)
    {
        if note_gap {
            w.blank();
        }
        let minutes = ((a / watts.abs()) * b.time_remaining_min.unwrap_or(0) as f64).round();
        w.push(Line::from(span(
            truncate(
                &format!(
                    "Killing {} could save ~{} W → about +{} min of battery.",
                    process_name(&top.command),
                    to_fixed(a, 1),
                    minutes as i64
                ),
                width,
            ),
            theme::CPU_MID,
        )));
    }
    w.into_lines()
}

pub fn detail(target: &ProcessSample, screen: &Screen) -> Vec<Line<'static>> {
    let Screen {
        app, width, budget, ..
    } = *screen;
    let mut w = RowWriter::new("detail", width, budget);
    w.push(Line::from(bold(
        truncate(&process_name(&target.command), width),
        theme::HEADLINE,
    )));
    if target.protected {
        w.push(Line::from(span(
            "protected — this process cannot be closed",
            theme::DANGER,
        )));
    }
    w.push(Line::from(dim(format!(
        "pid {}  parent {}  user {}  state {}",
        target.pid, target.ppid, target.user, target.state
    ))));
    if w.remaining() > PATH_BLOCK_ROWS {
        w.blank();
        w.push(Line::from(dim("PATH")));
        // Wrapped by display width, not by code units: a 66-unit CJK path
        // measures 98 cells, and slicing by length once threw two thirds of it
        // away on the one panel whose job is telling two helpers apart.
        for line in wrap_to_width(&target.command, width)
            .into_iter()
            .take(w.remaining().saturating_sub(PATH_BLOCK_TAIL))
        {
            w.push(Line::from(plain(line)));
        }
    }
    if let Some(cmd) = &app.command_line {
        if w.remaining() > DETAIL_BLOCK_ROWS {
            w.blank();
            w.push(Line::from(dim("COMMAND")));
            for line in wrap_to_width(cmd, width)
                .into_iter()
                .take(w.remaining().saturating_sub(DETAIL_BLOCK_ROWS))
            {
                w.push(Line::from(plain(line)));
            }
        }
    }
    if let Some(h) = app.proc_history.get(&target.pid) {
        if w.remaining() > DETAIL_BLOCK_ROWS {
            w.blank();
            let sw = width
                .saturating_sub(DETAIL_SPARK_RESERVE)
                .min(DETAIL_SPARK_MAX);
            w.push(Line::from(vec![
                dim("cpu     ".to_string()),
                sparkline(&h.cpu.to_vec(), sw, FULL_SCALE_PCT, theme::CPU_LOW),
            ]));
            let max_mem = h.mem.to_vec().into_iter().fold(1.0f64, f64::max);
            w.push(Line::from(vec![
                dim("memory  ".to_string()),
                sparkline(&h.mem.to_vec(), sw, max_mem, theme::MEM),
            ]));
            w.push(Line::from(vec![
                dim("energy  ".to_string()),
                sparkline(&h.energy.to_vec(), sw, FULL_SCALE_PCT, theme::BATTERY),
            ]));
        }
    }
    if w.remaining() > 0 {
        w.push(Line::from(dim("k close this app    esc back")));
    }
    w.into_lines()
}

/// The kill confirmation. I-15: confirmed **by name**, on screen, and SIGKILL
/// needs a second, distinct press.
pub fn kill_modal(target: &ProcessSample, screen: &Screen) -> Vec<Line<'static>> {
    let Screen {
        app,
        ui,
        width,
        budget,
        ..
    } = *screen;
    let mut w = RowWriter::new("kill", width, budget);
    let ctx = GuardContext {
        self_pid: std::process::id() as i32,
        parents: app
            .processes
            .sample()
            .map(|d| d.parents.clone())
            .unwrap_or_default(),
    };
    let check = check_kill(target, &ctx, None);

    match &check {
        KillCheck::Refused(r) => {
            w.push(Line::from(bold("REFUSED", theme::DANGER)));
            w.blank();
            for line in wrap_to_width(&r.message, width)
                .into_iter()
                .take(w.remaining().saturating_sub(2))
            {
                w.push(Line::from(span(line, theme::DANGER)));
            }
            w.blank();
            w.push(Line::from(dim("esc back")));
        }
        KillCheck::Allowed(_) => {
            /*
             * Priority order, not source order — and the legend outranks the
             * stats, because the legend is the only thing that shows whether
             * SIGKILL is armed.
             *
             * `RowWriter::push` drops silently once the budget is spent, so
             * writing these in reading order meant that between 10 and 13 rows
             * the `t`/`k`/`esc` lines fell off the bottom. The armed frame was
             * then byte-identical to the unarmed one: a user pressed k, saw
             * nothing change, pressed again, and force-closed the process. That
             * is I-15's own rule — a confirmation you cannot see is not a
             * confirmation — failing one size class above where it was first
             * fixed. The `Refused` branch already reserves room for `esc back`;
             * this is the same reservation on the branch that can actually
             * signal.
             */
            const LEGEND_ROWS: usize = 3;

            w.push(Line::from(bold("CLOSE THIS APP?", theme::CPU_MID)));
            // The name is the confirmation. See I-15.
            w.push(Line::from(bold(
                truncate(&process_name(&target.command), width),
                theme::HEADLINE,
            )));

            /* Detail, in the order it is given up: the list is truncated from
            the end, so the trailing blank goes before the note, and the note
            before the numbers. */
            let optional: Vec<Line<'static>> = vec![
                Line::from(""),
                Line::from(dim(format!(
                    "pid {}  owner {}  parent {}",
                    target.pid, target.user, target.ppid
                ))),
                Line::from(dim(format!(
                    "cpu {}   memory {}   energy {}",
                    percent(target.cpu_percent, 1),
                    bytes(target.rss_bytes),
                    percent(target.energy, 1)
                ))),
                Line::from(""),
                Line::from(dim(format!(
                    "closing it reclaims about {}",
                    bytes(target.rss_bytes)
                ))),
                Line::from(""),
            ];
            let spare = w.remaining().saturating_sub(LEGEND_ROWS);
            for line in optional.into_iter().take(spare) {
                w.push(line);
            }

            w.push(Line::from(vec![
                span("t", theme::CPU_MID),
                dim("  ask it to close (SIGTERM)".to_string()),
            ]));
            w.push(Line::from(if ui.armed_kill {
                vec![
                    span("k", theme::DANGER),
                    span(
                        "  press again to force close — unsaved work will be lost",
                        theme::DANGER,
                    ),
                ]
            } else {
                vec![
                    span("k", theme::CPU_MID),
                    dim("  force close (SIGKILL)".to_string()),
                ]
            }));
            w.push(Line::from(dim("esc  cancel")));
        }
    }
    let _ = BRAND;
    w.into_lines()
}
