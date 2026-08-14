import { memo } from 'react';
import { Text } from 'ink';
import { VIEW_LABELS, VIEW_ORDER, viewKey, type View } from '../core/views.js';
import { displayWidth } from '../core/width.js';
import { theme } from './theme.js';

/** One hue per data family, matching each screen's cards and columns. */
const TAB_COLOR: Readonly<Record<View, string>> = {
  overview: theme.headline,
  cpu: theme.cpuMid,
  memory: theme.mem,
  battery: theme.battery,
  disk: theme.disk,
};

const HINT = '   ←/→ or 1-5';
const SHORT_HINT = '  ←/→';

/** The active tab always carries its name; only the others lose theirs. */
function tabText(v: View, active: View, compact: boolean): string {
  if (v === active) return `[${viewKey(v)} ${VIEW_LABELS[v]}]`;
  return compact ? ` ${viewKey(v)} ` : ` ${viewKey(v)} ${VIEW_LABELS[v]} `;
}

function stripWidth(active: View, compact: boolean): number {
  const tabs = VIEW_ORDER.map((v) => tabText(v, active, compact)).join('');
  return displayWidth(tabs + (compact ? SHORT_HINT : HINT));
}

/**
 * The row of screen names above the dashboard.
 *
 * Without it there is nothing on screen saying which of the five screens you
 * are on, or that the other four exist — the only hint used to be `1-5 view` in
 * a footer that truncates at 80 columns.
 *
 * I-23: the active tab is marked by brackets as well as by colour and weight,
 * so it survives NO_COLOR and a plain-text capture.
 *
 * I-27: the strip must name the screen you are on at *every* width. Relying on
 * `wrap="truncate"` alone meant that at 50 columns the fifth screen rendered as
 * `[5 DISK…` with its closing bracket and the arrow hint cut off, which reads
 * as a broken frame rather than a narrow one. Below the width the full strip
 * needs, the inactive tabs drop to bare numbers instead — they are still the
 * keys that select them.
 */
export const ViewTabs = memo(function ViewTabs({
  active,
  width = 80,
}: {
  active: View;
  width?: number;
}) {
  const compact = stripWidth(active, false) > width;
  return (
    <Text wrap="truncate">
      {VIEW_ORDER.map((v) => {
        const on = v === active;
        return (
          <Text key={v} bold={on} color={on ? TAB_COLOR[v] : theme.dim}>
            {tabText(v, active, compact)}
          </Text>
        );
      })}
      <Text color={theme.dim}>{compact ? SHORT_HINT : HINT}</Text>
    </Text>
  );
});
