import { memo } from 'react';
import { Box, Text } from 'ink';
import { fitter, truncate } from '../core/width.js';
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
            {truncate(unavailable, inner)}
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
          {/* I-10b: fitted rather than left to `wrap="truncate"`. The CPU
              card's detail line is one of only two nodes in the app that
              overflow their box, and every distinct value it overflows with
              is kept forever in ink's unevictable wrap cache. */}
          {lines.map(([label, value], i) => {
            const fit = fitter(inner);
            return (
              <Text key={i} wrap="truncate">
                <Text color={theme.text}>{fit(label)}</Text>
                <Text color={theme.dim}>{fit(value ? ` ${value}` : '')}</Text>
              </Text>
            );
          })}
        </>
      )}
    </Box>
  );
});
