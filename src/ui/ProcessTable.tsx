import { memo } from 'react';
import { Box, Text } from 'ink';
import { bytes, percent } from '../core/format.js';
import { estimateWatts } from '../core/scoring.js';
import type { OthersRollup, ProcessSample } from '../core/types.js';
import { padEnd, padStart, truncate } from '../core/width.js';
import { processName } from '../kill/guards.js';
import { barCells, severity, theme } from './theme.js';

const CURSOR = 3;
const PID = 7;
const MARK = 1;
const CPU_BAR = 8;
const CPU_NUM = 6;
const MEM_NUM = 6;
const EN_BAR = 5;
const EN_NUM = 6;
const USER = 9;
const NAME_MAX = 34;
const NAME_MIN = 10;
/** Trailing gutter: one space plus the scrollbar cell. */
const GUTTER = 2;
/** Columns before the name that no layout gives up: cursor, PID, CPU, memory. */
const CORE = CURSOR + PID + 1 + MARK + CPU_BAR + CPU_NUM + 2 + MEM_NUM + 2;
/** The energy bar, its number, and the gap after it. */
const ENERGY = EN_BAR + 1 + EN_NUM + 2;

/** Narrowest table that can still name a process. Below this, see MIN_COLUMNS. */
export const MIN_TABLE_WIDTH = CORE + GUTTER + NAME_MIN;

export interface ColumnLayout {
  name: number;
  /** 0 when the terminal cannot carry the USER column. */
  user: number;
  /** False when it cannot carry the energy bar and its number either. */
  energy: boolean;
}

/**
 * Widths derived from terminal width so nothing ever wraps. See I-19.
 *
 * The name used to be floored at 10 columns and everything else kept, which
 * meant that at 72 columns or fewer the row was simply wider than the box Ink
 * drew it in: every row wrapped onto a second line carrying nothing but the
 * scrollbar glyph, the table cost twice the height `CHROME_ROWS` had budgeted,
 * and the frame ran off the bottom of the terminal. A floor is not a fit.
 *
 * So columns are given up instead, in the order they stop being worth their
 * width. USER goes first — on a personal Mac nearly every row says the same
 * name. Energy goes second; the BATTERY screen still shows it in full. The
 * name goes last, because it is the only column here that cannot be recovered
 * from another screen, and a table of bare PIDs is not a system monitor.
 */
export function columnLayout(totalWidth: number): ColumnLayout {
  for (const [user, energy] of [[USER, true], [0, true], [0, false]] as const) {
    const name = totalWidth - CORE - (energy ? ENERGY : 0) - user - GUTTER;
    if (name >= NAME_MIN) return { name: Math.min(NAME_MAX, name), user, energy };
  }
  // Narrower than MIN_TABLE_WIDTH; app.tsx refuses to draw the dashboard at all
  // rather than let this overflow.
  return { name: NAME_MIN, user: 0, energy: false };
}

/**
 * Per-row scrollbar glyphs for a window of `windowSize` rows starting at
 * `offset` within a list of `total`.
 *
 * Returns an empty array when everything fits, so a short list renders no
 * gutter content at all rather than a full-height thumb that cannot move.
 */
export function scrollbarCells(total: number, windowSize: number, offset: number): boolean[] {
  if (windowSize <= 0 || total <= windowSize) return [];
  const thumb = Math.max(1, Math.round((windowSize * windowSize) / total));
  const maxOffset = total - windowSize;
  const travel = windowSize - thumb;
  const top = maxOffset > 0 ? Math.round((offset / maxOffset) * travel) : 0;
  // Clamped so a rounded thumb can never hang off the end of the track.
  const start = Math.min(Math.max(0, top), travel);
  return Array.from({ length: windowSize }, (_, i) => i >= start && i < start + thumb);
}

/**
 * Watts are only meaningful when the battery is actually moving charge. Plugged
 * in and holding, total draw is ~0 and every row would read "0.0W" — a column
 * of zeros. In that case the whole column falls back to the unitless energy
 * score, header included, so units never mix within one column.
 */
