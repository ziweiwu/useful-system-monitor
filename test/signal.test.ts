import { describe, expect, it, vi } from 'vitest';
import type { GuardContext } from '../src/kill/guards.js';
import { sendSignal } from '../src/kill/signal.js';
import { sample } from './helpers.js';

const ctx: GuardContext = { selfPid: 999, parents: new Map() };

function errno(code: string): NodeJS.ErrnoException {
  const e = new Error(code) as NodeJS.ErrnoException;
  e.code = code;
  return e;
}

describe('I-15 / I-17: signal delivery', () => {
  it('sends the requested signal when allowed', () => {
    const kill = vi.fn();
    const out = sendSignal(sample({ pid: 700 }), 'SIGTERM', ctx, { kill });
    expect(out.ok).toBe(true);
    expect(kill).toHaveBeenCalledWith(700, 'SIGTERM');
  });

  it('treats ESRCH as success — the process is already gone', () => {
    const kill = vi.fn(() => {
      throw errno('ESRCH');
    });
    const out = sendSignal(sample({ pid: 700 }), 'SIGTERM', ctx, { kill });
    expect(out.ok).toBe(true);
  });

  it('surfaces EPERM with a remedy and never escalates on its own', () => {
    const kill = vi.fn(() => {
      throw errno('EPERM');
    });
    const out = sendSignal(sample({ pid: 700 }), 'SIGKILL', ctx, { kill });
    expect(out.ok).toBe(false);
    if (!out.ok && 'error' in out) {
      expect(out.error).toMatch(/sudo/);
    }
    expect(kill).toHaveBeenCalledTimes(1);
  });

  it('reports unexpected errors rather than swallowing them', () => {
    const kill = vi.fn(() => {
      throw new Error('boom');
    });
    const out = sendSignal(sample({ pid: 700 }), 'SIGTERM', ctx, { kill });
    expect(out.ok).toBe(false);
    if (!out.ok && 'error' in out) expect(out.error).toBe('boom');
  });
});
