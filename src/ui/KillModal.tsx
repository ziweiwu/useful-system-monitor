import { Box, Text } from 'ink';
import { bytes, percent } from '../core/format.js';
import type { ProcessSample } from '../core/types.js';
import { estimateWatts } from '../core/scoring.js';
import { processName, type KillCheck } from '../kill/guards.js';
import { theme } from './theme.js';

/**
 * Confirmation always names the process. SIGKILL needs a second, distinct
 * keypress so it cannot be reached by holding one key. See I-15.
 */
export function KillModal({
  target,
  check,
  armedKill,
  totalEnergy,
  totalWatts,
}: {
  target: ProcessSample;
  check: KillCheck;
  armedKill: boolean;
  totalEnergy: number;
  totalWatts: number | null;
}) {
  const watts = estimateWatts(target.energy, totalEnergy, totalWatts);

  return (
    <Box
      flexDirection="column"
      borderStyle="round"
      borderColor={check.allowed ? theme.battery : theme.danger}
      paddingX={2}
      paddingY={0}
      width={64}
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
          <Box marginTop={1}>
            <Text color={theme.text}>
              CPU {percent(target.cpuPercent)}%   MEM {bytes(target.rssBytes)}   ENERGY{' '}
              {percent(target.energy)}
            </Text>
          </Box>
          <Text color={theme.dim}>
            Reclaims ~{bytes(target.rssBytes)}
            {watts !== null ? ` and ~${watts.toFixed(1)} W` : ''}
          </Text>
          <Box marginTop={1}>
            <Text color={theme.cpuMid}>! Unsaved work in this process will be lost.</Text>
          </Box>
          <Box marginTop={1} flexDirection="column">
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
        <Box marginTop={1} flexDirection="column">
          <Text color={theme.danger} wrap="wrap">
            {check.refusal.message}
          </Text>
          <Box marginTop={1}>
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
