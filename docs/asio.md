# ASIO® and better Windows audio

<img src="../assets/trademarks/asio/ASIO-compatible-logo-Steinberg-TM-BW.png" alt="ASIO Compatible Logo" width="140"/>

*ASIO is a registered trademark of Steinberg Media Technologies GmbH.*

Users on Windows reported clicky, crackly audio. Ty's decision
(2026-10-12): ship ASIO now under Steinberg's new GPLv3 option, as a
**separate helper program**, so Fontelle stays MIT/Apache and closed
add-ons never load into a GPL process — and make plain Windows audio better
too. This page is the whole of it: the licence, the design, the protocol
between Fontelle and the helper (public and versioned), what was measured,
and what Ty tests on his laptop.

Settings → Audio output → **Backend** on Windows now offers, besides
`cpal`'s **WASAPI** (shared, as before):

| Backend | What it is | Automatic buffer |
|---|---|---|
| **WASAPI (low latency)** | Shared with other programs, at the smallest engine period the driver allows (`IAudioClient3`) instead of Windows's 10 ms | 240 frames (5 ms) |
| **WASAPI (exclusive)** | Fontelle alone on the device, event-driven, in the device's own format | 512 frames (10.7 ms) |
| **ASIO® (via fontelle-asio helper)** | The interface's own ASIO driver, hosted by `fontelle-asio.exe`; listed only when that program is beside `fontelle.exe` | the driver's own (its control panel) |

Whatever opens, the page says **whose** dropouts they were and how busy the
audio thread is, and so does the transport bar (below).

## 1. The licence

### What Steinberg offers (checked 2026-10-12)

`https://www.steinberg.net/asiosdk` downloads
`https://download.steinberg.net/sdk_downloads/ASIO-SDK_2.3.4_2025-10-15.zip`:

| | |
|---|---|
| Version | ASIO SDK 2.3.4, 2025-10-15 (`changes.txt`: "new DUAL licensing: Proprietary and GPVv3") |
| sha256 | `d5ebf0c20dd2c5f43771fd0c1418f4b361bf52434ee670097cfa6b3a335e2eca` |
| Kept at (not in any repository) | `/mnt/disks/6tb/FontelleTestData/asiosdk/` on the devbox |

It is the same zip the earlier `feature/asio` branch pinned as "proprietary";
that branch read only the proprietary half. `LICENSE.txt` (and the identical
`common/LICENSE.txt`) says, word for word:

> This Software Development Kit is licensed under the terms of the Steinberg
> ASIO License, or alternatively under the terms of the General Public
> License (GPL) Version 3. You may use the Software Development Kit
> according to either of these licenses as it is most appropriate for your
> project on a case-by-case basis (commercial or not).
> …
> b) General Public License (GPL) Version 3
> Details of these licenses can be found at: www.gnu.org/licenses/gpl-3.0.html

`README.md`, "Dual Licensing":

> Developers can choose between two options:
> - Use of the **proprietary ASIO SDK license**, for use cases that benefit
>   from closed-source licensing.
> - Adopt the General Public License (GPL) Version 3 for full open-source
>   integration.
>
> Both options grant access to the same SDK.

and its FAQ: *"Can I switch to GPLv3 License? — Yes, if your product is
released under GPLv3 License terms (including source code disclosure)."*

"Version 3", with no "or any later version": the helper is
**GPL-3.0-only**. The `host/` helper files (`asiodrivers.*`, `asiolist.*`)
carry a BSD-style notice of their own instead; nothing here uses them except
as a description of how drivers are found.

### The trademark

The README: *"Trademark usage (e.g. "ASIO" name or logo) is optional under
GPLv3 License, but, if used, must comply with Steinberg's official trademark
rules"*, and *"Non-compliance with logo guidelines does not affect your
GPLv3 rights (but may result in trademark violations)."* The *Steinberg ASIO
Usage Guidelines* (for SDK 2.3.4 or higher) ask, among other things:

