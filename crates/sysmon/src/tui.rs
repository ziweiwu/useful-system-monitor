//! The dashboard: terminal setup, the event loop, and putting it all back.

use std::io::{self, Stdout};
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent as CtKeyEvent,
    KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

use sysmon_core::app::input::{
    reconcile_mode, reconcile_selection, reduce, toast, Effect, InputContext, Key, KeyEvent, Tone,
};
use sysmon_core::app::state::{Mode, UiState};
use sysmon_core::cli::options::Options;
use sysmon_core::domain::types::Panel;
use sysmon_core::kill::guards::{check_kill, GuardContext};
use sysmon_core::layout::screens::too_small;

use crate::collect::Collector;
use crate::render::{self, Size};
use crate::runtime::store::AppData;
use crate::runtime::supervisor::{self, CollectorKind, Msg, Sources, Tiers};
use sysmon_core::domain::types::ProcessSample;

/// How long the loop waits for a key before checking for a new sample.
///
/// Not a render tick: the screen is redrawn only when the data or the state
/// actually changed. An idle app therefore wakes about twice a second, does
/// nothing, and goes back to sleep — the clock on screen advances off the
/// newest *sample*, not off this.
const POLL: Duration = Duration::from_millis(500);

/// Puts the terminal back, whatever happened.
///
/// A panic in raw mode leaves a wrecked terminal — a failure mode the
/// TypeScript build did not have, because a JS exception unwound into React's
/// error path rather than out of a raw-mode program. So the restore is
/// installed as a panic hook as well as run on the way out.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
            previous(info);
        }));
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// crossterm's key, as the reducer's key.
///
/// Only `Press` is translated: `Repeat` from a held key must not count as the
/// "second, distinct press" SIGKILL asks for.
fn translate(k: CtKeyEvent) -> Option<Key> {
    if k.kind != KeyEventKind::Press {
        return None;
    }
    Some(match k.code {
        KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => Key::CtrlC,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Escape,
        KeyCode::Backspace | KeyCode::Delete => Key::Backspace,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return None,
    })
}

pub fn run(options: &Options) -> Result<(), String> {
    let _guard = TerminalGuard::enter().map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal: Terminal<CrosstermBackend<Stdout>> =
        Terminal::new(backend).map_err(|e| e.to_string())?;

    let (tx, rx) = mpsc::channel::<Msg>();
    let mut ui = UiState::default();
    let collectors = if options.mock {
        CollectorKind::Mock
    } else {
        CollectorKind::Live
    };
    let handles = start_collectors(options, &tx, collectors, &ui)?;

    let mut app = AppData {
        mock: options.mock,
        ..AppData::default()
    };
    event_loop(
        &mut terminal,
        &mut ui,
        &mut app,
        Wiring {
            handles: &handles,
            outbox: &tx,
            inbox: &rx,
            collectors,
        },
    )?;
    handles.shutdown();
    Ok(())
}

/// The channels and collector handles the loop runs against.
struct Wiring<'a> {
    handles: &'a supervisor::Handles,
    outbox: &'a mpsc::Sender<Msg>,
    inbox: &'a mpsc::Receiver<Msg>,
    collectors: CollectorKind,
}

/// Draw, drain input, absorb samples, settle — until the user quits.
fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ui: &mut UiState,
    app: &mut AppData,
    wiring: Wiring,
) -> Result<(), String> {
    let mut dirty = true;
    let mut kill_modal_drawn = false;
    // Every key drained together shares a batch id — see the reducer.
    let mut batch: u64 = 0;

    loop {
        if dirty {
            kill_modal_drawn = draw(terminal, app, ui)?;
            dirty = false;
        }

        let world = World {
            handles: wiring.handles,
            outbox: wiring.outbox,
            kill_modal_drawn,
            collectors: wiring.collectors,
        };
        match pump_input(ui, app, &mut batch, &world)? {
            Input::Quit => return Ok(()),
            Input::Changed => dirty = true,
            Input::Idle => {}
        }

        while let Ok(msg) = wiring.inbox.try_recv() {
            absorb(ui, app, msg, wiring.handles);
            dirty = true;
        }

        if settle(ui, app) {
            dirty = true;
        }
    }
}

/// Reconcile the selection and any open mode against the newest sample, so a
/// process that exits cannot leave the cursor stranded or a confirmation open on
/// something that is gone. Also expires the toast.
///
/// Returns true when something changed that the next frame must show.
fn settle(ui: &mut UiState, app: &AppData) -> bool {
    let rows = render::visible_rows(app, ui);
    reconcile_selection(ui, &rows);
    reconcile_mode(ui, &rows);
    let expired = ui
        .toast
        .as_ref()
        .is_some_and(|t| now_ms() >= t.expires_at_ms);
    if expired {
        ui.toast = None;
    }
    expired
}

