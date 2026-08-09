import { DarwinProvider } from '../src/providers/darwin/provider.js';
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const main = async () => {
  const p = new DarwinProvider({ accurateEnergy: true });
  const first = await p.processes();
  console.log(`tick 1: energyAccurate=${first.energyAccurate} (slow lane still in flight)`);
  await wait(6000);
  const second = await p.processes();
  console.log(`tick 2: energyAccurate=${second.energyAccurate}`);
  const top = second.visible.toSorted((a, b) => (b.energy ?? 0) - (a.energy ?? 0)).slice(0, 6);
  console.log('\n  PID    ENERGY   CPU%   NAME');
  for (const t of top) {
    console.log(
      `  ${String(t.pid).padEnd(7)}${(t.energy ?? 0).toFixed(1).padStart(6)}  ${(t.cpuPercent ?? 0).toFixed(1).padStart(5)}   ${t.command.split('/').pop()}`,
    );
  }
  console.log('\n(energy differs from CPU% => measured, not the estimate)');
  process.exit(0);
};
void main();
