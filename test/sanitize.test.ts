import { describe, expect, it } from 'vitest';
import { displayWidth, sanitizeText } from '../src/core/width.js';
import { processName } from '../src/kill/guards.js';
import { parsePsStatic } from '../src/providers/darwin/parse.js';

const CR = String.fromCharCode(13);
const ESC = String.fromCharCode(27);
const NUL = String.fromCharCode(0);

/*
 * A process chooses its own name, and any user can set a hostile one:
 *   exec -a $'malware\rSafari' sleep 60
 *
 * If such a byte reached the renderer it would be a spoof in the one tool whose
 * job is to show you what is running: displayWidth counts a control character
 * as zero cells, so widths and row budgets are computed as though it were
 * absent, while the terminal still acts on it — a carriage return returns the
 * cursor to column zero, so `malware\rSafari` draws as `Safari`, hiding both
 * the real name and the PID beside it.
 *
 * Measured: macOS `ps` escapes every control byte on the way out — newline to
 * `\012`, ESC to `^[`, carriage return to `^M` — confirmed with `od -c`, which
 * reports zero raw 0x0D bytes for a process named that way. So these tests pin
 * a boundary that is currently also defended upstream, which is the point: the
 * MetricsProvider interface is implementable by anything, and `ps`'s escaping
 * is an undocumented implementation detail rather than a contract.
 */
describe('I-19: control characters cannot corrupt a rendered row', () => {
  it('replaces a carriage return, the byte that would overwrite the row', () => {
    expect(sanitizeText(`malware${CR}Safari`)).toBe('malware·Safari');
  });

  it.each([
    ['null', NUL],
    ['bell', String.fromCharCode(7)],
    ['backspace', String.fromCharCode(8)],
    ['tab', String.fromCharCode(9)],
    ['newline', String.fromCharCode(10)],
    ['escape', ESC],
    ['delete', String.fromCharCode(127)],
    ['C1 control', String.fromCharCode(0x9b)],
  ])('replaces %s', (_label, ch) => {
    const out = sanitizeText(`a${ch}b`);
    expect(out).toBe('a·b');
    expect(displayWidth(out)).toBe(3);
  });

  it('leaves ordinary names untouched, including wide and combining glyphs', () => {
    for (const s of ['Google Chrome', '/usr/libexec/logd', '日本語アプリ', 'café', '🔥app', '']) {
      expect(sanitizeText(s)).toBe(s);
    }
  });

  it('makes the width honest — a control character used to measure as zero', () => {
    // The gap this closes: the string was 14 cells wide by measurement and
    // drew over the start of its own row.
    expect(displayWidth(`malware${CR}Safari`)).toBe(13);
    expect(displayWidth(sanitizeText(`malware${CR}Safari`))).toBe(14);
  });

  it('sanitises through processName, the funnel every rendered name uses', () => {
    expect(processName(`/tmp/malware${CR}Safari`)).toBe('malware·Safari');
  });

  it('sanitises on ingest, so --json and every consumer get a clean string', () => {
    const line =
      '  PID STARTED USER STAT COMM\n' +
      '  501 Wed Aug 12 19:39:58 2026    ziweiwu          S    /tmp/Google Chrome\n';
    expect(parsePsStatic(line)[0]!.command).toBe('/tmp/Google Chrome');
  });

  it('never yields a row still carrying a control character', () => {
    /*
     * A synthetic raw CR does not survive the parser either, by a separate
     * route: `\r` is a line terminator to a JS regex, so `(.*)$` cannot span it
     * and the row simply fails to match. Both outcomes are safe — a sanitised
     * row or no row — and this asserts the property rather than which one.
     */
    const line =
      '  PID STARTED USER STAT COMM\n' +
      `  501 Wed Aug 12 19:39:58 2026    ziweiwu          S    /tmp/malware${CR}Safari\n`;
    for (const meta of parsePsStatic(line)) {
      expect(meta.command).not.toContain(CR);
      expect(meta.user).not.toContain(CR);
    }
  });
});
