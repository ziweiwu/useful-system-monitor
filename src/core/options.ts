/**
 * Command-line parsing, kept free of I/O so it can be tested directly and so
 * importing it never starts the app.
 *
 * Two rules keep this surface small:
 *
 * Anything the tool does not understand is an error naming the cause and the
 * remedy, never something to ignore (I-24). Ignoring them is how `--jsonn`
 * silently produced text for a script that asked for JSON, and how
 * `--sort bogus` returned rows in no order at all.
 *
 * And an option has to buy something the terminal cannot. Row counts and
 * ordering are `head` and `jq`'s job, so `--json` hands over the whole working
 * set and stays out of the way. What is left either changes what is measured
 * (`--energy`), how often (`--interval`), or where it comes from (`--mock`).
 */

/** Refresh floor. Below a second the render cost dominates the sample, and a
    negative value used to reach `setInterval`, which clamps to 1ms and spins
    the render loop at ~1000Hz. There is deliberately no ceiling: a very lazy
    background pane is a legitimate thing to want. */
export const MIN_INTERVAL_SEC = 1;

export interface Options {
  accurateEnergy: boolean;
  mock: boolean;
  json: boolean;
  interval: number | null;
  help: boolean;
  version: boolean;
}

export const DEFAULT_OPTIONS: Options = {
  accurateEnergy: false,
  mock: false,
  json: false,
  interval: null,
  help: false,
  version: false,
};

export type ParseResult =
  | { ok: true; options: Options }
  /** Cause, phrased for a terminal. The caller adds the pointer to --help. */
  | { ok: false; error: string };

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
        o.version = true;
        break;
      case '--interval': {
        const raw = take();
        if (raw === null) return { ok: false, error: `${name} needs a value` };
        const n = Number(raw);
        if (!Number.isFinite(n) || n < MIN_INTERVAL_SEC) {
          return {
            ok: false,
            error: `--interval needs a number of seconds, ${MIN_INTERVAL_SEC} or more (got "${raw}")`,
          };
        }
        o.interval = n;
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
