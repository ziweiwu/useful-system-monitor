import { render } from 'ink-testing-library';
import { describe, expect, it } from 'vitest';
import { useProcessHistory } from '../src/hooks/useProcessHistory.js';
import type { ProcessesData } from '../src/core/types.js';
import { sample } from './helpers.js';

/**
 * Drives the hook through a real render and snapshots after each frame.
 *
 * The hook writes in an effect, so a snapshot taken during render is always
 * one frame stale — reading it there is what made the first version of this
 * test fail against correct code.
 */
function drive(frames: ProcessesData[]) {
  const seen: Array<Map<number, { cpu: number[]; startTime: number }>> = [];
  let live: ReturnType<typeof useProcessHistory> | null = null;
  let i = 0;
  function Probe() {
    live = useProcessHistory(frames[i] ?? null);
    return null;
  }
  const snapshot = () =>
    new Map(
      [...(live ?? new Map())].map(([pid, h]) => [
        pid,
        { cpu: h.cpu.toArray(), startTime: h.startTime },
      ]),
    );

  const app = render(<Probe />);
  seen[0] = snapshot();
  for (i = 1; i < frames.length; i++) {
    app.rerender(<Probe />);
    seen[i] = snapshot();
  }
  app.unmount();
  return seen;
}

const data = (procs: Array<{ pid: number; startTime: number; cpu: number }>): ProcessesData => ({
  total: procs.length,
  visible: procs.map((p) =>
    sample({ pid: p.pid, startTime: p.startTime, cpuPercent: p.cpu }),
  ),
  others: { count: 0, cpuPercent: 0, rssBytes: 0, energy: 0 },
  parents: new Map(procs.map((p) => [p.pid, 1])),
  energyAccurate: false,
});

describe('I-10: per-process history belongs to a process, not to a PID', () => {
  it('starts a fresh ring when a PID is reused', () => {
    /*
     * A PID on its own is not an identity. Recycle one while it is still
     * inside the working set and the new process inherited the dead one's
     * rings, so the detail panel drew another program's CPU and memory
     * history under this process's name. The delta tracker already refuses to
     * carry a counter across a reuse for the same reason (I-3).
     */
    const seen = drive([
      data([{ pid: 700, startTime: 1_000, cpu: 90 }]),
      data([{ pid: 700, startTime: 1_000, cpu: 80 }]),
      // Same PID, different process.
      data([{ pid: 700, startTime: 5_000, cpu: 4 }]),
    ]);

    const before = seen[1]!.get(700)!;
    expect(before.cpu).toEqual([90, 80]);

    const after = seen[2]!.get(700)!;
    expect(after.startTime).toBe(5_000);
    // The new process's history is its own, not the dead one's.
    expect(after.cpu).toEqual([4]);
  });

  it('keeps accumulating while the process is the same one', () => {
    const seen = drive([
      data([{ pid: 700, startTime: 1_000, cpu: 10 }]),
      data([{ pid: 700, startTime: 1_000, cpu: 20 }]),
      data([{ pid: 700, startTime: 1_000, cpu: 30 }]),
    ]);
    expect(seen[2]!.get(700)!.cpu).toEqual([10, 20, 30]);
  });

  it('still evicts a process that leaves the working set', () => {
    const seen = drive([
      data([{ pid: 700, startTime: 1_000, cpu: 10 }]),
      data([{ pid: 800, startTime: 2_000, cpu: 10 }]),
    ]);
    expect(seen[1]!.has(700)).toBe(false);
    expect(seen[1]!.has(800)).toBe(true);
  });
});
