/**
 * Display width, not code-unit length. Wide (CJK / emoji) glyphs occupy two
 * terminal cells and combining marks occupy none; using String.length for
 * layout silently breaks alignment. See I-19.
 */

/**
 * Ranges MUST stay in ascending order: `isWide` early-returns on the first
 * range that starts above the code point. An out-of-order entry silently makes
 * every later range unreachable (which is how ⛔ was first measured as width 1).
 */
const WIDE_RANGES: ReadonlyArray<readonly [number, number]> = [
  [0x1100, 0x115f],
  [0x231a, 0x231b], [0x2329, 0x232a], [0x23e9, 0x23ec], [0x23f0, 0x23f0],
  [0x23f3, 0x23f3], [0x25fd, 0x25fe], [0x2614, 0x2615], [0x2648, 0x2653],
  [0x267f, 0x267f], [0x2693, 0x2693], [0x26a1, 0x26a1], [0x26aa, 0x26ab],
  [0x26bd, 0x26be], [0x26c4, 0x26c5], [0x26ce, 0x26ce], [0x26d4, 0x26d4],
  [0x26ea, 0x26ea], [0x26f2, 0x26f3], [0x26f5, 0x26f5], [0x26fa, 0x26fa],
  [0x26fd, 0x26fd], [0x2705, 0x2705], [0x270a, 0x270b], [0x2728, 0x2728],
  [0x274c, 0x274c], [0x274e, 0x274e], [0x2753, 0x2755], [0x2757, 0x2757],
  [0x2795, 0x2797], [0x27b0, 0x27b0], [0x27bf, 0x27bf], [0x2b1b, 0x2b1c],
  [0x2b50, 0x2b50], [0x2b55, 0x2b55],
  [0x2e80, 0x303e], [0x3041, 0x33ff], [0x3400, 0x4dbf], [0x4e00, 0x9fff],
  [0xa000, 0xa4cf], [0xa960, 0xa97f], [0xac00, 0xd7a3], [0xf900, 0xfaff],
  [0xfe10, 0xfe19], [0xfe30, 0xfe6f], [0xff00, 0xff60], [0xffe0, 0xffe6],
  [0x1f004, 0x1f004], [0x1f0cf, 0x1f0cf], [0x1f18e, 0x1f18e],
  [0x1f191, 0x1f19a], [0x1f200, 0x1f320], [0x1f330, 0x1f335],
  [0x1f337, 0x1f37c], [0x1f380, 0x1f393], [0x1f3a0, 0x1f3ca],
  [0x1f3cf, 0x1f3d3], [0x1f3e0, 0x1f3f0], [0x1f400, 0x1f43e],
  [0x1f440, 0x1f4fc], [0x1f500, 0x1f53d], [0x1f550, 0x1f567],
  [0x1f5fb, 0x1f64f], [0x1f680, 0x1f6c5], [0x1f900, 0x1f9ff],
  [0x20000, 0x3fffd],
];

const VARIATION_SELECTOR_16 = 0xfe0f;

function isWide(cp: number): boolean {
  for (const [lo, hi] of WIDE_RANGES) {
    if (cp < lo) return false;
    if (cp <= hi) return true;
  }
  return false;
}

function isZeroWidth(cp: number): boolean {
  // Combining marks, ZWJ/ZWNJ, variation selectors, control chars.
  return (
    (cp >= 0x0300 && cp <= 0x036f) ||
    (cp >= 0x200b && cp <= 0x200f) ||
    (cp >= 0xfe00 && cp <= 0xfe0f) ||
    cp === 0x20e3 ||
    (cp < 0x20 && cp !== 0x00) ||
    cp === 0x7f
  );
}

/** Matches C0 and C1 control characters — Unicode's "Other, control" class. */
const CONTROL = /\p{Cc}/u;

