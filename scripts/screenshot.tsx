/**
 * Renders the mock dashboard to stdout as a static frame, for review.
 * Uses compressed tiers so history buffers fill in seconds rather than minutes.
 *
 * Usage: npx tsx scripts/screenshot.tsx [keys...]
 */
import { render } from 'ink-testing-library';
import { App } from '../src/app.js';
import { DarwinProvider } from '../src/providers/darwin/provider.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { Tiers } from '../src/providers/types.js';

/*
 * FORCE_COLOR has to be set in the *environment*, not here — hence the npm
 * script. Ink and chalk resolve their colour-support level at import time, and
 * ESM evaluates imports before a module's own top-level statements, so an
 * assignment on this line runs too late to have any effect. It silently did
 * nothing: every frame this script printed was colourless, which is a poor
 * property for the tool used to review colour. Measured: 0 escape sequences
 * without it in the environment, 20 with.
 */
process.stdout.columns = Number(process.env['COLS'] ?? 100);
process.stdout.rows = Number(process.env['ROWS'] ?? 34);

const FAST: Tiers = { cpu: 90, memory: 300, processes: 300, battery: 600, disk: 1500 };

const live = process.env['LIVE'] === '1';
const provider = live ? new DarwinProvider() : new MockProvider();
const { lastFrame, stdin, unmount } = render(
  <App provider={provider} tiers={FAST} demo={!live} killFn={live ? undefined : () => {}} />,
);

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

const main = async () => {
  // Let the ring buffers fill so sparklines are meaningful.
  await wait(3000);
  for (const key of process.argv.slice(2)) {
    stdin.write(key === 'DOWN' ? String.fromCharCode(27) + '[B' : key);
    await wait(300);
  }
  await wait(300);
  process.stdout.write((lastFrame() ?? '(no frame)') + '\n');
  unmount();
  process.exit(0);
};

void main();