fn start_collectors(
    options: &Options,
    outbox: &mpsc::Sender<Msg>,
    collectors: CollectorKind,
    ui: &UiState,
) -> Result<supervisor::Handles, String> {
    supervisor::start(
        outbox.clone(),
        options
            .interval
            .map(Tiers::with_interval)
            .unwrap_or_default(),
        Sources {
            accurate_energy: options.accurate_energy,
            collectors,
            initial_cap: ui.working_set_cap(),
        },
    )
}

/// Draw one frame, and report whether the kill confirmation actually reached
/// the screen. See I-15.
fn draw(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &AppData,
    ui: &UiState,
) -> Result<bool, String> {
    let size = terminal.size().map_err(|e| e.to_string())?;
    let size = Size {
        columns: size.width as usize,
        rows: size.height as usize,
    };
    let frame = render::build(app, ui, size, now_ms());
    let kill_modal_drawn = frame.kill_modal_drawn;
    terminal
        .draw(|f| {
            // Wrapping is never enabled: every line is already fitted by our own
            // width rules, and ratatui's wrap would re-measure with a different
            // one. See render::writer.
            f.render_widget(Paragraph::new(frame.lines), f.area());
        })
        .map_err(|e| e.to_string())?;
    Ok(kill_modal_drawn)
}

/// What one pass over the pending terminal events amounted to.
enum Input {
    Quit,
    Changed,
    Idle,
}

/// Drain every event that is already waiting.
///
/// One drain, one batch id: a pasted burst cannot both open and confirm a
/// destructive action.
fn pump_input(
    ui: &mut UiState,
    app: &mut AppData,
    batch: &mut u64,
    world: &World,
) -> Result<Input, String> {
    if !crossterm::event::poll(POLL).map_err(|e| e.to_string())? {
        return Ok(Input::Idle);
    }
    *batch += 1;
    let mut outcome = Input::Idle;
    while crossterm::event::poll(Duration::ZERO).map_err(|e| e.to_string())? {
        let event = crossterm::event::read().map_err(|e| e.to_string())?;
        match handle_event(ui, app, event, (*batch, world)) {
            Input::Idle => {}
            Input::Changed => outcome = Input::Changed,
            Input::Quit => return Ok(Input::Quit),
        }
    }
    Ok(outcome)
}

/// One terminal event.
fn handle_event(
    ui: &mut UiState,
    app: &mut AppData,
    event: Event,
    context: (u64, &World),
) -> Input {
    let (batch, world) = context;
    match event {
        Event::Key(k) => {
            let Some(key) = translate(k) else {
                return Input::Idle;
            };
            if apply(ui, app, KeyEvent { key, batch }, world) {
                return Input::Quit;
            }
            Input::Changed
        }
        // Bracketed paste: text, never commands. Accepted only where text is
        // what the screen is asking for.
        Event::Paste(text) if ui.mode == Mode::Filter => {
            ui.filter.push_str(&text);
            Input::Changed
        }
        Event::Resize(..) => Input::Changed,
        _ => Input::Idle,
    }
}

/// Fold one collector message into the app state, recording the history point
/// each panel contributes to its sparkline.
fn absorb(ui: &mut UiState, app: &mut AppData, msg: Msg, handles: &supervisor::Handles) {
    match msg {
        Msg::Host(h) => app.host = Some(*h),
        Msg::Cpu(p) => {
            if let Panel::Ok { data: c, .. } = p.as_ref() {
                app.histories.cpu.push(c.system);
            }
            app.cpu = *p;
        }
        Msg::Memory(p) => {
            if let Panel::Ok { data: m, .. } = p.as_ref() {
                app.histories
                    .memory
                    .push(render::widgets::ratio(m.used_bytes, m.total_bytes));
            }
            app.memory = *p;
        }
        Msg::Disk(p) => {
            if let Panel::Ok { data: d, .. } = p.as_ref() {
                app.histories
                    .disk
                    .push(render::widgets::ratio(d.used_bytes, d.total_bytes));
            }
            app.disk = *p;
        }
        other => absorb_rest(ui, app, other, handles),
    }
}

