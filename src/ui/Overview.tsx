import { memo } from 'react';
import { Box, Text } from 'ink';
import { bytes, minutesToHm } from '../core/format.js';
import type { Snapshot } from '../core/types.js';
import type { Histories } from '../hooks/useSampler.js';
import { CoreStrip } from './CoreStrip.js';
import { Gauge } from './Gauge.js';
import { severity, theme } from './theme.js';

export const Overview = memo(function Overview({
  snapshot,
  histories,
  width,
  showCards = true,
  showCores = true,
}: {
  snapshot: Snapshot;
  histories: Histories;
  width: number;
  /** I-26: dropped, in this order, when the terminal is too short for both. */
  showCards?: boolean;
  showCores?: boolean;
}) {
  const gap = 1;
  /*
   * I-19: how many cards the row can hold, not how wide four of them would
   * have to be.
   *
   * The width used to be floored at 14 columns, so on a terminal narrower than
   * 62 the four cards asked for more room than the row had and Ink pushed the
   * overflow into the next line — the whole card block redrew ragged and two
   * lines taller than budgeted. A card below MIN_CARD has nothing left to show
   * once its border and padding are paid for, so the honest move at that width
   * is to draw fewer cards, dropping from the right: battery and disk both
   * have a screen of their own, CPU and memory are what the overview is for.
   */
  const MIN_CARD = 12;
  const fit = Math.max(1, Math.min(4, Math.floor((width + gap) / (MIN_CARD + gap))));
  const cardWidth = Math.floor((width - gap * (fit - 1)) / fit);

  const cpu = snapshot.cpu.status === 'ok' ? snapshot.cpu.data : null;
  const mem = snapshot.memory.status === 'ok' ? snapshot.memory.data : null;
  const disk = snapshot.disk.status === 'ok' ? snapshot.disk.data : null;
  const batt = snapshot.battery.status === 'ok' ? snapshot.battery.data : null;

  const memPct = mem ? (mem.usedBytes / mem.totalBytes) * 100 : 0;
  const diskPct = disk ? (disk.usedBytes / disk.totalBytes) * 100 : 0;

  /* Ordered most to least worth the width; `fit` takes from the front. */
  const cards = [
    <Gauge
      key="cpu"
      title="CPU"
      width={cardWidth}
      color={severity(cpu?.system ?? 0)}
      pct={cpu?.system ?? 0}
      headline={cpu ? `${cpu.system.toFixed(1)}%` : '—'}
      history={histories.cpu.toArray()}
      unavailable={snapshot.cpu.status === 'error' ? snapshot.cpu.message : undefined}
      lines={
        cpu
          ? [
              [`user ${cpu.userPercent.toFixed(0)}%`, `sys ${cpu.sysPercent.toFixed(0)}%`],
              ['load', cpu.loadAvg.slice(0, 2).map((n) => n.toFixed(1)).join(' ')],
            ]
          : []
      }
    />,
    <Gauge
      key="mem"
      title="MEM"
      width={cardWidth}
      color={theme.mem}
      pct={memPct}
      headline={mem ? `${memPct.toFixed(1)}%` : '—'}
      history={histories.memory.toArray()}
      unavailable={snapshot.memory.status === 'error' ? snapshot.memory.message : undefined}
      lines={
        mem
          ? [
              [bytes(mem.usedBytes), `/ ${bytes(mem.totalBytes)}`],
              [
                'swap',
                `${bytes(mem.swapUsedBytes)}${mem.swapUsedBytes > mem.swapTotalBytes * 0.7 ? '  !' : ''}`,
              ],
            ]
          : []
      }
    />,
    <Gauge
      key="disk"
      title="DISK"
      width={cardWidth}
      color={theme.disk}
      pct={diskPct}
      headline={disk ? `${diskPct.toFixed(0)}%` : '—'}
      history={histories.disk.toArray()}
      unavailable={snapshot.disk.status === 'error' ? snapshot.disk.message : undefined}
      lines={
        disk
          ? [
              [bytes(disk.usedBytes), `/ ${bytes(disk.totalBytes)}`],
              ['free', bytes(disk.freeBytes)],
            ]
          : []
      }
    />,
    <Gauge
    key="batt"
    title="BATT"
    width={cardWidth}
    color={theme.battery}
    // A desktop Mac has no battery; that is a fact to report, not a gap.
    unavailable={
      snapshot.battery.status === 'error'
        ? snapshot.battery.message
        : batt && !batt.present
          ? 'no battery — on AC power'
          : undefined
    }
    pct={batt?.percent ?? 0}
    headline={
      batt
        ? `${batt.percent}% ${batt.charging ? '^' : batt.onAcPower ? '=' : 'v'}`
        : '—'
    }
    history={histories.battery.toArray()}
    lines={
      batt && batt.present
        ? [
            [
              batt.watts !== null && Math.abs(batt.watts) >= 0.5
                ? `${batt.watts.toFixed(1)}W`
                : batt.onAcPower
                  ? 'on AC'
                  : '—',
              batt.timeRemainingMin !== null ? minutesToHm(batt.timeRemainingMin) : '',
            ],
            [
              `${batt.cycleCount ?? '—'}cy`,
              batt.temperatureC !== null ? `${batt.temperatureC.toFixed(1)}C` : '',
            ],
          ]
        : []
    }
    />,
  ];

  return (
    <Box flexDirection="column">
      {/* One gapped row: wrapper Boxes around each card would shrink and
          clip the card inside them at wider terminals. */}
      {showCards && <Box gap={gap}>{cards.slice(0, fit)}</Box>}
      {showCores && (
        <Box marginTop={1}>
          {cpu ? (
            <CoreStrip perCore={cpu.perCore} perfCores={snapshot.host.perfCores} width={width} />
          ) : (
            <Text color={theme.dim}>CORES  sampling…</Text>
          )}
        </Box>
      )}
    </Box>
  );
});
