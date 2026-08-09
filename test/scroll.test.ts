import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { scrollbarCells } from '../src/ui/ProcessTable.js';

/** [start, size] of the single run of thumb cells, or null when there is none. */
function thumb(cells: readonly boolean[]): [number, number] | null {
  const start = cells.indexOf(true);
  if (start < 0) return null;
  let end = start;
  while (cells[end] === true) end++;
  // Anything after the run must be track, or it is not one contiguous thumb.
  expect(cells.slice(end).every((c) => !c)).toBe(true);
  return [start, end - start];
}

describe('I-26: the scrollbar reports the real window position', () => {
  it('renders nothing when the whole list fits', () => {
    expect(scrollbarCells(10, 20, 0)).toEqual([]);
    expect(scrollbarCells(20, 20, 0)).toEqual([]);
    expect(scrollbarCells(0, 20, 0)).toEqual([]);
  });

  it('pins the thumb to the top at the start of the list', () => {
    expect(thumb(scrollbarCells(100, 10, 0))![0]).toBe(0);
  });

  it('pins the thumb to the bottom at the end of the list', () => {
    const cells = scrollbarCells(100, 10, 90);
    const [start, size] = thumb(cells)!;
    expect(start + size).toBe(10);
  });

  it('stays inside the track for every window and offset', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 5000 }),
        fc.integer({ min: 1, max: 200 }),
        fc.nat(),
        (total, windowSize, rawOffset) => {
          const offset = Math.min(rawOffset, Math.max(0, total - windowSize));
          const cells = scrollbarCells(total, windowSize, offset);
          if (total <= windowSize) {
            expect(cells).toEqual([]);
            return;
          }
          expect(cells).toHaveLength(windowSize);
          const t = thumb(cells);
          // A thumb always exists and is never wider than its track.
          expect(t).not.toBeNull();
          const [start, size] = t!;
          expect(size).toBeGreaterThanOrEqual(1);
          expect(start).toBeGreaterThanOrEqual(0);
          expect(start + size).toBeLessThanOrEqual(windowSize);
        },
      ),
    );
  });

  it('never moves the thumb backwards as the offset grows', () => {
    fc.assert(
      fc.property(fc.integer({ min: 21, max: 5000 }), fc.integer({ min: 2, max: 20 }), (total, windowSize) => {
        let prev = -1;
        for (let offset = 0; offset <= total - windowSize; offset++) {
          const [start] = thumb(scrollbarCells(total, windowSize, offset))!;
          expect(start).toBeGreaterThanOrEqual(prev);
          prev = start;
        }
      }),
      { numRuns: 40 },
    );
  });
});
