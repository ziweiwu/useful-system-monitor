import type { ProcessSample } from '../core/types.js';

/**
 * All kill-safety rules, kept pure so every refusal path is unit-testable
 * without spawning anything. See I-12..I-16.
 */

/**
 * Killing any of these either logs you out, wedges the window server, or takes
 * the whole session down. Refused outright rather than merely warned about.
 */
export const PROTECTED_NAMES: ReadonlySet<string> = new Set([
  'kernel_task',
  'launchd',
  'WindowServer',
  'loginwindow',
  'SystemUIServer',
  'opendirectoryd',
  'mds',
  'mds_stores',
  'securityd',
  'configd',
  'coreaudiod',
  'Finder',
]);

export type KillRefusal =
  | { kind: 'init'; message: string }
  | { kind: 'self'; message: string }
  | { kind: 'ancestor'; message: string }
  | { kind: 'protected'; message: string }
  | { kind: 'recycled'; message: string };

export type KillCheck = { allowed: true } | { allowed: false; refusal: KillRefusal };

const ALLOWED: KillCheck = { allowed: true };

/**
 * Basename of a full executable path, for denylist matching and display.
 *
 * Must take the basename of the *whole* string: macOS paths routinely contain
 * spaces ("/Library/Application Support/...", ".../Google Chrome Helper"), so
 * splitting on whitespace first turns that path into "Application".
 * `ps -o comm` gives the executable path without arguments, so there is nothing
 * to strip.
 */
export function processName(command: string): string {
  const trimmed = command.trim();
  const parts = trimmed.split('/');
  return parts[parts.length - 1] || trimmed;
}

export function isProtectedName(command: string): boolean {
  return PROTECTED_NAMES.has(processName(command));
}

export interface GuardContext {
  /** Our own PID. */
  selfPid: number;
  /** pid -> ppid for the current sample, used to walk the ancestor chain. */
  parents: ReadonlyMap<number, number>;
}

/** Walks up from `selfPid`; true when `pid` is us or one of our ancestors. */
export function isSelfOrAncestor(pid: number, ctx: GuardContext): boolean {
  if (pid === ctx.selfPid) return true;
  let cur = ctx.selfPid;
  const seen = new Set<number>();
  while (cur > 1 && !seen.has(cur)) {
    seen.add(cur);
    const parent = ctx.parents.get(cur);
    if (parent === undefined) return false;
    if (parent === pid) return true;
    cur = parent;
  }
  return false;
}

/**
 * The single entry point every kill must pass through.
 *
 * `liveStartTime` is the start time observed at signal time. Binding the target
 * to (pid, startTime) is what stops a kill landing on an unrelated process that
 * inherited a recycled PID between selection and confirmation. See I-16.
 */
export function checkKill(
  target: ProcessSample,
  ctx: GuardContext,
  liveStartTime?: number,
): KillCheck {
  if (target.pid <= 1) {
    return {
      allowed: false,
      refusal: {
        kind: 'init',
        message: `PID ${target.pid} is the init process — killing it would halt the system.`,
      },
    };
  }

  if (target.pid === ctx.selfPid) {
    return {
      allowed: false,
      refusal: { kind: 'self', message: 'That is useful-system-monitor itself. Press q to quit instead.' },
    };
  }

  if (isSelfOrAncestor(target.pid, ctx)) {
    return {
      allowed: false,
      refusal: {
        kind: 'ancestor',
        message: `PID ${target.pid} is a parent of useful-system-monitor — killing it would take this session down.`,
      },
    };
  }

  if (isProtectedName(target.command)) {
    return {
      allowed: false,
      refusal: {
        kind: 'protected',
        message: `${processName(target.command)} is a critical system process — killing it would log you out or wedge the UI.`,
      },
    };
  }

  if (liveStartTime !== undefined && liveStartTime !== target.startTime) {
    return {
      allowed: false,
      refusal: {
        kind: 'recycled',
        message: `PID ${target.pid} has been reused by a different process since you selected it. Aborted.`,
      },
    };
  }

  return ALLOWED;
}
