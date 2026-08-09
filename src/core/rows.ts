/**
 * Vertical budgeting. The horizontal equivalent lives in `width.ts`.
 *
 * Every screen in this app draws at least one list whose length comes from the
 * machine rather than from the design — cores, mounted volumes, top processes.
 * Rendering all of them is how a frame ends up taller than the terminal, which
 * scrolls the app's own header away. See I-26.
 */

export interface Fit {
  /** Items to render. */
  shown: number;
  /** Items rolled up into a single "… N more" line. Zero when everything fits. */
  hidden: number;
}

/**
 * How many of `total` items fit in `budget` lines.
 *
 * When some must be dropped, one line of the budget is spent on the roll-up
 * that says so: a list that silently stops is indistinguishable from a machine
 * that has nothing more to show.
 *
 * A budget of zero or less means the section does not fit at all; the caller
 * renders neither rows nor roll-up.
 */
export function fitList(total: number, budget: number): Fit {
  if (budget <= 0) return { shown: 0, hidden: total };
  if (total <= budget) return { shown: total, hidden: 0 };
  const shown = Math.max(0, budget - 1);
  return { shown, hidden: total - shown };
}
