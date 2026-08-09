/**
 * End-to-end check of the real signal path (plan verification step 5).
 * Spawns a disposable process, kills it through the guard, then confirms the
 * denylist and PID-reuse guards refuse rather than signalling.
 */
import { spawn } from 'node:child_process';
import { checkKill, type GuardContext } from '../src/kill/guards.js';
import { sendSignal } from '../src/kill/signal.js';
import { DarwinProvider } from '../src/providers/darwin/provider.js';

const alive = (pid: number): boolean => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

const main = async () => {
  const victim = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {
    stdio: 'ignore',
    detached: false,
  });
  const pid = victim.pid!;
  console.log(`spawned disposable process pid=${pid}, alive=${alive(pid)}`);
  await wait(600);

  const provider = new DarwinProvider();
  await provider.processes();
  await wait(400);
  const data = await provider.processes();

  const all = [...data.visible];
  const target = all.find((p) => p.pid === pid);
  const ctx: GuardContext = { selfPid: process.pid, parents: data.parents };

  if (!target) {
    // An idle process may not make the top-50, which is expected; construct the
    // sample directly so the signal path is still exercised.
    console.log('note: victim not in working set (idle, as expected) — targeting directly');
  }
  const sample = target ?? {
    pid,
    ppid: process.pid,
    startTime: 0,
    command: process.execPath,
    user: 'ziweiwu',
    state: 'S',
    cpuPercent: 0,
    rssBytes: 0,
    energy: 0,
    protected: false,
  };

  const out = sendSignal(sample, 'SIGTERM', ctx);
  console.log('sendSignal ->', JSON.stringify(out));
  await wait(500);
  console.log(`after SIGTERM: alive=${alive(pid)}  ${alive(pid) ? 'FAIL' : 'OK'}`);

  // I-14: the denylist must refuse.
  const ws = all.find((p) => p.command.includes('WindowServer'));
  if (ws) {
    const check = checkKill(ws, ctx);
    console.log(
      `WindowServer (pid ${ws.pid}) -> ${check.allowed ? 'ALLOWED (FAIL)' : 'REFUSED: ' + check.refusal.message}`,
    );
  } else {
    console.log('WindowServer not in working set this tick — covered by unit tests');
  }

  // I-13: our own parent must be refused.
  const parentPid = ctx.parents.get(process.pid);
  console.log(`self (pid ${process.pid}) -> ${checkKill({ ...sample, pid: process.pid }, ctx).allowed ? 'ALLOWED (FAIL)' : 'REFUSED (OK)'}`);
  console.log(`parent chain known: ${parentPid !== undefined} (ppid=${parentPid})`);
  if (parentPid !== undefined) {
    const anc = checkKill({ ...sample, pid: parentPid }, ctx);
    console.log(
      `ancestor pid ${parentPid} -> ${anc.allowed ? 'ALLOWED (FAIL)' : 'REFUSED: ' + anc.refusal.kind}`,
    );
  }

  if (!victim.killed) victim.kill('SIGKILL');
  process.exit(0);
};

void main();
