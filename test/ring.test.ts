import { describe, expect, it } from 'vitest';
import { Ring } from '../src/core/ring.js';

describe('I-10: history is bounded', () => {
  it('never grows beyond capacity', () => {
    const r = new Ring(5);
    for (let i = 0; i < 1000; i++) r.push(i);
    expect(r.size).toBe(5);
    expect(r.toArray()).toHaveLength(5);
  });

  it('keeps the most recent values, oldest first', () => {
    const r = new Ring(3);
    [1, 2, 3, 4, 5].forEach((v) => r.push(v));
    expect(r.toArray()).toEqual([3, 4, 5]);
    expect(r.last()).toBe(5);
  });

  it('reports empty state without inventing values', () => {
    const r = new Ring(3);
    expect(r.size).toBe(0);
    expect(r.toArray()).toEqual([]);
    expect(r.last()).toBeNull();
  });

  it('rejects a non-positive capacity', () => {
    expect(() => new Ring(0)).toThrow(RangeError);
  });
});
