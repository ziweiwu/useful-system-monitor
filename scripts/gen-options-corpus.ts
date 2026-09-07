/**
 * Records how the TypeScript parser answers a spread of argument vectors, so
 * the Rust port can be checked against it exactly.
 *
 * The interesting part is not the flags — it is `Number()`. The validation of
 * `--interval` runs its argument through it, and no Rust parser reproduces its
 * behaviour: `Number("")` is 0, `Number("0x10")` is 16, and `Number("inf")` is
 * NaN while Rust's `parse::<f64>()` happily returns infinity. Each of those is
 * reachable from a shell.
 */
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { MAX_INTERVAL_SEC, parseArgs } from '../src/core/options.js';

/** Values aimed squarely at Number()'s edges. */
const VALUES = [
  '1', '2', '10', '7200', '2.5', '1e3', '1E3', '.5', '5.', '+5', '-5', '0', '-0',
  '', ' ', '  3  ', '\t4\n', 'abc', 'inf', 'Infinity', '-Infinity', 'NaN', 'nan',
  '0x10', '0X10', '0b101', '0o17', '0xzz', '1_000', '1,5', '3000000',
  String(MAX_INTERVAL_SEC), String(MAX_INTERVAL_SEC + 1), '1e-3', '999999999999',
];

const ARGVS: string[][] = [
  [], ['--mock'], ['--json'], ['-h'], ['--help'], ['--version'],
  ['--json', '--mock'], ['--json', '--mock', '--interval=3'],
  ['--energy', 'accurate'], ['--energy=accurate'], ['--energy', 'fast'], ['--energy=fast'],
  ['--energy'], ['--interval'], ['--interval', '--json'], ['--energy', '--json'],
  ['--jsonn'], ['-x'], ['report.txt'], ['--top'], ['--sort'], ['-v'], ['-V'],
  ['--interval=2', '--energy=accurate', '--json'],
  ['--mock', '--mock'], ['--json', '--json'],
  ['--interval=1', '--interval=5'],
  ['--', 'x'], ['-'], ['--'],
];
for (const v of VALUES) {
  ARGVS.push(['--interval', v], [`--interval=${v}`], ['--energy', v]);
}

const cases = ARGVS.map((argv) => {
  const r = parseArgs(argv);
  return r.ok ? { argv, ok: true as const, options: r.options } : { argv, ok: false as const, error: r.error };
});

const path = fileURLToPath(
  new URL('../crates/sysmon-core/tests/fixtures/options-corpus.json', import.meta.url),
);
writeFileSync(path, `${JSON.stringify({ note: 'Generated from src/core/options.ts by scripts/gen-options-corpus.ts.', cases }, null, 1)}\n`);
process.stdout.write(`wrote ${cases.length} argv cases (${cases.filter((c) => !c.ok).length} rejected)\n`);
