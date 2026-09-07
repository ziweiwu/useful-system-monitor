//! I-22b: the built binary really mounts and draws — and then keeps its
//! promises when you press keys at it.
//!
//! A build that loads is not a build that mounts, and nothing cheaper can tell
//! the difference: the unit tests never run the binary, `--json` returns before
//! the dashboard starts, and a heap check calls a dead app "flat", because an
//! app that never redraws never allocates. The TypeScript build shipped exactly
//! that once — six bytes of output, exit 0, every check green.
//!
//! So this drives the real binary through a real pty and reads the **cell grid**
//! back, rather than grepping a stream with the escape codes stripped off. The
//! difference matters: a regex over a transcript cannot tell you what is on
//! screen *now*, only what was written at some point, and the whole question
//! here is whether the frame the user is looking at contains a given thing.
//!
//! Usage:
//!   cargo run -p sysmon-harness --bin verify_tui -- [--bin PATH] [--cols N] [--rows N]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize};

/// The terminal this drives, unless `--rows` says otherwise.
const DEFAULT_ROWS: u16 = 32;
/// Big enough that draining the pty never becomes the thing being measured.
const DRAIN_BUFFER_BYTES: usize = 8192;
/// Long enough for a keypress to reach a redrawn frame.
const SETTLE: Duration = Duration::from_millis(250);
/// The first real sample waits on a collector tier, not just a redraw.
const FIRST_SAMPLE_TIMEOUT: Duration = Duration::from_secs(12);
/// Everything after that is a redraw, which is far quicker.
const SCREEN_TIMEOUT: Duration = Duration::from_secs(5);
struct Config {
    bin: String,
    cols: u16,
    rows: u16,
    /// Drive the scripted data instead of the machine.
    ///
    /// Worth having as the default in CI: the frames are deterministic, the
    /// process list holds still, and — critically — the kill check cannot reach
    /// a real process, because `--mock` swaps in a killer that signals nothing.
    mock: bool,
}

