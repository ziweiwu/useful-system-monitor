import { render } from 'ink-testing-library';
import { describe, expect, it } from 'vitest';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { DEFAULT_TIERS, type MetricsProvider } from '../src/providers/types.js';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Records when each collector ran, so the schedule can be asserted directly. */
class CountingProvider extends MockProvider {
  readonly cpuAt: number[] = [];
  readonly procAt: number[] = [];
  readonly memAt: number[] = [];
  private readonly t0 = Date.now();

  override async cpu() {
    this.cpuAt.push(Date.now() - this.t0);
    return super.cpu();
  }
  override async processes(limit?: number) {
    this.procAt.push(Date.now() - this.t0);
    return super.processes(limit);
  }
  override async memory() {
    this.memAt.push(Date.now() - this.t0);
    return super.memory();
  }
}

describe('I-29: the dashboard shows real numbers without waiting out a tier', () => {
  it('samples the delta-based collectors twice, long before the tier elapses', async () => {
    const provider = new CountingProvider();
    const prevCols = process.stdout.columns;
    const prevRows = process.stdout.rows;
    process.stdout.columns = 100;
    process.stdout.rows = 30;
    const app = render(
      <App provider={provider as MetricsProvider} tiers={DEFAULT_TIERS} demo killFn={() => {}} />,
    );
    try {
      await wait(2_000);

      // CPU% is a delta, so two samples are what turns "—" into a number.
      expect(provider.procAt.length).toBeGreaterThanOrEqual(2);
      expect(provider.cpuAt.length).toBeGreaterThanOrEqual(2);
      /* The claim is "not a whole tier later", not a stopwatch reading: the
         tier is 10s, and a loaded CI runner is allowed to be slow. */
      expect(provider.procAt[1]).toBeLessThan(DEFAULT_TIERS.processes / 2);
      expect(provider.cpuAt[1]).toBeLessThan(DEFAULT_TIERS.cpu / 2);

      // ...and it is one extra sample, not a faster tier. A third would mean
      // the priming timer had become an interval.
      expect(provider.procAt.length).toBeLessThanOrEqual(2);
      expect(provider.cpuAt.length).toBeLessThanOrEqual(2);

      // Collectors that are not deltas are left on their own tier: memory is an
      // absolute reading and is correct from its first sample.
      expect(provider.memAt.length).toBe(1);
    } finally {
      app.unmount();
      process.stdout.columns = prevCols;
      process.stdout.rows = prevRows;
    }
  });

  it('stops the priming timer when the app unmounts', async () => {
    const provider = new CountingProvider();
    const app = render(
      <App provider={provider as MetricsProvider} tiers={DEFAULT_TIERS} demo killFn={() => {}} />,
    );
    await wait(100);
    app.unmount();
    const after = provider.procAt.length;
    await wait(1_000);
    expect(provider.procAt.length).toBe(after);
  });
});
