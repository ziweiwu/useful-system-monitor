import { describe, expect, it } from 'vitest';
import { selectWorkingSet } from '../src/core/workingSet.js';
import { sample } from './helpers.js';

describe('C-7 / C-9: top-50 working set', () => {
  const many = Array.from({ length: 400 }, (_, i) =>
    sample({ pid: i + 1, cpuPercent: i, rssBytes: (400 - i) * 1024, energy: i }),
  );

  it('keeps the union within the budget', () => {
    const { visible } = selectWorkingSet(many, 50);
    expect(visible.length).toBeLessThanOrEqual(50);
  });

  it('keeps memory hogs that use no CPU', () => {
    // pid 1 is the largest by RSS and the smallest by CPU. A CPU-only cut
    // would hide it, which is exactly the leak you want to find.
    const { visible } = selectWorkingSet(many, 50);
    expect(visible.some((p) => p.pid === 1)).toBe(true);
    expect(visible.some((p) => p.pid === 400)).toBe(true);
  });

  it('rolls the tail up so totals still reconcile', () => {
    const { visible, others } = selectWorkingSet(many, 50);
    const totalCpu =
      visible.reduce((s, p) => s + (p.cpuPercent ?? 0), 0) + others.cpuPercent;
    const expected = many.reduce((s, p) => s + (p.cpuPercent ?? 0), 0);
    expect(totalCpu).toBeCloseTo(expected, 5);
    expect(visible.length + others.count).toBe(many.length);
  });

  it('passes everything through when under the limit', () => {
    const few = many.slice(0, 10);
    const { visible, others } = selectWorkingSet(few, 50);
    expect(visible).toHaveLength(10);
    expect(others.count).toBe(0);
  });
});
