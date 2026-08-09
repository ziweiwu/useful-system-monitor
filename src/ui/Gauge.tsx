import { memo } from 'react';
import { Box, Text } from 'ink';
import { Bar } from './Bar.js';
import { Sparkline } from './Sparkline.js';
import { theme } from './theme.js';

/**
 * One metric card: sparkline, headline percentage, bar, and two lines of
 * detail. Border, sparkline and bar all share the metric's hue.
 */
export const Gauge = memo(function Gauge({
  title,
  pct,
  headline,
  history,
  color,
  lines,
  width,
  unavailable,
}: {
  title: string;
  pct: number;
  headline: string;
  history: readonly number[];
  color: string;
  lines: Array<[string, string]>;
  width: number;
  unavailable?: string;
}) {
  const inner = Math.max(6, width - 4);

  return (
    <Box
      flexDirection="column"
      borderStyle="round"
      borderColor={theme.frame}
      width={width}
      paddingX={1}
    >
      <Text color={theme.dim}>{title}</Text>
      {unavailable ? (
        <>
          <Text color={theme.danger}>unavailable</Text>
          <Text color={theme.dim} wrap="truncate">
            {unavailable}
          </Text>
        </>
      ) : (
        <>
          <Sparkline values={history} width={inner} color={color} />
          <Box justifyContent="center">
            <Text bold color={theme.headline}>
              {headline}
            </Text>
          </Box>
          <Bar pct={pct} width={inner} color={color} />
          {lines.map(([label, value], i) => (
            <Text key={i} wrap="truncate">
              <Text color={theme.text}>{label}</Text>
              <Text color={theme.dim}>{value ? ` ${value}` : ''}</Text>
            </Text>
          ))}
        </>
      )}
    </Box>
  );
});
