import { render } from 'ink-testing-library';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { Tiers } from '../src/providers/types.js';

process.env['FORCE_COLOR'] = '3';
process.stdout.columns = Number(process.env['COLS'] ?? 100);
process.stdout.rows = Number(process.env['ROWS'] ?? 34);

const FAST: Tiers = { cpu: 90, memory: 300, processes: 300, battery: 600, disk: 1500 };
const provider = new MockProvider();
const { lastFrame, unmount } = render(
  <App provider={provider} tiers={FAST} demo={true} killFn={() => {}} />,
);
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const main = async () => {
  await wait(Number(process.env['DELAY'] ?? 0));
  process.stdout.write((lastFrame() ?? '(no frame)') + '\n');
  unmount();
  process.exit(0);
};
void main();
