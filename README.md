# Fontelle

A lightweight, Linux-first digital audio workstation built around SoundFont-based
sample playback. An SF2 file supplies *defaults* — loop points, envelopes, filtering,
mapping, tuning — and every one of them is yours to override. Built by
[Fopull LLC](https://fopull.com).

**Status: early, and runnable.** There is a window you can write music in; there
is a great deal that is not built. See [Project status](#project-status).

## Why

Every existing SoundFont player treats the file's internal parameters as authoritative.
Fontelle inverts that: the SF2 supplies a starting point, and nothing about it is locked.
See [`FONTELLE_TDD.md`](FONTELLE_TDD.md) for the full design — product thesis, invariants,
crate layout, milestones, and the open questions still to resolve.

## Running it

```sh
cargo run --release -p fontelle-app
```

That opens a window with a channel rack, a soundfont browser and a piano roll,
over a real audio device.

Your soundfonts live in a folder Fontelle scans, shown at the bottom of the
browser. **Open folder** shows you that folder in your file manager, creating it
if it is not there yet — drop your `.sf2` files in and they appear.
**Change…** picks a different folder (`Ctrl`+click to add one alongside rather
than replace), and the choice is remembered. The default is
`~/.local/share/fontelle/soundfonts`; there is a command-line equivalent if you
prefer:

```sh
cargo run --release -p fontelle-app -- --soundfonts /path/to/your/soundfonts
```

Then: pick a soundfont in the browser, pick a preset (a plain click puts it on
the selected channel; `Ctrl`+click puts it on a new one), and draw.

| | |
|---|---|
| Draw / delete a note | left mouse / right mouse |
| Draw one to length | drag out from an empty cell |
| Lengthen one | drag its right edge |
| Tools | `P` draw, `B` paint, `E` select, `D` delete |
| Snap | `S` cycles bar / beat / 1&frasl;8 / 1&frasl;16 / 1&frasl;32 / triplet / off |
| Off the grid, one axis | hold `Alt`, hold `Shift` |
| Select a region | the Select tool, or `Ctrl`+drag |
| Clipboard | `Ctrl+C` / `X` / `V`, `Ctrl+B` duplicate |
| Undo / redo / save | `Ctrl+Z` / `Ctrl+Y` / `Ctrl+S` |
| Play | `Space` |
| Scroll | wheel for pitch, `Shift`+wheel for time |
| Zoom, about the pointer | `Ctrl`+wheel for time, `Ctrl+Shift`+wheel for pitch |
| Find a soundfont | `Ctrl+F`, then type — it matches letters in order, so `gus` finds `GeneralUser GS` |

The command line is still there and does more than the window does — offline
WAV bounces, MIDI file import, live MIDI in, recording a take. `--play-sf2
<file>` plays a demo phrase through a preset; add `--window` to open the same
project in the editor, `--open <project.fontelle>` to reopen a saved one.

## Project status

**Working, and tested end to end:** the SF2 importer (filters, envelopes, LFOs,
the modulation matrix), the sampler and its voice architecture, the mixer and
master bus with a brickwall limiter, a piecewise tempo map, the transport with
looping and seeking, live MIDI input with hot-plug, MIDI recording, MIDI file
import, offline rendering to WAV, the project document with commands and undo,
saving and reopening a project, and the window described above.

**Not built yet:** the arrangement timeline, the mixer panel, the sampler
editor, automation, effects beyond the master limiter, sample streaming (a
soundfont is fully resident), plugin export, and most of what
[`FONTELLE_TDD.md`](FONTELLE_TDD.md) describes. Read
[`PROGRESS.md`](PROGRESS.md) for where things actually stand — it is the living
status document and it is blunt about what is missing.

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
