import { describe, expect, it, vi } from 'vitest';
import {
  checkKill,
  isSelfOrAncestor,
  processName,
  type GuardContext,
  type LiveIdentity,
} from '../src/kill/guards.js';
import { sendSignal } from '../src/kill/signal.js';
import { sample } from './helpers.js';

/*
 * A non-empty parent map by default.
 *
 * An empty one now means "no process sample", which is refused outright — see
 * the fail-closed tests below. Every case here that is about some *other* rule
 * needs a map that lets the ancestor walk actually run, or it would be testing
 * the new refusal instead of itself.
 */
const ctx = (selfPid: number, parents: Array<[number, number]> = []): GuardContext => ({
  selfPid,
  parents: new Map(parents.length ? parents : [[selfPid, 1]]),
});

describe('processName', () => {
  it('handles macOS paths containing spaces', () => {
    // The bug this pins: splitting on whitespace first turned
    // "/Library/Application Support/..." into "Application".
    expect(
      processName('/Library/Application Support/Logitech/logioptionsplus_agent'),
    ).toBe('logioptionsplus_agent');
    expect(
      processName('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome Helper'),
    ).toBe('Google Chrome Helper');
  });
});

describe('I-12: never signal PID <= 1', () => {
  it.each([0, 1])('refuses PID %i', (pid) => {
    const r = checkKill(sample({ pid }), ctx(999));
    expect(r.allowed).toBe(false);
    if (!r.allowed) expect(r.refusal.kind).toBe('init');
  });
});

describe('I-13: never signal ourselves or an ancestor', () => {
  it('refuses our own PID', () => {
    const r = checkKill(sample({ pid: 500 }), ctx(500));
    expect(r.allowed).toBe(false);
    if (!r.allowed) expect(r.refusal.kind).toBe('self');
  });

  it('refuses a direct parent', () => {
    const r = checkKill(sample({ pid: 400 }), ctx(500, [[500, 400]]));
    expect(r.allowed).toBe(false);
    if (!r.allowed) expect(r.refusal.kind).toBe('ancestor');
  });

  it('refuses a grandparent, walking the whole chain', () => {
    const r = checkKill(sample({ pid: 300 }), ctx(500, [[500, 400], [400, 300]]));
    expect(r.allowed).toBe(false);
    if (!r.allowed) expect(r.refusal.kind).toBe('ancestor');
  });

  it('allows an unrelated process', () => {
    expect(checkKill(sample({ pid: 700 }), ctx(500, [[500, 400]])).allowed).toBe(true);
  });

  it('terminates on a cyclic parent map instead of hanging', () => {
    expect(isSelfOrAncestor(999, ctx(500, [[500, 400], [400, 500]]))).toBe(false);
  });
});

describe('I-14: critical system processes are refused outright', () => {
  it.each([
    '/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer',
    '/sbin/launchd',
    '/System/Library/CoreServices/loginwindow.app/Contents/MacOS/loginwindow',
  ])('refuses %s', (command) => {
    const r = checkKill(sample({ pid: 616, command }), ctx(999));
    expect(r.allowed).toBe(false);
    if (!r.allowed) expect(r.refusal.kind).toBe('protected');
  });

  it('explains the consequence rather than just saying no', () => {
    const r = checkKill(sample({ pid: 616, command: '/usr/bin/WindowServer' }), ctx(999));
    if (!r.allowed) expect(r.refusal.message).toMatch(/log you out|wedge/i);
  });
});

const known = (startTime: number): LiveIdentity => ({ known: true, startTime });

