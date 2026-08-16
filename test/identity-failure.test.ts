import { describe, expect, it, vi } from 'vitest';

const BIN_STUB = {
  ps: '/bin/ps', vmStat: '/usr/bin/vm_stat', sysctl: '/usr/sbin/sysctl', df: '/bin/df',
  pmset: '/usr/bin/pmset', ioreg: '/usr/sbin/ioreg', top: '/usr/bin/top',
};

/** What the next `run` call does. Set per test. */
let behaviour: () => Promise<string> = async () => '';

vi.mock('../src/providers/darwin/exec.js', async () => {
  const actual =
    await vi.importActual<typeof import('../src/providers/darwin/exec.js')>(
      '../src/providers/darwin/exec.js',
    );
  return {
    ...actual,
    BIN: BIN_STUB,
    collectorEnv: (e: unknown) => e,
    run: vi.fn(() => behaviour()),
  };
});

const { CommandError } = await import('../src/providers/darwin/exec.js');
const { DarwinProvider } = await import('../src/providers/darwin/provider.js');

/**
 * I-16 turns on a three-state answer: the PID is this process, the PID is gone,
 * or nothing could be read. The third refuses too, but it is a different fact
 * and it has to be spelled differently.
 *
 * `identity()` used to catch *every* failure and return null, which the guard
 * renders as "PID N has already exited — nothing to signal." A `ps` timeout on
 * a loaded machine — the exact scenario I-13 already calls out as reachable —
 * therefore told the user that a process which is still running had exited.
 */
describe('I-16: "ps could not answer" is not spelled the same as "the process is gone"', () => {
  it('reports gone when ps ran and found no such process', async () => {
    const p = new DarwinProvider();
    // `ps -p PID` exits 1 with no output when nothing matches. That is an answer.
    behaviour = () => Promise.reject(new CommandError('/bin/ps failed: exit 1', 1));
    await expect(p.identity(4242)).resolves.toBeNull();
  });

  it('reports gone for an empty result, which is the same answer', async () => {
    const p = new DarwinProvider();
    behaviour = async () => '\n';
    await expect(p.identity(4242)).resolves.toBeNull();
  });

  it('refuses to answer at all when ps timed out', async () => {
    const p = new DarwinProvider();
    // A killed child carries no exit status, so CommandError.exitCode is null.
    behaviour = () => Promise.reject(new CommandError('/bin/ps failed: timed out', null));
    // Rejecting is what lets app.tsx map this to `unverifiable` rather than
    // `gone`; both refuse the kill, only one of them says something true.
    await expect(p.identity(4242)).rejects.toThrow(/timed out/);
  });

  it('refuses to answer when ps is not on the system', async () => {
    const p = new DarwinProvider();
    behaviour = () => Promise.reject(new CommandError('/bin/ps not found on this system', null));
    await expect(p.identity(4242)).rejects.toThrow(/not found/);
  });

  it('still reads the start time when ps answers normally', async () => {
    const p = new DarwinProvider();
    behaviour = async () => 'Wed Aug 12 10:00:00 2026\n';
    const id = await p.identity(4242);
    expect(id?.startTime).toBe(Date.parse('Wed Aug 12 10:00:00 2026'));
  });
});