const MIN_MEANINGFUL_WATTS = 0.5;

export function wattsAreMeaningful(totalWatts: number | null): boolean {
  return totalWatts !== null && Math.abs(totalWatts) >= MIN_MEANINGFUL_WATTS;
}

function powerLabel(watts: number | null, energy: number | null, showWatts: boolean): string {
  if (showWatts && watts !== null) return `${watts.toFixed(1)}W`;
  return percent(energy);
}

const Row = memo(function Row({
  p,
  selected,
  width,
  totalEnergy,
  totalWatts,
  bar,
}: {
  p: ProcessSample;
  selected: boolean;
  width: number;
  totalEnergy: number;
  totalWatts: number | null;
  /** Scrollbar gutter: true = thumb, false = track, null = list fits. */
  bar: boolean | null;
}) {
  const { name, user, energy: showEnergy } = columnLayout(width);
  const cpu = p.cpuPercent ?? 0;
  const cpuBar = barCells(cpu, CPU_BAR);
  const enBar = barCells(p.energy ?? 0, EN_BAR);
  const showWatts = wattsAreMeaningful(totalWatts);
  const watts = estimateWatts(p.energy, totalEnergy, totalWatts);

  return (
    <Text backgroundColor={selected ? theme.selectionBg : undefined}>
      <Text bold color={selected ? theme.mem : theme.dim}>
        {selected ? ' > ' : '   '}
      </Text>
      {/* Every other cell brightens on the selected row; this one did not, so
          the PID sat at 1.97:1 on the selection background — on the very row
          the user is about to press enter or k against. */}
      <Text color={selected ? theme.text : theme.dim}>{padEnd(String(p.pid), PID)}</Text>
      <Text> </Text>
      <Text bold={selected} color={selected ? theme.headline : theme.text}>
        {padEnd(processName(p.command), name)}
      </Text>
      {/* Protection is marked with a glyph, not colour alone, so it survives
          NO_COLOR and stays greppable. See I-23. */}
      <Text color={theme.danger}>{p.protected ? '!' : ' '}</Text>
      <Text color={severity(cpu)}>{'█'.repeat(cpuBar.filled)}</Text>
      <Text color={theme.track}>{'░'.repeat(cpuBar.empty)}</Text>
      <Text bold color={p.cpuPercent === null ? theme.dim : theme.headline}>
        {padStart(percent(p.cpuPercent), CPU_NUM)}
      </Text>
      <Text>{'  '}</Text>
      <Text color={theme.mem}>{padStart(bytes(p.rssBytes), MEM_NUM)}</Text>
      {showEnergy && (
        <>
          <Text>{'  '}</Text>
          <Text color={theme.battery}>{'█'.repeat(enBar.filled)}</Text>
          <Text color={theme.track}>{'░'.repeat(enBar.empty)}</Text>
          <Text> </Text>
          <Text color={theme.battery}>
            {padStart(powerLabel(watts, p.energy, showWatts), EN_NUM)}
          </Text>
        </>
      )}
      {user > 0 && (
        <>
          <Text>{'  '}</Text>
          {/* Selection-aware for the same reason as the PID cell: `dim` is
              3.25:1 against the selection background, below 4.5:1. */}
          <Text color={p.user === 'root' ? theme.root : selected ? theme.text : theme.dim}>
            {padEnd(truncate(p.user, user), user)}
          </Text>
        </>
      )}
      <Text> </Text>
      <Text color={bar ? theme.mem : theme.track}>{bar === null ? ' ' : bar ? '█' : '│'}</Text>
    </Text>
  );
});

