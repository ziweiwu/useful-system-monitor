# useful-system-monitor

[![npm](https://img.shields.io/npm/v/useful-system-monitor?color=cb3837&logo=npm)](https://www.npmjs.com/package/useful-system-monitor)
[![CI](https://github.com/ziweiwu/useful-system-monitor/actions/workflows/ci.yml/badge.svg)](https://github.com/ziweiwu/useful-system-monitor/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![macOS](https://img.shields.io/badge/macOS-Apple%20Silicon%20%26%20Intel-lightgrey?logo=apple)](#requirements)
[![sponsor](https://img.shields.io/github/sponsors/ziweiwu?logo=githubsponsors&color=ea4aaa)](https://github.com/sponsors/ziweiwu)

**Your Mac is hot, the fan is loud, and the battery says two hours. Which app is
doing it?**

A terminal dashboard that ranks every process by what it actually costs you —
CPU, memory, and watts — and kills the offender safely.

## Install

```sh
npx useful-system-monitor
```

Or install it for good:

```sh
npm install -g useful-system-monitor
useful-system-monitor
```

Requires **macOS** and **Node 20+**. No config, no setup, no permissions to
grant.

Most people add a shorter name:

```sh
echo "alias usm='useful-system-monitor'" >> ~/.zshrc && source ~/.zshrc
```

![The dashboard: CPU, memory, disk and battery gauges above a process table ranked by CPU](docs/screenshot.svg)

## Keys

| Key | Action |
|---|---|
| <kbd>↑</kbd> <kbd>↓</kbd> | move selection |
| <kbd>enter</kbd> | process detail |
| <kbd>k</kbd> | kill the selected process |
| <kbd>/</kbd> | filter |
| <kbd>c</kbd> <kbd>m</kbd> <kbd>e</kbd> | sort by CPU, memory, energy |
| <kbd>1</kbd>–<kbd>4</kbd> | overview, CPU, memory, battery |
| <kbd>q</kbd> | quit |

## What's draining the battery

![Battery view: draw, drain history, health, and the processes spending it ranked in watts](docs/battery.svg)

Press <kbd>4</kbd>. Energy is estimated from CPU time; `--energy=accurate` uses
macOS's real Energy Impact instead, at the cost of ~1s of CPU per sample.

## Killing things safely

Critical system processes and your own ancestors are refused outright, SIGTERM
is the default, and the target is bound to `(pid, start time)` — so a recycled
PID aborts the kill rather than hitting something innocent.

## Options

```
--mock              Scripted data; touches nothing on your system
--json              One-shot JSON, for scripting
--top N             Rows in one-shot output (default 10)
--sort cpu|mem|energy
--interval SECONDS  Refresh interval (default 10)
--energy=accurate   Real macOS Energy Impact instead of the estimate
-h, --help
```

No TTY means no dashboard, so `useful-system-monitor --json | jq` composes.
`NO_COLOR` is honoured.

## Requirements

macOS only — the collectors read `ps`, `vm_stat`, `df`, `pmset` and `ioreg`, and
npm refuses to install elsewhere rather than shipping you something that cannot
work. Linux support would mean one new file behind the existing
`MetricsProvider` interface; PRs welcome.

It costs **1.3% of one core** to run. A monitor that drains your battery would
be a bad joke.

## Contributing

```sh
npm install
npm test && npm run lint && npm run typecheck
npm run mock         # iterate on the UI without touching the system
```

Behaviour is specified as numbered invariants in [INVARIANTS.md](./INVARIANTS.md),
each with a test named after it (`npx vitest run -t "I-16"`). If you change
anything under `src/providers/`, add a fixture in `test/fixtures/` captured from
real command output.

Releases are automated — `npm version patch && git push --follow-tags`.

## Sponsor

`useful-system-monitor` is built and maintained by one person, in evenings,
around a full-time job. It is MIT licensed and will stay that way.

If it has saved you a battery-drain hunt, [sponsorship](https://github.com/sponsors/ziweiwu)
buys the evenings that keep it moving. One-time works as well as monthly.

**Using it at work?** The $100/month tier includes priority response on issues
you file and your logo in this README. Invoiced sponsorships are available if
that is easier for your finance team.

## License

MIT © [Ziwei Wu](https://github.com/ziweiwu)
