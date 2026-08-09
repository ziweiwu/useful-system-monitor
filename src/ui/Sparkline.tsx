import { Text } from 'ink';
import { sparkline } from './theme.js';

export function Sparkline({
  values,
  width,
  color,
  max = 100,
}: {
  values: readonly number[];
  width: number;
  color: string;
  max?: number;
}) {
  return (
    <Text color={color} dimColor>
      {sparkline(values, width, max)}
    </Text>
  );
}
