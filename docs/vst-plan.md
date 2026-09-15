# VST hosting: the design

*Written 2026-09-14. Nothing in it is built; the phases at the end say what
comes first. It supersedes §4 of `docs/plugin-compatibility-plan.md`, which
was written when the VST 3 SDK could not be linked here, and it closes the
TDD's standing "verify VST3 SDK licensing terms" action.*

> *"a lot of people may not want to switch to the daw if they can't use their
> paid vsts in it ... maximum compatibility is what's most important to me. i
> want this to be a daw anyone could pick up and easily be able to use their
> tools in it so they could just point the plugins folder and stuff wherever
> they want."*

## 1. What was verified, and what it changes

**VST 3 is MIT.** Steinberg released VST 3 SDK 3.8.0 on 31 October 2025 under
the MIT licence. The developer portal is explicit: *"Licensing under GPLv3 and
the Steinberg proprietary license is no longer available"*, no fees, no
membership, no signed document, perpetual. The SDK is `base`,
`pluginterfaces`, `public.sdk` and `vstgui4` on GitHub, `LICENSE.txt` is the
MIT text. Every reason the previous plan put VST 3 behind an out-of-tree bridge
is gone: **MIT is on §3.4's allowlist, so the VST 3 interfaces can be hosted in
`fontelle-host` next to CLAP and LV2.** The Rust binding to use is `vst3`
(coupler-rs, 0.3, MIT OR Apache-2.0, generated from the MIT-era headers) — not
`vst3-sys`, which is GPLv3 and is to be replaced by it.

**The trademark is separate from the licence.** Using the VST name or the
*VST Compatible* logo is optional; if used, Steinberg's usage guidelines
apply: the logo unaltered and only per the guidelines, "VST" never in a
company or product name, the first use of "VST" in product material marked ®,
and *"VST is a registered trademark of Steinberg Media Technologies GmbH"* in
credits and documentation. Non-compliance does not touch the MIT grant; it is
a trademark matter. The rule for this project: **we describe formats, we do
not brand with them.** No logo. The About box and README carry the ® line
once. Menus say "VST3" and "VST2" as file-format words the way they say "LV2"
and "CLAP".

**VST 2 is not licensed, and cannot be.** Steinberg's FAQ: new VST 2
development and distribution without an agreement signed before October 2018
*"is not permitted"*; the VST 2 headers (`aeffect.h`, `aeffectx.h`) may not be
shared; the VST 2 API is expressly *not* part of the VST 3 licence. There is
no agreement to sign. What the rest of the world does instead: every
open-source host that loads VST 2 today — Ardour, Carla, LMMS, yabridge —
does it against a **clean-room description of the VST 2.4 interface** written
without Steinberg's files (VeSTige, FST, Xaymar's `vst2sdk` under BSD-3, which
went through external developers and lawyers to arrive at a document its
author never saw the original of). The legal basis is interoperability:
reimplementing an interface so existing programs can talk to each other is
lawful in the EU (Software Directive art. 6) and has fared well in the US
(*Sega v. Accolade*, *Google v. Oracle*). Steinberg has sent DMCA notices for
copies of *its own headers*, including some clean-room files that kept the
original file name; it has not, in seven years, acted against a host built on
one. Two things remain genuinely uncertain and are for counsel, not us: the
strength of the interoperability position for a *commercial* host in the
jurisdictions Fopull sells into, and the trademark line for saying the product
loads VST 2 plugins at all. Xaymar's own README says: consult a lawyer before
shipping it in a paid product.

**What follows for the shape of the thing:**

- **VST 3 is a first-class hosted format in the tree.** Same as CLAP: scan,
  instantiate, parameters, audio, notes, state, editor, latency. No bridge, no
  download, nothing to set up. This is the format most paid Linux plugins ship,
  and the one `yabridge` presents Windows VST 3 plugins as.
- **VST 2 is an *extension*: a separate repository, a separate download, and
  the only place the interoperability question lives.** The bridge seam that
  exists (`fontelle-bridge-abi` at ABI 3, `fontelle_host::Bridges`) is exactly
  this: a shared library in Fontelle's data folder that the host loads at run
  time. If the position ever has to change, the extension is withdrawn and
  Fontelle is untouched. The extension is under the same MIT OR Apache-2.0 as
  Fontelle — the GPLv3 that was contemplated for a VST 3 bridge was only ever
  the SDK's condition, and there is no SDK here.
