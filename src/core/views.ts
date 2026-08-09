/**
 * The dashboard screens, in the order the tab strip shows them.
 *
 * The order is the navigation order: left/right step through this list, and the
 * number key is the 1-based position, so the strip on screen and the keymap
 * cannot drift apart. See I-27.
 */
export const VIEW_ORDER = ['overview', 'cpu', 'memory', 'battery', 'disk'] as const;

export type View = (typeof VIEW_ORDER)[number];

/** Short enough that all five fit on one line at the 80-column minimum. */
export const VIEW_LABELS: Readonly<Record<View, string>> = {
  overview: 'OVERVIEW',
  cpu: 'CPU',
  memory: 'MEMORY',
  battery: 'BATTERY',
  disk: 'DISK',
};

/** The number key that jumps straight to a view, derived from the order. */
export function viewKey(view: View): string {
  return String(VIEW_ORDER.indexOf(view) + 1);
}

export const VIEW_KEYS: Readonly<Record<string, View>> = Object.fromEntries(
  VIEW_ORDER.map((v) => [viewKey(v), v]),
);

/**
 * Step `delta` views along the strip, wrapping at both ends.
 *
 * Wrapping matters more than it looks: a tab strip whose arrow key silently
 * does nothing at the last tab reads as a broken key, not as a boundary.
 */
export function stepView(current: View, delta: number): View {
  const n = VIEW_ORDER.length;
  const i = Math.max(0, VIEW_ORDER.indexOf(current));
  return VIEW_ORDER[(((i + delta) % n) + n) % n]!;
}
