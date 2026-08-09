import type { SortKey } from './scoring.js';

/**
 * Command-line parsing, kept free of I/O so it can be tested directly and so
 * importing it never starts the app.
 *
 * The rule here is I-24: an option this tool does not understand, or a value it
 * cannot use, is an error naming the cause and the remedy — never something to
 * ignore. Ignoring them is how `--jsonn` silently produced text for a script
 * that asked for JSON, and how `--sort bogus` returned rows in no order at all.
 */

export const SORT_KEYS = ['cpu', 'mem', 'energy'] as const satisfies readonly SortKey[];

/** Refresh bounds. Below 1s the render cost dominates; above an hour it is not
    a monitor. A negative value used to reach `setInterval`, which clamps to 1ms
    and spins the render loop at ~1000Hz. */
export const MIN_INTERVAL_SEC = 1;
export const MAX_INTERVAL_SEC = 3600;
export const MAX_TOP = 10_000;

export interface Options {
  accurateEnergy: boolean;
  mock: boolean;
  json: boolean;
  top: number;
  sort: SortKey;
  interval: number | null;
  help: boolean;
  version: boolean;
}

export const DEFAULT_OPTIONS: Options = {
  accurateEnergy: false,
  mock: false,
  json: false,
  top: 10,
  sort: 'cpu',
  interval: null,
  help: false,
  version: false,
};

export type ParseResult =
  | { ok: true; options: Options }
  /** Cause, phrased for a terminal. The caller adds the pointer to --help. */
  | { ok: false; error: string };

function positiveInt(name: string, raw: string, max: number): number | string {
  const n = Number(raw);
  if (!Number.isInteger(n) || n < 1 || n > max) {
    return `${name} needs a whole number between 1 and ${max} (got "${raw}")`;
  }
  return n;
}

export function parseArgs(argv: readonly string[]): ParseResult {
  const o: Options = { ...DEFAULT_OPTIONS };

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]!;
    /* `--name=value` and `--name value` are the same option; split once here so
       each case below sees one shape. */
    const eq = arg.startsWith('--') ? arg.indexOf('=') : -1;
    const name = eq > 0 ? arg.slice(0, eq) : arg;
    const inline = eq > 0 ? arg.slice(eq + 1) : null;

    /** The option's value, or null when none was supplied. Consumes the next
        argument only when it is a value rather than another option — a leading
        `-` counts as a value when it parses as a number, so `--interval -5`
        reports the real problem instead of "missing value". */
    const take = (): string | null => {
      if (inline !== null) return inline;
      const next = argv[i + 1];
      if (next === undefined) return null;
      if (next.startsWith('-') && !Number.isFinite(Number(next))) return null;
      i++;
      return next;
    };
    const missing = `${name} needs a value`;

    switch (name) {
      case '--mock':
        o.mock = true;
        break;
      case '--json':
        o.json = true;
        break;
      case '--help':
      case '-h':
        o.help = true;
        break;
      case '--version':
      case '-v':
      case '-V':
        o.version = true;
        break;
      case '--top': {
        const raw = take();
        if (raw === null) return { ok: false, error: missing };
        const n = positiveInt('--top', raw, MAX_TOP);
        if (typeof n === 'string') return { ok: false, error: n };
        o.top = n;
        break;
      }
      case '--interval': {
        const raw = take();
        if (raw === null) return { ok: false, error: missing };
        const n = Number(raw);
        if (!Number.isFinite(n) || n < MIN_INTERVAL_SEC || n > MAX_INTERVAL_SEC) {
          return {
            ok: false,
            error: `--interval needs a number of seconds between ${MIN_INTERVAL_SEC} and ${MAX_INTERVAL_SEC} (got "${raw}")`,
          };
        }
        o.interval = n;
        break;
      }
      case '--sort': {
        const raw = take();
        if (raw === null) return { ok: false, error: missing };
        if (!(SORT_KEYS as readonly string[]).includes(raw)) {
          return {
            ok: false,
            error: `--sort needs one of ${SORT_KEYS.join(', ')} (got "${raw}")`,
          };
        }
        o.sort = raw as SortKey;
        break;
      }
      case '--energy': {
        const raw = take();
        if (raw === null) return { ok: false, error: `${name} needs a value: accurate` };
        if (raw !== 'accurate') {
          return { ok: false, error: `--energy only accepts "accurate" (got "${raw}")` };
        }
        o.accurateEnergy = true;
        break;
      }
      default:
        return {
          ok: false,
          error: arg.startsWith('-')
            ? `unknown option "${arg}"`
            : `unexpected argument "${arg}" — this tool takes options only`,
        };
    }
  }

  return { ok: true, options: o };
}
