import { Text } from 'ink';
import { barCells, theme } from './theme.js';

/** Filled portion in the metric's hue, remainder in a dim track. */
export function Bar({ pct, width, color }: { pct: number; width: number; color: string }) {
  const { filled, empty } = barCells(pct, width);
  return (
    <Text>
      <Text color={color}>{'█'.repeat(filled)}</Text>
      <Text color={theme.track}>{'░'.repeat(empty)}</Text>
    </Text>
  );
}
