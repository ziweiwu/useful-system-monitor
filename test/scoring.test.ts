import { describe, expect, it } from 'vitest';
import { comparator, estimateWatts, sortProcesses } from '../src/core/scoring.js';
import { sample } from './helpers.js';

describe('I-20: sort is stable and total', () => {
  it('breaks ties by PID so equal rows cannot swap between frames', () => {
    const procs = [
      sample({ pid: 30, cpuPercent: 10 }),
      sample({ pid: 10, cpuPercent: 10 }),
      sample({ pid: 20, cpuPercent: 10 }),
    ];
    const once = sortProcesses(procs, 'cpu').map((p) => p.pid);
    const twice = sortProcesses(procs.toReversed(), 'cpu').map((p) => p.pid);
    expect(once).toEqual([10, 20, 30]);
    // Same input set in a different order must produce the same output order,
    // otherwise the table visibly jitters at equal CPU.
    expect(twice).toEqual(once);
  });

  it('orders descending by the chosen key', () => {
    const procs = [
      sample({ pid: 1, cpuPercent: 5, rssBytes: 900 }),
      sample({ pid: 2, cpuPercent: 50, rssBytes: 100 }),
    ];
    expect(sortProcesses(procs, 'cpu').map((p) => p.pid)).toEqual([2, 1]);
    expect(sortProcesses(procs, 'mem').map((p) => p.pid)).toEqual([1, 2]);
  });

  it('sorts unknown CPU below every known value', () => {
    const procs = [sample({ pid: 1, cpuPercent: null }), sample({ pid: 2, cpuPercent: 0 })];
    expect(sortProcesses(procs, 'cpu').map((p) => p.pid)).toEqual([2, 1]);
  });

  it('is a total order: comparator returns 0 only for identical pids', () => {
    const a = sample({ pid: 1, cpuPercent: 7 });
    const b = sample({ pid: 2, cpuPercent: 7 });
    expect(comparator('cpu')(a, b)).not.toBe(0);
    expect(comparator('cpu')(a, a)).toBe(0);
  });
});

describe('watt estimation', () => {
  it('splits total draw in proportion to energy share', () => {
    expect(estimateWatts(25, 100, -20)).toBeCloseTo(5, 5);
  });

  it('returns null rather than guessing when inputs are missing', () => {
    expect(estimateWatts(null, 100, -20)).toBeNull();
    expect(estimateWatts(25, 100, null)).toBeNull();
    expect(estimateWatts(25, 0, -20)).toBeNull();
  });
});
