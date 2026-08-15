import { render } from 'ink-testing-library';
import { describe, expect, it } from 'vitest';
import { App } from '../src/app.js';
import { displayWidth } from '../src/core/width.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { ProcessesData } from '../src/core/types.js';
import type { Tiers } from '../src/providers/types.js';

const FAST: Tiers = { cpu: 90, memory: 300, processes: 300, battery: 600, disk: 1500 };
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

/** Poisons the metrics of alternate rows, keeping the rows themselves. */
class PoisonedRows extends MockProvider {
  constructor(private readonly value: number) {
    super();
  }
  override async processes(limit?: number): Promise<ProcessesData> {
    const d = await super.processes(limit);
    return {
      ...d,
      visible: d.visible.map((p, i) => {
        if (i % 2 === 0) return p;
        /* Object.assign onto a fresh object rather than a spread: the lint
           rule is right that spreading in a map is wasteful, and this runs
           once per row per frame. */
        return Object.assign({}, p, {
          cpuPercent: this.value,
          energy: this.value,
          rssBytes: this.value,
        });
      }),
    };
  }
}

async function rowWidths(provider: MockProvider): Promise<number[]> {
  const prev = [process.stdout.columns, process.stdout.rows] as const;
  process.stdout.columns = 100;
  process.stdout.rows = 30;
  const app = render(<App provider={provider} tiers={FAST} demo killFn={() => {}} />);
  try {
    await wait(500);
    return (app.lastFrame() ?? '')
      .replace(ANSI, '')
      .split('\n')
      .filter((l) => /^\s*>?\s*\d{2,}\s/.test(l) && /[█░]/.test(l))
      .map(displayWidth);
  } finally {
    app.unmount();
    process.stdout.columns = prev[0];
    process.stdout.rows = prev[1];
  }
}

/*
 * Every process row is padded to the same width by construction, so a short row
 * means a cell collapsed — which is exactly what a non-finite value does:
 * `'█'.repeat(NaN)` is `''`, silently, and `barCells` returning NaN takes the
 * whole bar with it.
 *
 * Row *equality* is the assertion that catches it. Searching a frame for the
 * text "NaN" cannot: nothing is printed, only the geometry changes. Measured
 * with the clamp in `barCells` removed: rows of 95 and 82 cells side by side.
 */
describe('I-19: every process row is the same width', () => {
  it('with ordinary data', async () => {
    const widths = await rowWidths(new MockProvider());
    expect(widths.length).toBeGreaterThan(1);
    expect(new Set(widths).size, `row widths: ${widths.join(',')}`).toBe(1);
  }, 20_000);

  it.each([
    ['NaN', NaN],
    ['Infinity', Infinity],
    ['-Infinity', -Infinity],
    ['negative', -42],
  ])('when half the rows carry %s metrics', async (_label, value) => {
    const widths = await rowWidths(new PoisonedRows(value));
    expect(widths.length).toBeGreaterThan(1);
    expect(new Set(widths).size, `row widths: ${widths.join(',')}`).toBe(1);
  }, 20_000);
});
