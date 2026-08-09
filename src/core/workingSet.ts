import type { OthersRollup, ProcessSample } from './types.js';

export const WORKING_SET_SIZE = 50;

/**
 * Processes outside the top 50 are counted but never materialised: no history
 * ring buffer, no render work. See C-7.
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
