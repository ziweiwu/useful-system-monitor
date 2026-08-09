/**
 * I-9c: what each `+` step actually costs.
 *
 * Interleaved A/B/C/D with a rotating arm order, because the machine this runs
 * on is never idle and a sequential A-then-B comparison drifts more than the
 * effect being measured — an early attempt at exactly that reported the widest
 * cap as *cheaper*, twice, with opposite signs.
 */
import { DarwinProvider } from '../src/providers/darwin/provider.js';
import { stepLabel, WORKING_SET_STEPS } from '../src/core/workingSet.js';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
const N = Number(process.env['N'] ?? 25);
const median = (a: number[]) => a.toSorted((x, y) => x - y)[Math.floor(a.length / 2)]!;

const main = async () => {
  // One provider per arm, so each keeps its own metadata cache like the app does.
  const provs = WORKING_SET_STEPS.map(() => new DarwinProvider());
  for (let i = 0; i < WORKING_SET_STEPS.length; i++) await provs[i]!.processes(WORKING_SET_STEPS[i]!);
  await wait(500);

  const wall: number[][] = WORKING_SET_STEPS.map(() => []);
  for (let i = 0; i < N; i++) {
    for (let j = 0; j < WORKING_SET_STEPS.length; j++) {
      const k = (j + i) % WORKING_SET_STEPS.length;
      const t = performance.now();
      await provs[k]!.processes(WORKING_SET_STEPS[k]!);
      wall[k]!.push(performance.now() - t);
    }
    await wait(300);
  }

  console.log(`n=${N} interleaved rounds of provider.processes()\n`);
  const base = median(wall[0]!);
  let worst = 0;
  for (let i = 0; i < WORKING_SET_STEPS.length; i++) {
    const m = median(wall[i]!);
    const pct = (m - base) / 10 / 10;
    worst = Math.max(worst, pct);
    console.log(
      `cap ${stepLabel(WORKING_SET_STEPS[i]!).padEnd(4)} median ${m.toFixed(1).padStart(6)} ms` +
        `   ${(m - base >= 0 ? '+' : '')}${(m - base).toFixed(1).padStart(5)} ms vs cap 50` +
        `   = ${pct.toFixed(2)}% of one core at the 10s tier`,
    );
  }
  // Wall time here bounds the true CPU cost: the child `ps` never shows up in
  // process.cpuUsage(), so treating the whole wait as busy is the safe reading.
  console.log(`\nI-9c: ${worst < 1 ? 'PASS' : 'FAIL'} — widest step costs ${worst.toFixed(2)}% of one core (budget < 1%)`);
  process.exit(worst < 1 ? 0 : 1);
};
void main();
