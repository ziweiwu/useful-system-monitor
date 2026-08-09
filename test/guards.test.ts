import { describe, expect, it, vi } from 'vitest';
import { checkKill, isSelfOrAncestor, processName, type GuardContext } from '../src/kill/guards.js';
import { sendSignal } from '../src/kill/signal.js';
import { sample } from './helpers.js';

const ctx = (selfPid: number, parents: Array<[number, number]> = []): GuardContext => ({
  selfPid,
  parents: new Map(parents),
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

describe('I-16: PID reuse aborts the kill', () => {
  it('refuses when the live start time no longer matches', () => {
    const target = sample({ pid: 700, startTime: 1_000 });
    const r = checkKill(target, ctx(999), 2_000);
    expect(r.allowed).toBe(false);
    if (!r.allowed) expect(r.refusal.kind).toBe('recycled');
  });

  it('allows when the start time still matches', () => {
    const target = sample({ pid: 700, startTime: 1_000 });
    expect(checkKill(target, ctx(999), 1_000).allowed).toBe(true);
  });

  it('sends no signal at all when the PID was recycled', () => {
    const kill = vi.fn();
    const target = sample({ pid: 700, startTime: 1_000 });
    const out = sendSignal(target, 'SIGKILL', ctx(999), { liveStartTime: 2_000, kill });
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
    const out = sendSignal(target, 'SIGTERM', ctx(999), { kill });
    expect(out.ok).toBe(false);
    expect(kill).not.toHaveBeenCalled();
  });
});