- **Extensions are a feature, not a workaround.** A page on the start menu and
  in the options lists what is installed, what is available, and installs it
  with the updater's own downloader. First run offers it. This is the home for
  anything else that belongs beside the product rather than in it.
- **Plugin folders are the user's to set**, many of them, in the same list
  regardless of format, with the platform's conventional folders searched by
  default and a one-click import of the folders another DAW already uses.

## 2. VST 3 in the tree

### 2.1 What a VST 3 plugin is, for the host

A `.vst3` bundle (a folder; on Linux `Contents/x86_64-linux/<name>.so`) that
exports `GetPluginFactory`, plus since SDK 3.7.9 a `Contents/moduleinfo.json`
that lists the classes without loading the library. The factory makes
**classes**; an instrument or effect is a `kVstAudioEffectClass` whose object
implements `IComponent` + `IAudioProcessor` (the processor) and, usually as a
second object, `IEditController` (parameters, state for the UI, the editor's
`IPlugView`). The two halves are connected through `IConnectionPoint` and
talk in `IMessage`s; a "single component" plugin implements both in one
object, and the host has to handle either.

The host provides: `IHostApplication` (name, and the factory for attribute
lists and messages), `IComponentHandler` (parameter edits *from* the editor —
begin/perform/end, and `restartComponent` for latency and bus changes),
`IPlugFrame` (the editor asking to resize), `IParamValueQueue`/
`IParameterChanges` (parameter automation *to* the plugin per block),
`IEventList` (notes in and out), `IBStream` (state), and the `ProcessData`
struct with its bus buffers. Every one of these is a COM-style interface with
an IID; the `vst3` crate gives both the plugin-side interfaces to call and the
machinery to implement host-side ones from Rust.

### 2.2 Where it goes

