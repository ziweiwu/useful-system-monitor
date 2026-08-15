import type { RawProcess } from './types.js';

/**
 * Turns cumulative CPU-time counters into instantaneous CPU%.
 *
 * `ps` reports lifetime-average %CPU, which cannot answer "what is hogging the
 * CPU right now". Its TIME column has centisecond resolution, so sampling twice
 * and dividing by the wall-clock gap gives true instantaneous utilisation.
 *
 * Invariants: I-1 (always a delta, null on first sight), I-3 (monotonicity /
 * PID-reuse detection), I-4b (tracks every PID, not just the working set).
 */

interface Prev {
  cpuTimeMs: number;
  at: number;
}

export interface DeltaResult {
  /** Raw CPU%, in [0, 100 * ncpu]. Normalised only at render. See I-2. */
  cpuPercent: number | null;
}

export class CpuDeltaTracker {
  private prev = new Map<number, Prev>();

  /**
   * PIDs whose cumulative counter went *backwards* on the last update.
   *
   * That is the signature of PID reuse (I-3), and it is different from a PID
   * simply being seen for the first time — both yield a null CPU%, but only
   * this one means "the thing behind this number is a different program now".
   * Exposed because anything else caching per-PID has to hear about it: a name
   * cache keyed on `has(pid)` will happily keep showing the dead process's
   * name, which is what it used to do.
   */
  readonly recycled = new Set<number>();

  constructor(private readonly maxPercent: number) {}

  /**
   * @param procs current raw sample
   * @param now   wall-clock ms for this sample
   * @returns pid -> CPU% (null when there is no usable previous sample)
   */
  update(procs: readonly RawProcess[], now: number): Map<number, number | null> {
    const out = new Map<number, number | null>();
    const next = new Map<number, Prev>();
    this.recycled.clear();

    for (const p of procs) {
      const prev = this.prev.get(p.pid);
      next.set(p.pid, { cpuTimeMs: p.cpuTimeMs, at: now });

      if (!prev) {
        // I-1: first observation yields null, never 0.
        out.set(p.pid, null);
        continue;
      }

      const dt = now - prev.at;
      if (dt <= 0) {
        out.set(p.pid, null);
        continue;
      }

      const dCpu = p.cpuTimeMs - prev.cpuTimeMs;
      if (dCpu < 0) {
        // I-3: cumulative CPU time went backwards -> this PID was recycled.
        // Discard the delta and treat it as a process we have not seen.
        this.recycled.add(p.pid);
        out.set(p.pid, null);
        continue;
      }

      // I-2: clamp to the physical ceiling; scheduler jitter can otherwise
      // produce a hair over 100% per core across a short window.
      const pct = Math.min((dCpu / dt) * 100, this.maxPercent);
      out.set(p.pid, pct);
    }

    // I-4b/I-10: drop entries for processes that have exited so the map tracks
    // live PIDs only and cannot grow without bound.
    this.prev = next;
    return out;
  }

  /** Number of tracked PIDs. Exposed for the memory-bound test. */
  get trackedCount(): number {
    return this.prev.size;
  }

  reset(): void {
    this.prev.clear();
    this.recycled.clear();
  }
}

/**
 * Per-core utilisation from two `os.cpus()` samples.
 * System CPU is the mean across cores and never a sum of process rows (I-6),
 * because kernel_task (PID 0) is invisible to `ps`.
 */
export interface CpuTimes {
  user: number;
  nice: number;
  sys: number;
  idle: number;
  irq: number;
}

export function coreUtilisation(prev: readonly CpuTimes[], cur: readonly CpuTimes[]): number[] {
  const out: number[] = [];
  for (let i = 0; i < cur.length; i++) {
    const a = prev[i];
    const b = cur[i];
    if (!a || !b) {
      out.push(0);
      continue;
    }
    const dIdle = b.idle - a.idle;
    const dTotal =
      b.user - a.user + (b.nice - a.nice) + (b.sys - a.sys) + dIdle + (b.irq - a.irq);
    if (dTotal <= 0) {
      out.push(0);
      continue;
    }
    // I-2: clamp into [0, 100].
    out.push(Math.max(0, Math.min(100, (1 - dIdle / dTotal) * 100)));
  }
  return out;
}
