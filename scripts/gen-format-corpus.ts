/**
 * Records what the TypeScript formatters produce, so the Rust port can be
 * checked against them exactly.
 *
 * Formatting is on every row of the table, and the two languages round
 * differently by default: `Number.prototype.toFixed` rounds half *up* per
 * ECMA-262, while Rust's `{:.1}` rounds half to *even*. That is reachable here —
 * 1280 bytes is exactly 1.25 K, which JavaScript renders `1.3K` and Rust would
 * render `1.2K` — so it is checked rather than assumed.
 */
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { age, bytes, duration, minutesToHm, percent } from '../src/core/format.js';

/** Byte counts either side of the first unit boundary. */
const AROUND_ONE_K = [0, 1, 512, 1023, 1024, 1025];
/** K, M, G and T. */
const UNIT_COUNT = 4;
/** How far up each unit the sweep walks, and how coarsely. */
const WHOLES_PER_UNIT = 1100;
const WHOLE_STEP = 7;
/** Exact binary ties: the fractions that round badly. */
const TIE_FRACTIONS = [0, 0.25, 0.5, 0.75, 0.125, 0.375];
/** The LCG the Rust side uses too, so both corpora are reproducible. */
const LCG_MULTIPLIER = 1664525;
const LCG_INCREMENT = 1013904223;
const RANDOM_SAMPLES = 600;
/** Big enough to reach the byte counts a real filesystem reports. */
const RANDOM_MAGNITUDE = 42;
/** 0, 0.025, 0.05 … 100, which is every hundredth a percentage can print. */
const PERCENT_STEPS = 4000;
const PERCENT_DIVISOR = 40;
/** Values that sit exactly on a rounding boundary. */
const PERCENT_TIES = [0.05, 0.15, 0.25, 0.35, 0.45, 1.005, 99.95, 99.99, 100];

/** A deterministic spread, weighted towards the boundaries that round badly. */
function byteInputs(): number[] {
  const out = new Set<number>(AROUND_ONE_K);
  for (let k = 1; k <= UNIT_COUNT; k++) {
    const unit = 1024 ** k;
    for (let whole = 1; whole <= WHOLES_PER_UNIT; whole += WHOLE_STEP) {
      for (const frac of TIE_FRACTIONS) {
        out.add(Math.round((whole + frac) * unit));
      }
    }
  }
  let seed = 0x1234_5678;
  for (let i = 0; i < RANDOM_SAMPLES; i++) {
    seed = (Math.imul(seed, LCG_MULTIPLIER) + LCG_INCREMENT) >>> 0;
    out.add(Math.floor((seed / 0x1_0000_0000) * 2 ** RANDOM_MAGNITUDE));
  }
  return [...out].filter((n) => Number.isSafeInteger(n) && n >= 0).toSorted((a, b) => a - b);
}

function percentInputs(): number[] {
  const out = new Set<number>();
  for (let i = 0; i <= PERCENT_STEPS; i++) out.add(i / PERCENT_DIVISOR);
  for (const v of PERCENT_TIES) out.add(v);
  return [...out].toSorted((a, b) => a - b);
}

/* Prime-ish steps, so the sweep lands on every carry rather than marching in
   step with the units it is testing. */
const DURATION_SAMPLES = 400;
const DURATION_STEP_SEC = 971;
const HM_SAMPLES = 300;
const HM_STEP_MIN = 7;
const AGE_SAMPLES = 400;
const AGE_STEP_MS = 1499;

const out = {
  note: 'Generated from src/core/format.ts by scripts/gen-format-corpus.ts.',
  bytes: byteInputs().map((n) => [n, bytes(n)] as const),
  percent1: percentInputs().map((n) => [n, percent(n, 1)] as const),
  percent0: percentInputs().map((n) => [n, percent(n, 0)] as const),
  duration: Array.from({ length: DURATION_SAMPLES }, (_, i) => i * DURATION_STEP_SEC).map(
    (s) => [s, duration(s)] as const,
  ),
  minutesToHm: Array.from({ length: HM_SAMPLES }, (_, i) => i * HM_STEP_MIN).map(
    (m) => [m, minutesToHm(m)] as const,
  ),
  age: Array.from({ length: AGE_SAMPLES }, (_, i) => i * AGE_STEP_MS).map(
    (ms) => [ms, age(0, ms)] as const,
  ),
};

const path = fileURLToPath(
  new URL('../crates/sysmon-core/tests/fixtures/format-corpus.json', import.meta.url),
);
writeFileSync(path, `${JSON.stringify(out)}\n`);
process.stdout.write(
  `wrote ${out.bytes.length} byte cases, ${out.percent1.length} percent cases\n`,
);
