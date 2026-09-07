//! M11: how close the Rust frames are to the TypeScript ones, measured.
//!
//! Both builds are driven through their own pty at the same size with `--mock`,
//! sent the same keys, and their screens read back as **cell grids** rather than
//! as transcripts. The output is a number, not an impression.
//!
//! # What this can and cannot answer today
//!
//! The two mocks are different implementations, so the *data* rows legitimately
//! differ: different process names, different values. Comparing them would
//! measure the mocks, not the renderers.
//!
//! So the comparison is split. **Chrome** — the header, the tab strip, the
//! footer legend, the column header, the card borders and the section titles —
//! is data-independent, and any difference there is a real difference between
//! the two renderers. **Data rows** are reported separately, as context rather
//! than as a verdict.
//!
//! Closing the second half needs what the plan calls the recorded mock trace:
//! dump N ticks of the TypeScript mock to a file and have both builds replay it,
//! so the only variable left is the renderer. That is the remaining work before
//! cell-for-cell parity can be claimed — and it is deliberately not faked here
//! by comparing two things that were never going to match.
//!
//! Usage:
//!   cargo run -p sysmon-harness --bin frame_diff -- [--cols N] [--rows N]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize};

/// Big enough that draining the pty never becomes the thing being measured.
const DRAIN_BUFFER_BYTES: usize = 16384;
/// Long enough for a keypress to reach a redrawn frame in both builds.
const SETTLE: Duration = Duration::from_millis(400);
/// Both builds need a sample on screen before their frames are comparable.
const WARMUP: Duration = Duration::from_secs(4);
/// Two frames that agree completely.
const IDENTICAL_PCT: f64 = 100.0;
/// The five screens the tab strip offers.
const SCREEN_COUNT: usize = 5;
struct Screen {
    parser: vt100::Parser,
    rx: mpsc::Receiver<Vec<u8>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _pair: portable_pty::PtyPair,
}

impl Screen {
    fn start(argv: &[&str], cols: u16, rows: u16) -> Result<Self, String> {
        let pty = portable_pty::native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(argv[0]);
        for a in &argv[1..] {
            cmd.arg(a);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("FORCE_COLOR", "3");
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; DRAIN_BUFFER_BYTES];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    return;
                }
            }
        });
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        Ok(Self {
            parser: vt100::Parser::new(rows, cols, 0),
            rx,
            writer,
            child,
            _pair: pair,
        })
    }

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

    fn lines(&self) -> Vec<String> {
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
            .collect()
    }

    fn send(&mut self, keys: &str) {
        let _ = self.writer.write_all(keys.as_bytes());
        let _ = self.writer.flush();
        self.pump(SETTLE);
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Lines whose content does not depend on the sampled data, and which must
/// therefore be identical if the two renderers agree.
fn is_chrome(line: &str) -> bool {
    line.contains("OVERVIEW")
        || line.contains("q quit")
        || line.starts_with("PID")
        || line.trim_start().starts_with("PID ")
        || line.contains('╭')
        || line.contains('╰')
        || line.contains("VOLUMES")
        || line.contains("TOP ")
}

/// Longest-common-subsequence similarity over characters, as a percentage.
///
/// Better than a cell-by-cell equality count for this: two lines that agree on
/// everything but a shifted column would score near zero by position and near
/// one hundred here, and "shifted by one" is exactly the kind of difference
/// worth telling apart from "completely different".
fn similarity(left: &str, right: &str) -> f64 {
    let (x, y): (Vec<char>, Vec<char>) = (left.chars().collect(), right.chars().collect());
    if x.is_empty() && y.is_empty() {
        return IDENTICAL_PCT;
    }
    let mut prev = vec![0usize; y.len() + 1];
    let mut cur = vec![0usize; y.len() + 1];
    for i in 1..=x.len() {
        for j in 1..=y.len() {
            cur[j] = if x[i - 1] == y[j - 1] {
                prev[j - 1] + 1
            } else {
                cur[j - 1].max(prev[j])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
        cur.iter_mut().for_each(|v| *v = 0);
    }
    // Dice coefficient: the shared run counts once in each string.
    let shared = prev[y.len()] as f64;
    (shared + shared) / (x.len() + y.len()) as f64 * IDENTICAL_PCT
}

/// `--cols N --rows N`, or `None` if the command line held something else.
fn parse_size(args: &[String]) -> Option<(u16, u16)> {
    let (mut cols, mut rows) = (100u16, 32u16);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cols" => {
                cols = args[i + 1].parse().unwrap_or(cols);
                i += 1;
            }
            "--rows" => {
                rows = args[i + 1].parse().unwrap_or(rows);
                i += 1;
            }
            other => {
                eprintln!("frame_diff: unknown option {other}");
                return None;
            }
        }
        i += 1;
    }
    Some((cols, rows))
}

