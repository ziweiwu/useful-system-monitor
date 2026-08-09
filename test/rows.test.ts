import fc from 'fast-check';
import { describe, expect, it } from 'vitest';
import { fitList } from '../src/core/rows.js';

describe('I-26: list row budgets', () => {
  it('shows everything when it fits', () => {
    expect(fitList(5, 10)).toEqual({ shown: 5, hidden: 0 });
    expect(fitList(5, 5)).toEqual({ shown: 5, hidden: 0 });
  });

  it('spends one line of the budget on the roll-up when it does not', () => {
    // 4 lines for 10 items: three rows plus "… 7 more".
    expect(fitList(10, 4)).toEqual({ shown: 3, hidden: 7 });
  });

  it('shows nothing at a budget of zero or less', () => {
    // A section with no budget must not be rendered at all — including its
    // roll-up, which is why callers gate on the budget rather than on `hidden`.
    expect(fitList(10, 0)).toEqual({ shown: 0, hidden: 10 });
    expect(fitList(10, -3)).toEqual({ shown: 0, hidden: 10 });
  });

  it('never renders more lines than the budget, and never loses an item', () => {
    fc.assert(
      fc.property(fc.nat({ max: 500 }), fc.integer({ min: 1, max: 200 }), (total, budget) => {
        const { shown, hidden } = fitList(total, budget);
        expect(shown + hidden).toBe(total);
        expect(shown).toBeGreaterThanOrEqual(0);
        // The roll-up line only exists when something was actually hidden.
        expect(shown + (hidden > 0 ? 1 : 0)).toBeLessThanOrEqual(budget);
      }),
    );
  });
});
