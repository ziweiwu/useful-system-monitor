/**
 * Sweeps every screen and mode across a grid of terminal sizes and fails if any
 * frame is taller or wider than the terminal it was given. See I-19, I-26.
 *
 * The test suite asserts this at a handful of sizes; this covers the grid. It
 * is the instrument that caught the row-wrapping and card-floor bugs, and it is
 * cheap enough to run before a release.
 *
 * Usage:
 *   npm run verify:layout                 # the standard grid
 *   COLS=44,80,140 ROWS=8,24,40 npm run verify:layout
 */
import { render } from 'ink-testing-library';
import { App } from '../src/app.js';
import { displayWidth } from '../src/core/width.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { Tiers } from '../src/providers/types.js';

/* Compressed so history fills and the frame settles in a fraction of a tier. */
const FAST: Tiers = { cpu: 90, memory: 300, processes: 300, battery: 600, disk: 1500 };
const ESC = String.fromCharCode(27);
const ENTER = '\r';
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

const nums = (env: string | undefined, fallback: number[]): number[] =>
  env ? env.split(',').map((n) => Number(n.trim())).filter(Number.isFinite) : fallback;

/* Deliberately awkward: below the minimum, exactly at it, and either side of
   each width where a column or a card is given up. */
const COLS = nums(process.env['COLS'], [44, 49, 50, 53, 58, 62, 66, 70, 72, 76, 80, 88, 104, 140]);
const ROWS = nums(process.env['ROWS'], [8, 10, 11, 14, 17, 20, 24, 30, 41]);

/** Every screen, plus the two modes that take over the whole frame. */
const MODES: Array<[string, string[]]> = [
  ['overview', ['1']],
  ['cpu', ['2']],
  ['memory', ['3']],
  ['battery', ['4']],
  ['disk', ['5']],
  ['detail', ['1', ENTER]],
  ['kill', ['1', 'k']],
  ['kill-refused', ['1', '/', 'WindowServer', ENTER, 'k']],
  ['filter', ['1', '/', 'e']],
  ['expanded', ['1', '+', '+']],
];

async function main(): Promise<void> {
  const failures: string[] = [];
  const renderErrors: string[] = [];
  const realError = console.error;
  console.error = (...a: unknown[]) => renderErrors.push(a.map(String).join(' '));
  let checked = 0;

  for (const columns of COLS) {
    for (const rows of ROWS) {
      for (const [label, keys] of MODES) {
        process.stdout.columns = columns;
        process.stdout.rows = rows;
        const app = render(<App provider={new MockProvider()} tiers={FAST} demo killFn={() => {}} />);
        await wait(220);
        for (const k of keys) {
          app.stdin.write(k);
          await wait(90);
        }
        await wait(90);

        const lines = (app.lastFrame() ?? '').replace(ANSI, '').split('\n');
        const height = lines.length;
        const width = Math.max(0, ...lines.map(displayWidth));
        checked++;
        if (height > rows || width > columns) {
          failures.push(
            `${columns}x${rows} ${label}: ${height} rows (max ${rows}), ${width} cells (max ${columns})`,
          );
        }
        /* An infinite render loop shows up here, not as an overflow. */
        if (renderErrors.length) {
          failures.push(`${columns}x${rows} ${label}: ${renderErrors[0]!.slice(0, 100)}`);
          renderErrors.length = 0;
        }
        app.unmount();
      }
    }
    realError(`  ${columns} columns: done (${failures.length} failures so far)`);
  }

  console.error = realError;
  console.log(`\nchecked ${checked} frames across ${COLS.length} widths and ${ROWS.length} heights`);
  if (failures.length) {
    console.error(`\n${failures.length} FAILURES:`);
    for (const f of failures.slice(0, 40)) console.error(`  ${f}`);
    if (failures.length > 40) console.error(`  … and ${failures.length - 40} more`);
    process.exit(1);
  }
  console.log('layout: PASS — nothing overflowed its terminal');
  process.exit(0);
}

void main();
