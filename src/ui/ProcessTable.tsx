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

/** Widths are derived from terminal width so nothing ever wraps. See I-19. */
export function columnLayout(totalWidth: number) {
  const fixed = CURSOR + PID + 1 + MARK + CPU_BAR + CPU_NUM + 2 + MEM_NUM + 2 + EN_BAR + 1 + EN_NUM + 2;
  const name = Math.max(10, Math.min(NAME_MAX, totalWidth - fixed - USER));
  return { name, user: USER };
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
}: {
  p: ProcessSample;
  selected: boolean;
  width: number;
  totalEnergy: number;
  totalWatts: number | null;
}) {
  const { name, user } = columnLayout(width);
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
      <Text color={theme.dim}>{padEnd(String(p.pid), PID)}</Text>
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
      <Text>{'  '}</Text>
      <Text color={theme.battery}>{'█'.repeat(enBar.filled)}</Text>
      <Text color={theme.track}>{'░'.repeat(enBar.empty)}</Text>
      <Text> </Text>
      <Text color={theme.battery}>{padStart(powerLabel(watts, p.energy, showWatts), EN_NUM)}</Text>
      <Text>{'  '}</Text>
      <Text color={p.user === 'root' ? theme.root : theme.dim}>
        {padEnd(truncate(p.user, user), user)}
      </Text>
    </Text>
  );
});

export const ProcessTable = memo(function ProcessTable({
  processes,
  others,
  selectedPid,
  width,
  rows,
  totalEnergy,
  totalWatts,
  energyAccurate = false,
}: {
  processes: readonly ProcessSample[];
  others: OthersRollup;
  selectedPid: number | null;
  width: number;
  rows: number;
  totalEnergy: number;
  totalWatts: number | null;
  energyAccurate?: boolean;
}) {
  const { name, user } = columnLayout(width);
  const shown = processes.slice(0, rows);
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
        {'  '}
        {/* "est" is dropped only when the numbers are genuinely measured. */}
        {padStart(
          showWatts
            ? energyAccurate
              ? 'POWER'
              : 'POWER est'
            : energyAccurate
              ? 'ENERGY'
              : 'ENERGY est',
          EN_BAR + 1 + EN_NUM,
        )}
        {'  '}
        {padEnd('USER', user)}
      </Text>
      {shown.map((p) => (
        <Row
          key={p.pid}
          p={p}
          selected={p.pid === selectedPid}
          width={width}
          totalEnergy={totalEnergy}
          totalWatts={totalWatts}
        />
      ))}
      {others.count > 0 && (
        <Text color={theme.dim}>
          {' '.repeat(CURSOR)}
          {padEnd('', PID)} {padEnd(`… ${others.count} others`, name)}
          {' '.repeat(MARK)}
          {padStart(others.cpuPercent.toFixed(1), CPU_BAR + CPU_NUM)}
          {'  '}
          {padStart(bytes(others.rssBytes), MEM_NUM)}
          {'  '}
          {padStart(powerLabel(othersWatts, others.energy, showWatts), EN_BAR + 1 + EN_NUM)}
        </Text>
      )}
    </Box>
  );
});