/**
 * Replaces control characters with a visible placeholder.
 *
 * Defence in depth, not a fix for a live exploit — the distinction matters, so
 * here is what was actually measured.
 *
 * A process chooses its own name, and any user can set a hostile one:
 * `exec -a $'malware\rSafari' sleep 60`. If such a byte reached the renderer it
 * would be genuinely bad: `displayWidth` counts a control character as zero
 * cells, so every width and row budget would be computed as though it were
 * absent, and the terminal would still act on it — a carriage return returns
 * the cursor to column zero, so `malware\rSafari` draws as `Safari`, hiding
 * both the real name and the PID beside it. A spoof in the one tool whose job
 * is showing you what is running.
 *
 * macOS `ps` prevents that today: it escapes every control byte on the way out
 * — newline to the four characters `\012`, ESC to `^[`, carriage return to
 * `^M` — verified with `od -c`, which reports zero raw 0x0D bytes for a process
 * named that way. (`cat -v` renders a raw CR as `^M` too, so it cannot tell the
 * two apart; that mistake is why this was briefly believed to be reachable.)
 *
 * So this guards the `MetricsProvider` boundary rather than a live hole: the
 * interface is implementable by anything, the mock is not bound by `ps`'s
 * escaping, and that escaping is an undocumented implementation detail rather
 * than a contract. The fast path below makes it free on clean input.
 *
 * A visible placeholder rather than a silent strip: a name containing something
 * strange should look like it does.
 */
export function sanitizeText(s: string): string {
  // Fast path: the overwhelming majority of names are clean, and this runs for
  // every process on every sample.
  if (!CONTROL.test(s)) return s;
  let out = '';
  for (const ch of s) {
    const cp = ch.codePointAt(0)!;
    out += cp < 0x20 || cp === 0x7f || (cp >= 0x80 && cp <= 0x9f) ? '·' : ch;
  }
  return out;
}

/**
 * Each character of `s` paired with the cells it adds, left to right.
 *
 * The single definition of how wide anything is. `truncate` used to have its
 * own, calling `displayWidth` once per character — which cannot see that
 * U+FE0F widens the character *before* it. `⚠` measures 1 alone and the
 * variation selector measures 0 alone, but "⚠️" occupies 2 cells, so every
 * such pair was under-counted by one: `truncate('⚠️abc', 3)` returned 4 cells
 * and `padEnd` padded to one cell too many, overflowing the row. A rule
 * implemented twice is a rule that disagrees with itself.
 */
function* cells(s: string): Generator<readonly [string, number]> {
  let prevWidth = 0;
  for (const ch of s) {
    const cp = ch.codePointAt(0)!;
    if (cp === VARIATION_SELECTOR_16) {
      // U+FE0F requests emoji presentation, which renders the preceding
      // narrow character in two cells (e.g. "⚠️" vs "⚠").
      if (prevWidth === 1) {
        prevWidth = 2;
        yield [ch, 1];
      } else {
        yield [ch, 0];
      }
      continue;
    }
    if (isZeroWidth(cp)) {
      yield [ch, 0];
      continue;
    }
    prevWidth = isWide(cp) ? 2 : 1;
    yield [ch, prevWidth];
  }
}

/** Terminal cells occupied by `s`. */
export function displayWidth(s: string): number {
  let w = 0;
  for (const [, cw] of cells(s)) w += cw;
  return w;
}

/** Truncate to at most `max` cells, appending an ellipsis when cut. */
export function truncate(s: string, max: number): string {
  if (max <= 0) return '';
  if (displayWidth(s) <= max) return s;
  if (max === 1) return '…';
  let out = '';
  let w = 0;
  for (const [ch, cw] of cells(s)) {
    if (w + cw > max - 1) break;
    out += ch;
    w += cw;
  }
  return out + '…';
}

/** Pad to exactly `n` cells (truncating if needed). */
export function padEnd(s: string, n: number): string {
  const t = truncate(s, n);
  return t + ' '.repeat(Math.max(0, n - displayWidth(t)));
}

export function padStart(s: string, n: number): string {
  const t = truncate(s, n);
  return ' '.repeat(Math.max(0, n - displayWidth(t))) + t;
}
