import { describe, expect, it } from 'vitest';
import { barCells, sparkline } from '../src/ui/theme.js';
import { displayWidth } from '../src/core/width.js';

/*
 * I-19 is about display width, and the two functions that produce most of the
 * app's width are pure arithmetic on numbers that come from division. NaN
 * propagates straight through: `'█'.repeat(NaN)` is '' and `BLOCKS[NaN]` is
 * undefined, so before this a single bad sample made a bar 0 cells wide
 * instead of 10 and a sparkline 4 characters instead of 5 — a layout break
 * with no error raised anywhere to notice it by.
 */
describe('I-19: a bar always occupies the width it was given', () => {
  it.each([NaN, Infinity, -Infinity])('survives pct = %s', (pct) => {
    const { filled, empty } = barCells(pct, 10);
    expect(filled + empty).toBe(10);
    expect(displayWidth('█'.repeat(filled) + '░'.repeat(empty))).toBe(10);
  });

  it.each([0, 1, 50, 99.9, 100, 150, -20])('is exactly the width at pct = %s', (pct) => {
    const { filled, empty } = barCells(pct, 24);
    expect(filled + empty).toBe(24);
    expect(filled).toBeGreaterThanOrEqual(0);
    expect(empty).toBeGreaterThanOrEqual(0);
  });

  it('treats a non-finite width as no width rather than a negative one', () => {
    for (const w of [NaN, Infinity, -5]) {
      const { filled, empty } = barCells(50, w);
      expect(filled).toBeGreaterThanOrEqual(0);
      expect(empty).toBeGreaterThanOrEqual(0);
      expect(Number.isFinite(filled + empty)).toBe(true);
    }
  });
});

describe('I-19: a sparkline always occupies the width it was given', () => {
  it('renders full width when a sample is NaN', () => {
    expect(displayWidth(sparkline([1, NaN, 3], 5))).toBe(5);
  });

  it('renders full width when the scale is unusable', () => {
    // memMax comes from Math.max(1, ...history); one NaN in the ring made the
    // whole panel's sparkline vanish rather than flatten.
    for (const max of [NaN, 0, -1, Infinity]) {
      expect(displayWidth(sparkline([1, 2, 3, 4, 5], 5, max))).toBe(5);
    }
  });

  it.each([
    [[], 8],
    [[0, 0, 0], 8],
    [[100, 100], 8],
    [[NaN, NaN, NaN, NaN], 8],
  ])('is %o wide at width %i', (values, width) => {
    expect(displayWidth(sparkline(values as number[], width))).toBe(width);
  });

  it('is empty, not ragged, for a width that cannot be drawn', () => {
    for (const w of [0, -3, NaN]) expect(sparkline([1, 2, 3], w)).toBe('');
  });
});
