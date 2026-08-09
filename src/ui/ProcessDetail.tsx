import { memo } from 'react';
import { Box, Text } from 'ink';
import { bytes, percent } from '../core/format.js';
import type { ProcessSample } from '../core/types.js';
import { processName } from '../kill/guards.js';
import type { ProcHistory } from '../hooks/useProcessHistory.js';
import { Sparkline } from './Sparkline.js';
import { severity, theme } from './theme.js';

function Field({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <Text>
      <Text color={theme.dim}>{label.padEnd(10)}</Text>
      <Text color={color ?? theme.text}>{value}</Text>
    </Text>
  );
}

/**
 * Long values (paths, argv) get their own indented block.
 *
 * Inline wrapping would push continuation lines back to column zero and break
 * the label alignment of every field below, so anything that can exceed one
 * line is laid out separately rather than truncated — the full path is often
 * the only way to tell two identically-named helpers apart.
 */
function Block({
  label,
  value,
  width,
  maxLines = 4,
}: {
  label: string;
  value: string;
  width: number;
  maxLines?: number;
}) {
  const indent = '  ';
  const avail = Math.max(20, width - indent.length);
  const lines: string[] = [];
  let rest = value;
  while (rest.length > 0 && lines.length < maxLines) {
    lines.push(rest.slice(0, avail));
    rest = rest.slice(avail);
  }
  if (rest.length > 0 && lines.length > 0) {
    lines[lines.length - 1] = `${lines[lines.length - 1]!.slice(0, avail - 1)}…`;
  }
  return (
    <Box flexDirection="column">
      <Text color={theme.dim}>{label}</Text>
      {lines.map((l, i) => (
        <Text key={i} color={theme.text}>
          {indent}
          {l}
        </Text>
      ))}
    </Box>
  );
}

export const ProcessDetail = memo(function ProcessDetail({
  p,
  history,
  commandLine,
  width,
  maxLinesPerBlock = 4,
  compact = false,
}: {
  p: ProcessSample;
  history: ProcHistory | undefined;
  commandLine: string | null;
  width: number;
  /**
   * Wrapped path/argv lines to allow per block. The caller derives this from
   * the terminal height: the panel is fixed-height apart from these two blocks,
   * so they are the only thing that can push it past the bottom. See I-26.
   */
  maxLinesPerBlock?: number;
  /**
   * Drop the blank lines between the panel's four groups.
   *
   * They are the first thing to give on a short terminal: at the documented
   * 80x24 minimum the panel needs every one of those five rows back before it
   * fits, and a protected process — which adds a warning line — used to push it
   * two rows past the bottom.
   */
  compact?: boolean;
}) {
  const gap = compact ? 0 : 1;
  const boxWidth = Math.min(width, 84);
  const sparkW = Math.min(40, Math.max(10, width - 24));
  const memHistory = history?.mem.toArray() ?? [];
  const memMax = Math.max(1, ...memHistory);

  return (
    <Box flexDirection="column" borderStyle="round" borderColor={theme.frame} paddingX={2} width={boxWidth}>
      <Text bold color={theme.headline} wrap="truncate">
        {processName(p.command)}
      </Text>
      <Box marginTop={gap} flexDirection="column">
        <Field label="PID" value={String(p.pid)} />
        <Field label="Parent" value={String(p.ppid)} />
        <Field label="User" value={p.user} color={p.user === 'root' ? theme.root : theme.text} />
        <Field label="State" value={p.state} />
        <Field
          label="Started"
          value={p.startTime > 0 ? new Date(p.startTime).toLocaleString() : 'unknown'}
        />
      </Box>

      <Box marginTop={gap} flexDirection="column">
        <Block label="Path" value={p.command} width={boxWidth - 6} maxLines={maxLinesPerBlock} />
        {/* Full argv is fetched on demand: it is not worth carrying for 800
            processes when only the selected one is ever displayed. */}
        <Block
          label="Command"
          value={commandLine ?? 'loading…'}
          width={boxWidth - 6}
          maxLines={maxLinesPerBlock}
        />
        {p.protected && (
          <Box marginTop={gap}>
            <Text color={theme.danger}>! Protected — this tool will refuse to kill this process.</Text>
          </Box>
        )}
      </Box>

      <Box marginTop={gap} flexDirection="column">
        <Text>
          <Text color={theme.dim}>{'CPU     '}</Text>
          <Sparkline values={history?.cpu.toArray() ?? []} width={sparkW} color={severity(p.cpuPercent ?? 0)} />
          <Text bold color={theme.headline}>
            {'  '}
            {percent(p.cpuPercent)}%
          </Text>
        </Text>
        <Text>
          <Text color={theme.dim}>{'MEM     '}</Text>
          <Sparkline values={memHistory} width={sparkW} color={theme.mem} max={memMax} />
          <Text bold color={theme.mem}>
            {'  '}
            {bytes(p.rssBytes)}
          </Text>
        </Text>
        <Text>
          <Text color={theme.dim}>{'ENERGY  '}</Text>
          <Sparkline values={history?.energy.toArray() ?? []} width={sparkW} color={theme.battery} />
          <Text bold color={theme.battery}>
            {'  '}
            {percent(p.energy)}
          </Text>
        </Text>
      </Box>

      <Box marginTop={gap}>
        <Text>
          <Text bold color={theme.mem}>
            {'  [k] '}
          </Text>
          <Text color={theme.dim}>kill</Text>
          <Text bold color={theme.mem}>
            {'   [esc] '}
          </Text>
          <Text color={theme.dim}>back</Text>
        </Text>
      </Box>
    </Box>
  );
});
