import { describe, expect, it } from 'vitest';
import { CpuDeltaTracker } from '../src/core/deltas.js';
import { PRIMING_DELAY_MS } from '../src/hooks/useSampler.js';
import { DEFAULT_TIERS } from '../src/providers/types.js';

/*
 * I-1 says the first observation of a PID yields null, rendered "—", never 0.
 * That is correct, and it has a consequence the app used to leave to the user:
 * the *second* observation is the first one that can show a number, and it
 * arrived a whole tier later.
 *
 * On the default 10s tier that meant the dashboard opened with every CPU%
 * reading "—" and the CPU gauge reading 0.0% for ten seconds — which is the
 * entire time most people look at it. `useSampler` now takes one extra early
 * sample of the two delta-based collectors.
 */
describe('I-29: the first screen is not empty for a whole tier', () => {
  it('primes well inside a second, not on the next tick', () => {
    expect(PRIMING_DELAY_MS).toBeLessThan(1_000);
    expect(PRIMING_DELAY_MS).toBeLessThan(DEFAULT_TIERS.processes);
    expect(PRIMING_DELAY_MS).toBeLessThan(DEFAULT_TIERS.cpu);
  });

  it('leaves a window ps can still resolve', () => {
    /*
     * `ps` reports CPU time to a centisecond, so the delta over the priming
     * window quantises to 10ms / PRIMING_DELAY_MS. Below ~500ms that coarsens
     * past the point where a quiet process is distinguishable from an idle one.
     */
    const quantisationPercent = (10 / PRIMING_DELAY_MS) * 100;
    expect(quantisationPercent).toBeLessThan(2);
  });

  it('a second sample one priming window later yields a real percentage', () => {
    const tracker = new CpuDeltaTracker(100 * 10);
    const t0 = 1_000_000;
    const proc = { pid: 42, ppid: 1, rssBytes: 1024 };

    // I-1: first sight is null, whatever the window.
    expect(tracker.update([{ ...proc, cpuTimeMs: 5_000 }], t0).get(42)).toBeNull();

    // 70ms of CPU time over the 700ms priming window is 10% of one core.
    const primed = tracker.update([{ ...proc, cpuTimeMs: 5_070 }], t0 + PRIMING_DELAY_MS);
    expect(primed.get(42)).toBeCloseTo(10, 5);
  });
});
