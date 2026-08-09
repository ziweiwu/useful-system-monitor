import { Box, Text } from 'ink';
import { age, bytes } from '../core/format.js';
import { fitList } from '../core/rows.js';
import type { Snapshot, VolumeUsage } from '../core/types.js';
import { padEnd, padStart, truncate } from '../core/width.js';
import type { Histories } from '../hooks/useSampler.js';
import { Bar } from './Bar.js';
import { Sparkline } from './Sparkline.js';
import { theme } from './theme.js';

/**
 * Fullness is not the same risk curve as CPU load, so this does not reuse
 * `severity`: a disk at 60% is entirely healthy, while one at 90% is close to
 * the point where macOS starts failing writes and Time Machine snapshots.
 */
export function diskSeverity(pct: number): string {
  if (pct < 75) return theme.disk;
  if (pct < 90) return theme.cpuMid;
  return theme.cpuHigh;
}

function pctOf(v: VolumeUsage): number {
  return v.totalBytes > 0 ? (v.usedBytes / v.totalBytes) * 100 : 0;
}

export function DiskView({
  snapshot,
  histories,
  width,
  maxRows,
  now,
}: {
  snapshot: Snapshot;
  histories: Histories;
  width: number;
  /** Lines this screen may draw into. See I-26. */
  maxRows: number;
  /** For the sample age. Disk is on a 5-minute tier, so staleness is visible
      here in a way it is not for CPU, and a number with no age reads as live. */
  now: number;
}) {
  const disk = snapshot.disk.status === 'ok' ? snapshot.disk.data : null;
  if (!disk) {
    const msg = snapshot.disk.status === 'error' ? snapshot.disk.message : null;
    return <Text color={theme.dim}>{msg ? `Disk data unavailable: ${msg}` : 'Disk data unavailable.'}</Text>;
  }

  const rootPct = disk.totalBytes > 0 ? (disk.usedBytes / disk.totalBytes) * 100 : 0;
  /*
   * Older samples predate the per-volume field, and a provider is free to
   * report none, so the root volume is synthesised rather than assumed. This is
   * also what keeps the panel useful on a machine with a single filesystem.
   */
  const volumes: VolumeUsage[] =
    disk.volumes.length > 0
      ? disk.volumes
      : [
          {
            mount: disk.mount,
            device: '',
            totalBytes: disk.totalBytes,
            usedBytes: disk.usedBytes,
            freeBytes: disk.freeBytes,
            network: false,
          },
        ];

  /* Columns are budgeted from the widest mount actually present, so a lone `/`
     does not reserve 20 blank columns, and a deep path is truncated rather than
     allowed to wrap (I-19). */
  const full = volumes.filter((v) => pctOf(v) >= 90);
  const anyNet = volumes.some((v) => v.network);
  const widest = Math.max(...volumes.map((v) => v.mount.length));
  const mountW = Math.max(6, Math.min(24, widest));
  /* 4 columns for the `net` marker, the rest for the three numeric columns. */
  const barW = Math.max(8, Math.min(30, width - mountW - 36));

  /*
   * Row budget. Fixed: the headline, the sparkline and its blank, and the
   * VOLUMES title; the two notes below cost a blank and a line each.
   *
   * Both notes are computed from every volume, not just the visible ones — a
   * full disk that got rolled up is exactly the one worth warning about.
   */
  const CHROME = 4;
  const listBudget = maxRows - CHROME - (anyNet ? 2 : 0) - (full.length ? 2 : 0);
  const fit = fitList(volumes.length, listBudget);

  return (
    <Box flexDirection="column">
      <Text>
        <Text bold color={theme.headline}>
          {bytes(disk.usedBytes)}
        </Text>
        <Text color={theme.dim}>
          {' / '}
          {bytes(disk.totalBytes)} used on {disk.mount}
          {'   '}
        </Text>
        <Text color={diskSeverity(rootPct)}>{bytes(disk.freeBytes)} free</Text>
        {snapshot.disk.status === 'ok' && (
          <Text color={theme.dim}>{`   sampled ${age(snapshot.disk.sampledAt, now)}`}</Text>
        )}
      </Text>
      <Box marginBottom={1}>
        <Sparkline
          values={histories.disk.toArray()}
          width={Math.min(60, width - 4)}
          color={theme.disk}
        />
      </Box>

      <Text bold color={theme.disk}>
        VOLUMES
      </Text>
      {volumes.slice(0, fit.shown).map((v) => {
        const pct = pctOf(v);
        return (
          <Text key={v.mount}>
            <Text color={theme.text}>{padEnd(truncate(v.mount, mountW), mountW + 1)}</Text>
            <Bar pct={pct} width={barW} color={diskSeverity(pct)} />
            <Text color={theme.headline}>{padStart(pct.toFixed(0) + '%', 5)}</Text>
            <Text color={theme.dim}>{padStart(bytes(v.freeBytes) + ' free', 12)}</Text>
            <Text color={theme.dim}>{padStart('of ' + bytes(v.totalBytes), 10)}</Text>
            <Text color={theme.dim}>{v.network ? '  net' : ''}</Text>
          </Text>
        );
      })}

      {listBudget > 0 && fit.hidden > 0 && (
        <Text color={theme.dim}>{`… ${fit.hidden} more volumes — taller terminal to see them`}</Text>
      )}

      {anyNet && (
        <Box marginTop={1}>
          <Text color={theme.dim} wrap="truncate">
            {/* Worth saying: a stalled share is the usual reason this panel is
                the slowest one to sample. */}
            net = network share, measured by the remote host
          </Text>
        </Box>
      )}

      {full.length > 0 && (
        <Box marginTop={1}>
          {/* I-19: one truncated line. Wrapping this pushed the footer off a
              24-row terminal whenever two volumes were full at once. */}
          <Text color={theme.cpuHigh} wrap="truncate">
            {`! ${full.map((v) => v.mount).join(', ')} above 90% — writes and snapshots start to fail`}
          </Text>
        </Box>
      )}
    </Box>
  );
}
