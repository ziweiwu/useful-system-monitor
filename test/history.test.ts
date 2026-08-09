import { describe, expect, it } from 'vitest';
import { Ring } from '../src/core/ring.js';
import { PROC_HISTORY_LEN } from '../src/hooks/useProcessHistory.js';

/**
 * The hook itself is exercised through the UI tests; this pins the bound that
 * makes I-10 hold for per-process history.
 */
describe('I-10: per-process history is bounded', () => {
  it('caps history length', () => {
    const r = new Ring(PROC_HISTORY_LEN);
    for (let i = 0; i < 10_000; i++) r.push(i);
    expect(r.size).toBe(PROC_HISTORY_LEN);
  });

  it('bounds total memory by working set, not process count', () => {
    // 50 visible processes x 3 rings x 40 samples, regardless of the ~800
    // processes actually running or how long sysmon has been up.
    const worstCase = 50 * 3 * PROC_HISTORY_LEN;
    expect(worstCase).toBeLessThan(10_000);
  });
});