export const ProcessTable = memo(function ProcessTable({
  processes,
  others,
  selectedPid,
  width,
  rows,
  offset = 0,
  totalEnergy,
  totalWatts,
  energyAccurate = false,
  canExpand = false,
  filterActive = false,
}: {
  processes: readonly ProcessSample[];
  others: OthersRollup;
  selectedPid: number | null;
  width: number;
  rows: number;
  /**
   * First row of the visible window. I-26: the window follows the selection,
   * so the cursor is always on screen no matter how far down the list it is.
   */
  offset?: number;
  totalEnergy: number;
  totalWatts: number | null;
  energyAccurate?: boolean;
  /** Whether `+` would still widen the working set, for the roll-up hint. */
  canExpand?: boolean;
  /**
   * Whether a filter is narrowing `processes`.
   *
   * The roll-up exists so the table reconciles with the CPU and MEM cards:
   * everything on screen, plus `others`, is the whole machine. A filter breaks
   * that — `others` is the tail of the *working-set cap*, which has nothing to
   * do with the search — so its totals stop describing anything the user asked
   * for. Filtering for "chrome" and reading "… 774 others" beneath two matches
   * implies 774 more Chrome processes; filtering for something absent printed
   * "no matches" and a roll-up of 775 in the very next line.
   */
  filterActive?: boolean;
}) {
  const { name, user, energy: showEnergy } = columnLayout(width);
  // Clamped here as well as by the caller: `rows` shrinks on a terminal resize,
  // and a stale offset would otherwise render a window past the end.
  const start = Math.max(0, Math.min(offset, Math.max(0, processes.length - rows)));
  const shown = processes.slice(start, start + rows);
  const bars = scrollbarCells(processes.length, rows, start);
  const showWatts = wattsAreMeaningful(totalWatts);
  const othersWatts = estimateWatts(others.energy, totalEnergy, totalWatts);

  return (
    <Box flexDirection="column">
      <Text color={theme.dim}>
        {' '.repeat(CURSOR)}
        {padEnd('PID', PID)} {padEnd('PROCESS', name)}
        {' '.repeat(MARK)}
        {padStart('CPU%', CPU_BAR + CPU_NUM)}
        {'  '}
        {padStart('MEM', MEM_NUM)}
        {/* Both headers follow the row layout exactly, so a column dropped at a
            narrow width cannot leave its heading behind. */}
        {showEnergy && '  '}
        {showEnergy &&
          /* "est" is dropped only when the numbers are genuinely measured. */
          padStart(
            showWatts
              ? energyAccurate
                ? 'POWER'
                : 'POWER est'
              : energyAccurate
                ? 'ENERGY'
                : 'ENERGY est',
            EN_BAR + 1 + EN_NUM,
          )}
        {user > 0 && '  '}
        {user > 0 && padEnd('USER', user)}
      </Text>
      {shown.map((p, i) => (
        <Row
          key={p.pid}
          p={p}
          selected={p.pid === selectedPid}
          width={width}
          totalEnergy={totalEnergy}
          totalWatts={totalWatts}
          bar={bars.length ? (bars[i] ?? false) : null}
        />
      ))}
      {/* Under a filter the aggregate is meaningless, but the *scope* is worth
          saying: the search only ever covered the working set, and `+` widens
          it — which is exactly what a user who found nothing needs to know. */}
      {others.count > 0 && filterActive && (
        <Text color={theme.dim} wrap="truncate">
          {' '.repeat(CURSOR)}
          {`… ${others.count} outside this view were not searched`}
          {canExpand ? '  (+ widen)' : ''}
        </Text>
      )}
      {others.count > 0 && !filterActive && (
        <Text color={theme.dim}>
          {' '.repeat(CURSOR)}
          {padEnd('', PID)}{' '}
          {padEnd(
            truncate(canExpand ? `… ${others.count} others  (+ show more)` : `… ${others.count} others`, name),
            name,
          )}
          {' '.repeat(MARK)}
          {padStart(others.cpuPercent.toFixed(1), CPU_BAR + CPU_NUM)}
          {'  '}
          {padStart(bytes(others.rssBytes), MEM_NUM)}
          {showEnergy && '  '}
          {showEnergy &&
            padStart(powerLabel(othersWatts, others.energy, showWatts), EN_BAR + 1 + EN_NUM)}
        </Text>
      )}
    </Box>
  );
});