- §15: the first use of "ASIO" in product material carries ®, and product
  credits and documentation say *"ASIO is a registered trademark of
  Steinberg Media Technologies GmbH."*
- §2, §6, §1(f): plain-text "ASIO" (including in a product name like
  "MyCoolEngine for ASIO") is allowed **with the ASIO Compatible Logo in
  the relevant context**; where ASIO is only switched on by hand, the logo
  belongs in every dialog that enables or configures it. The exception: where
  space is too short for the logo, the text with the notice.
- §7, §10, §13: never "ASIO" in a company name or logo, never a variant like
  "ASIOd".

The guidelines set no size, clear space or colour; they ask for the logo
unaltered (README), in one of §14's official forms, wherever the product is
presented, and for the ® and the attribution. Fontelle uses Steinberg's own
`ASIO-compatible-logo-Steinberg-TM-BW.png` — §14's form with the
attribution beneath the logo — copied byte for byte to
`assets/trademarks/asio/` (sha256 `5b5c4aae…`, pinned by
`crates/fontelle-app/tests/asio_trademark.rs`), decoded without the tint
Fontelle's own mark gets, and only ever scaled at its own proportions. It is
Steinberg's trademark, not covered by any of Fontelle's licences: its
`NOTICE`, `licenses/TRADEMARKS.md` and the README's licence section say so.
It is compiled into Windows builds (and debug builds, for the tests); a
released Linux or macOS Fontelle offers no ASIO and carries no mark of
Steinberg's. Where each requirement is met:

