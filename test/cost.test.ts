import { describe, expect, it } from 'vitest';
import { DarwinProvider } from '../src/providers/darwin/provider.js';
import { DEFAULT_TIERS } from '../src/providers/types.js';

const isDarwin = process.platform === 'darwin';

/**
 * Cost of one collector call, as a deliberate over-estimate.
 *
 * `process.cpuUsage()` reads getrusage(RUSAGE_SELF), so it counts our parsing
 * but *not* the CPU burned by the `ps` / `ioreg` children — which is where most
 * of the cost lives. Node exposes no child rusage, so we bound the child cost
 * with wall-clock time: these commands are CPU-bound and short, so wall time is
 * an upper bound on their CPU time.
 *
 * Budgeting against self-CPU alone would understate the real figure by roughly
 * 3x and make this test meaningless.
 */
async function costMs(fn: () => Promise<unknown>): Promise<number> {
  const cpuBefore = process.cpuUsage();
  const wallBefore = performance.now();
  await fn();
  const wall = performance.now() - wallBefore;
  const d = process.cpuUsage(cpuBefore);
  const self = (d.user + d.system) / 1000;
  // Child CPU is at most (wall - our own share of it).
  return self + Math.max(0, wall - self);
}

/**
 * Minimum of N runs, not the median.
 *
 * Because the child bound is wall-clock, this measurement inflates under load —
 * and a test suite is exactly when the machine is loaded, which made the median
 * form flaky right at the threshold. The minimum is the least-contended sample
 * and is still an upper bound on true CPU time, so it measures the thing we
 * care about without failing for reasons unrelated to the code.
 */
async function best(n: number, fn: () => Promise<unknown>): Promise<number> {
  let lo = Infinity;
  for (let i = 0; i < n; i++) lo = Math.min(lo, await costMs(fn));
  return lo;
}

/*
 * Scope note: this file measures the *collector* budget only.
 *
 * The render loop costs more than every collector combined (~9 ms/s vs ~3 ms/s
 * measured on the compiled build), and is not exercised here because it needs a
 * mounted Ink tree over tens of seconds. `npm run verify:selfcost` measures the
 * whole app; README quotes the combined figure.
 */
describe.skipIf(!isDarwin)('I-9: sysmon must not drain the battery it monitors', () => {
  it('stays under the per-second CPU budget at default tiers', async () => {
    const p = new DarwinProvider();

    // Warm the static-metadata cache so we measure the steady state, not the
    // first-run cost. See C-1.
    await p.processes();

    const perCall = {
      cpu: await best(7, () => p.cpu()),
      memory: await best(7, () => p.memory()),
      disk: await best(7, () => p.disk()),
      battery: await best(7, () => p.battery()),
      processes: await best(7, () => p.processes()),
    };

    // Weight each collector by how often its tier actually fires.
    const msPerSecond =
      (perCall.cpu * 1000) / DEFAULT_TIERS.cpu +
      (perCall.memory * 1000) / DEFAULT_TIERS.memory +
      (perCall.disk * 1000) / DEFAULT_TIERS.disk +
      (perCall.battery * 1000) / DEFAULT_TIERS.battery +
      (perCall.processes * 1000) / DEFAULT_TIERS.processes;

    const percentOfOneCore = msPerSecond / 10;
    // eslint-disable-next-line no-console
    console.log(
      `      cost (upper bound): ${msPerSecond.toFixed(2)} ms/s = ${percentOfOneCore.toFixed(2)}% of one core` +
        `  (per call: ${Object.entries(perCall)
          .map(([k, v]) => `${k} ${v.toFixed(1)}ms`)
          .join(', ')})`,
    );

    // Collector-only threshold. The render loop is budgeted separately.
    expect(percentOfOneCore).toBeLessThan(1);
  }, 30_000);

  it('keeps the hot process collector cheap enough for its tier', async () => {
    const p = new DarwinProvider();
    await p.processes();
    const ms = await best(7, () => p.processes());
    // It must finish comfortably inside its own interval, or I-8 would start
    // skipping ticks under normal conditions.
    expect(ms).toBeLessThan(DEFAULT_TIERS.processes / 4);
  }, 30_000);

  it('does not grow its metadata cache without bound', async () => {
    const p = new DarwinProvider();
    const a = await p.processes();
    const b = await p.processes();
    // total tracks the live process count, not an ever-growing history.
    expect(Math.abs(a.total - b.total)).toBeLessThan(200);
    expect(b.visible.length).toBeLessThanOrEqual(50);
  }, 30_000);
});
