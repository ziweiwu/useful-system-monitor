import { Box, Text } from 'ink';
import { bytes, percent } from '../core/format.js';
import type { Snapshot } from '../core/types.js';
import { padEnd, padStart } from '../core/width.js';
import type { Histories } from '../hooks/useSampler.js';
import { processName } from '../kill/guards.js';
import { Bar } from './Bar.js';
import { Sparkline } from './Sparkline.js';
import { severity, theme } from './theme.js';

export function CpuView({
  snapshot,
  histories,
  width,
}: {
  snapshot: Snapshot;
  histories: Histories;
  width: number;
}) {
  const cpu = snapshot.cpu.status === 'ok' ? snapshot.cpu.data : null;
  const procs = snapshot.processes.status === 'ok' ? snapshot.processes.data : null;
  if (!cpu) return <Text color={theme.dim}>CPU data unavailable.</Text>;

  const barW = Math.min(36, Math.max(10, width - 34));
  const nameW = Math.max(12, Math.min(34, width - 30));

  return (
    <Box flexDirection="column">
      <Text>
        <Text bold color={theme.headline}>
          {cpu.system.toFixed(1)}%
        </Text>
        <Text color={theme.dim}>
          {'  user '}
          {cpu.userPercent.toFixed(1)}%{'  sys '}
          {cpu.sysPercent.toFixed(1)}%{'  load '}
          {cpu.loadAvg.map((n) => n.toFixed(2)).join('  ')}
        </Text>
      </Text>
      <Box marginBottom={1}>
        <Sparkline values={histories.cpu.toArray()} width={Math.min(60, width - 4)} color={severity(cpu.system)} />
      </Box>
      {cpu.perCore.map((v, i) => (
        <Text key={i}>
          <Text color={theme.dim}>
            {padEnd(i < snapshot.host.perfCores ? `P${i}` : `E${i - snapshot.host.perfCores}`, 4)}
          </Text>
          <Bar pct={v} width={barW} color={severity(v)} />
          <Text color={theme.headline}>{padStart(v.toFixed(0) + '%', 6)}</Text>
        </Text>
      ))}
      {procs && (
        <Box flexDirection="column" marginTop={1}>
          <Text bold color={theme.cpuMid}>
            TOP CPU
          </Text>
          {procs.visible
            .toSorted((a, b) => (b.cpuPercent ?? 0) - (a.cpuPercent ?? 0))
            .slice(0, 6)
            .map((p) => (
              <Text key={p.pid}>
                <Text color={theme.dim}>{padEnd(String(p.pid), 8)}</Text>
                <Text color={theme.text}>{padEnd(processName(p.command), nameW)}</Text>
                <Text bold color={severity(p.cpuPercent ?? 0)}>
                  {padStart(percent(p.cpuPercent) + '%', 8)}
                </Text>
              </Text>
            ))}
        </Box>
      )}
    </Box>
  );
}

export function MemView({
  snapshot,
  histories,
  width,
}: {
  snapshot: Snapshot;
  histories: Histories;
  width: number;
}) {
  const mem = snapshot.memory.status === 'ok' ? snapshot.memory.data : null;
  const procs = snapshot.processes.status === 'ok' ? snapshot.processes.data : null;
  if (!mem) return <Text color={theme.dim}>Memory data unavailable.</Text>;

  const barW = Math.min(36, Math.max(10, width - 34));
  const nameW = Math.max(12, Math.min(34, width - 30));
  // These partition physical memory: they must not overlap, which is why
  // `available` (free + inactive) is shown below rather than in this list.
  const rows: Array<[string, number]> = [
    ['wired', mem.wiredBytes],
    ['active', mem.activeBytes],
    ['inactive', mem.inactiveBytes],
    ['compressed', mem.compressedBytes],
    ['free', mem.freeBytes],
  ];

  return (
    <Box flexDirection="column">
      <Text>
        <Text bold color={theme.headline}>
          {bytes(mem.usedBytes)}
        </Text>
        <Text color={theme.dim}>
          {' / '}
          {bytes(mem.totalBytes)} used
        </Text>
      </Text>
      <Box marginBottom={1}>
        <Sparkline values={histories.memory.toArray()} width={Math.min(60, width - 4)} color={theme.mem} />
      </Box>
      {rows.map(([label, v]) => (
        <Text key={label}>
          <Text color={theme.dim}>{padEnd(label, 12)}</Text>
          <Bar pct={(v / mem.totalBytes) * 100} width={barW} color={theme.mem} />
          <Text color={theme.text}>{padStart(bytes(v), 8)}</Text>
        </Text>
      ))}
      <Box marginTop={1}>
        <Text color={theme.dim}>
          available {bytes(mem.availableBytes)}
          <Text color={theme.dim}>{'   (free + reclaimable inactive)'}</Text>
        </Text>
      </Box>
      <Box>
        <Text color={mem.swapUsedBytes > mem.swapTotalBytes * 0.7 ? theme.cpuHigh : theme.dim}>
          swap {bytes(mem.swapUsedBytes)} / {bytes(mem.swapTotalBytes)}
          {mem.swapUsedBytes > mem.swapTotalBytes * 0.7 ? '   ! heavy swap pressure' : ''}
        </Text>
      </Box>
      {procs && (
        <Box flexDirection="column" marginTop={1}>
          <Text bold color={theme.mem}>
            TOP MEMORY
          </Text>
          {procs.visible
            .toSorted((a, b) => b.rssBytes - a.rssBytes)
            .slice(0, 6)
            .map((p) => (
              <Text key={p.pid}>
                <Text color={theme.dim}>{padEnd(String(p.pid), 8)}</Text>
                <Text color={theme.text}>{padEnd(processName(p.command), nameW)}</Text>
                <Text bold color={theme.mem}>
                  {padStart(bytes(p.rssBytes), 8)}
                </Text>
              </Text>
            ))}
        </Box>
      )}
    </Box>
  );
}
