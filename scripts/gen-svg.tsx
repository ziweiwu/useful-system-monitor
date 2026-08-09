/**
 * Render a real dashboard frame to a self-contained SVG for the README.
 *
 * GitHub renders SVG images but strips CSS, so every colour is an inline
 * attribute and each coloured run is positioned at an explicit x. Relying on
 * font metrics alone would let box-drawing glyphs drift out of alignment on
 * machines with different monospace fonts.
 */
import { writeFileSync } from 'node:fs';
import { render } from 'ink-testing-library';
import { App } from '../src/app.js';
import { MockProvider } from '../src/providers/mock/provider.js';
import type { Tiers } from '../src/providers/types.js';

/*
 * FORCE_COLOR must be set in the *environment*, not here: ESM imports are
 * hoisted, so Ink resolves colour support before any statement in this file
 * runs. Setting it inline silently produces a colourless SVG.
 */
if (process.env['FORCE_COLOR'] !== '3') {
  console.error('run with FORCE_COLOR=3, e.g. FORCE_COLOR=3 npx tsx scripts/gen-svg.tsx out.svg');
  process.exit(1);
}
process.stdout.columns = Number(process.env['COLS'] ?? 100);
process.stdout.rows = Number(process.env['ROWS'] ?? 30);

const ESC = String.fromCharCode(27);
const FAST: Tiers = { cpu: 80, memory: 250, processes: 250, battery: 500, disk: 1200 };
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

const FONT_SIZE = 13;
const CHAR_W = FONT_SIZE * 0.6;
const LINE_H = FONT_SIZE * 1.36;
const PAD = 20;
const BG = '#16161e';
const FG = '#c0caf5';

interface Run { text: string; col: number; fg: string | null; bold: boolean; dim: boolean; bgc: string | null }

function parseLine(line: string): Run[] {
  const runs: Run[] = [];
  let fg: string | null = null;
  let bgc: string | null = null;
  let bold = false;
  let dim = false;
  let col = 0;
  const re = new RegExp(`${ESC}\\[([0-9;]*)m`, 'g');
  let last = 0;
  let m: RegExpExecArray | null;

  const push = (text: string) => {
    if (!text) return;
    runs.push({ text, col, fg, bold, dim, bgc });
    col += [...text].length;
  };

  while ((m = re.exec(line))) {
    push(line.slice(last, m.index));
    const parts = m[1]!.split(';').map(Number);
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i]!;
      if (p === 0) { fg = null; bgc = null; bold = false; dim = false; }
      else if (p === 1) bold = true;
      else if (p === 2) dim = true;
      else if (p === 22) { bold = false; dim = false; }
      else if (p === 39) fg = null;
      else if (p === 49) bgc = null;
      else if (p === 38 && parts[i + 1] === 2) { fg = `rgb(${parts[i+2]},${parts[i+3]},${parts[i+4]})`; i += 4; }
      else if (p === 48 && parts[i + 1] === 2) { bgc = `rgb(${parts[i+2]},${parts[i+3]},${parts[i+4]})`; i += 4; }
    }
    last = re.lastIndex;
  }
  push(line.slice(last));
  return runs;
}

const xmlEscape = (s: string) =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function toSvg(frame: string): string {
  const lines = frame.split('\n');
  const cols = Math.max(...lines.map((l) => [...l.replace(new RegExp(`${ESC}\\[[0-9;]*m`, 'g'), '')].length));
  const w = Math.ceil((cols + 1) * CHAR_W + PAD * 2);
  const h = Math.ceil(lines.length * LINE_H + PAD * 2);

  const body: string[] = [];
  lines.forEach((line, i) => {
    const y = PAD + (i + 1) * LINE_H - LINE_H * 0.25;
    for (const r of parseLine(line)) {
      const x = PAD + r.col * CHAR_W;
      if (r.bgc) {
        body.push(
          `<rect x="${x.toFixed(1)}" y="${(y - FONT_SIZE * 0.85).toFixed(1)}" width="${([...r.text].length * CHAR_W).toFixed(1)}" height="${LINE_H.toFixed(1)}" fill="${r.bgc}"/>`,
        );
      }
      if (!r.text.trim()) continue;
      /*
       * textLength pins each run to exactly (chars x cell width).
       *
       * Positioning runs at an explicit x is not enough on its own: glyphs
       * *within* a run still advance at the font's natural width, so on any
       * machine whose monospace font is wider than the assumed cell the last
       * run on a line overflows and gets clipped. Fixing the run's length makes
       * the whole frame render identically regardless of the available font.
       */
      const cells = [...r.text].length;
      const attrs = [
        `x="${x.toFixed(1)}"`,
        `y="${y.toFixed(1)}"`,
        `textLength="${(cells * CHAR_W).toFixed(1)}"`,
        'lengthAdjust="spacing"',
        `fill="${r.fg ?? FG}"`,
        r.bold ? 'font-weight="700"' : '',
        r.dim ? 'opacity="0.62"' : '',
      ].filter(Boolean).join(' ');
      body.push(`<text ${attrs} xml:space="preserve">${xmlEscape(r.text)}</text>`);
    }
  });

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', monospace" font-size="${FONT_SIZE}">
<rect width="${w}" height="${h}" rx="8" fill="${BG}"/>
${body.join('\n')}
</svg>
`;
}

const main = async () => {
  const keys = process.argv.slice(3);
  const app = render(<App provider={new MockProvider()} tiers={FAST} demo killFn={() => {}} />);
  await wait(2600);
  for (const k of keys) { app.stdin.write(k); await wait(320); }
  await wait(260);
  const frame = app.lastFrame() ?? '';
  app.unmount();
  const out = process.argv[2] ?? 'docs/screenshot.svg';
  const svg = toSvg(frame);
  // Fail loudly rather than shipping a screenshot with no colour in it.
  const colours = new Set(svg.match(/fill="rgb\([^)]+\)"/g) ?? []).size;
  if (colours < 5) {
    console.error(`only ${colours} colours in the output — the frame was not rendered in colour`);
    process.exit(1);
  }
  writeFileSync(out, svg);
  console.log(`wrote ${out} (${colours} colours)`);
  process.exit(0);
};

void main();
