import { describe, expect, it } from 'vitest';
import {
  DEFAULT_OPTIONS,
  MAX_INTERVAL_SEC,
  MIN_INTERVAL_SEC,
  parseArgs,
} from '../src/core/options.js';

/** The parsed options, or a failure if the arguments were rejected. */
function ok(argv: string[]) {
  const r = parseArgs(argv);
  if (!r.ok) throw new Error(`expected success, got: ${r.error}`);
  return r.options;
}

/** The error message, or a failure if the arguments were accepted. */
function err(argv: string[]): string {
  const r = parseArgs(argv);
  if (r.ok) throw new Error(`expected an error, got: ${JSON.stringify(r.options)}`);
  return r.error;
}

describe('I-24: unknown or unusable options are rejected, never ignored', () => {
  it('names the option it does not know', () => {
    // A typo'd --json used to be ignored, so a script asking for JSON silently
    // got text and failed somewhere further downstream.
    expect(err(['--jsonn'])).toContain('unknown option "--jsonn"');
    expect(err(['-x'])).toContain('unknown option "-x"');
  });

  it('rejects positional arguments, which this tool has none of', () => {
    expect(err(['report.txt'])).toContain('unexpected argument');
  });

  it.each([
    ['-5', 'a negative interval reached setInterval, which clamps to 1ms'],
    ['0', 'below the 1s floor'],
    ['0.5', 'below the 1s floor'],
    ['abc', 'not a number'],
  ])('rejects --interval %s (%s)', (value) => {
    expect(err(['--interval', value])).toContain(value);
  });

  it('has no upper bound on --interval: a very lazy pane is a fair thing to want', () => {
    expect(ok(['--interval', '7200']).interval).toBe(7200);
    expect(ok(['--interval', String(MIN_INTERVAL_SEC)]).interval).toBe(MIN_INTERVAL_SEC);
  });

  it('rejects an --energy mode it does not have', () => {
    expect(err(['--energy', 'fast'])).toContain('accurate');
    expect(err(['--energy=fast'])).toContain('accurate');
  });

  it('says which option is missing its value', () => {
    expect(err(['--interval'])).toBe('--interval needs a value');
  });

  it('does not swallow the next option as a value', () => {
    expect(err(['--interval', '--json'])).toBe('--interval needs a value');
  });
});

describe('option parsing', () => {
  it('defaults to the dashboard with nothing set', () => {
    expect(ok([])).toEqual(DEFAULT_OPTIONS);
  });

  it('accepts both --name value and --name=value', () => {
    expect(ok(['--interval', '2']).interval).toBe(2);
    expect(ok(['--interval=2']).interval).toBe(2);
    expect(ok(['--energy', 'accurate']).accurateEnergy).toBe(true);
    expect(ok(['--energy=accurate']).accurateEnergy).toBe(true);
  });

  it('takes the flags with no value', () => {
    expect(ok(['--mock']).mock).toBe(true);
    expect(ok(['--json']).json).toBe(true);
    expect(ok(['-h']).help).toBe(true);
    expect(ok(['--help']).help).toBe(true);
    expect(ok(['--version']).version).toBe(true);
  });

  it('keeps the option surface to what a terminal cannot already do', () => {
    // Row counts and ordering belong to head and jq, so there is nothing here
    // to configure them with. Guard the list so it does not creep back.
    expect(Object.keys(DEFAULT_OPTIONS).toSorted()).toEqual([
      'accurateEnergy',
      'help',
      'interval',
      'json',
      'mock',
      'version',
    ]);
    for (const gone of ['--top', '--sort', '-v', '-V']) {
      expect(err([gone])).toContain('unknown option');
    }
  });

  it('combines options in any order', () => {
    const o = ok(['--json', '--mock', '--interval=3']);
    expect(o).toMatchObject({ json: true, mock: true, interval: 3 });
  });
});

describe('I-24: an interval a timer cannot hold is an error, not a no-op', () => {
  /*
   * setInterval takes a signed 32-bit millisecond count, so a delay past
   * 2^31-1 ms is silently clamped to 1 ms. `--interval 3000000` measured 265
   * fires in 300 ms — the same ~1000Hz render spin the lower bound exists to
   * prevent, reached from the other end.
   */
  it('rejects a value past the 32-bit timer ceiling', () => {
    const r = parseArgs(['--interval', String(MAX_INTERVAL_SEC + 1)]);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toMatch(/between/);
  });

  it('accepts the ceiling itself, and it still fits a timer', () => {
    const r = parseArgs(['--interval', String(MAX_INTERVAL_SEC)]);
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.options.interval! * 1000).toBeLessThanOrEqual(2_147_483_647);
  });

  it('still rejects the value that used to spin the loop', () => {
    expect(parseArgs(['--interval', '3000000']).ok).toBe(false);
  });
});
