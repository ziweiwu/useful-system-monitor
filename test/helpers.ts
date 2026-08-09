import type { ProcessSample } from '../src/core/types.js';

export function sample(over: Partial<ProcessSample> & { pid: number }): ProcessSample {
  return {
    ppid: 1,
    startTime: 1_000,
    command: `/usr/bin/proc${over.pid}`,
    user: 'ziweiwu',
    state: 'S',
    cpuPercent: 0,
    rssBytes: 1024,
    energy: 0,
    protected: false,
    ...over,
  };
}
