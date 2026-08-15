import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const read = (rel: string) =>
  readFileSync(fileURLToPath(new URL(`../${rel}`, import.meta.url)), 'utf8');

/*
 * `r` was bound, useful, and documented nowhere — not in the footer legend, not
 * in `--help`, not in the README — while being the only way to force a sample
 * before the next tier (five minutes on the disk tier), the remedy for every
 * "data unavailable" panel, and the key a guard message already instructed the
 * user to press. An accelerator documented nowhere is an accelerator nobody has.
 *
 * This checks the keymap against its own documentation, so the next key added
 * cannot go missing the same way.
 */
describe('every bound key is documented where a user would look', () => {
  const app = read('src/app.tsx');
  const help = read('src/cli.tsx');
  const readme = read('README.md');

  /** Single-character keys the input handler acts on. */
  const BOUND = ['c', 'm', 'e', 'r', 'k', 'q', '/', '+', '-'];

  it.each(BOUND)('%s appears in the on-screen legend', (key) => {
    const legends = [...app.matchAll(/'([^']*(?:quit|cancel|back|apply)[^']*)'/g)].map((m) => m[1]!);
    expect(legends.length).toBeGreaterThan(0);
    expect(legends.some((l) => l.includes(key)), `no legend mentions "${key}"`).toBe(true);
  });

  it.each(BOUND.filter((k) => !'+-'.includes(k)))('%s appears in --help', (key) => {
    const keysSection = help.slice(help.indexOf('Keys (dashboard)'), help.indexOf('Examples'));
    expect(keysSection.includes(key), `--help does not mention "${key}"`).toBe(true);
  });

  it.each(['r', 'k', 'q', '/'])('%s appears in the README key table', (key) => {
    expect(readme.includes(`<kbd>${key}</kbd>`), `README has no <kbd>${key}</kbd>`).toBe(true);
  });
});