fn parse_config() -> Config {
    let mut cfg = Config {
        bin: "target/debug/sysmon".into(),
        cols: 100,
        rows: 32,
        mock: false,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bin" => {
                cfg.bin = args[i + 1].clone();
                i += 1;
            }
            "--cols" => {
                cfg.cols = args[i + 1].parse().unwrap_or(100);
                i += 1;
            }
            "--rows" => {
                cfg.rows = args[i + 1].parse().unwrap_or(DEFAULT_ROWS);
                i += 1;
            }
            "--mock" => cfg.mock = true,
            other => {
                eprintln!("verify_tui: unknown option {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    cfg
}

/// A running dashboard, and the screen as it currently looks.
struct Session {
    parser: vt100::Parser,
    rx: mpsc::Receiver<Vec<u8>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _pair: portable_pty::PtyPair,
}

impl Session {
    fn start(cfg: &Config) -> Result<Self, String> {
        let pty = portable_pty::native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: cfg.rows,
                cols: cfg.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let child = pair
            .slave
            .spawn_command(Self::app_command(cfg)?)
            .map_err(|e| e.to_string())?;

        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let rx = Self::drain(reader);

        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        Ok(Self {
            parser: vt100::Parser::new(cfg.rows, cfg.cols, 0),
            rx,
            writer,
            child,
            _pair: pair,
        })
    }

    /// The app under test, with colour on so the run exercises the same styled
    /// path a user sees.
    fn app_command(cfg: &Config) -> Result<CommandBuilder, String> {
        let mut cmd = CommandBuilder::new(
            std::fs::canonicalize(&cfg.bin).map_err(|e| format!("{}: {e}", cfg.bin))?,
        );
        if cfg.mock {
            cmd.arg("--mock");
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        Ok(cmd)
    }

    /// Read the pty in the background.
    ///
    /// A pty read never reaches EOF while the child lives, so this thread is
    /// deliberately detached and dies with the process.
    fn drain(mut reader: Box<dyn std::io::Read + Send>) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; DRAIN_BUFFER_BYTES];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    return;
                }
            }
        });
        rx
    }

    /// Feed everything that has arrived into the terminal emulator.
    fn pump(&mut self, for_: Duration) {
        let deadline = Instant::now() + for_;
        while Instant::now() < deadline {
            match self
                .rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(chunk) => self.parser.process(&chunk),
                Err(_) => break,
            }
        }
    }

    /// The screen, as a person would read it.
    fn screen(&self) -> String {
        let (rows, cols) = self.parser.screen().size();
        (0..rows)
            .map(|r| {
                (0..cols)
                    .map(|c| {
                        self.parser
                            .screen()
                            .cell(r, c)
                            .map(|cell| cell.contents())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| " ".to_string())
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn send(&mut self, keys: &str) {
        let _ = self.writer.write_all(keys.as_bytes());
        let _ = self.writer.flush();
    }

    /// Escape, alone.
    ///
    /// A lone ESC immediately followed by another byte is `Alt+<key>` to every
    /// terminal parser there is, so sending "\x1bq" would ask the app to quit
    /// with a modifier it does not bind — and look exactly like the escape
    /// having been ignored. Real keyboards have this gap; the harness has to
    /// have it too.
    fn escape(&mut self) {
        self.send("\x1b");
        self.pump(SETTLE);
    }

    /// Waits until the screen satisfies `pred`, or gives up.
    ///
    /// Polling for the state the check is actually about turns "slow machine"
    /// into "slower" rather than into "failed" — a fixed sleep is a bet that
    /// the machine is idle, and this suite runs precisely when it is not.
    fn wait_for(&mut self, what: &str, timeout: Duration, pred: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.pump(Duration::from_millis(100));
            if pred(&self.screen()) {
                return true;
            }
        }
        eprintln!("  timed out waiting for {what}");
        false
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
}

/// It mounts, draws, and gets a first sample onto the screen.
fn check_startup(session: &mut Session, checks: &mut Vec<Check>) {
    // The six-bytes case: does it draw at all.
    let drew = session.wait_for("the first frame", Duration::from_secs(10), |screen| {
        screen.contains("useful-system-monitor")
    });
    let screen = session.screen();
    checks.push(Check {
        name: "mounts and draws",
        ok: drew,
        detail: format!(
            "{} non-blank lines",
            screen.lines().filter(|l| !l.is_empty()).count()
        ),
    });

    // The process table needs its first sample before it has a header to draw,
    // so it is waited for rather than assumed.
    let table = session.wait_for("the process table", FIRST_SAMPLE_TIMEOUT, |sc| {
        sc.contains("PID")
    });
    checks.push(Check {
        name: "draws the process table",
        ok: table,
        detail: String::new(),
    });

    check_furniture(session, checks);

    // Real numbers arrive, not just chrome.
    let sampled = session.wait_for("a real sample", FIRST_SAMPLE_TIMEOUT, |sc| {
        sc.contains('%') && sc.lines().any(|l| l.contains("load "))
    });
    checks.push(Check {
        name: "samples the machine",
        ok: sampled,
        detail: String::new(),
    });
}

/// The furniture is on screen. Chrome rather than numbers: a value would make
/// this flaky on an idle or a loaded machine.
fn check_furniture(session: &mut Session, checks: &mut Vec<Check>) {
    let screen = session.screen();
    for (name, needle) in [
        ("tab strip", "OVERVIEW"),
        ("cards", "CPU"),
        ("card borders", "╭"),
        ("footer legend", "q quit"),
    ] {
        checks.push(Check {
            name,
            ok: screen.contains(needle),
            detail: format!("{needle:?}"),
        });
    }
}

/// The keymap is wired to the renderer, and filter mode takes text.
fn check_navigation(session: &mut Session, checks: &mut Vec<Check>) {
    session.send("2");
    let cpu_screen = session.wait_for("the CPU screen", SCREEN_TIMEOUT, |sc| {
        sc.contains("[2 CPU]")
    });
    checks.push(Check {
        name: "number key switches screen",
        ok: cpu_screen,
        detail: String::new(),
    });

    session.send("\x1b[C"); // right arrow
    let stepped = session.wait_for("the MEMORY screen", SCREEN_TIMEOUT, |sc| {
        sc.contains("[3 MEMORY]")
    });
    checks.push(Check {
        name: "arrow steps the strip",
        ok: stepped,
        detail: String::new(),
    });

    // A pasted burst is text rather than commands — the regression that
    // re-sorted by energy mid-paste.
    session.send("1");
    let _ = session.wait_for("the overview", SCREEN_TIMEOUT, |sc| {
        sc.contains("[1 OVERVIEW]")
    });
    session.send("/chrome");
    let filtered = session.wait_for("the filter box", SCREEN_TIMEOUT, |sc| {
        sc.contains("filter chrome")
    });
    checks.push(Check {
        name: "filter takes a pasted burst as text",
        ok: filtered && session.screen().contains("sort cpu"),
        detail: "sort must still be cpu".into(),
    });
    session.escape();
}

/// **The safety check.** A kill confirmation must be on screen, by name, before
/// any key can act on it. See I-15.
fn check_kill_confirmation(session: &mut Session, checks: &mut Vec<Check>) {
    let _ = session.wait_for("the table", SCREEN_TIMEOUT, |sc| sc.contains("PID"));
    session.send("k");
    let modal = session.wait_for("the kill confirmation", SCREEN_TIMEOUT, |sc| {
        sc.contains("CLOSE THIS APP?") || sc.contains("REFUSED")
    });
    checks.push(Check {
        name: "k opens a confirmation, by name",
        ok: modal,
        detail: String::new(),
    });
    session.escape();
    let closed = session.wait_for("the dashboard again", SCREEN_TIMEOUT, |sc| {
        !sc.contains("CLOSE THIS APP?") && !sc.contains("REFUSED")
    });
    checks.push(Check {
        name: "esc closes the confirmation",
        ok: closed,
        detail: String::new(),
    });
}

/// It quits when asked, rather than having to be killed.
fn check_quits(session: &mut Session, checks: &mut Vec<Check>) {
    session.send("q");
    let mut exited = false;
    let deadline = Instant::now() + SCREEN_TIMEOUT;
    while Instant::now() < deadline {
        if matches!(session.child.try_wait(), Ok(Some(_))) {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    checks.push(Check {
        name: "q quits",
        ok: exited,
        detail: String::new(),
    });
}

fn report(checks: &[Check], session: &mut Session) -> std::process::ExitCode {
    let failed = checks.iter().filter(|c| !c.ok).count();
    println!();
    for c in checks {
        println!(
            "  {} {:<36} {}",
            if c.ok { "ok  " } else { "FAIL" },
            c.name,
            c.detail
        );
    }
    if failed > 0 {
        println!("\nlast screen:\n{}", session.screen());
    }
    println!("\nI-22b: {}", if failed == 0 { "PASS" } else { "FAIL" });
    if failed == 0 {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(1)
    }
}

fn main() -> std::process::ExitCode {
    let cfg = parse_config();
    println!(
        "bin:  {}{}\nsize: {}x{}\n",
        cfg.bin,
        if cfg.mock { " --mock" } else { "" },
        cfg.cols,
        cfg.rows
    );

    let mut checks: Vec<Check> = Vec::new();
    let mut session = match Session::start(&cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("verify_tui: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    check_startup(&mut session, &mut checks);
    check_navigation(&mut session, &mut checks);
    check_kill_confirmation(&mut session, &mut checks);
    check_quits(&mut session, &mut checks);
    report(&checks, &mut session)
}
