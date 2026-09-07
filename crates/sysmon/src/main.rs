//! `useful-system-monitor`, in Rust.
//!
//! This is what the npm `bin` runs as of 0.10.0. The TypeScript build is still
//! in the repository and still tested; it is no longer what a user runs. See
//! `POST-CUTOVER.md` for the behaviour deliberately left unchanged by the port.

use std::process::ExitCode;

use sysmon::{oneshot, tui, BRAND, HELP, VERSION};
use sysmon_core::cli::options::{parse_args, Options, ParseResult};

/// What this platform cannot honour, said plainly rather than failed obscurely.
///
/// `--energy=accurate` reads macOS's Energy Impact, a private Apple algorithm
/// with no Linux analogue. Accepting the flag and quietly serving the CPU-time
/// estimate would put two different units behind one word, in the one column
/// where I-1b says that must never happen. See I-24.
fn platform_refusal(options: &Options) -> Option<ExitCode> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        eprintln!(
            "{BRAND}: only macOS and Linux are supported (this is {}).",
            std::env::consts::OS
        );
        return Some(ExitCode::from(1));
    }
    if options.accurate_energy && !cfg!(target_os = "macos") {
        eprint!(
            "{BRAND}: --energy=accurate is macOS-only — it reads Energy Impact, which this platform does not provide.\n        Drop the flag to use the CPU-time estimate.\n"
        );
        return Some(ExitCode::from(2));
    }
    None
}

/// The dashboard, or the one-shot summary.
///
/// I-22: no dashboard unless there is a real terminal on BOTH ends. stdout
/// alone is not enough — taking over the keyboard needs raw mode on stdin, and
/// when stdin is a pipe or /dev/null (`useful-system-monitor < /dev/null`, or
/// the process backgrounded from a script) that fails. Falling back to one-shot
/// output is both more useful and more composable.
fn run(options: &Options) -> Result<(), String> {
    let stdout_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let stdin_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if stdout_tty && stdin_tty && !options.json {
        return tui::run(options);
    }
    if stdout_tty && !stdin_tty && !options.json {
        // I-24: say why the dashboard did not appear, and how to get it.
        eprint!(
            "{BRAND}: stdin is not a terminal, so the interactive dashboard is unavailable.\n        Showing a one-shot summary. Run it directly from a shell for the TUI.\n"
        );
    }
    oneshot::run(options)
}

/// The two options that print and exit rather than sampling anything.
fn print_and_exit(options: &Options) -> Option<ExitCode> {
    if options.help {
        print!("{HELP}");
        return Some(ExitCode::SUCCESS);
    }
    if options.version {
        println!("{VERSION}");
        return Some(ExitCode::SUCCESS);
    }
    None
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let options = match parse_args(&argv) {
        ParseResult::Ok(o) => o,
        ParseResult::Err(e) => {
            // I-24: cause and remedy, to stderr, with a usage exit code of its
            // own so a script can tell a typo apart from a machine it could
            // not read.
            eprint!("{BRAND}: {e}\n        Run `{BRAND} --help` for the full list of options.\n");
            return ExitCode::from(2);
        }
    };

    if let Some(code) = print_and_exit(&options) {
        return code;
    }

    if let Some(code) = platform_refusal(&options) {
        return code;
    }

    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // I-24: errors to stderr, non-zero exit.
            eprintln!("{BRAND}: {e}");
            ExitCode::from(1)
        }
    }
}
