import { memo } from 'react';
import { Box, Text } from 'ink';
import { BLOCKS, severity, theme } from './theme.js';

function cell(v: number): string {
  const i = Math.min(BLOCKS.length - 1, Math.max(0, Math.round((v / 100) * (BLOCKS.length - 1))));
  return BLOCKS[i]!.repeat(2);
}

/**
 * Per-core utilisation, split performance / efficiency — the meaningful
 * grouping on Apple Silicon, where a busy E-core means something very
 * different from a busy P-core.
 */
export const CoreStrip = memo(function CoreStrip({
  perCore,
  perfCores,
}: {
  perCore: readonly number[];
  perfCores: number;
}) {
  const p = perCore.slice(0, perfCores);
  const e = perCore.slice(perfCores);
  return (
    <Box>
      <Text color={theme.dim}>{'CORES  '}</Text>
      <Text color={theme.text}>P </Text>
      {p.map((v, i) => (
        <Text key={`p${i}`} color={severity(v)}>
          {cell(v)}{' '}
        </Text>
      ))}
      {e.length > 0 && <Text color={theme.text}>{'  E '}</Text>}
      {e.map((v, i) => (
        <Text key={`e${i}`} color={severity(v)}>
          {cell(v)}{' '}
        </Text>
      ))}
    </Box>
  );
});
