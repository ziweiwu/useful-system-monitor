import { BIN, run } from './exec.js';

/**
 * macOS's real per-process "Energy Impact" — the number Activity Monitor shows.
 *
 * Only `top -stats power` exposes it, and it costs ~1.0s of CPU and ~2.0s of
 * wall clock per sample. At a 60s interval that is ~17 ms/s, roughly 5x the
 * entire default budget of this app, which is why it is opt-in via
 * `--energy=accurate` rather than the default.
 */

/** `top -l 2 -o power -stats pid,power`. */
export function parseTopPower(stdout: string): Map<number, number> {
  const out = new Map<number, number>();

  /*
   * `-l 2` prints two sample blocks. The first is always all zeros, because
   * energy impact is itself a rate and top has nothing to compare against yet.
   * Only the final block carries real numbers.
   */
  const headerIdx = stdout.lastIndexOf('PID');
  if (headerIdx < 0) return out;
  const body = stdout.slice(stdout.indexOf('\n', headerIdx) + 1);

  for (const line of body.split('\n')) {
    const m = /^\s*(\d+)\s+([\d.]+)\s*$/.exec(line);
    if (!m) continue;
    out.set(Number(m[1]), Number(m[2]));
  }
  return out;
}

export async function sampleEnergyImpact(rows = 60): Promise<Map<number, number>> {
  // -l 2 is mandatory: a single sample reports 0.0 for everything.
  const stdout = await run(
    BIN.top,
    ['-l', '2', '-o', 'power', '-n', String(rows), '-stats', 'pid,power'],
    15_000,
  );
  return parseTopPower(stdout);
}
