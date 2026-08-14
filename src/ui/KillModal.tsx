import { Box, Text } from 'ink';
import { bytes, percent } from '../core/format.js';
import type { ProcessSample } from '../core/types.js';
import { estimateWatts } from '../core/scoring.js';
import { processName, type KillCheck } from '../kill/guards.js';
import { theme } from './theme.js';

/**
 * Rows the full panel needs: two borders, the title, the name, the identity
 * line, three blank separators, two lines of detail, the warning, and three
 * key hints.
 */
const FULL_ROWS = 14;
/** The same panel with the separators and the detail lines given up. */
const COMPACT_ROWS = 9;

/**
 * Confirmation always names the process. SIGKILL needs a second, distinct
 * keypress so it cannot be reached by holding one key. See I-15.
 *
 * The panel is sized to the terminal rather than to a constant. It used to be
 * `width={64}` with no height budget at all, so on a 50x10 terminal it drew a
 * 64-cell box 14 rows tall into a 50x10 frame — Ink wrapped it and the app
 * scrolled its own header away at the exact moment the user is being asked to
 * confirm something irreversible. That is the one screen where a corrupted
 * frame is least acceptable, so it degrades in three steps instead, and the
 * two things it will not give up are the process's name and the key that
 * cancels. See I-15, I-26.
 */
export function KillModal({
  target,
  check,
  armedKill,
  totalEnergy,
  totalWatts,
  width = 64,
  maxRows = FULL_ROWS,
}: {
  target: ProcessSample;
  check: KillCheck;
  armedKill: boolean;
  totalEnergy: number;
  totalWatts: number | null;
  width?: number;
  maxRows?: number;
}) {
  const watts = estimateWatts(target.energy, totalEnergy, totalWatts);
  const boxWidth = Math.min(64, width);
  const full = maxRows >= FULL_ROWS;
  const gap = full ? 1 : 0;

  /*
   * Below even the compact panel, the border and the box go: three unadorned
   * lines that still say what is about to be killed, that it is irreversible,
   * and how to back out.
   */
  if (maxRows < COMPACT_ROWS) {
    return (
      <Box flexDirection="column" width={boxWidth}>
        <Text bold color={check.allowed ? theme.battery : theme.danger} wrap="truncate">
          {check.allowed ? 'KILL PROCESS ' : 'REFUSED '}
          <Text color={theme.headline}>
            {processName(target.command)} ({target.pid})
          </Text>
        </Text>
        <Text color={check.allowed ? theme.cpuMid : theme.danger} wrap="truncate">
          {check.allowed ? '! Unsaved work will be lost.' : check.refusal.message}
        </Text>
        <Text color={theme.dim} wrap="truncate">
          {check.allowed ? 't SIGTERM   k SIGKILL (twice)   esc cancel' : 'esc back'}
        </Text>
      </Box>
    );
  }

  return (
    <Box
      flexDirection="column"
      borderStyle="round"
      borderColor={check.allowed ? theme.battery : theme.danger}
      paddingX={2}
      paddingY={0}
      width={boxWidth}
    >
      <Text bold color={check.allowed ? theme.battery : theme.danger}>
        {check.allowed ? 'KILL PROCESS' : 'REFUSED'}
      </Text>
      <Text bold color={theme.headline} wrap="truncate">
        {processName(target.command)}
      </Text>
      <Text color={theme.dim} wrap="truncate">
        PID {target.pid} · owner {target.user} · parent {target.ppid}
      </Text>

      {check.allowed ? (
        <>
          {full && (
            <>
              <Box marginTop={1}>
                <Text color={theme.text} wrap="truncate">
                  CPU {percent(target.cpuPercent)}%   MEM {bytes(target.rssBytes)}   ENERGY{' '}
                  {percent(target.energy)}
                </Text>
              </Box>
              <Text color={theme.dim} wrap="truncate">
                Reclaims ~{bytes(target.rssBytes)}
                {watts !== null ? ` and ~${watts.toFixed(1)} W` : ''}
              </Text>
            </>
          )}
          <Box marginTop={gap}>
            <Text color={theme.cpuMid} wrap="truncate">
              ! Unsaved work in this process will be lost.
            </Text>
          </Box>
          <Box marginTop={gap} flexDirection="column">
            <Text>
              <Text bold color={theme.mem}>
                {'  [t] '}
              </Text>
              <Text color={theme.text}>SIGTERM</Text>
              <Text color={theme.dim}> graceful — recommended</Text>
            </Text>
            <Text>
              <Text bold color={armedKill ? theme.danger : theme.mem}>
                {'  [k] '}
              </Text>
              <Text color={theme.text}>SIGKILL</Text>
              <Text color={armedKill ? theme.danger : theme.dim}>
                {armedKill ? ' press k again to confirm' : ' force — press twice'}
              </Text>
            </Text>
            <Text>
              <Text bold color={theme.mem}>
                {'  [esc] '}
              </Text>
              <Text color={theme.dim}>cancel</Text>
            </Text>
          </Box>
        </>
      ) : (
        <Box marginTop={gap} flexDirection="column">
          <Text color={theme.danger} wrap={full ? 'wrap' : 'truncate'}>
            {check.refusal.message}
          </Text>
          <Box marginTop={gap}>
            <Text>
              <Text bold color={theme.mem}>
                {'  [esc] '}
              </Text>
              <Text color={theme.dim}>back</Text>
            </Text>
          </Box>
        </Box>
      )}
    </Box>
  );
}
