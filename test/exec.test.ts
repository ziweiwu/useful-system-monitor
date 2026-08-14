import { describe, expect, it } from 'vitest';
import { BIN, collectorEnv, run } from '../src/providers/darwin/exec.js';
import { parseMemory, parsePsStatic } from '../src/providers/darwin/parse.js';

/*
 * The collectors are text parsers pointed at commands that format their output
 * through the C library's locale. Inheriting the user's locale made `ps -o
 * lstart` unparseable on any Mac not set to English, which showed every row in
 * the process table as "pid 1234" — and, because an unnameable process counts
 * as protected (I-14), disabled the kill path for the whole machine.
 */
describe('I-28: collectors run in a locale they can parse', () => {
  it('pins the two categories that reformat output', () => {
    const env = collectorEnv({ LANG: 'de_DE.UTF-8' });
    expect(env['LC_TIME']).toBe('C');
    expect(env['LC_NUMERIC']).toBe('C');
  });

  it('removes LC_ALL, which outranks the categories it would otherwise set', () => {
    // Overriding LC_TIME while LC_ALL survives does nothing at all: this is the
    // difference between the fix working and appearing to work.
    const env = collectorEnv({ LC_ALL: 'de_DE.UTF-8', LC_TIME: 'de_DE.UTF-8' });
    expect(env['LC_ALL']).toBeUndefined();
    expect(env['LC_TIME']).toBe('C');
  });

  it('leaves the character encoding alone, so non-ASCII names survive', () => {
    // Forcing the whole locale to C would force LC_CTYPE too, and a process
    // whose name is not ASCII should come back as UTF-8 rather than escaped.
    expect(collectorEnv({ LC_CTYPE: 'en_US.UTF-8' })['LC_CTYPE']).toBe('en_US.UTF-8');
  });

  it('does not mutate the environment it was handed', () => {
    const base = { LC_ALL: 'de_DE.UTF-8' };
    collectorEnv(base);
    expect(base.LC_ALL).toBe('de_DE.UTF-8');
  });

  /*
   * The unit tests above assert the shape of the environment; this asserts the
   * thing that actually matters, by spawning the real `ps` from a process whose
   * own locale is one of the broken ones.
   */
  it('parses real ps output when the app itself was launched under LC_ALL=zh_CN', async () => {
    /*
     * zh_CN, not de_DE, on purpose. V8's Date.parse is lenient enough to find
     * "12 Aug ... 2026" inside the German form, so a German locale cannot tell
     * a working collector from a broken one. zh_CN prints "三  8月/12 ...",
     * which has no Latin month at all — the fallback parser recovers the name
     * from it but cannot recover the instant, so this assertion fails unless
     * the spawn really is pinned to LC_TIME=C.
     */
    const prev = process.env['LC_ALL'];
    process.env['LC_ALL'] = 'zh_CN.UTF-8';
    try {
      const out = await run(BIN.ps, ['-Ao', 'pid,lstart,user,state,comm']);
      const metas = parsePsStatic(out);
      expect(metas.length).toBeGreaterThan(50);
      expect(metas.find((m) => m.pid === 1)?.command).toBe('/sbin/launchd');
      // I-16 needs a real instant, not the 0 the fallback parser reports.
      expect(metas.find((m) => m.pid === 1)!.startTime).toBeGreaterThan(0);
    } finally {
      if (prev === undefined) delete process.env['LC_ALL'];
      else process.env['LC_ALL'] = prev;
    }
  });

  it('parses real swap usage under a comma-decimal locale', async () => {
    const prev = process.env['LC_ALL'];
    process.env['LC_ALL'] = 'de_DE.UTF-8';
    try {
      const [vmStat, swap] = await Promise.all([
        run(BIN.vmStat, []),
        run(BIN.sysctl, ['-n', 'vm.swapusage']),
      ]);
      expect(parseMemory(vmStat, swap, 16 * 1024 ** 3).swapTotalBytes).toBeGreaterThan(0);
    } finally {
      if (prev === undefined) delete process.env['LC_ALL'];
      else process.env['LC_ALL'] = prev;
    }
  });
});
