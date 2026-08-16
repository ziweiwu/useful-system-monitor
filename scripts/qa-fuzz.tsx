/**
 * Randomised keyboard exploration at random terminal sizes.
 *
 * This is the instrument that found the mode-stacking bug, which the size sweep
 * could not: it sends keys in **bursts with no await between them**, so two
 * arrive in one chunk and `useInput` handles both against the same stale
 * closure. Terminals deliver keystrokes in batches, so that is realistic input,
 * not an artificial race.
 *
 * Every run is seeded, so a failure reproduces exactly.
 *
 * Usage:
 *   npm run qa:fuzz                                  # seeds 1..40
 *   npm run qa:fuzz -- --seeds 200 --steps 120
 *   npm run qa:fuzz -- --seed 33                     # replay one, verbosely
 */
import { render } from 'ink-testing-library';
import { App } from '../src/app.js';
import { displayWidth } from '../src/core/width.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { Tiers } from '../src/providers/types.js';

const FAST: Tiers = { cpu: 120, memory: 250, processes: 250, battery: 400, disk: 600 };
const ESC = String.fromCharCode(27);
const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/**
 * Every key the app binds, plus junk it does not: an empty string, a tab, a
 * wide CJK glyph and an emoji, which are the inputs that break width arithmetic.
 * `q` is deliberately absent — it would exit and end the run early.
 */
const KEYS = [
  '1', '2', '3', '4', '5', 'c', 'm', 'e', 'r', 'k', 't', '/', '+', '-', '=', '_',
  '\r', ESC, `${ESC}[A`, `${ESC}[B`, `${ESC}[C`, `${ESC}[D`, `${ESC}[5~`, `${ESC}[6~`,
  '', 'a', 'Z', '9', ' ', '中', '\u{1f525}', '\t',
];

/** Deterministic PRNG, so `--seed N` replays a failure exactly. */
function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => (s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32;
}

function arg(name: string, fallback: number): number {
  const i = process.argv.indexOf(`--${name}`);
  if (i < 0) return fallback;
  const v = Number(process.argv[i + 1]);
  return Number.isFinite(v) ? v : fallback;
}

interface Defect {
  seed: number;
  size: string;
  kind: string;
  detail: string;
  keys: string[];
}

async function runSeed(seed: number, steps: number, verbose: boolean): Promise<Defect[]> {
  const rand = rng(seed);
  /*
   * Deliberately reaches *below* MIN_COLUMNS x MIN_ROWS.
   *
   * This range used to start at exactly the minimum, so the one size class the
   * app treats specially — the one where it draws nothing but its own size
   * complaint — was never fuzzed at all. That is where `k` then `t` sent
   * SIGTERM with the confirmation panel unrendered: the mode was entered, just
   * not drawn. A fuzzer whose floor is the contract cannot find a bug that
   * lives under it.
   */
  const columns = 40 + Math.floor(rand() * 100);
  const rows = 6 + Math.floor(rand() * 38);
  const size = `${columns}x${rows}`;
  const found: Defect[] = [];

  process.stdout.columns = columns;
  process.stdout.rows = rows;
  /*
   * I-15 wants a kill confirmed by name, so a signal that leaves the app while
   * no confirmation is on screen is a defect however tidy the frame looks.
   * `killFn` runs synchronously inside the key handler, so the frame it reads
   * is the one the user was looking at when they pressed the key.
   */
  let mounted: { lastFrame: () => string | undefined } | null = null;
  const blind: string[] = [];
  const app = render(
    <App
      provider={new MockProvider()}
      tiers={FAST}
      demo
      killFn={(pid, signal) => {
        const f = (mounted?.lastFrame() ?? '').replace(ANSI, '');
        if (!/KILL PROCESS|REFUSED/.test(f)) blind.push(`${signal} to ${pid}`);
      }}
    />,
  );
  mounted = app;
  await wait(300);

  const keys: string[] = [];
  for (let i = 0; i < steps; i++) {
    const k = KEYS[Math.floor(rand() * KEYS.length)]!;
    keys.push(JSON.stringify(k));
    app.stdin.write(k);
    // Bursts of seven: the point is that several land in one chunk.
    if (i % 7 === 0) await wait(25);
  }
  await wait(250);

  const lines = (app.lastFrame() ?? '').replace(ANSI, '').split('\n');
  const height = lines.length;
  const width = Math.max(0, ...lines.map(displayWidth));
  if (height > rows || width > columns) {
    found.push({
      seed, size, kind: 'overflow',
      detail: `${height} rows (max ${rows}), ${width} cells (max ${columns})`,
      keys,
    });
  }
  const text = lines.join('\n');
  /* Two full-screen modes at once: the detail hints beside a confirmation. */
  if (text.includes('[k] kill') && /KILL PROCESS|REFUSED/.test(text)) {
    found.push({ seed, size, kind: 'two-modes', detail: 'detail panel and kill confirmation both drawn', keys });
  }
  /* A signal sent with nothing on screen naming its target. See I-15. */
  for (const b of blind) {
    found.push({ seed, size, kind: 'blind-kill', detail: `${b} with no confirmation drawn`, keys });
  }
  if (verbose) {
    console.log(`seed ${seed} (${size}) final frame:`);
    lines.forEach((l, n) => console.log(`${String(n + 1).padStart(3)}|${String(displayWidth(l)).padStart(3)}| ${l}`));
  }
  app.unmount();
  return found;
}

async function main(): Promise<void> {
  const one = process.argv.indexOf('--seed');
  const seeds = one >= 0 ? [Number(process.argv[one + 1])] : range(1, arg('seeds', 40));
  const steps = arg('steps', 60);

  const defects: Defect[] = [];
  const renderErrors: string[] = [];
  const rejections: string[] = [];
  const realError = console.error;
  console.error = (...a: unknown[]) => renderErrors.push(a.map(String).join(' '));
  process.on('unhandledRejection', (e) => rejections.push(String(e)));

  for (const seed of seeds) {
    defects.push(...(await runSeed(seed, steps, one >= 0)));
    if (renderErrors.length) {
      /* "Maximum update depth exceeded" is an infinite render loop, not a warning. */
      const serious = renderErrors.filter((e) => /Maximum update depth|not a function|Cannot read/.test(e));
      for (const e of serious) {
        defects.push({ seed, size: '-', kind: 'render-error', detail: e.slice(0, 160), keys: [] });
      }
      renderErrors.length = 0;
    }
  }
  console.error = realError;

  for (const r of rejections) {
    defects.push({ seed: -1, size: '-', kind: 'unhandled-rejection', detail: r.slice(0, 160), keys: [] });
  }

  console.log(`\nfuzz: ${seeds.length} seeds x ${steps} steps`);
  if (!defects.length) {
    console.log('fuzz: PASS — no crash, no overflow, no stuck mode');
    process.exit(0);
  }
  console.error(`\n${defects.length} DEFECTS:`);
  for (const d of defects) {
    console.error(`  [${d.kind}] seed ${d.seed} ${d.size}: ${d.detail}`);
    if (d.keys.length) console.error(`    replay: npm run qa:fuzz -- --seed ${d.seed} --steps ${d.keys.length}`);
  }
  process.exit(1);
}

function range(from: number, to: number): number[] {
  return Array.from({ length: Math.max(0, to - from + 1) }, (_, i) => from + i);
}

void main();
