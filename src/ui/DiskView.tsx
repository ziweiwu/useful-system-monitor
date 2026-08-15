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
   * Row budget, spent in priority order. See I-26.
   *
   * The two notes below the list were subtracted from the budget but drawn
   * unconditionally, so on a short terminal they were charged for and printed
   * anyway — at 10 rows this screen drew eight lines into a budget of five.
   * They give way before the list does now.
   *
   * Both notes are computed from every volume, not just the visible ones — a
   * full disk that got rolled up is exactly the one worth warning about, so
   * the over-90% warning is the last thing dropped.
   */
  const HEAD = 3;
  const LIST_CHROME = 1;
  /*
   * A note costs one row, and its blank separator is bought separately.
   *
   * It used to cost two, switched on the moment it became affordable — and a
   * two-row section can never switch on without the list beneath it losing a
   * row, because it arrives one row later than the row that paid for it. So
   * growing the terminal by one *shrank* the list: at 70 columns the disk
   * screen showed one volume at 13 rows and none at 14, with the roll-up and
   * a note about the hidden volumes still on screen. The arithmetic only works
   * at a cost of one, so the separator waits until there is slack for it.
   */
  const NOTE = 1;
  const affords = (spare: number) => spare - NOTE - LIST_CHROME >= 1;
  let spare = Math.max(0, maxRows - HEAD);
  const showNet = anyNet && affords(spare);
  if (showNet) spare -= NOTE;
  const showFull = full.length > 0 && affords(spare);
  if (showFull) spare -= NOTE;
  const listBudget = spare - LIST_CHROME;
  const showList = listBudget > 0;
  const fit = fitList(volumes.length, listBudget);
  /* The blank lines above the notes, only once the list needs no more rows. */
  const notes = (showNet ? 1 : 0) + (showFull ? 1 : 0);
  const noteGap = listBudget - volumes.length >= notes ? 1 : 0;

  return (
    <Box flexDirection="column">
      {/* I-19: mount paths and the sample age both grow this line. */}
      <Text wrap="truncate">
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

      {showList && (
        <Text bold color={theme.disk}>
          VOLUMES
        </Text>
      )}
      {volumes.slice(0, fit.shown).map((v) => {
        const pct = pctOf(v);
        return (
          <Text key={v.mount} wrap="truncate">
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

      {showNet && (
        <Box marginTop={noteGap}>
          <Text color={theme.dim} wrap="truncate">
            {/* Worth saying: a stalled share is the usual reason this panel is
                the slowest one to sample. */}
            net = network share, measured by the remote host
          </Text>
        </Box>
      )}

      {showFull && (
        <Box marginTop={noteGap}>
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
