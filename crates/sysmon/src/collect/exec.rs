//! Running the collector commands.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

/// The environment collectors are spawned with.
///
/// Every collector is a text parser pointed at a command that formats its
/// output through the C library's locale, so inheriting the user's locale makes
/// the output unparseable on any Mac that is not set to English:
///
/// - `ps -o lstart` runs the start time through `strftime`. Under
///   `LC_TIME=de_DE` it prints "Mi. 12 Aug. 19:39:58 2026", under `zh_CN`
///   "三  8月/12 …" — neither of which is the five-token
///   "Wed Aug 12 19:39:58 2026" the parser reads. The parse yielded nothing, so
///   every row fell back to "pid 1234" with user "?", and because an unnameable
///   process is treated as protected (I-14), **the kill path silently disabled
///   itself for the entire machine**.
/// - `sysctl vm.swapusage` prints through `LC_NUMERIC`: "total = 1024,00M" in
///   every comma-decimal locale, which was read as 0 B of swap.
///
/// `LC_ALL` has to be **removed** rather than overridden, because it outranks
/// the individual categories: setting `LC_TIME=C` does nothing for a user who
/// exports `LC_ALL=de_DE.UTF-8`.
///
/// `LC_CTYPE` is deliberately left as the user set it. Forcing the whole locale
/// to C would also force the character encoding, and a process whose name is not
/// ASCII should still come back as UTF-8 rather than escaped.
///
/// See I-28.
pub fn collector_env(base: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = base.clone();
    env.insert("LC_TIME".into(), "C".into());
    env.insert("LC_NUMERIC".into(), "C".into());
    env.remove("LC_ALL");
    env
}

/// A collector command that failed.
///
/// `exit_code` is the command's own status when it ran and exited non-zero, and
/// `None` when it never ran or was killed — a missing binary, or the timeout.
///
/// That difference is not decoration. `ps -p PID` exits 1 with no output when
/// no process matches, which is a real answer meaning "the process is gone"; a
/// timeout is *no answer at all*. Collapsing both into one error made the kill
/// path tell the user a live process "has already exited". See I-16.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub message: String,
    pub exit_code: Option<i32>,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs a command by absolute path with no shell.
///
/// Absolute paths matter: an interactive shell can alias these names, and going
/// through a shell would pick the alias up. No shell also removes any quoting
/// concerns.
///
/// # The deadlock this avoids
///
/// The obvious shape — `wait_timeout()` and then read stdout — deadlocks
/// exactly when it matters most. macOS pipe buffers are 64 KB and
/// `ps -Ao pid,ppid,time,rss` on a busy machine is ~50 KB and climbing, so the
/// child blocks writing while the parent blocks waiting, and neither the
/// timeout nor the command ever completes. The buffer being *nearly* big enough
/// is the worst case: it works on a quiet machine and hangs on a loaded one,
/// which is the machine someone actually runs this on.
///
/// So the read happens on its own thread, and the timeout applies to the wait.
/// The collector, started with a scrubbed environment so a shell alias or a
/// locale setting cannot change what it prints.
fn spawn(path: &str, args: &[&str]) -> Result<std::process::Child, CommandError> {
    let env = collector_env(&std::env::vars().collect());
    Command::new(path)
        .args(args)
        .env_clear()
        .envs(&env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => CommandError {
                message: format!("{path} not found on this system"),
                exit_code: None,
            },
            _ => CommandError {
                message: format!("{path} failed: {e}"),
                exit_code: None,
            },
        })
}

pub fn run(path: &str, args: &[&str], timeout: Duration) -> Result<String, CommandError> {
    let mut child = spawn(path, args)?;

    let mut stdout = child.stdout.take().expect("stdout was piped");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let status = child.wait_timeout(timeout).map_err(|e| CommandError {
        message: format!("{path} failed: {e}"),
        exit_code: None,
    })?;

    let Some(status) = status else {
        // No answer at all, which is not the same as a command that ran and
        // said no. Killing it also closes the pipe, so the reader can finish.
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        return Err(CommandError {
            message: format!("{path} timed out after {}s", timeout.as_secs()),
            exit_code: None,
        });
    };

    let out = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(CommandError {
            message: format!(
                "{path} failed: exited with status {}",
                status.code().unwrap_or(-1)
            ),
            exit_code: status.code(),
        });
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Absolute paths, so a shell alias cannot redirect a collector.
pub mod bin {
    pub const PS: &str = "/bin/ps";
    pub const VM_STAT: &str = "/usr/bin/vm_stat";
    pub const SYSCTL: &str = "/usr/sbin/sysctl";
    pub const DF: &str = "/bin/df";
    pub const PMSET: &str = "/usr/bin/pmset";
    /// Note: `ioreg` lives in /usr/sbin, not /usr/bin.
    pub const IOREG: &str = "/usr/sbin/ioreg";
    pub const TOP: &str = "/usr/bin/top";
}