| Guideline | Asks | Where |
|---|---|---|
| README "Trademark and Logo Usage" | logos unaltered | Steinberg's file, byte for byte; drawn scaled only (`fontelle-ui/src/trademarks.rs`) |
| §1(f), §2 | if ASIO is switched on by hand, the logo in every dialog that enables or configures it, adjacent to the word | Settings → Audio output: the logo beside the **Backend** row (once per page; the Driver settings row is on the same page), whenever ASIO is offered — Windows, with `fontelle-asio` beside Fontelle (`Session::setting_badges`; headless check `where_asio_is_offered_steinbergs_logo_is_beside_it_on_the_settings_page`) |
| §1(e) | the logo in the About box or equivalent (help menu, splash) | the start menu — Fontelle's About — carries the logo and the credit "ASIO® output by fontelle-asio, a separate GPLv3 program. ASIO is a registered trademark of Steinberg Media Technologies GmbH." where ASIO is offered (headless check `where_asio_is_offered_the_start_menu_credits_steinberg`) |
| §1(c) | the logo in all product documentation, or the attribution where it cannot go | this page and the README (the image); the Windows zip's `fontelle-asio-README.txt` and `fontelle-asio --help` (text: the attribution, and where the logo is); the logo itself in the zip under `licenses/trademarks/asio/` |
| §1(b), §4, §11 | on web pages and posts naming ASIO | the GitHub release notes carry the logo and the attribution (`release.yml`); the README is the repository's page. Fopull's website (agent W's Fontelle page) and any video or post naming ASIO must carry it too — not in this repository |
| §6 | "ASIO" in a product name only in plain text, logo in immediate proximity | "ASIO® (via fontelle-asio helper)" in plain text with the logo beside it; `fontelle-asio` is named beside the logo in the start menu credit and the docs |
| §7, §10, §13 | no "ASIO" in a company name or logo, no variants | none: Fontelle's logo is its own; no "ASIOx" anywhere |
| §15 | ® at first use; the attribution in credits and documentation | "ASIO®" at first use on the settings page (the backend's name), in the start menu, the README, this page, the helper's README and `--help`; the attribution on its own line under the backend (the **ASIO®** row, never cut short), in the credit, README, docs, NOTICE, helper README and `--help` |

### Why a separate program, and what its source is

Fontelle is MIT OR Apache-2.0 and loads closed plugins and extensions; a
GPLv3 SDK linked into it would make that GPL. So the SDK's code lives in
`fontelle-asio.exe` (`crates/fontelle-asio-host/`, GPL-3.0-only, its own
`LICENSE`), and Fontelle talks to it the way two programs do — a shared
block of plain numbers and samples, two named events, a few lines of text —
never linking it, never loading its code, and it never loads Fontelle's or a
plugin's. The protocol crate both use (`crates/fontelle-audio-ipc/`) is
MIT/Apache and has nothing of Steinberg's. `deny.toml` allows GPL-3.0-only
for that one crate only, and `crates/fontelle-asio-host/tests/separate.rs`
fails if anything else ever depends on it.

**Corresponding source.** The helper uses only the SDK's *declarations* —
the driver interface's method order, struct layouts, numbers — translated to
Rust in `crates/fontelle-asio-host/src/asio/sys.rs` with Steinberg's notice;
no SDK file is compiled, vendored or fetched at build time. So the
Corresponding Source of the shipped `fontelle-asio.exe` is exactly this
repository at the release's tag: the helper, the protocol crate, the
workspace manifest and the `Cargo.lock` that pins every dependency (all
permissive, all on crates.io). That beats both alternatives considered: a
fetch-by-sha256 build would make the SDK zip part of the Corresponding
Source we must offer (and depends on Steinberg's URL living forever), and
vendoring the SDK's C++ would need a C++ toolchain and bindgen on every
build. The release offers the source from the same place as the binary
(GPLv3 §6(d)): `fontelle-asio-source-<version>.tar.gz` (a `git archive` of
the tag) is attached to every release beside the zip, and the zip's
`fontelle-asio-README.txt` says so. Steinberg's licence text travels with
the binary as `licenses/fontelle-asio/STEINBERG-ASIO-SDK-LICENSE.txt`.

## 2. How it fits together

```text
 fontelle.exe (MIT/Apache)                          fontelle-asio.exe (GPLv3)
 ─────────────────────────                          ─────────────────────────
 Settings → ASIO ─ AudioOutput::open
   │  makes  Local\fontelle-audio-<pid>-<n>        (shared block, §3.1)
   │         …-request, …-reply                    (auto-reset events)
   │  spawns  fontelle-asio run --driver … ───────► main thread (COM STA,
   │          (no console window)                   pumps messages): loads the
   │                                                driver, sets rate & buffer,
   │  ◄── stdout: fontelle-asio 1 / ready … ─────── createBuffers, start
   │
   │  supervisor thread: reads the lines,     driver's thread, per buffer:
   │  logs events, notices the helper going     bufferSwitch(i) → Pump::cycle:
   │                                              input → slot[seq&1]
   │  audio thread (MMCSS "Pro Audio"):          request_seq = seq, SetEvent ───┐
   │   wait request ◄─────────────────────────────────────────────────────────┘
   │   Renderer::render → output slot              wait reply (≤ 75% of buffer)
   │   reply_seq = seq, SetEvent ──────────────►   convert → driver's buffer[i]
   │                                                (or silence, counted late)
   │  stdin: panel / stop  ────────────────────►   outputReady()
```

- **Fontelle's side** (`crates/fontelle-engine/src/output_asio.rs`): the
  audio thread waits on the request event, renders the buffer straight into
  the shared slot with the same `Renderer` `cpal`'s callback runs (no
  allocation, no lock — INVARIANT 1, checked by `RtGuardAllocator` in
  `tests/output_asio_helper.rs`), and rings back. The helper's counters are
  copied into `OutputStats` as it goes.
- **The helper** (`crates/fontelle-asio-host/`): `asio/` finds drivers under
  `HKLM\SOFTWARE\ASIO` (as the SDK's `asiolist.cpp` does), makes one with
  `CoCreateInstance`, and runs it; `pump.rs` is one buffer's work;
  `convert.rs` writes every ASIO sample type (16/24/32-bit integers, the
  32-bit containers of 16/18/20/24 bits, float and double, either
  endianness); `run.rs` is the loop around it all.
- **Synchronous**: Fontelle renders inside the driver's own `bufferSwitch`,
  so the helper adds **no buffer of latency** — the cost is two wake-ups
  (§4). If Fontelle is not back within 75% of the buffer, that buffer plays
  silence and is counted *late*; the driver is never stalled.

What happens when:

| Event | What the helper does | What Fontelle does |
|---|---|---|
| Driver asks for a reset (`kAsioResetRequest`, often after its control panel changed something) | stops, disposes, opens the driver again — at its new preferred size if that changed, else at Fontelle's — and starts; says `event reset` (and `buffer`, `latencies` if they moved) | nothing: the next buffer arrives with its new frame count; *Playing through* shows the new size |
| Driver's rate moves (`sampleRateDidChange`) | puts it back to the studio's rate, or stops with the words of the refusal | as below |
| `kAsioLatenciesChanged` | reads them again, publishes them | shows them |
| `kAsioOverload`, `kAsioResyncRequest` | counts a device dropout, says `event overload` | counts it as the device's |
| Device unplugged or taken (no `bufferSwitch` for 1 s or 20 buffers) | says so, stops, exits | marks the output lost; plays through the default instead, saying why |
| Fontelle exits or crashes | stdin closes (and on Windows it watches Fontelle's process): stops the driver cleanly, exits | — |
| Helper crashes or is killed | — | the supervisor sees it go, marks the output lost with its last words; the session plays through the system default (WASAPI) and says *"… would not open (fontelle-asio stopped …); playing through WASAPI instead"*. It does not start the helper again on its own; choosing ASIO again in Settings does |
| Helper alive but silent for 3 s | — | treated as gone, as above |

## 3. The protocol (version 1)

Public and stable: a helper built from one Fontelle version and a Fontelle
from another either agree on the version or refuse in words. Anything may
be **added** (reserved header bytes, new line types, new `key=value` fields
on `ready`) without a new version; anything else is version 2. The code is
`crates/fontelle-audio-ipc/`; its tests hold every offset below.

### 3.1 The shared block

Named `Local\fontelle-audio-<fontelle pid>-<n>` on Windows (a page-file
backed file mapping), `/fontelle-audio-<pid>-<n>` on Linux (POSIX shared
memory, for development). Fontelle makes it; the helper opens it.

```text
offset 0     header, 512 bytes
       512   input slot 0    max_frames × max_inputs  f32, interleaved
             input slot 1
             output slot 0   max_frames × max_outputs f32, interleaved
             output slot 1
```

Cycle `seq` uses slot `seq & 1`. All fields little-endian, naturally
aligned, touched only atomically or by the slot's owner.

| Offset | Field | Type | Written by | Meaning |
|---|---|---|---|---|
| 0 | `magic` | u32 | Fontelle | `"FAIO"` (0x4F494146) |
| 4 | `version` | u32 | Fontelle | 1 |
| 8 | `header_bytes` | u32 | Fontelle | 512 |
| 12 | `max_frames` | u32 | Fontelle | slot size in frames (Fontelle: 8192) |
| 16 | `max_inputs` | u32 | Fontelle | input channels exchanged per frame (Fontelle: 0) |
| 20 | `max_outputs` | u32 | Fontelle | output channels exchanged per frame (Fontelle: 2) |
| 24 | `state` | u32 | both | 0 starting, 1 running, 2 helper stopped, 3 Fontelle closed |
| 28 | `buffer_frames` | u32 | helper | the driver's buffer as running |
| 32 | `sample_rate` | u32 | helper | Hz |
| 36 | `inputs` | u32 | helper | the driver's input channels (informational) |
| 40 | `outputs` | u32 | helper | the driver's output channels (informational) |
| 44 | `input_latency` | u32 | helper | frames, as the driver says |
| 48 | `output_latency` | u32 | helper | frames |
| 52 | `helper_pid` | u32 | helper | |
| 56 | `client_pid` | u32 | Fontelle | |
| 64 | `request_seq` | u32 | helper | the cycle rung for (own cache line) |
| 68 | `request_frames` | u32 | helper | frames in it, stored before `request_seq` |
| 128 | `reply_seq` | u32 | Fontelle | the last cycle answered (own cache line) |
| 132 | `render_ns` | u32 | Fontelle | how long rendering it took |
| 192 | `cycles` | u64 | helper | buffers rung for |
| 200 | `device_xruns` | u64 | helper | buffers the device lost (gaps, overloads, resyncs) |
| 208 | `late_replies` | u64 | helper | buffers Fontelle answered after the deadline |
| 216 | `resets` | u64 | helper | driver resets done |
| 224 | `handoff_last_ns` | u32 | helper | last round trip minus `render_ns` |
| 228 | `handoff_max_ns` | u32 | helper | the largest since the block was made |
| 232 | `deadline_ns` | u32 | helper | how long it waits for an answer |
| 236–511 | reserved | | | zero |

### 3.2 One buffer

Helper (in `bufferSwitch`): write the driver's input to input slot
`seq & 1`; store `request_frames`; store `request_seq = seq` (release); set
`<name>-request`; wait on `<name>-reply` until `reply_seq == seq` (acquire)
or the deadline; answered → convert output slot `seq & 1` into the driver's
buffer; late → silence, `late_replies += 1`; `state == 3` → stop.

Fontelle: wait on `<name>-request` until `request_seq` differs from the last
seen (or `state == 2`); render into output slot `seq & 1`; store
`render_ns`; store `reply_seq = seq` (release); set `<name>-reply`. A
Fontelle that fell behind answers the newest `request_seq` only.

The events are auto-reset; both sides re-check the word after every wake,
so a ring with nobody waiting is never lost and a stale one costs one loop.
Waits shorter than 20 ms use a high-resolution waitable timer (a 64-frame
buffer's deadline is about a millisecond). On Linux the doorbells are a
futex on the word itself.

### 3.3 The command line and the text lines

```text
fontelle-asio --version                     → fontelle-asio<TAB>1
fontelle-asio list                          → the hello, then driver<TAB><name> per driver
fontelle-asio panel --driver <name>         → opens the driver's panel; exits when it closes
fontelle-asio run --driver <name> --channel <block name> --rate <hz>
                  --inputs <n> --outputs <n> --parent <pid> [--frames <n>]
```

Lines are UTF-8, one message each, fields separated by tabs; a side ignores
what it does not know. Helper → Fontelle (stdout): `fontelle-asio<TAB>1`
(always first), `driver<TAB>name`, `ready<TAB>driver=…<TAB>frames=…<TAB>rate=…
<TAB>inputs=…<TAB>outputs=…<TAB>input_latency=…<TAB>output_latency=…
<TAB>min_frames=…<TAB>max_frames=…<TAB>preferred_frames=…<TAB>granularity=…
<TAB>format=…`, `event<TAB>reset`, `event<TAB>rate<TAB>hz`,
`event<TAB>buffer<TAB>frames`, `event<TAB>latencies<TAB>in<TAB>out`,
`event<TAB>overload`, `error<TAB>words`, `bye<TAB>words`. Fontelle → helper
(stdin): `panel`, `stop`; stdin closing means stop.

## 4. What the handoff costs

Measured on the devbox (Linux, load ~3–30 from other work, ordinary
scheduling — not real-time threads), release build, `fontelle-asio`'s
`tests/process.rs` against its test clock at 256 frames, the helper's own
measure (round trip minus rendering):

| | median | 49th of 50 | worst of a run |
|---|---|---|---|
| Two processes, futex doorbells (Linux) | **12 µs** | 20 µs | 70–410 µs |
| Two processes, Windows build under Wine (events, timer) | 14 µs | 33 µs | 380 µs |
| Two mappings in one process (Linux) | 4.5 µs | p99 11 µs | 47 µs |

The target was < 100 µs: the median is well under, and the worst cases are
the test threads being descheduled on a busy machine, which MMCSS ("Pro
Audio", what Fontelle's audio thread asks for) and the driver's own
high-priority thread are there to prevent. **The number that matters is the
one on real Windows**: while ASIO plays, Settings → *Processing load* shows
"…, ASIO handoff N µs" (the helper's last measure), and the session log
gets it with every dropout line.

## 5. WASAPI, better (`crates/fontelle-engine/src/wasapi.rs`)

`cpal`'s WASAPI is shared mode only, and in shared mode the callback comes
once per engine period — Windows's 10 ms whatever buffer was asked for —
through Windows's mixer and resampler (`AUTOCONVERTPCM`). Two modes of
Fontelle's own, each on a thread of its own, event-driven, rendering with
the same `Renderer`:

- **Exclusive**: tries float, then 32-, 24-in-32-, 24- and 16-bit integers
  at 48 kHz stereo (`DeviceFormat` writes each); asks for the chosen buffer
  (never below the device's minimum period), and when the device says
  `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` asks again for exactly the size it
  wants. Refusals are said: another program holds it exclusively; Windows
  forbids exclusive control on it (Sound settings → device → Advanced →
  "Allow applications to take exclusive control"); it will not play 48 kHz.
  Other programs are silent while it plays — Windows's rule.
- **Low latency**: `IAudioClient3::InitializeSharedAudioStream` at the
  smallest engine period at or above the size asked
  (`low_latency_period`, on the driver's own steps). It plays the engine's
  mix format and rate, so a device Windows runs at 44.1 kHz is refused in
  words (set it to 48 kHz, or use plain WASAPI, which converts). A driver
  without small periods (Microsoft's generic HD Audio driver says 10 ms only)
  simply runs at 10 ms — still event-driven, no worse than before.
- Either one that will not open falls back to the default backend, saying
  why, as every choice does.

## 6. Whose click was it?

`OutputStats` (every backend) keeps two counts and a load:

- **device** — the backend said the device ran dry (`cpal`'s underrun), the
  ASIO driver reported an overload or a resync or did not call on time, the
  WASAPI device did not wake the thread on time or its shared buffer was
  found empty while Fontelle was on time. *A driver, USB or system problem:
  a bigger buffer rarely helps; another backend often does.*
- **late** — Fontelle's render took as long as the buffer lasts or longer
  (timed around every render), or the ASIO helper's deadline passed. *A
  larger buffer cures it; so does a lighter song.*
- **load** — the render's share of each buffer, smoothed, and the worst
  recent one.

Shown in Settings → Audio output (**Dropouts**: "3 from the device, 1 late
(Fontelle)"; **Processing load**: "12% (peak 40%)", with the ASIO handoff),
on the transport bar (a strip under the meter — green, red from 75% — and a
red mark once any buffer was lost, until Dropouts → Reset), once as a
message when they pile up (naming the cure for whichever it is), and in the
session log (Start menu → *Logs folder*) as a line whenever the counts move,
at most every five seconds:

```text
fontelle: audio: 3 device dropouts, 1 late buffer, load 12% (peak 40%), handoff 14 µs · ASIO® (via fontelle-asio helper) · Focusrite USB ASIO · 128 frames (2.7 ms) · in 3.4 ms, out 4.1 ms
```

The helper's own events and errors are in the same log, prefixed
`fontelle: fontelle-asio:`.

## 7. Building and testing here

```sh
cargo build -p fontelle-asio-host                  # the helper (a no-op stub off Windows)
cargo test -p fontelle-audio-ipc -p fontelle-asio-host
cargo test -p fontelle-engine --test output_asio_helper --test output_health --test device_format
```

None of these needs an ASIO driver or makes a sound: the helper's **test
clock** stands in for a driver, offered only with
`FONTELLE_ASIO_TEST_CLOCK=1`. It can be told to misbehave:
`FONTELLE_ASIO_TEST_CLOCK_SKIP_EVERY=n` (skip two buffers every n),
`…_RESET_AFTER=n` with `…_RESET_FRAMES=f` (ask for a reset, come back with
another preferred size), `…_STOP_AFTER=n` (stop calling, as an unplugged
device does). `FONTELLE_ASIO_HELPER=<path>` points Fontelle at a helper
somewhere other than beside its own program.

On the devbox the Windows build also runs under Wine, headless, with the
audio drivers off (no sound, no window):

```sh
cargo build --target x86_64-pc-windows-gnu -p fontelle-asio-host --bin fontelle-asio
env -u DISPLAY -u WAYLAND_DISPLAY -u CARGO WINEPREFIX=<scratch> WINEDEBUG=-all \
  WINEDLLOVERRIDES="mscoree,mshtml=;winepulse.drv,winealsa.drv=d" \
  CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine \
  cargo test --target x86_64-pc-windows-gnu -p fontelle-audio-ipc -p fontelle-asio-host
```

CI: the `asio-helper` job (`windows-latest`) builds the helper as it is
released, checks `--version`, and runs all of the above for real on
Windows. The release builds `fontelle-asio.exe` into the Windows zip with
`fontelle-asio-README.txt` and `licenses/fontelle-asio/` (GPLv3 and
Steinberg's terms), and attaches `fontelle-asio-source-<version>.tar.gz`;
the Linux and macOS archives are unchanged. The in-app updater brings the
helper, its README and its licences along with `fontelle.exe`.

## 8. Ty's checklist on Windows

Nothing in this branch has run on real Windows hardware: Wine runs the
shared memory, the events and the helper process, but not an ASIO driver or
a WASAPI device. Please take the Windows zip CI builds from this branch (or
`cargo build --release -p fontelle-app -p fontelle-asio-host` and put
`target\release\fontelle-asio.exe` beside `fontelle.exe`), and go through
this with the session log open (Start menu → *Logs folder*, the newest
file). Write down for each step what Settings → Audio output said in
*Playing through*, *Dropouts* and *Processing load*.

**A. The helper is found, and the licences are there**

1. Unzip; check `fontelle-asio.exe`, `fontelle-asio-README.txt` and
   `licenses\fontelle-asio\LICENSE` + `STEINBERG-ASIO-SDK-LICENSE.txt` are
   in the folder.
2. Start Fontelle. The start menu shows Steinberg's ASIO Compatible Logo
   at the foot of the recent list with the credit beside it. Settings →
   Audio output → Backend: the list has **WASAPI (low latency)**, **WASAPI
   (exclusive)** and **ASIO® (via fontelle-asio helper)**; the logo stands
   beside the Backend row, black with its white letters (never tinted),
   and the row under it, **ASIO®**, reads "ASIO is a registered trademark of
   Steinberg Media Technologies GmbH" in full. Check both in the dark and the
   light theme, and at the window sizes you use.
3. Move `fontelle-asio.exe` away and restart: ASIO is not offered, and
   neither the logo nor the credit appears anywhere. Put it back.

**B. Your interface's own ASIO driver**

4. Backend → ASIO®; Output device lists your interface's driver (and
   ASIO4ALL / FlexASIO if installed). Choose it. No console window should
   flash up. *Playing through* says the driver, a buffer, and "in … ms, out
   … ms".
5. Play a song for a minute. Listen for clicks. *Dropouts* should stay
   "None"; note *Processing load* and the **ASIO handoff** in µs (target:
   well under 100 µs).
6. Buffer size: try 64, 128, 256, 1024 and The driver's own. Each should
   take effect (Playing through shows the size the driver accepted —
   drivers that take only powers of two round up).
7. **Driver settings → Open**: the driver's control panel appears. Change
   its buffer size there and close it. Within a second or two *Playing
   through* should show the new size, the music carries on (a short gap is
   expected), and the log has `the driver reset itself` and `the driver's
   buffer is now …`.
8. In the panel (or Windows), change the interface to 44.1 kHz. Fontelle
   should put it back to 48 kHz, or — if the driver refuses — fall back to
   WASAPI with a message naming the rates.
9. Unplug the interface while playing. Within a couple of seconds:
   a message that the output went away (with the helper's words) and sound
   through WASAPI's default. Plug it back in; choose ASIO again in Settings
   (or press *Playing through* → Restart) — it should play again.
10. Task Manager → end `fontelle-asio.exe` while playing. Fontelle should
    carry on through WASAPI and say *fontelle-asio stopped …*. It must not
    start the helper again on its own; choosing ASIO again does.
11. Close Fontelle while ASIO plays: `fontelle-asio.exe` must be gone from
    Task Manager within a second or two. Also end `fontelle.exe` from Task
    Manager while ASIO plays: the helper must go too.
12. Load a heavy song (or many plugins) at 64 frames until it crackles:
    *Dropouts* should say "… late (Fontelle)", the transport strip goes red,
    and the message suggests a larger buffer.

**C. ASIO4ALL and FlexASIO** (install from their sites if missing)

13. Repeat 4–7 and 11 with **ASIO4ALL v2** (its panel is a window of its
    own: check *Driver settings → Open* works while playing and while not).
14. Repeat 4–7 and 11 with **FlexASIO** (it has no panel: *Driver settings*
    should say so in words; its buffer is set in its `FlexASIO.toml`).

**D. Plain Windows audio**

15. Backend → **WASAPI (low latency)**, Automatic: *Playing through* says
    a buffer; on most onboard sound it is 240–480 frames. Play: clicks?
    Dropouts? Try 128. If the device runs at 44.1 kHz in Windows, the
    message should say so and plain WASAPI plays instead.
16. Backend → **WASAPI (exclusive)**, Automatic, then 128 and 64: play; other
    programs (a browser video) are silent meanwhile and come back after.
    Turn off "Allow applications to take exclusive control" in Sound
    settings: choosing exclusive should fall back with that advice.
17. In each mode, unplug headphones / switch the default device while
    playing: it should fall back or move on, and say so.

**E. The updater** (needs the next release): updating from the start menu
keeps ASIO working (`fontelle-asio.exe` replaced, licences present).

Send the session logs and the notes. Anything that does not match this list
is a bug report.

## 9. Gaps

- **Not run on real Windows.** Everything above is compiled for Windows,
  clippy-checked, and the IPC, the helper and Fontelle's side of it run
  under Wine; the ASIO driver calls and both WASAPI modes are only checked
  by section 8.
- **The logo off this repository.** The website's Fontelle page, store
  pages, videos and posts that name ASIO need the logo too (§4, §8, §11) —
  agent W's and Ty's, not code here.
- **The logo is small on the settings row** (as tall as the row, about 56
  points): its built-in attribution is not readable at that size, which is
  why the attribution also has a line of its own right under it.
- **Output only, first two channels.** The protocol carries inputs and any
  channel count; Fontelle asks for stereo out only. Recording through ASIO,
  and choosing which output pair, are next.
- **One rate.** The studio runs at 48 kHz; a driver that will not is
  refused rather than resampled.
- **Corresponding source of dependencies.** The release's source archive is
  the repository at the tag, with `Cargo.lock` pinning the helper's crates
  (`windows-sys`, `fontelle-audio-ipc`, …), which are on crates.io under
  permissive licences. If a stricter reading is wanted, a `cargo vendor` of
  the helper's dependencies can be added to that archive.
- **WASAPI exclusive picks stereo** and does not try a device's other
  channel counts; a device that only opens exclusively with 4+ channels is
  refused with words and falls back.
