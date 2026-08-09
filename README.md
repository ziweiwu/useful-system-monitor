# useful-system-monitor

[![npm](https://img.shields.io/npm/v/useful-system-monitor?color=cb3837&logo=npm)](https://www.npmjs.com/package/useful-system-monitor)
[![CI](https://github.com/ziweiwu/useful-system-monitor/actions/workflows/ci.yml/badge.svg)](https://github.com/ziweiwu/useful-system-monitor/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![macOS](https://img.shields.io/badge/macOS-Apple%20Silicon%20%26%20Intel-lightgrey?logo=apple)](#requirements)
[![sponsor](https://img.shields.io/github/sponsors/ziweiwu?logo=githubsponsors&color=ea4aaa)](https://github.com/sponsors/ziweiwu)

See what's using up your Mac — and close the apps that are hogging it.

It shows your CPU, memory, disk and battery at a glance, lists the apps using
the most of each, and lets you quit any of them without leaving the terminal.

## Install

```sh
npx useful-system-monitor
```

Or install it so you can run it any time:

```sh
npm install -g useful-system-monitor
useful-system-monitor
```

Works on a Mac. Nothing to set up.

Want a shorter name to type?

```sh
echo "alias usm='useful-system-monitor'" >> ~/.zshrc && source ~/.zshrc
```

![The dashboard: CPU, memory, disk and battery at the top, with the busiest apps listed below](docs/screenshot.png)

## Using it

| Key | What it does |
|---|---|
| <kbd>←</kbd> <kbd>→</kbd> | move between the five screens |
| <kbd>1</kbd>–<kbd>5</kbd> | jump straight to one |
| <kbd>↑</kbd> <kbd>↓</kbd> | pick an app — the list scrolls to follow you |
| <kbd>+</kbd> <kbd>-</kbd> | show more apps, or fewer (50 → 150 → 300 → all) |
| <kbd>enter</kbd> | see more about it |
| <kbd>k</kbd> | close it |
| <kbd>/</kbd> | search |
| <kbd>c</kbd> <kbd>m</kbd> <kbd>e</kbd> | sort by CPU, memory, or battery use |
| <kbd>q</kbd> | quit |

The strip along the top shows which screen you are on and what the other four
are, so nothing is hidden behind a key you have to know about.

The dashboard wants a terminal at least 80x24. Below that it drops detail
rather than spilling past the bottom of your screen.

## Finding what's draining your battery

![Battery screen: charge, how fast it is draining, and which apps are responsible](docs/battery.png)

Press <kbd>4</kbd> (or walk there with <kbd>→</kbd>) to see how much power is
left, how fast it's going, and which apps are responsible — with an estimate of how much time you'd get back by
closing one.

## Seeing where your disk went

Press <kbd>5</kbd> for every mounted volume at a glance — how full each one is,
how much is left, and which are network shares. Volumes past 90% are called out,
because that's where macOS starts failing writes and dropping snapshots.

Your Mac's internal storage appears once, as `/`. macOS actually mounts it as
several APFS volumes sharing one pool, and `df` lists each with the same total —
so a raw listing makes one 1 TB disk look like four.

## Closing apps safely

You can't accidentally break your Mac with it. Apps that macOS needs to keep
running are off limits, and so is the terminal you're using. It asks an app to
close politely first, and if the app has already closed on its own, it won't
hit something else by mistake.

## Using it from a script

Piped or redirected, it prints one shot of plain text instead of taking over
the terminal — so it composes, and it works from cron:

```sh
useful-system-monitor | head -5
useful-system-monitor --json | jq '.processes[0].name'
useful-system-monitor --top 20 --sort mem     # the 20 biggest memory users
```

`--json` includes the version that produced it, so a consumer can tell which
shape it is reading.

## Extras

```
--json              Print the numbers instead of the dashboard
--top N             How many apps to list in that output (default 10)
--sort cpu|mem|energy   How to order them (default cpu)
--interval SECONDS  How often the dashboard refreshes, 1-3600 (default 10)
--energy=accurate   More precise battery numbers, at a small cost in speed
--mock              Try it with fake data, without touching your system
-h, --help          The full help, with examples
-v, --version       Which version you have
```

It exits 0 when it worked, 2 if you passed something it does not understand,
and 1 if it could not read the machine. An option it does not recognise is an
error, not something it ignores.

## If something looks wrong

**It printed text instead of the dashboard.** Something on either end is not a
terminal — usually a pipe, a redirect, or a launcher that gives it no stdin.
That is deliberate: the dashboard needs a real terminal on both ends. Run it
directly from a shell for the full screen.

**It says "no battery — on AC power".** That is a desktop Mac reporting
honestly; there is nothing to attribute energy against. Every other screen
still works.

**The colours are wrong or unreadable.** Set `NO_COLOR=1`. Everything is
readable without colour: state is always marked with a glyph as well as a hue.

**Numbers look stale.** Each panel shows its own age, next to the clock. Panels
refresh on different schedules on purpose — see [INVARIANTS.md](./INVARIANTS.md).

**Reporting a bug.** Include `useful-system-monitor --version` and your macOS
version.

## Requirements

A Mac, and [Node.js](https://nodejs.org) 20 or newer. Linux isn't supported yet
— contributions welcome.

Running it barely costs anything: about 1% of one CPU core. A monitor that ran
your battery down would rather defeat the point.

## Contributing

```sh
npm install
npm test
npm run mock         # work on the interface without touching your system
```

What changed between releases is in [CHANGELOG.md](./CHANGELOG.md). The
behaviour it promises is written down in [INVARIANTS.md](./INVARIANTS.md),
and every item there has a test. If you touch anything that reads from the
system, please add a sample of the real command output to `test/fixtures/`.

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
