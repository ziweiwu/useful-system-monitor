import { render } from 'ink-testing-library';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import { displayWidth } from '../src/core/width.js';
import type { Tiers } from '../src/providers/types.js';

process.stdout.columns = Number(process.env['COLS'] ?? 80);
process.stdout.rows = Number(process.env['ROWS'] ?? 24);
const FAST: Tiers = { cpu: 90, memory: 300, processes: 300, battery: 600, disk: 1500 };
const provider = new MockProvider();
const { lastFrame, stdin, unmount } = render(
  <App provider={provider} tiers={FAST} demo={true} killFn={() => {}} />,
);
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const main = async () => {
  await wait(3000);
  for (const key of process.argv.slice(2)) {
    stdin.write(key === 'DOWN' ? String.fromCharCode(27) + '[B' : key);
    await wait(300);
  }
  await wait(300);
  const frame = lastFrame() ?? '';
  const lines = frame.split('\n');
  const cols = Number(process.env['COLS'] ?? 80);
  const rows = Number(process.env['ROWS'] ?? 24);
  console.log(`rows=${lines.length} (budget ${rows}), max width found:`);
  let maxw = 0;
  lines.forEach((l, i) => {
    const w = displayWidth(l);
    if (w > maxw) maxw = w;
    if (w > cols) console.log(`  line ${i} OVERFLOW width=${w}: ${JSON.stringify(l)}`);
  });
  console.log('maxWidth=', maxw, 'cols budget=', cols);
  unmount();
  process.exit(0);
};
void main();