/// Both builds, running side by side under their own ptys.
fn both_builds(cols: u16, rows: u16) -> Result<(Screen, Screen), String> {
    let node_bin = std::fs::canonicalize("dist/cli.js")
        .map_err(|e| format!("dist/cli.js — run `npm run build` first ({e})"))?;
    let rust_bin = std::fs::canonicalize("target/release/sysmon")
        .or_else(|_| std::fs::canonicalize("target/debug/sysmon"))
        .map_err(|e| format!("no sysmon binary ({e})"))?;

    let node_path = node_bin.to_string_lossy().to_string();
    let rust_path = rust_bin.to_string_lossy().to_string();
    let node = Screen::start(&["node", &node_path, "--mock"], cols, rows)
        .map_err(|e| format!("node: {e}"))?;
    let rust =
        Screen::start(&[&rust_path, "--mock"], cols, rows).map_err(|e| format!("rust: {e}"))?;
    Ok((node, rust))
}

/// One screen's chrome comparison, folded into the running totals.
fn compare_screen(node: &mut Screen, rust: &mut Screen, name: &str, tally: &mut Tally) {
    let (a, b) = (node.lines(), rust.lines());
    let mut chrome_here = 0usize;
    let mut same_here = 0usize;

    for (an, bn) in a.iter().zip(b.iter()) {
        tally.scores.push(similarity(an, bn));
        if !(is_chrome(an) || is_chrome(bn)) {
            continue;
        }
        chrome_here += 1;
        if an == bn {
            same_here += 1;
        } else if tally.first_diff.is_none() {
            tally.first_diff = Some((name.to_string(), an.clone(), bn.clone()));
        }
    }
    tally.chrome_total += chrome_here;
    tally.chrome_same += same_here;
    println!(
        "  {name:<9} chrome {same_here}/{chrome_here} identical   whole screen {:.1}% similar",
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| similarity(x, y))
            .sum::<f64>()
            / a.len().max(1) as f64
    );
}

/// What the sweep has found so far.
#[derive(Default)]
struct Tally {
    chrome_total: usize,
    chrome_same: usize,
    scores: Vec<f64>,
    first_diff: Option<(String, String, String)>,
}

/// `DUMPALL=<key>` prints every line side by side; `DUMP` prints only chrome.
fn dump_if_asked(node: &mut Screen, rust: &mut Screen) {
    if let Ok(which) = std::env::var("DUMPALL") {
        let key = if which.is_empty() {
            "1"
        } else {
            which.as_str()
        };
        node.send(key);
        rust.send(key);
        println!("\n--- every line, node | rust ---");
        for (i, (a, b)) in node.lines().iter().zip(rust.lines().iter()).enumerate() {
            let mark = if a == b { ' ' } else { '*' };
            println!("{i:>2}{mark}N {a}");
            println!("{i:>2}{mark}R {b}");
        }
    }

    if std::env::var("DUMP").is_ok() {
        node.send("1");
        rust.send("1");
        println!("\n--- every chrome line, node vs rust ---");
        for (a, b) in node.lines().iter().zip(rust.lines().iter()) {
            if is_chrome(a) || is_chrome(b) {
                println!("  N {a}\n  R {b}\n");
            }
        }
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cols, rows)) = parse_size(&args) else {
        return std::process::ExitCode::from(2);
    };
    let (mut node, mut rust) = match both_builds(cols, rows) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("frame_diff: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    println!("size {cols}x{rows}, both --mock\n");
    node.pump(WARMUP);
    rust.pump(WARMUP);

    let screens: [(&str, &str); SCREEN_COUNT] = [
        ("overview", "1"),
        ("cpu", "2"),
        ("memory", "3"),
        ("battery", "4"),
        ("disk", "5"),
    ];

    let mut tally = Tally::default();
    for (name, key) in screens {
        node.send(key);
        rust.send(key);
        compare_screen(&mut node, &mut rust, name, &mut tally);
    }

    report(&tally);
    dump_if_asked(&mut node, &mut rust);
    std::process::ExitCode::SUCCESS
}

fn report(tally: &Tally) {
    let overall = tally.scores.iter().sum::<f64>() / tally.scores.len().max(1) as f64;
    println!(
        "\nchrome: {}/{} lines identical ({:.0}%)",
        tally.chrome_same,
        tally.chrome_total,
        tally.chrome_same as f64 / tally.chrome_total.max(1) as f64 * IDENTICAL_PCT
    );
    println!("whole frame: {overall:.1}% similar (data rows included, so not a verdict)");

    if let Some((screen, a, b)) = &tally.first_diff {
        println!("\nfirst chrome difference, on {screen}:\n  node: {a:?}\n  rust: {b:?}");
    }

    println!(
        "\nM11: cell-for-cell parity is NOT claimed. The two mocks are different\n\
         implementations, so data rows cannot match; closing that needs the recorded\n\
         mock trace both builds replay. This measures how far apart the renderers are."
    );
}
