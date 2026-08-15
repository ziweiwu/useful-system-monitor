import { render } from 'ink-testing-library';
import { describe, expect, it } from 'vitest';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { Tiers } from '../src/providers/types.js';

const FAST: Tiers = { cpu: 90, memory: 300, processes: 300, battery: 600, disk: 1500 };
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

async function listRows(cols: number, rows: number, key: string, match: RegExp): Promise<number> {
  const prev = [process.stdout.columns, process.stdout.rows] as const;
  process.stdout.columns = cols;
  process.stdout.rows = rows;
  const app = render(<App provider={new MockProvider()} tiers={FAST} demo killFn={() => {}} />);
  try {
    await wait(350);
    app.stdin.write(key);
    await wait(220);
    return (app.lastFrame() ?? '')
      .replace(ANSI, '')
      .split('\n')
      .filter((l) => match.test(l)).length;
  } finally {
    app.unmount();
    process.stdout.columns = prev[0];
    process.stdout.rows = prev[1];
  }
}

/*
 * I-26 says sections are dropped in priority order as the terminal shrinks.
 * The unstated half is that the reverse must hold: **growing** the terminal
 * must never show less.
 *
 * It did. An optional note below a list cost two rows and switched on the
 * moment it became affordable — and a two-row section cannot switch on without
 * the list beneath it losing a row, because it arrives one row later than the
 * row that paid for it. At 70 columns the disk screen showed one volume at 13
 * rows and none at 14; the battery screen showed one consumer at 14 rows and
 * none at 15. Both left a heading, a roll-up, and a note *about* the data they
 * had just stopped showing.
 */
describe('I-26: growing the terminal never shows fewer rows', () => {
  it.each([
    ['disk volumes', 70, '5', /^\s*\/\S*\s+[█░]/, 11, 20],
    ['battery consumers', 80, '4', /^\s*>?\s{2,}\S.*[█░]{3}/, 12, 20],
    ['cpu cores', 80, '2', /^\s*[PE]\d+\s+[█░]/, 10, 20],
  ] as const)('%s', async (_label, cols, key, match, from, to) => {
    const counts: number[] = [];
    for (let r = from; r <= to; r++) counts.push(await listRows(cols, r, key, match));
    for (let i = 1; i < counts.length; i++) {
      expect(
        counts[i],
        `${from + i - 1} rows showed ${counts[i - 1]}, ${from + i} rows showed ${counts[i]}`,
      ).toBeGreaterThanOrEqual(counts[i - 1]!);
    }
    // And the sweep has to actually exercise the transition, or it proves nothing.
    expect(Math.max(...counts)).toBeGreaterThan(Math.min(...counts));
  }, 60_000);
});