The fourth arm of `HostedPlugin::Inner` and `HostedProcessor` in
`fontelle-host`, gated `cfg` the way nothing needs to be — the SDK interfaces
are plain C ABI on every platform. `PluginFormat::Vst3` is already an enum
variant (it was the bridge's example); `PluginFormat::hosted()` starts
returning true for it on every build, which is what makes the Windows and
macOS binaries load their own VST 3 plugins without a bridge.

The mapping onto what the other two arms already do:

| Fontelle | VST 3 |
|---|---|
| scan a folder | `moduleinfo.json` when present; else load, `GetPluginFactory`, `getClassInfo` per class, unload. Classes of category `Audio Module Class`; `Instrument` in the subcategory string says instrument |
| open | `createInstance` (component), `initialize(host)`, query `IEditController` or create the controller class it names, `setComponentState`, connect the points |
| ports | `getBusCount`/`getBusInfo` for audio in/out and event in/out; activate the main busses and every aux bus, and hand every activated bus a buffer — the CLAP lesson (2026-09-05) that a plugin reads the ports it declared off the end of the array it is given applies here word for word |
| activate | `setupProcessing(sample rate, max block, kRealtime)`, `setActive(true)`, `setProcessing(true)` |
| params | `getParameterCount`/`getParameterInfo` (id, title, units, default, step count, flags — `kIsReadOnly`, `kIsBypass`, `kIsHidden`); values are **normalised 0..1** on the wire and `normalizedParamToPlain` for display. Changes to the plugin go in `IParameterChanges` per block with a sample offset; changes from the editor come back through `IComponentHandler` and are the same thing as CLAP's `param_gesture` — the host's parameter address does not change (INVARIANT 7) |
| notes | `IEventList` in: `kNoteOnEvent`/`kNoteOffEvent` with `noteId` and `tuning` in cents; `kPolyPressureEvent`; `kNoteExpressionValueEvent` with `kTuningTypeID` for the **per-note pitch a slide is** — VST 3 carries what CLAP's note expressions carry, so slides into hosted instruments (2026-09-06) reach VST 3 instruments the same way. Wheel and bend are not events: they are parameters the plugin exposes through `IMidiMapping::getMidiControllerAssignment`, resolved once at open and driven as parameter changes |
| state | `IComponent::getState`/`setState` (the processor's) and `IEditController::getState`/`setState` (the controller's), both `IBStream`s, saved as two blobs — the LV2 state design (2026-09-06) already has the shape of "the plugin's bytes, opaque" |
| latency | `getLatencySamples` after `setupProcessing`, and again on `restartComponent(kLatencyChanged)`, into the delay-compensation path that exists |
| editor | `createView("editor")`, `isPlatformTypeSupported("X11EmbedWindowID")`, `attached(window id, "X11EmbedWindowID")`, `getSize`/`onSize`, `IPlugFrame::resizeView` from the plugin; `IRunLoop` on the frame for plugins that need the host to pump their file descriptors and timers — the same two host services CLAP's `posix-fd` and `timer` are, so `PluginWindow::tick_editor` already knows what to do. Windows is `HWND`, macOS `NSView`; the window in `gui.rs` is the piece that is X11-only today, and it stays the piece |

### 2.3 The fixture

`fontelle-testvst3`: a real VST 3 plugin bundle, written in Rust with the same
`vst3` crate, built by `cargo build -p fontelle-testvst3` into a `.vst3` folder
with a `moduleinfo.json` — the sine-with-a-sub-port, the face-that-answers-
false, the parameter-that-gestures, in VST 3 form, so every test that holds the
CLAP arm has a twin. Real plugins for listening: Surge XT, Vital, Dexed and
the u-he demos all ship Linux `.vst3`; `yabridge` makes a Windows one look
identical.

### 2.4 The one thing to prove first

A **spike** before any of the above is committed to: open Surge XT's `.vst3`
through the `vst3` crate, process a block of silence with a note in it, and
read audio out. Half a day, and it answers whether the crate's host-side
support (implementing `IHostApplication`, `IComponentHandler`, `IEventList`
and `IParameterChanges` as COM objects from Rust) is complete enough or needs
a thin C++ shim over `public.sdk`'s hosting classes. Either answer keeps the
design; only the crate list changes.

## 3. VST 2, as an extension

### 3.1 What it is

A repository `fontelle-vst2` (MIT OR Apache-2.0, Fopull's, public) that builds
one shared library implementing `fontelle-bridge-abi` — the scan/open/params/
activate/process/notes/state/editor table at ABI 3 — and loads VST 2.4
plugins: `.so` on Linux (including everything `yabridge` makes of a Windows
`.dll`), `.dll` on Windows, `.vst` on macOS. It links **no Steinberg file**.
The interface it uses is a Rust transliteration of Xaymar's BSD-3 clean-room
description: the `AEffect` struct and its function pointers, the opcodes,
`VstEvents`, `VstTimeInfo`, `VstParameterProperties`. That file is written
*from that document*, is under our copyright, and says at the top what it was
written from and why. Nobody on it ever reads Steinberg's `aeffectx.h`; the
repository's CONTRIBUTING says so.

The mapping is smaller than VST 3's because the API is: one `AEffect` per
plugin, `dispatcher(opcode)` for everything that is not audio,
`processReplacing` for audio, `getParameter`/`setParameter` in 0..1, notes as
`VstMidiEvent`s in a `VstEvents` array handed with `effProcessEvents` before
each block, state as `effGetChunk`/`effSetChunk` when the plugin sets
`effFlagsProgramChunks` and as the parameter list when it does not, the editor
as `effEditOpen(window handle)` + `effEditGetRect` + `effEditIdle` on a timer,
latency as `AEffect::initialDelay`. Per-note pitch does not exist in VST 2;
a slide becomes pitch bend on the note's channel, which is what every VST 2
host does with it.

### 3.2 Why this shape and not another

- *Not in the tree* so that Fontelle's own licence audit (cargo-deny, the
  README's promise) is about Fontelle, and so that withdrawing VST 2 support
  is deleting a download rather than cutting a product.
- *Through the bridge seam that exists* rather than a separate process,
  because the reason a separate process was worth its weeks — a GPL library
  in an MIT process — no longer exists. Crash isolation is still worth having
  some day, and `yabridge` already gives it for the plugins most likely to
  crash (Windows ones under Wine). It can be added behind the same ABI later
  without the extension knowing.
- *Under our licence, not GPL*, because the only reason to go GPL was an SDK
  that required it, and there is none.

### 3.3 What is asked of counsel before the first release of the extension

Two questions, written down so the answer can be written down:

1. Is hosting the VST 2.4 interface from a clean-room description, in a
   product Fopull sells, defensible in the US and the EU under the
   interoperability exceptions — and does distributing it as a separate free
   download from the same vendor change that analysis either way?
2. May the product say it "loads VST 2 plugins" with the ® attribution, or
   must it say "loads .so/.dll plugins in the VST 2.4 format", or nothing?

Until answered, the extension is built and tested but its release stays at
*needs-review* — the same gate every public step here goes through.

**Status (2026-09-14): built and released.** `fontelle-vst2` exists as its own
repository (`Fopull-LLC/fontelle-vst2`) — a clean-room VST 2.4 host bridge
implementing `fontelle-bridge-abi` v3 (scan/open/params/audio/notes/performance/
chunk state/editor), a fixture, and a loader test that runs the bridge against
it. The clean-room provenance rule is written into its `CONTRIBUTING.md`.

The two questions above went to **Ty rather than to counsel**: told the risk
was small but real and untested for a commercial clean-room host, and that
shipping the extension free, separate and opt-in is the low-risk shape of it,
Ty chose to ship it (option B of that conversation) and to own that residual
risk rather than wait on a formal legal opinion. So VST 2 ships as the
extension, and the product describes the *format* it loads — "loads plugins in
the VST 2.4 format" — with no VST logo and the ® attribution, which is the
trademark-cautious framing that costs nothing. The gate is cleared by that
decision; it is recorded here so the record says who cleared it and how.

## 4. Extensions

### 4.1 What an extension is

A bridge (`fontelle-bridge-abi`) or, later, anything else that lives beside
the product: a shared library or a folder of files that Fontelle finds in
`$XDG_DATA_HOME/fontelle/extensions/<id>/` and knows about from a
**catalogue** compiled into the binary:

```
id: vst2
name: VST 2 plugins
summary: Loads plugins in the VST 2.4 format, including ones yabridge makes of Windows plugins.
repo: Fopull-LLC/fontelle-vst2
asset: fontelle-vst2-<version>-<target>.tar.gz    (+ SHA256SUMS)
kind: bridge
abi: 3
```

The catalogue is code, not a network resource: what Fontelle offers to install
is what its release was reviewed with. The *version* offered is read from the
extension repository's latest release the way `updates.rs` reads Fontelle's
own — same `curl`, same `SHA256SUMS` check, same rename-into-place — and an
installed extension is offered an update the same way. A bridge whose ABI is
not this build's is listed as *needs Fontelle x.y* and not loaded; that is the
refusal `Bridges` already makes, given a sentence.

### 4.2 Where it is reached

- **The start menu** gets an *Extensions* entry beside *Website* and
  *Repository*, and a line on first run — *Fontelle can also load VST 2
  plugins. Install the extension?* — that goes away once answered either way
  (`settings.extensions_offered`).
- **Options** gets an *Extensions* heading: one row per catalogue entry with
  its state (*not installed* / *0.1.0 installed* / *update to 0.1.1* / *needs
  Fontelle 0.3*) and the action as the row's button, above the plugin-folder
  rows it affects.
- Installing or removing a bridge is only possible while no plugin is open
  through it (`PluginRack::set_bridge_folders` asserts this) — the row says
  *close the project first* rather than being greyed to no explanation.

### 4.3 Kept out

No third-party catalogue, no user-added URLs, no scripts: an extension is
something Fopull published and reviewed. The folder is scanned only for the
ids in the catalogue. Anything a user drops into `bridges/` by hand still
loads as before — that path is unchanged and is for developers.

## 5. Plugin folders

What exists: `settings.plugin_dirs` (several), *Add plugin folder* and
*Rescan plugins* in Options, and the formats' own standard folders searched
underneath unless a test turns that off. What is missing is the rest of
managing a list:

- **The list is visible**, one row per folder, with a remove. Today a second
  folder is invisible except as a count.
- **Every format's conventional folders are in the standard set**, so "point
  it at the folder" is usually unnecessary: Linux `~/.vst3`, `/usr/lib/vst3`,
  `/usr/local/lib/vst3`, `$VST3_PATH`, and for the extension `~/.vst`,
  `/usr/lib/vst`, `/usr/lib/lxvst`, `$VST_PATH`; Windows `%COMMONPROGRAMFILES%\VST3`,
  `%PROGRAMFILES%\VstPlugins`, `%PROGRAMFILES%\Steinberg\VstPlugins`; macOS
  `/Library/Audio/Plug-Ins/VST3`, `~/Library/Audio/Plug-Ins/VST3`, and the
  `VST` siblings. A bridge nominates its own — `search_paths_with(&bridges)`
  already asks.
- **Import from another DAW.** One row, *Use the folders FL Studio searches*,
  that reads FL's extra search folder from its settings (the Windows registry
  under `HKCU\Software\Image-Line`, or the same keys in a Wine prefix's
  `user.reg` on Linux, where a Windows FL install lives) and adds it. Same
  shape for REAPER (`reaper.ini`, `vstpath`) and Bitwig when somebody asks.
  This is what *"sync it to their FL"* means in practice: one folder both
  programs read, and Fontelle finding it without being told.
- **A folder is a folder**: any format in it is found, so a user with one
  `Plugins` folder holding `.vst3`, `.clap` and `.so` points at it once.

## 6. Order of work

Each step is tests first against the intended API, confirmed failing, then
built; `PROGRESS.md` gets an entry per step.

0. **Policy.** TDD §3.4 rewritten for the MIT SDK and the trademark rule
   above; the *ACTION REQUIRED* closed; `docs/plugin-compatibility-plan.md`
   §4 pointed here and its yabridge sentence corrected (yabridge presents
   Windows VST 2 as Linux VST 2 and VST 3 as VST 3; it does not convert
   between them). *Done with this document.*
1. **The VST 3 spike** (§2.4). Half a day; decides the crate list.
2. **VST 3 hosting** (§2.2–2.3), in the order the CLAP arm was built: scan →
   open + params → audio → notes → state → editor → latency → note
   expression. The fixture grows with each. Linux first; the editor's
   `HWND`/`NSView` arms are the Windows/macOS work and are the same piece the
   CLAP editor is missing there.
3. **Plugin folders** (§5): the list, the standard sets, the FL import. Small,
   independent of 2, and worth having on the day VST 3 lands.
4. **Extensions** (§4): the catalogue, the page, the downloader. Built and
   tested against `fontelle-testbridge` published as a throwaway release, so
   the whole install path is exercised before there is a real extension.
5. **`fontelle-vst2`** (§3): the repository, the clean-room interface file,
   the bridge, its own fixture (a VST 2 plugin written against the same file).
   Release gated on §3.3.

Steps 2 and 3 are the ones that change what a user can do the day they land;
4 and 5 are what "maximum compatibility" costs on top, and the legal question
sits only on 5.

## Sources

- Steinberg, VST 3 Developer Portal — *VST 3 License*:
  <https://steinbergmedia.github.io/vst3_dev_portal/pages/VST+3+Licensing/VST3+License.html>
- Steinberg, VST 3 Developer Portal — *Licensing FAQ* (VST 2 answers):
  <https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Licensing.html>
- Steinberg, VST 3 Developer Portal — *Usage guidelines* (trademark):
  <https://steinbergmedia.github.io/vst3_dev_portal/pages/VST+3+Licensing/Usage+guidelines.html>
- `steinbergmedia/vst3sdk` — README and `LICENSE.txt` (MIT):
  <https://github.com/steinbergmedia/vst3sdk>
- Sound On Sound, *Steinberg adopt MIT License for VST3* (31 Oct 2025):
  <https://www.soundonsound.com/news/steinberg-adopt-mit-license-vst3>
- `coupler-rs/vst3-rs` — the `vst3` crate, MIT OR Apache-2.0:
  <https://github.com/coupler-rs/vst3-rs>; `RustAudio/vst3-sys` is GPLv3:
  <https://github.com/RustAudio/vst3-sys/blob/master/license.md>
- `Xaymar/vst2sdk` — clean-room VST 2.x interface, BSD-3-Clause:
  <https://github.com/Xaymar/vst2sdk>
- `RustAudio/vst-rs` — archived 2024, uses Steinberg's headers, not usable:
  <https://github.com/RustAudio/vst-rs>
- `robbert-vdh/yabridge` — VST 2 via VeSTige; Windows VST2/VST3/CLAP on Linux:
  <https://github.com/robbert-vdh/yabridge>
