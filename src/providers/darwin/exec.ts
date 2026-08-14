import { execFile } from 'node:child_process';

/**
 * The environment collectors are spawned with.
 *
 * Every collector here is a text parser pointed at a command that formats its
 * output through the C library's locale, so inheriting the user's locale makes
 * the output unparseable on any Mac that is not set to English:
 *
 *   - `ps -o lstart` runs the start time through strftime. Under LC_TIME=de_DE
 *     it prints "Mi. 12 Aug. 19:39:58 2026", under en_GB "Wed 12 Aug ...", and
 *     under zh_CN "三  8月/12 ..." — none of which match the five-token
 *     "Wed Aug 12 19:39:58 2026" that `parsePsStatic` reads. The parse yielded
 *     nothing, so every row in the table fell back to "pid 1234" with user "?",
 *     and, because an unnameable process is treated as protected (I-14), the
 *     kill path was silently disabled for the entire machine.
 *   - `sysctl vm.swapusage` prints through LC_NUMERIC: "total = 1024,00M" in
 *     every comma-decimal locale, which `parseMemory` read as 0 B of swap.
 *
 * LC_ALL has to be *removed* rather than overridden, because it outranks the
 * individual categories: setting LC_TIME=C does nothing for a user who exports
 * LC_ALL=de_DE.UTF-8.
 *
 * LC_CTYPE is deliberately left as the user set it. Forcing the whole locale to
 * C would also force the character encoding, and a process whose name is not
 * ASCII should still come back as UTF-8 rather than escaped.
 */
export function collectorEnv(base: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { ...base, LC_TIME: 'C', LC_NUMERIC: 'C' };
  delete env['LC_ALL'];
  return env;
}

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
      /* Read per call rather than cached at import: execFile copies the
         environment into the child anyway, so a spread here is free next to a
         spawn, and caching it would make the locale depend on import order. */
      { timeout: timeoutMs, maxBuffer: 8 * 1024 * 1024, encoding: 'utf8', env: collectorEnv() },
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
