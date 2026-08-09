/**
 * I-9b: sysmon must not appear in its own top-20 energy consumers while idle.
 * Runs the real render loop at default tiers, then asks its own provider where
 * this process ranks.
 */
import { render } from 'ink-testing-library';
import { App } from '../src/app.js';
import { DarwinProvider } from '../src/providers/darwin/provider.js';
import { DEFAULT_TIERS } from '../src/providers/types.js';

process.stdout.columns = 104;
process.stdout.rows = 32;

const RUN_MS = Number(process.env['RUN_MS'] ?? 45_000);
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

const main = async () => {
  const provider = new DarwinProvider();
  const app = render(<App provider={provider} tiers={DEFAULT_TIERS} />);

  const cpu0 = process.cpuUsage();
  const t0 = Date.now();
  await wait(RUN_MS);
  const elapsed = (Date.now() - t0) / 1000;
  const d = process.cpuUsage(cpu0);
  const selfMs = (d.user + d.system) / 1000;

  app.unmount();

  const probe = new DarwinProvider();
  await probe.processes();
  await wait(1200);
  const data = await probe.processes();

  const ranked = data.visible.toSorted((a, b) => (b.energy ?? 0) - (a.energy ?? 0));
  const rank = ranked.findIndex((p) => p.pid === process.pid);

  console.log(`ran ${elapsed.toFixed(0)}s at default tiers (cpu ${DEFAULT_TIERS.cpu/1000}s, proc ${DEFAULT_TIERS.processes/1000}s, batt ${DEFAULT_TIERS.battery/1000}s)`);
  console.log(`self CPU: ${selfMs.toFixed(0)} ms over ${elapsed.toFixed(0)}s = ${(selfMs / elapsed).toFixed(2)} ms/s = ${(selfMs / elapsed / 10).toFixed(2)}% of one core`);
  console.log(`  (self only; child ps/ioreg cost is measured separately in test/cost.test.ts)`);
  console.log(`rank in own top-${ranked.length} by energy: ${rank < 0 ? 'ABSENT' : `#${rank + 1}`}`);
  console.log(`I-9b: ${rank < 0 || rank >= 20 ? 'PASS' : 'FAIL'}`);
  console.log('\ntop 5 by energy for context:');
  for (const p of ranked.slice(0, 5)) {
    console.log(`  ${String(p.pid).padEnd(7)} ${(p.energy ?? 0).toFixed(1).padStart(5)}  ${p.command.split('/').pop()}`);
  }
  process.exit(0);
};

void main();
