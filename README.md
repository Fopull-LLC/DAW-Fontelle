# Fontelle

A lightweight, Linux-first digital audio workstation built around SoundFont-based
sample playback. An SF2 file supplies *defaults* — loop points, envelopes, filtering,
mapping, tuning — and every one of them is yours to override. Built by
[Fopull LLC](https://fopull.com).

**Status: early scaffolding.** No feature is implemented yet; see [Project status](#project-status).

## Why

Every existing SoundFont player treats the file's internal parameters as authoritative.
Fontelle inverts that: the SF2 supplies a starting point, and nothing about it is locked.
See [`FONTELLE_TDD.md`](FONTELLE_TDD.md) for the full design — product thesis, invariants,
crate layout, milestones, and the open questions still to resolve.

## Project status

This repository currently holds the M0 skeleton: every crate in the workspace exists with
its real module boundaries and dependency graph, and the whole thing compiles. No feature
is implemented — most functions are `todo!()` stubs describing what belongs there and
pointing at the relevant section of the TDD. The M0 exit gate (a real audio callback
playing one sampler voice from an SF2 file, end to end, with the zero-allocation
assertion active — TDD §22) has not been reached yet.

See [`docs/scaffolding-notes.md`](docs/scaffolding-notes.md) for the one structural
decision made during scaffolding that isn't explicit in the TDD.

## Building

Requires Rust 2024 edition (MSRV 1.88.0). On Linux you'll also need ALSA and
windowing/XKB development headers:

```sh
sudo apt install libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev \
    libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev

cargo check --workspace
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

"SoundFont" is a Creative/E-mu trademark and is used here only descriptively.
