import { execFile } from 'node:child_process';

/**
 * Runs a command by absolute path with no shell.
 *
 * Absolute paths matter: an interactive shell can alias these names (this
 * machine aliases `ps`), and going through a shell would pick the alias up.
 * `execFile` without a shell also removes any quoting concerns.
 */
export function run(
  path: string,
  args: readonly string[],
  timeoutMs = 5_000,
): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(
      path,
      [...args],
      { timeout: timeoutMs, maxBuffer: 8 * 1024 * 1024, encoding: 'utf8' },
      (err, stdout) => {
        if (err) {
          // I-11 / I-24: state which command failed and why, so the panel can
          // show something actionable instead of a bare stack trace.
          const code = (err as NodeJS.ErrnoException).code;
          if (code === 'ENOENT') {
            reject(new Error(`${path} not found on this system`));
            return;
          }
          reject(new Error(`${path} failed: ${err.message.split('\n')[0]}`));
          return;
        }
        resolve(stdout);
      },
    );
  });
}

export const BIN = {
  ps: '/bin/ps',
  vmStat: '/usr/bin/vm_stat',
  sysctl: '/usr/sbin/sysctl',
  df: '/bin/df',
  pmset: '/usr/bin/pmset',
  // Note: ioreg lives in /usr/sbin, not /usr/bin.
  ioreg: '/usr/sbin/ioreg',
  top: '/usr/bin/top',
} as const;
