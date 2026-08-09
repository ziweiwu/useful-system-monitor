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
}: {
  snapshot: Snapshot;
  histories: Histories;
  width: number;
}) {
  const gap = 1;
  const cardWidth = Math.max(14, Math.floor((width - gap * 3) / 4));

  const cpu = snapshot.cpu.status === 'ok' ? snapshot.cpu.data : null;
  const mem = snapshot.memory.status === 'ok' ? snapshot.memory.data : null;
  const disk = snapshot.disk.status === 'ok' ? snapshot.disk.data : null;
  const batt = snapshot.battery.status === 'ok' ? snapshot.battery.data : null;

  const memPct = mem ? (mem.usedBytes / mem.totalBytes) * 100 : 0;
  const diskPct = disk ? (disk.usedBytes / disk.totalBytes) * 100 : 0;

  return (
    <Box flexDirection="column">
      <Box>
        <Box marginRight={gap}>
          <Gauge
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
          />
        </Box>
        <Box marginRight={gap}>
          <Gauge
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
          />
        </Box>
        <Box marginRight={gap}>
          <Gauge
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
          />
        </Box>
        <Gauge
          title="BATT"
          width={cardWidth}
          color={theme.battery}
          pct={batt?.percent ?? 0}
          headline={
            batt
              ? `${batt.percent}% ${batt.charging ? '^' : batt.onAcPower ? '=' : 'v'}`
              : '—'
          }
          history={histories.battery.toArray()}
          unavailable={snapshot.battery.status === 'error' ? snapshot.battery.message : undefined}
          lines={
            batt
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
        />
      </Box>
      <Box marginTop={1}>
        {cpu ? (
          <CoreStrip perCore={cpu.perCore} perfCores={snapshot.host.perfCores} />
        ) : (
          <Text color={theme.dim}>CORES  sampling…</Text>
        )}
      </Box>
    </Box>
  );
});
