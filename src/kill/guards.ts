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
  | { kind: 'recycled'; message: string }
  | { kind: 'unverifiable'; message: string };

/**
 * What the process table knows about the target at signal time.
 *
 * Deliberately a three-state value rather than an optional number. It used to
 * be `liveStartTime?: number`, and "the caller could not find the process" and
 * "the caller did not look" were the same value — so the PID-reuse check
 * (I-16) silently did nothing in exactly the case it exists for. A recycled
 * PID belongs to a brand-new process, which has no CPU history and a small
 * RSS, so it is almost never in the top-50 working set the caller searched.
 */
export type LiveIdentity =
  /** Read at signal time; compare it against the target. */
  | { known: true; startTime: number }
  /** No process with this PID exists any more. */
  | { known: false; gone: true }
  /** Nothing could be read. A guard that cannot verify must refuse. */
  | { known: false; gone: false };

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
 * `live` is the identity observed at signal time. Binding the target to
 * (pid, startTime) is what stops a kill landing on an unrelated process that
 * inherited a recycled PID between selection and confirmation. See I-16.
 * Omit it to run only the static checks — that is what the confirmation panel
 * renders, before the user has committed to anything.
 */
export function checkKill(
  target: ProcessSample,
  ctx: GuardContext,
  live?: LiveIdentity,
): KillCheck {
  /*
   * I-13 depends on the parent map, so an empty one is not "no ancestors" —
   * it is "no answer". The map comes from the process sample, which goes away
   * whenever that collector errors (a `ps` timeout on a loaded machine is
   * enough), and the confirmation panel stays open across that. Refusing is
   * the only safe reading: the alternative silently drops the guard that stops
   * a user killing the shell they are sitting in.
   */
  if (ctx.parents.size === 0) {
    return {
      allowed: false,
      refusal: {
        kind: 'unverifiable',
        message:
          'No current process list, so this cannot be checked against the ancestors of useful-system-monitor. Press r to refresh.',
      },
    };
  }

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

  if (live !== undefined) {
    if (!live.known) {
      return live.gone
        ? {
            allowed: false,
            refusal: {
              kind: 'recycled',
              message: `PID ${target.pid} has already exited — nothing to signal.`,
            },
          }
        : {
            allowed: false,
            refusal: {
              kind: 'unverifiable',
              message: `Could not confirm that PID ${target.pid} is still ${processName(target.command)}. Refusing rather than signalling a process it cannot identify.`,
            },
          };
    }
    /*
     * A start time of 0 means the date could not be read at all (see
     * parseLstart), which is unknown, not "epoch". Treating it as a value
     * would make two unreadable processes compare equal.
     */
    if (live.startTime === 0 || target.startTime === 0 || live.startTime !== target.startTime) {
      return {
        allowed: false,
        refusal: {
          kind: 'recycled',
          message: `PID ${target.pid} is no longer the process you selected — it has been reused. Aborted.`,
        },
      };
    }
  }

  return ALLOWED;
}
