import { describe, expect, it, vi } from 'vitest';

const BIN_STUB = {
  ps: '/bin/ps', vmStat: '/usr/bin/vm_stat', sysctl: '/usr/sbin/sysctl', df: '/bin/df',
  pmset: '/usr/bin/pmset', ioreg: '/usr/sbin/ioreg', top: '/usr/bin/top',
};

/*
 * Two live processes; `top -o power` ranks only one of them. That is the
 * ordinary case, not a contrived one: `top -n 60` measures at most 60 PIDs,
 * while `+` takes the working set to 150, 300 or every process on the machine.
 *
 * PID 200 burns ten seconds of CPU between the two samples, so it comes out
 * with a large CPU% — which is the point. Energy Impact is bounded around 40
 * while `energyProxy` returns CPU% in [0, 100 x ncpu], so an unranked row
 * borrowing the proxy does not merely report a wrong number, it reports one on
 * a scale that outranks every genuinely measured row in the `e` sort.
 */
let hotCalls = 0;
function hot(): string {
  const busy = hotCalls++ === 0 ? '0:04.00' : '0:14.00';
  return `  PID  PPID     TIME    RSS\n  100     1  0:09.00   1000\n  200     1  ${busy}   2000\n`;
}
const STATIC =
  'PID STARTED USER STAT COMM\n' +
  '100 Wed Aug 12 10:00:00 2026 alice S Measured\n' +
  '200 Wed Aug 12 10:00:00 2026 alice S Unmeasured\n';

vi.mock('../src/providers/darwin/exec.js', async () => {
  // The real CommandError, so the provider's `instanceof` check is the one
  // that ships rather than a stand-in that always passes.
  const actual =
    await vi.importActual<typeof import('../src/providers/darwin/exec.js')>(
      '../src/providers/darwin/exec.js',
    );
  return {
    ...actual,
    BIN: BIN_STUB,
    collectorEnv: (e: unknown) => e,
    run: vi.fn(async (path: string, args: readonly string[]) => {
      if (path === '/bin/ps' && args.some((a) => a.includes('lstart'))) return STATIC;
      if (path === '/bin/ps' && args[0] === '-Ao') return hot();
      // Only PID 100 is ranked. `-l 2` prints two blocks; the last one wins.
      if (path === '/usr/bin/top') return 'PID   POWER\n100   12.5\n\nPID   POWER\n100   12.5\n';
      return '';
    }),
  };
});

const { DarwinProvider } = await import('../src/providers/darwin/provider.js');

/** Primes the slow energy lane, which is fetched in the background (I-7/I-8). */
async function primed(p: InstanceType<typeof DarwinProvider>) {
  hotCalls = 0;
  await p.processes();
  await new Promise((r) => setTimeout(r, 80));
  return p.processes();
}

/**
 * A column carries one unit, or an honest gap — never a blend.
 *
 * `energyProxy` returns raw CPU%, which is in [0, 100 x ncpu]; macOS Energy
 * Impact tops out around 40. Falling back to the proxy for a row `top` had not
 * ranked put both scales in one column while `energyAccurate` told the header
 * to drop its "est" suffix — so the `e` sort placed every *estimated* row above
 * every measured one, and `estimateWatts` divided by a total that had summed
 * the two. See ProcessesData.energyAccurate.
 */
describe('I-1b: the energy column never mixes measured with estimated', () => {
  it('reports null, not the CPU-time estimate, for a row the measurement missed', async () => {
    const p = new DarwinProvider({ accurateEnergy: true });
    const t = await primed(p);

    expect(t.energyAccurate, 'the lane is live, so the header drops "est"').toBe(true);

    const measured = t.visible.find((r) => r.pid === 100);
    const missed = t.visible.find((r) => r.pid === 200);
    expect(measured?.energy, 'the ranked process carries its Energy Impact').toBeCloseTo(12.5, 3);
    expect(missed?.energy, 'an unranked row must not borrow the CPU-time scale').toBeNull();
    // The hazard, made concrete: the estimate this row would have borrowed is
    // an order of magnitude above the measured column it would have sat in.
    expect(missed!.cpuPercent!).toBeGreaterThan(100);
    expect(missed!.cpuPercent!).toBeGreaterThan(measured!.energy! * 5);
  });

  it('reverts the whole column together when the lane is off', async () => {
    const p = new DarwinProvider({ accurateEnergy: false });
    const t = await primed(p);

    expect(t.energyAccurate).toBe(false);
    // Estimates for everyone, and the header says "est". One unit either way.
    for (const row of t.visible) expect(row.energy).toBe(row.cpuPercent);
  });
});
