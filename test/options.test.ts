import { describe, expect, it } from 'vitest';
import { DEFAULT_OPTIONS, parseArgs, SORT_KEYS } from '../src/core/options.js';

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

  it('rejects a sort key it cannot sort by, and lists the ones it can', () => {
    // `as SortKey` used to make this a NaN comparator, which returned the rows
    // in no particular order at all — wrong output, exit 0.
    const e = err(['--sort', 'bogus']);
    expect(e).toContain('bogus');
    for (const k of SORT_KEYS) expect(e).toContain(k);
  });

  it.each([
    ['--top', 'abc'],
    ['--top', '0'],
    ['--top', '-5'],
    ['--top', '2.5'],
    ['--top', '999999'],
  ])('rejects %s %s', (flag, value) => {
    expect(err([flag, value])).toContain(value);
  });

  it.each([
    ['-5', 'a negative interval reached setInterval, which clamps to 1ms'],
    ['0', 'below the 1s floor'],
    ['0.5', 'below the 1s floor'],
    ['86400', 'above the 1h ceiling'],
    ['abc', 'not a number'],
  ])('rejects --interval %s (%s)', (value) => {
    expect(err(['--interval', value])).toContain(value);
  });

  it('rejects an --energy mode it does not have', () => {
    expect(err(['--energy', 'fast'])).toContain('accurate');
    expect(err(['--energy=fast'])).toContain('accurate');
  });

  it('says which option is missing its value', () => {
    expect(err(['--top'])).toBe('--top needs a value');
    expect(err(['--sort'])).toBe('--sort needs a value');
    expect(err(['--interval'])).toBe('--interval needs a value');
  });

  it('does not swallow the next option as a value', () => {
    expect(err(['--sort', '--json'])).toBe('--sort needs a value');
  });
});

describe('option parsing', () => {
  it('defaults to the dashboard with nothing set', () => {
    expect(ok([])).toEqual(DEFAULT_OPTIONS);
  });

  it('accepts both --name value and --name=value', () => {
    expect(ok(['--top', '25']).top).toBe(25);
    expect(ok(['--top=25']).top).toBe(25);
    expect(ok(['--sort', 'mem']).sort).toBe('mem');
    expect(ok(['--sort=mem']).sort).toBe('mem');
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
  });

  it('reports the version for every spelling of the flag', () => {
    for (const flag of ['-v', '-V', '--version']) expect(ok([flag]).version).toBe(true);
  });

  it('combines options in any order', () => {
    const o = ok(['--json', '--top=3', '--mock', '--sort', 'energy']);
    expect(o).toMatchObject({ json: true, top: 3, mock: true, sort: 'energy' });
  });
});
