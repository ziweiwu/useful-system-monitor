import type { ProcessSample } from './types.js';

export type SortKey = 'cpu' | 'mem' | 'energy';

/**
 * Energy-impact proxy.
 *
 * macOS's real "Energy Impact" is only available from `top -stats power`, which
 * costs ~1s of CPU per call — 100ms/s even on a slow lane, which would dominate
 * the entire budget of this app (see the Cost budget in the plan). CPU time is
 * the dominant term in Apple's own formula, so we derive a proxy from data we
 * already have for free, and label it as an estimate in the UI.
 *
 * Kept as a named function so `--energy=accurate` can swap in real numbers
 * without touching the table, the sort, or the battery view.
 */
export function energyProxy(cpuPercent: number | null): number | null {
  if (cpuPercent === null) return null;
  return cpuPercent;
}

/** Estimated watts attributable to a process, given the current total draw. */
export function estimateWatts(
  energy: number | null,
  totalEnergy: number,
  totalWatts: number | null,
): number | null {
  if (energy === null || totalWatts === null || totalEnergy <= 0) return null;
  return (energy / totalEnergy) * Math.abs(totalWatts);
}

function keyValue(p: ProcessSample, key: SortKey): number {
  switch (key) {
    case 'cpu':
      return p.cpuPercent ?? -1;
    case 'mem':
      return p.rssBytes;
    case 'energy':
      return p.energy ?? -1;
  }
}

/**
 * Total order, descending by `key`, ties broken by ascending PID.
 *
 * Totality matters: with only the metric as key, two processes at an identical
 * value can swap places between frames and the table visibly jitters. See I-20.
 */
export function comparator(key: SortKey): (a: ProcessSample, b: ProcessSample) => number {
  return (a, b) => {
    const d = keyValue(b, key) - keyValue(a, key);
    if (d !== 0) return d;
    return a.pid - b.pid;
  };
}

export function sortProcesses(procs: readonly ProcessSample[], key: SortKey): ProcessSample[] {
  return procs.toSorted(comparator(key));
}
