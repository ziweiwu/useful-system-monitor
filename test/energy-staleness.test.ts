import { describe, expect, it, vi } from 'vitest';

const BIN_STUB = {
  ps: '/bin/ps', vmStat: '/usr/bin/vm_stat', sysctl: '/usr/sbin/sysctl', df: '/bin/df',
  pmset: '/usr/bin/pmset', ioreg: '/usr/sbin/ioreg', top: '/usr/bin/top',
};

vi.mock('../src/providers/darwin/exec.js', () => {
  let topCalls = 0;
  return {
    BIN: BIN_STUB,
    collectorEnv: (e: unknown) => e,
    run: vi.fn(async (path: string, args: readonly string[]) => {
      if (path === '/bin/ps' && args.some((a) => a.includes('lstart')))
        return 'PID STARTED USER STAT COMM\n100 Wed Aug 12 10:00:00 2026 alice S SomeProcess\n';
      if (path === '/bin/ps' && args[0] === '-Ao' && args[1] === 'pid,ppid,time,rss')
        return '  PID  PPID     TIME    RSS\n  100     1  0:01.00   1000\n';
      if (path === '/usr/bin/top') {
        topCalls++;
        if (topCalls === 1) return 'PID   POWER\n100   99.9\n\nPID   POWER\n100   99.9\n';
        throw new Error('top failed: simulated timeout');
      }
      return '';
    }),
  };
});

const { DarwinProvider } = await import('../src/providers/darwin/provider.js');

describe('I-11: the accurate-energy lane falls back to the estimate when it fails', () => {
  it('stops claiming "measured" once top has started failing', async () => {
    const p = new DarwinProvider({ accurateEnergy: true });
    await p.processes();
    await new Promise((r) => setTimeout(r, 60));
    await p.processes(); // primed: energyAccurate true, energy 99.9

    for (let i = 0; i < 3; i++) {
      (p as unknown as { energyFetchedAt: number }).energyFetchedAt = 0;
      await p.processes();
      await new Promise((r) => setTimeout(r, 40));
    }
    const t = await p.processes();
    const row = t.visible.find((r) => r.pid === 100);
    expect(t.energyAccurate, 'still labelled measured after 3 failures').toBe(false);
    expect(row?.energy, 'still serving the last successful sample').not.toBeCloseTo(99.9, 1);
  }, 20_000);
});