/// The messages that carry no sparkline history.
fn absorb_rest(ui: &mut UiState, app: &mut AppData, msg: Msg, handles: &supervisor::Handles) {
    match msg {
        Msg::Battery(p) => {
            if let Panel::Ok { data: b, .. } = p.as_ref() {
                app.histories.battery.push(b.percent);
            }
            app.battery = *p;
        }
        Msg::Processes(p) => {
            app.processes = *p;
            app.record_processes();
        }
        Msg::CommandLine(c) => app.command_line = c,
        Msg::Killed { pid, text, bad } => {
            toast(ui, text, if bad { Tone::Bad } else { Tone::Ok }, now_ms());
            ui.mode = Mode::Normal;
            // A kill that went through is news the collector cannot get any
            // other way in `--mock`; the platform collectors ignore it and
            // learn the same thing from the refresh below.
            if !bad {
                handles.note_killed(pid);
            }
            handles.refresh_all();
        }
        Msg::Host(_) | Msg::Cpu(_) | Msg::Memory(_) | Msg::Disk(_) => {
            unreachable!("handled by absorb")
        }
    }
}

/// The terminal a keypress lands in, when crossterm cannot say what size it is.
const FALLBACK_COLUMNS: u16 = 80;
const FALLBACK_ROWS: u16 = 24;

/// Everything outside the UI state that handling a key may need to reach.
struct World<'a> {
    handles: &'a supervisor::Handles,
    outbox: &'a mpsc::Sender<Msg>,
    /// Whether the confirmation was on the last frame the user actually saw.
    /// The reducer will not let a signal through without it. See I-15.
    kill_modal_drawn: bool,
    collectors: CollectorKind,
}

/// Whether the guards would allow the currently targeted process.
///
/// Computed once so the modal and the keymap cannot disagree about it.
fn kill_allowed(ui: &UiState, app: &AppData, rows: &[ProcessSample]) -> bool {
    let Mode::Kill(pid) = ui.mode else {
        return false;
    };
    rows.iter()
        .find(|p| p.pid == pid)
        .map(|target| {
            let ctx = GuardContext {
                self_pid: std::process::id() as i32,
                parents: parents_of(app),
            };
            check_kill(target, &ctx, None).is_allowed()
        })
        .unwrap_or(false)
}

/// The pid -> ppid map, built from every row so the ancestor guard can walk out
/// of the working set. See I-13.
fn parents_of(app: &AppData) -> std::collections::HashMap<i32, i32> {
    app.processes
        .sample()
        .map(|d| d.parents.clone())
        .unwrap_or_default()
}

/// Apply one key. Returns true when the app should exit.
fn apply(ui: &mut UiState, app: &mut AppData, event: KeyEvent, world: &World) -> bool {
    let rows = render::visible_rows(app, ui);
    let size = crossterm::terminal::size().unwrap_or((FALLBACK_COLUMNS, FALLBACK_ROWS));

    let ctx = InputContext {
        too_small: too_small(size.0 as usize, size.1 as usize),
        filtered: &rows,
        kill_allowed: kill_allowed(ui, app, &rows),
        kill_modal_drawn: world.kill_modal_drawn,
        now_ms: now_ms(),
    };
    let (next, effects) = reduce(ui, event, &ctx);
    *ui = next;

    effects
        .into_iter()
        .any(|effect| run_effect(effect, ui, app, (&rows, world)))
}

/// Carry out one effect. Returns true only for the one that ends the app.
fn run_effect(
    effect: Effect,
    ui: &UiState,
    app: &mut AppData,
    world: (&[ProcessSample], &World),
) -> bool {
    let (rows, world) = world;
    match effect {
        Effect::Quit => return true,
        Effect::RefreshAll => world.handles.refresh_all(),
        Effect::RefreshProcesses => world.handles.set_cap(ui.working_set_cap()),
        Effect::FetchCommandLine(pid) => {
            app.command_line = None;
            spawn_command_line(world.outbox.clone(), pid, world.collectors);
        }
        Effect::RequestKill { pid, signal } => {
            let Some(target) = rows.iter().find(|p| p.pid == pid).cloned() else {
                return false;
            };
            let request = supervisor::KillRequest {
                target,
                signal,
                parents: parents_of(app),
            };
            supervisor::spawn_kill(world.outbox.clone(), request, world.collectors);
        }
    }
    false
}

/// `argv` for one process, off the main thread so a slow read cannot stall the
/// render.
fn spawn_command_line(outbox: mpsc::Sender<Msg>, pid: i32, collectors: CollectorKind) {
    std::thread::Builder::new()
        .name("sysmon-argv".into())
        .spawn(move || {
            let line = match collectors {
                CollectorKind::Mock => Collector::mock(),
                CollectorKind::Live => Collector::new(false),
            }
            .ok()
            .and_then(|c| c.command_line(pid));
            let _ = outbox.send(Msg::CommandLine(line));
        })
        .ok();
}
