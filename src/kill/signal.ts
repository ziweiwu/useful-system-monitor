import type { ProcessSample } from '../core/types.js';
import { checkKill, type GuardContext, type KillRefusal } from './guards.js';

export type Signal = 'SIGTERM' | 'SIGKILL';

export type KillOutcome =
  | { ok: true; note?: string }
  | { ok: false; refusal: KillRefusal }
  | { ok: false; error: string };

export type KillFn = (pid: number, signal: Signal) => void;

const defaultKill: KillFn = (pid, signal) => {
  process.kill(pid, signal);
};

/**
 * Sends a signal, but only after `checkKill` passes. See I-15, I-17.
 */
export function sendSignal(
  target: ProcessSample,
  signal: Signal,
  ctx: GuardContext,
  opts: { liveStartTime?: number; kill?: KillFn } = {},
): KillOutcome {
  const check = checkKill(target, ctx, opts.liveStartTime);
  if (!check.allowed) return { ok: false, refusal: check.refusal };

  const kill = opts.kill ?? defaultKill;
  try {
    kill(target.pid, signal);
    return { ok: true };
  } catch (err) {
    const code = (err as NodeJS.ErrnoException).code;
    if (code === 'ESRCH') {
      // I-17: the process already exited. That is the outcome we wanted.
      return { ok: true, note: 'Process had already exited.' };
    }
    if (code === 'EPERM') {
      // I-17: surface the remedy, never silently escalate to sudo.
      return {
        ok: false,
        error: `Not permitted to signal PID ${target.pid}. It belongs to another user — rerun useful-system-monitor with sudo if you are sure.`,
      };
    }
    return { ok: false, error: (err as Error).message };
  }
}
