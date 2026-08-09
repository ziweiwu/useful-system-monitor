import { memo } from 'react';
import { Text } from 'ink';
import { VIEW_LABELS, VIEW_ORDER, viewKey, type View } from '../core/views.js';
import { theme } from './theme.js';

/** One hue per data family, matching each screen's cards and columns. */
const TAB_COLOR: Readonly<Record<View, string>> = {
  overview: theme.headline,
  cpu: theme.cpuMid,
  memory: theme.mem,
  battery: theme.battery,
  disk: theme.disk,
};

/**
 * The row of screen names above the dashboard.
 *
 * Without it there is nothing on screen saying which of the five screens you
 * are on, or that the other four exist — the only hint used to be `1-5 view` in
 * a footer that truncates at 80 columns.
 *
 * I-23: the active tab is marked by brackets as well as by colour and weight,
 * so it survives NO_COLOR and a plain-text capture.
 */
export const ViewTabs = memo(function ViewTabs({ active }: { active: View }) {
  return (
    <Text wrap="truncate">
      {VIEW_ORDER.map((v) => {
        const on = v === active;
        return (
          <Text key={v} bold={on} color={on ? TAB_COLOR[v] : theme.dim}>
            {on ? `[${viewKey(v)} ${VIEW_LABELS[v]}]` : ` ${viewKey(v)} ${VIEW_LABELS[v]} `}
          </Text>
        );
      })}
      <Text color={theme.dim}>{'   ←/→ or 1-5'}</Text>
    </Text>
  );
});
