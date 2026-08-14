import { memo } from 'react';
import { Box, Text } from 'ink';
import { minutesToHm, percent } from '../core/format.js';
import { fitList } from '../core/rows.js';
import { estimateWatts } from '../core/scoring.js';
import type { Snapshot } from '../core/types.js';
import { padEnd, padStart } from '../core/width.js';
import type { Histories } from '../hooks/useSampler.js';
import { processName } from '../kill/guards.js';
import { Bar } from './Bar.js';
import { Sparkline } from './Sparkline.js';
import { wattsAreMeaningful } from './ProcessTable.js';
import { barCells, theme } from './theme.js';

/**
 * The emphasis screen: not just battery state, but which processes are
 * spending it, and what killing one would actually buy you.
 */
export const BatteryView = memo(function BatteryView({
  snapshot,
  histories,
  width,
  maxRows,
  selectedPid,
}: {
  snapshot: Snapshot;
  histories: Histories;
  width: number;
  /** Lines this screen may draw into. See I-26. */
  maxRows: number;
  selectedPid: number | null;
}) {
  const batt = snapshot.battery.status === 'ok' ? snapshot.battery.data : null;
  const procs = snapshot.processes.status === 'ok' ? snapshot.processes.data : null;

  if (!batt) {
    return <Text color={theme.dim}>Battery data unavailable.</Text>;
  }

  if (!batt.present) {
    return (
      <Text color={theme.dim}>
        This Mac has no battery — it runs on AC power, so there is nothing to
        attribute energy against.
      </Text>
    );
  }

  const ranked = procs
    ? procs.visible.toSorted((a, b) => (b.energy ?? 0) - (a.energy ?? 0)).slice(0, 8)
    : [];
  const totalEnergy =
    (procs?.visible.reduce((s, p) => s + (p.energy ?? 0), 0) ?? 0) + (procs?.others.energy ?? 0);
  const nameW = Math.max(12, Math.min(30, width - 44));
  const showWatts = wattsAreMeaningful(batt.watts);
  /*
   * I-19: the energy bar takes what is left, rather than a fixed 20 columns.
   *
   * At 50 columns the row came to 51 cells against a 48-cell box, so every
   * consumer wrapped onto a second line and the list silently cost twice the
   * height it was budgeted — the same floor-instead-of-fit mistake the process
   * table's name column used to make.
   */
  const CURSOR_W = 3;
  const PCT_W = 7;
  const WATTS_W = showWatts ? 10 : 0;
  const barW = Math.max(6, Math.min(20, width - CURSOR_W - nameW - PCT_W - WATTS_W));

  /*
   * Row budget, spent in priority order. See I-26.
   *
   * HEAD is the headline, the blank + gauge, the sparkline and the health line.
   * It used to include the list's own title in a constant that was counted but
   * never checked, so on a short terminal the whole screen was drawn regardless
   * and ran off the bottom. Now the advice goes first, then the list rolls up,
   * then the list drops entirely.
   */
  const HEAD = 5;
  const LIST_CHROME = 2;
  const advice =
    ranked[0] && showWatts && batt.watts !== null
      ? estimateWatts(ranked[0].energy, totalEnergy, batt.watts)
      : null;
  let spare = Math.max(0, maxRows - HEAD);
  const showAdvice = advice !== null && advice > 0.2 && spare >= LIST_CHROME + 1 + 2;
  if (showAdvice) spare -= 2;
  const listBudget = spare - LIST_CHROME;
  const showList = listBudget > 0;
  const fit = fitList(ranked.length, listBudget);
  const top = ranked.slice(0, fit.shown);

  return (
    <Box flexDirection="column">
      <Text wrap="truncate">
        <Text bold color={theme.battery}>
          BATTERY{'  '}
        </Text>
        <Text bold color={theme.headline}>
          {batt.percent}%
        </Text>
        <Text color={theme.dim}>
          {'  '}
          {batt.charging ? 'charging' : batt.onAcPower ? 'on AC, holding charge' : 'discharging'}
          {batt.timeRemainingMin !== null
            ? `  ~${minutesToHm(batt.timeRemainingMin)} remaining`
            : ''}
        </Text>
      </Text>
      <Box marginTop={1}>
        <Bar pct={batt.percent} width={Math.min(40, width - 20)} color={theme.battery} />
        <Text color={theme.text}>
          {'  '}
          {showWatts && batt.watts !== null
            ? `${batt.watts.toFixed(1)} W ${batt.watts > 0 ? 'charging' : 'draw'}`
            : 'no net draw'}
        </Text>
      </Box>
      <Box>
        <Sparkline
          values={histories.battery.toArray()}
          width={Math.min(40, width - 20)}
          color={theme.battery}
        />
        <Text color={theme.dim}>{'  charge history'}</Text>
      </Box>
      <Text color={theme.dim} wrap="truncate">
        {batt.cycleCount ?? '—'} cycles · {batt.healthPercent ?? '—'}% health ·{' '}
        {batt.temperatureC !== null ? `${batt.temperatureC.toFixed(1)}°C` : '—'}
      </Text>

      {showList && (
        <Box marginTop={1}>
          <Text bold color={theme.battery} wrap="truncate">
            TOP ENERGY CONSUMERS
            <Text color={theme.dim}>{'   (estimated from CPU time)'}</Text>
          </Text>
        </Box>
      )}
      {top.map((p) => {
        const w = estimateWatts(p.energy, totalEnergy, batt.watts);
        const b = barCells(p.energy ?? 0, barW);
        return (
          <Text key={p.pid} backgroundColor={p.pid === selectedPid ? theme.selectionBg : undefined}>
            <Text bold color={p.pid === selectedPid ? theme.mem : theme.dim}>
              {p.pid === selectedPid ? ' > ' : '   '}
            </Text>
            <Text color={theme.text}>{padEnd(processName(p.command), nameW)}</Text>
            <Text color={theme.battery}>{'█'.repeat(b.filled)}</Text>
            <Text color={theme.track}>{'░'.repeat(b.empty)}</Text>
            <Text color={theme.battery}>{padStart(percent(p.energy), PCT_W)}</Text>
            <Text color={theme.dim}>
              {showWatts && w !== null ? padStart(`~${w.toFixed(1)} W`, WATTS_W) : ''}
            </Text>
          </Text>
        );
      })}

      {listBudget > 0 && fit.hidden > 0 && (
        <Text color={theme.dim}>{`   … ${fit.hidden} more — taller terminal to see them`}</Text>
      )}

      {showAdvice && ranked[0] && batt.watts !== null && (
        <Box marginTop={1} flexDirection="column">
          <Text color={theme.cpuMid} wrap="truncate">
            Killing {processName(ranked[0].command)} could save ~{advice.toFixed(1)} W → about +
            {Math.round((advice / Math.abs(batt.watts)) * (batt.timeRemainingMin ?? 0))} min of
            battery.
          </Text>
        </Box>
      )}
    </Box>
  );
});
