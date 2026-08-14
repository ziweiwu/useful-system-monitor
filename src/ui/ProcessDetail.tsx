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
    <Text wrap="truncate">
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
        <Text key={i} color={theme.text} wrap="truncate">
          {indent}
          {l}
        </Text>
      ))}
    </Box>
  );
}

/* Row costs of the panel's parts, used to spend `maxRows` in priority order. */
const BORDER = 2;
const NAME = 1;
/** CPU, MEM and ENERGY, each a sparkline and its current value. */
const METRICS = 3;
const KEYS = 1;
/** PID and User: the two fields that identify what you are about to kill. */
const ID_FIELDS = 2;
/** Parent, State and Started. */
const EXTRA_FIELDS = 3;
/** A block is its label plus at least one wrapped line. */
const BLOCK_MIN = 2;
/** One blank line between each of the panel's four groups. */
const SEPARATORS = 4;

export const ProcessDetail = memo(function ProcessDetail({
  p,
  history,
  commandLine,
  width,
  maxRows = 40,
}: {
  p: ProcessSample;
  history: ProcHistory | undefined;
  commandLine: string | null;
  width: number;
  /**
   * Lines this panel may draw into. See I-26.
   *
   * The panel used to be fixed-height apart from the two wrapped blocks, and
   * the caller compensated with two derived numbers (`compact` and
   * `maxLinesPerBlock`) computed from a `DETAIL_FIXED_ROWS = 18` constant that
   * only held at 80 columns or wider. Below that the panel drew ~16 rows into
   * whatever it was given and ran off the bottom. Budgeting here instead keeps
   * the arithmetic next to the layout it describes, so the two cannot drift.
   */
  maxRows?: number;
}) {
  const boxWidth = Math.min(width, 84);
  const sparkW = Math.min(40, Math.max(10, width - 24));
  const memHistory = history?.mem.toArray() ?? [];
  const memMax = Math.max(1, ...memHistory);

  /*
   * What the panel will not give up: the name and the three current metrics
   * (the reason you opened it), the key that closes it, and — for a process
   * this tool refuses to kill — the line saying so. Everything else is bought
   * with what is left, in the order it earns its rows.
   */
  const protectedRows = p.protected ? 1 : 0;
  const floor = NAME + METRICS + KEYS + protectedRows;
  const bordered = maxRows >= floor + BORDER + ID_FIELDS;
  let spare = maxRows - floor - (bordered ? BORDER : 0);

  const showIds = spare >= ID_FIELDS;
  if (showIds) spare -= ID_FIELDS;
  /* Path before the extra fields: two helpers with the same name are told
     apart by their path, not by their parent PID. */
  const showPath = spare >= BLOCK_MIN;
  if (showPath) spare -= BLOCK_MIN;
  const showExtra = spare >= EXTRA_FIELDS;
  if (showExtra) spare -= EXTRA_FIELDS;
  const showCommand = spare >= BLOCK_MIN;
  if (showCommand) spare -= BLOCK_MIN;
  const gap = spare >= SEPARATORS ? 1 : 0;
  if (gap) spare -= SEPARATORS;
  /* Whatever survives goes to the two blocks, which are the only parts that
     can usefully absorb extra height. */
  const blockLines = Math.max(
    1,
    1 + Math.floor(spare / ((showPath ? 1 : 0) + (showCommand ? 1 : 0) || 1)),
  );

  const body = (
    <>
      <Text bold color={theme.headline} wrap="truncate">
        {processName(p.command)}
      </Text>
      {(showIds || showExtra) && (
        <Box marginTop={gap} flexDirection="column">
          {showIds && <Field label="PID" value={String(p.pid)} />}
          {showExtra && <Field label="Parent" value={String(p.ppid)} />}
          {showIds && (
            <Field label="User" value={p.user} color={p.user === 'root' ? theme.root : theme.text} />
          )}
          {showExtra && <Field label="State" value={p.state} />}
          {showExtra && (
            <Field
              label="Started"
              value={p.startTime > 0 ? new Date(p.startTime).toLocaleString() : 'unknown'}
            />
          )}
        </Box>
      )}

      {(showPath || showCommand || p.protected) && (
        <Box marginTop={gap} flexDirection="column">
          {showPath && (
            <Block label="Path" value={p.command} width={boxWidth - 6} maxLines={blockLines} />
          )}
          {/* Full argv is fetched on demand: it is not worth carrying for 800
              processes when only the selected one is ever displayed. */}
          {showCommand && (
            <Block
              label="Command"
              value={commandLine ?? 'loading…'}
              width={boxWidth - 6}
              maxLines={blockLines}
            />
          )}
          {p.protected && (
            <Box marginTop={gap}>
              <Text color={theme.danger} wrap="truncate">
                ! Protected — this tool will refuse to kill this process.
              </Text>
            </Box>
          )}
        </Box>
      )}

      <Box marginTop={gap} flexDirection="column">
        <Text wrap="truncate">
          <Text color={theme.dim}>{'CPU     '}</Text>
          <Sparkline values={history?.cpu.toArray() ?? []} width={sparkW} color={severity(p.cpuPercent ?? 0)} />
          <Text bold color={theme.headline}>
            {'  '}
            {percent(p.cpuPercent)}%
          </Text>
        </Text>
        <Text wrap="truncate">
          <Text color={theme.dim}>{'MEM     '}</Text>
          <Sparkline values={memHistory} width={sparkW} color={theme.mem} max={memMax} />
          <Text bold color={theme.mem}>
            {'  '}
            {bytes(p.rssBytes)}
          </Text>
        </Text>
        <Text wrap="truncate">
          <Text color={theme.dim}>{'ENERGY  '}</Text>
          <Sparkline values={history?.energy.toArray() ?? []} width={sparkW} color={theme.battery} />
          <Text bold color={theme.battery}>
            {'  '}
            {percent(p.energy)}
          </Text>
        </Text>
      </Box>

      <Box marginTop={gap}>
        <Text wrap="truncate">
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
    </>
  );

  /* On the shortest terminals the border costs two of the rows the panel needs
     for its content, so it goes as well. */
  return bordered ? (
    <Box flexDirection="column" borderStyle="round" borderColor={theme.frame} paddingX={2} width={boxWidth}>
      {body}
    </Box>
  ) : (
    <Box flexDirection="column" width={boxWidth}>
      {body}
    </Box>
  );
});
