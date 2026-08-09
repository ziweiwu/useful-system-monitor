import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { CpuDeltaTracker, coreUtilisation, type CpuTimes } from '../src/core/deltas.js';
import type { RawProcess } from '../src/core/types.js';

const proc = (pid: number, cpuTimeMs: number): RawProcess => ({
  pid,
  ppid: 1,
  cpuTimeMs,
  rssBytes: 1024,
});

describe('I-1: CPU% is always a delta, never a lifetime average', () => {
  it('returns null on first observation rather than 0', () => {
    const t = new CpuDeltaTracker(1000);
    const out = t.update([proc(100, 5_000)], 1_000);
    // A process with 5s of accumulated CPU must not read as 0% or as its
    // lifetime average; we simply do not know its rate yet.
    expect(out.get(100)).toBeNull();
  });

  it('computes the rate from the second sample onward', () => {
    const t = new CpuDeltaTracker(1000);
    t.update([proc(100, 1_000)], 0);
    // 500ms of CPU over 1000ms of wall clock = 50%.
    expect(t.update([proc(100, 1_500)], 1_000).get(100)).toBeCloseTo(50, 5);
  });

  it('reports 0 for a live-but-idle process (distinct from unknown)', () => {
    const t = new CpuDeltaTracker(1000);
    t.update([proc(100, 1_000)], 0);
    expect(t.update([proc(100, 1_000)], 1_000).get(100)).toBe(0);
  });
});

describe('I-3: monotonicity and PID reuse', () => {
  it('discards the delta when cumulative CPU time goes backwards', () => {
    const t = new CpuDeltaTracker(1000);
    t.update([proc(100, 60_000)], 0);
    // PID 100 was recycled: the new process has far less accumulated CPU.
    // Diffing would yield a large negative rate.
    expect(t.update([proc(100, 5)], 1_000).get(100)).toBeNull();
  });

  it('recovers on the sample after a reuse event', () => {
    const t = new CpuDeltaTracker(1000);
    t.update([proc(100, 60_000)], 0);
    t.update([proc(100, 5)], 1_000);
    expect(t.update([proc(100, 505)], 2_000).get(100)).toBeCloseTo(50, 5);
  });

  it('drops exited processes so the map cannot grow without bound', () => {
    const t = new CpuDeltaTracker(1000);
    t.update([proc(1, 0), proc(2, 0), proc(3, 0)], 0);
    expect(t.trackedCount).toBe(3);
    t.update([proc(1, 0)], 1_000);
    expect(t.trackedCount).toBe(1);
  });
});

describe('I-2: CPU% stays inside its physical bounds', () => {
  it('never returns a negative or over-ceiling value', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 10_000_000 }),
        fc.integer({ min: 0, max: 10_000_000 }),
        fc.integer({ min: 1, max: 60_000 }),
        (a, b, dt) => {
          const t = new CpuDeltaTracker(1000);
          t.update([proc(1, a)], 0);
          // Map#get is number | null | undefined; every sampled pid must be
          // present, so an undefined here would itself be a bug.
          const v = t.update([proc(1, b)], dt).get(1);
          expect(v).not.toBeUndefined();
          if (v === null || v === undefined) return b < a;
          return v >= 0 && v <= 1000;
        },
      ),
      { numRuns: 500 },
    );
  });
});

const times = (user: number, sys: number, idle: number): CpuTimes => ({
  user,
  nice: 0,
  sys,
  idle,
  irq: 0,
});

describe('I-2 / I-6: per-core utilisation', () => {
  it('derives utilisation from idle share', () => {
    const prev = [times(0, 0, 0)];
    const cur = [times(30, 20, 50)];
    expect(coreUtilisation(prev, cur)[0]).toBeCloseTo(50, 5);
  });

  it('clamps into [0,100] for every generated input', () => {
    fc.assert(
      fc.property(
        fc.array(fc.nat({ max: 100_000 }), { minLength: 5, maxLength: 5 }),
        fc.array(fc.nat({ max: 100_000 }), { minLength: 5, maxLength: 5 }),
        (a, b) => {
          const prev = [times(a[0]!, a[1]!, a[2]!)];
          const cur = [times(b[0]!, b[1]!, b[2]!)];
          const v = coreUtilisation(prev, cur)[0]!;
          return v >= 0 && v <= 100;
        },
      ),
      { numRuns: 300 },
    );
  });

  it('reports 0 rather than NaN when no time has passed', () => {
    const same = [times(10, 10, 10)];
    expect(coreUtilisation(same, same)[0]).toBe(0);
  });
});
