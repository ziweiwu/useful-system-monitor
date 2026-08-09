import type { OthersRollup, ProcessSample } from './types.js';

export const WORKING_SET_SIZE = 50;

/**
 * How far `+` / `-` can expand the working set, smallest first.
 *
 * The first three steps are free, and measurably so. `ps -Ao pid,ppid,time,rss`
 * already returns every process and the provider already builds a ProcessSample
 * for each, so a wider cut spawns nothing extra; and because the table is
 * windowed to the terminal height (I-26), it renders no extra rows either.
 * Measured median cost of one `processes()` call, interleaved A/B on a loaded
 * machine: cap 50 = 27.3ms, 150 = 25.1ms, 300 = 26.5ms — all within noise.
 *
 * `all` is the exception, at 90.4ms (+0.63% of one core at the 10s tier). The
 * cost is not the extra rows: it is that the *static* `ps` (which resolves
 * executable paths and maps UIDs, and costs ~2x the hot one) is refetched
 * whenever a visible process has no cached name. With every process visible,
 * any newly spawned helper triggers that refetch, so it fires on essentially
 * every tick instead of rarely. See I-9c; the UI labels the step accordingly.
 *
 * `Infinity` means every process; `selectWorkingSet` short-circuits on it.
 */
export const WORKING_SET_STEPS: readonly number[] = [50, 150, 300, Infinity];

/** Human label for a step, for the footer and the roll-up row. */
export function stepLabel(n: number): string {
  return Number.isFinite(n) ? String(n) : 'all';
}

/**
 * Cost warning for a step, or '' when the step is free.
 *
 * Shown for the same reason `--energy=accurate` and the "est" suffix exist: a
 * tool whose whole claim is that it does not drain your battery has to say so
 * when you ask it to cost more. See I-9c.
 */
export function stepCostNote(n: number): string {
  return Number.isFinite(n) ? '' : ' (+0.6% cpu)';
}

/**
 * Processes outside the top `n` are counted but never materialised: no history
 * ring buffer, no render work. See C-7. `n` defaults to 50 and is raised at
 * runtime by `+` (see WORKING_SET_STEPS).
 *
 * The union of top-N-by-CPU and top-N-by-RSS is deliberate — a memory hog at
 * 0% CPU (a leaked Electron helper) is exactly the thing you want to find, and
 * a CPU-only cut would hide it.
 *
 * Note this trims *display*, not *tracking*: CpuDeltaTracker still sees every
 * PID (I-4b), so a process entering the set already has a real CPU% rather than
 * showing "—" for a full interval.
 */
export function selectWorkingSet(
  all: readonly ProcessSample[],
  n: number = WORKING_SET_SIZE,
): { visible: ProcessSample[]; others: OthersRollup } {
  if (all.length <= n) {
    return { visible: [...all], others: { count: 0, cpuPercent: 0, rssBytes: 0, energy: 0 } };
  }

  // Half the budget to each list so the union stays within `n`. Taking the top
  // `n` of both would yield up to 2n rows.
  const half = Math.max(1, Math.floor(n / 2));
  const byCpu = all.toSorted((a, b) => (b.cpuPercent ?? -1) - (a.cpuPercent ?? -1)).slice(0, half);
  const byMem = all.toSorted((a, b) => b.rssBytes - a.rssBytes).slice(0, half);

  const keep = new Set<number>();
  for (const p of byCpu) keep.add(p.pid);
  for (const p of byMem) keep.add(p.pid);

  const visible: ProcessSample[] = [];
  const others: OthersRollup = { count: 0, cpuPercent: 0, rssBytes: 0, energy: 0 };

  for (const p of all) {
    if (keep.has(p.pid)) {
      visible.push(p);
    } else {
      // C-9: the tail is aggregated, so the table still reconciles with the
      // CPU and MEM cards instead of silently under-reporting.
      others.count++;
      others.cpuPercent += p.cpuPercent ?? 0;
      others.rssBytes += p.rssBytes;
      others.energy += p.energy ?? 0;
    }
  }

  return { visible, others };
}
