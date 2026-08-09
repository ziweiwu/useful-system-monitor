import { memo } from 'react';
import { Box, Text } from 'ink';
import { minutesToHm, percent } from '../core/format.js';
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
  selectedPid,
}: {
  snapshot: Snapshot;
  histories: Histories;
  width: number;
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

  const top = procs
    ? procs.visible.toSorted((a, b) => (b.energy ?? 0) - (a.energy ?? 0)).slice(0, 8)
    : [];
  const totalEnergy =
    (procs?.visible.reduce((s, p) => s + (p.energy ?? 0), 0) ?? 0) + (procs?.others.energy ?? 0);
  const nameW = Math.max(12, Math.min(30, width - 44));
  const showWatts = wattsAreMeaningful(batt.watts);

  return (
    <Box flexDirection="column">
      <Text>
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
      <Text color={theme.dim}>
        {batt.cycleCount ?? '—'} cycles · {batt.healthPercent ?? '—'}% health ·{' '}
        {batt.temperatureC !== null ? `${batt.temperatureC.toFixed(1)}°C` : '—'}
      </Text>

      <Box marginTop={1}>
        <Text bold color={theme.battery}>
          TOP ENERGY CONSUMERS
        </Text>
        <Text color={theme.dim}>{'   (estimated from CPU time)'}</Text>
      </Box>
      {top.map((p) => {
        const w = estimateWatts(p.energy, totalEnergy, batt.watts);
        const b = barCells(p.energy ?? 0, 20);
        return (
          <Text key={p.pid} backgroundColor={p.pid === selectedPid ? theme.selectionBg : undefined}>
            <Text bold color={p.pid === selectedPid ? theme.mem : theme.dim}>
              {p.pid === selectedPid ? ' > ' : '   '}
            </Text>
            <Text color={theme.text}>{padEnd(processName(p.command), nameW)}</Text>
            <Text color={theme.battery}>{'█'.repeat(b.filled)}</Text>
            <Text color={theme.track}>{'░'.repeat(b.empty)}</Text>
            <Text color={theme.battery}>{padStart(percent(p.energy), 7)}</Text>
            <Text color={theme.dim}>{showWatts && w !== null ? `  ~${w.toFixed(1)} W` : ''}</Text>
          </Text>
        );
      })}

      {top[0] && showWatts && batt.watts !== null && (
        <Box marginTop={1} flexDirection="column">
          {(() => {
            const w = estimateWatts(top[0]!.energy, totalEnergy, batt.watts);
            if (w === null || w <= 0.2) return null;
            const minutes = Math.round((w / Math.abs(batt.watts)) * (batt.timeRemainingMin ?? 0));
            return (
              <Text color={theme.cpuMid}>
                Killing {processName(top[0]!.command)} could save ~{w.toFixed(1)} W → about +
                {minutes} min of battery.
              </Text>
            );
          })()}
        </Box>
      )}
    </Box>
  );
});
