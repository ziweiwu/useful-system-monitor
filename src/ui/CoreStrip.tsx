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
  width,
}: {
  perCore: readonly number[];
  perfCores: number;
  width: number;
}) {
  /*
   * I-19: each core costs three columns, and core counts keep climbing — 24 on
   * an M2 Ultra, which wrapped this strip onto a second line at 80 columns and
   * pushed the frame past the bottom of the terminal. Cores that do not fit are
   * counted rather than dropped silently.
   */
  const fixed = 'CORES  P '.length + '  E '.length;
  const capacity = Math.max(1, Math.floor((width - fixed - 6) / 3));
  const shown = Math.min(perCore.length, capacity);
  const hidden = perCore.length - shown;
  const p = perCore.slice(0, Math.min(shown, perfCores));
  const e = perCore.slice(perfCores, shown);
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
      {hidden > 0 && <Text color={theme.dim}>{`+${hidden}`}</Text>}
    </Box>
  );
});
