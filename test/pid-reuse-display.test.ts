import { describe, expect, it, vi } from 'vitest';

vi.mock('../src/providers/darwin/exec.js', () => {
  let hotCalls = 0;
  let staticCalls = 0;
  return {
    BIN: {
      ps: '/bin/ps', vmStat: '/usr/bin/vm_stat', sysctl: '/usr/sbin/sysctl', df: '/bin/df',
      pmset: '/usr/bin/pmset', ioreg: '/usr/sbin/ioreg', top: '/usr/bin/top',
    },
    collectorEnv: (e: unknown) => e,
    run: vi.fn(async (path: string, args: readonly string[]) => {
      if (path === '/bin/ps' && args.some((a) => a.includes('lstart'))) {
        staticCalls++;
        // The second static fetch sees the *new* occupant of PID 100.
        return staticCalls === 1
          ? 'PID STARTED USER STAT COMM\n100 Wed Aug 12 10:00:00 2026 alice S OldOwner\n'
          : 'PID STARTED USER STAT COMM\n100 Wed Aug 13 11:00:00 2026 bob S NewOwner\n';
      }
      if (path === '/bin/ps' && args[0] === '-Ao' && args[1] === 'pid,ppid,time,rss') {
        hotCalls++;
        // TIME going backwards is the PID-reuse signature I-3 detects.
        return `  PID  PPID     TIME    RSS\n  100     1  ${hotCalls === 1 ? '0:01.00' : '0:00.02'}   500\n`;
      }
      return '';
    }),
  };
});

const { DarwinProvider } = await import('../src/providers/darwin/provider.js');

/*
 * I-3 already detects PID reuse — the cumulative counter going backwards — and
 * nulls the CPU%. But the *name* came from `metaCache`, consulted with
 * `.has(pid)`, which is not an identity check: the row kept the dead process's
 * command, user, start time and protected flag until some unrelated new PID
 * happened to force a static refetch.
 *
 * Kill safety was never affected — `checkKill` re-reads identity at signal time
 * (I-16) and refuses on a mismatch — so this was display-only. It is still the
 * monitor showing one program under another's name.
 */
describe('I-3: a recycled PID does not keep the old process name', () => {
  it('refetches the name when the counter goes backwards', async () => {
    const p = new DarwinProvider({});
    const first = await p.processes();
    expect(first.visible.find((r) => r.pid === 100)?.command).toContain('OldOwner');

    const second = await p.processes();
    const row = second.visible.find((r) => r.pid === 100);
    expect(row?.cpuPercent, 'I-3 should null the CPU% on reuse').toBeNull();
    expect(row?.command, 'still showing the dead process name').not.toContain('OldOwner');
    expect(row?.command).toContain('NewOwner');
    expect(row?.user).toBe('bob');
  }, 20_000);
});