describe('I-16: PID reuse aborts the kill', () => {
  it('refuses when the live start time no longer matches', () => {
    const target = sample({ pid: 700, startTime: 1_000 });
    const r = checkKill(target, ctx(999), known(2_000));
    expect(r.allowed).toBe(false);
    if (!r.allowed) expect(r.refusal.kind).toBe('recycled');
  });

  it('allows when the start time still matches', () => {
    const target = sample({ pid: 700, startTime: 1_000 });
    expect(checkKill(target, ctx(999), known(1_000)).allowed).toBe(true);
  });

  it('sends no signal at all when the PID was recycled', () => {
    const kill = vi.fn();
    const target = sample({ pid: 700, startTime: 1_000 });
    const out = sendSignal(target, 'SIGKILL', ctx(999), { live: known(2_000), kill });
    expect(out.ok).toBe(false);
    expect(kill).not.toHaveBeenCalled();
  });

  /*
   * The hole this closes: `liveStartTime` used to be an optional number, so
   * "the caller looked and found nothing" and "the caller did not look" were
   * the same value — and the check was skipped for both. A recycled PID
   * belongs to a brand-new process, which has no CPU history and a small RSS,
   * so it is almost never in the top-50 working set the caller searched. The
   * guard did nothing in exactly the case it exists for.
   */
  it('refuses rather than signals when the identity could not be read', () => {
    const kill = vi.fn();
    const target = sample({ pid: 700, startTime: 1_000 });
    const out = sendSignal(target, 'SIGKILL', ctx(999), {
      live: { known: false, gone: false },
      kill,
    });
    expect(out.ok).toBe(false);
    if (!out.ok && 'refusal' in out) expect(out.refusal.kind).toBe('unverifiable');
    expect(kill).not.toHaveBeenCalled();
  });

  it('sends nothing when the process has already exited', () => {
    const kill = vi.fn();
    const out = sendSignal(sample({ pid: 700, startTime: 1_000 }), 'SIGTERM', ctx(999), {
      live: { known: false, gone: true },
      kill,
    });
    expect(out.ok).toBe(false);
    expect(kill).not.toHaveBeenCalled();
  });

  /*
   * parseLstart returns 0 for a date it cannot read. Treating that as a
   * timestamp would make every unreadable process compare equal to every
   * other one, which is worse than refusing.
   */
  it('treats an unknown start time as unverifiable, not as epoch', () => {
    const kill = vi.fn();
    const out = sendSignal(sample({ pid: 700, startTime: 0 }), 'SIGKILL', ctx(999), {
      live: known(0),
      kill,
    });
    expect(out.ok).toBe(false);
    expect(kill).not.toHaveBeenCalled();
  });
});

describe('I-13: the ancestor guard fails closed when it cannot verify', () => {
  /*
   * `guardCtx` builds its parent map from the process sample, and that sample
   * is gone whenever the collector errors — a `ps` timeout on a loaded machine
   * is enough. The confirmation panel stays open across that, so the ancestor
   * walk silently returned false for everything and the rule that stops you
   * killing your own shell stopped applying.
   */
  it('refuses every target when there is no process sample', () => {
    const kill = vi.fn();
    const empty: GuardContext = { selfPid: 999, parents: new Map() };
    const r = checkKill(sample({ pid: 700 }), empty);
    expect(r.allowed).toBe(false);
    if (!r.allowed) expect(r.refusal.kind).toBe('unverifiable');

    const out = sendSignal(sample({ pid: 700 }), 'SIGKILL', empty, {
      live: { known: true, startTime: 1_000 },
      kill,
    });
    expect(out.ok).toBe(false);
    expect(kill).not.toHaveBeenCalled();
  });
});

describe('every refusal path emits no signal', () => {
  it.each([
    ['init', sample({ pid: 1 })],
    ['self', sample({ pid: 999 })],
    ['protected', sample({ pid: 616, command: '/usr/bin/WindowServer' })],
  ])('%s', (_label, target) => {
    const kill = vi.fn();
    const out = sendSignal(target, 'SIGTERM', ctx(999), {
      live: { known: true, startTime: target.startTime },
      kill,
    });
    expect(out.ok).toBe(false);
    expect(kill).not.toHaveBeenCalled();
  });
});
