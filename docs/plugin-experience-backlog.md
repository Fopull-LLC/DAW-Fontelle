# What still makes plugins frustrating

Written 2026-10-04, after v0.23.1 closed a user's report (Fedora; Vital,
Serum, Surge: instruments slipping back to their init patch, MIDI renders
coming out silent). This is everything found along the way that can still
make plugins act strangely or look broken, with a recommendation and a size
for each. **Work it top-down**: the tiers are ordered by how much
frustration each item causes, then by cost.

Sizes: **S** is under a day, **M** a few days, **L** a week or more.

**Status, 2026-10-05: items 1 to 9 of the order below are done (v0.24.0)**
— see `PROGRESS.md`. Item 7 (Wayland scaling) has the fix but has not been
looked at on a scaled screen. Left: Windows and macOS real-plugin CI (10),
LV2 `patch:` parameters (11), plugin-loaded samples travelling with a song
(12), macOS editors (13), plugins in their own process (14), upstream
reports (15). VST 3's offline process mode (part of 11 in the tiers) is not
done either; no plugin has been found that needs it.

---

## Tier 1: things that make people think their work is lost

### 1. A plugin that crashes takes the studio down, and nothing recovers the work

Plugins run inside the studio's process. Calf Wavetable, JuceOPL, Odin2 and
padthv1 crash inside their own code (see `PROGRESS.md`, "the sweep's last
word"), and any plugin a user installs could do the same. When that
happens:

- **Never-saved projects are not autosaved.** `Session::autosave` returns
  early with no bundle, so an hour on a new song is lost to one crash.
- **No recovery is offered.** A saved project's backup is written to
  `backups/autosave.fontelle`, but nothing on the next start says it is
  there or offers to open it.
- **The same plugin crashes the project again on reopening.** No record is
  kept of which plugin was loading or running when the studio died.

**Recommendation, in this order:**

1. (**S**) Autosave untitled projects to a folder in the config directory.
   On start, when a backup is newer than its project (or belongs to an
   untitled one), offer "Fontelle closed unexpectedly. Open the recovered
   project?"
2. (**S/M**) A crash guard. Write a small marker naming the plugin and the
   action (opening, restoring, processing, opening its editor) before each
   risky call, and clear it after. If the studio dies, the next start reads
   the marker and offers to open the project with that plugin bypassed
   (kept in the document, not run), with a line saying which one and why.
   Reaper and Bitwig both do something like this, and it turns a crash loop
   into a one-time annoyance.
3. (**L**, plan it separately) Run plugins in a process of their own. This
   contains a crash completely: the channel goes quiet and says so, and the
   studio carries on. It is a real architecture project (audio over shared
   memory, editors embedded across processes) and belongs with the
   bridge's design (`docs/vst-plan.md`), which already crosses a process
   boundary.

### 2. When something goes wrong quietly, the studio says nothing

v0.23.1 made several failures safe without making them visible:

- A plugin that plays NaN is silenced at its node (`only_numbers` in
  `plugin_node.rs`), but its channel just goes quiet. padthv1 keeps playing
  NaN once it has started, so the channel stays silent with no
  explanation.
- A render of a plugin that never makes a sound (a sampler with no kit
  loaded) is indistinguishable from a broken render.

**Recommendation** (**S**): have the node count what it silenced (an atomic
the rack reads on the main thread), and when it is non-zero put a line on
the status bar: "padthv1 on Bass played invalid audio and was silenced",
with a one-click "Reload plugin" that reopens it from its saved state.
After an export, name any channel whose plugin was silent or silenced.

### 3. Plugin editors do not open on macOS, and the message is wrong

`gui.rs` has no `NSView` embedding, so no plugin window opens on a Mac.
Every preset browser, wavetable editor and patch a person builds inside a
plugin is out of reach. The error reads *"a plugin editor needs an X server
(XWayland on a Wayland desktop): plugin editors are not shown on this
platform yet"*, which is wrong on a Mac.

**Recommendation:**

1. (**S**, now) Give macOS its own error: "Plugin windows are not available
   on macOS yet; the plugin's controls are in the generated panel." The
   panel already exists for plugins without editors, so point people there.
2. (**L**) Embed editors in an `NSView` (CLAP `cocoa` API, VST 3
   `kPlatformTypeNSView`). This needs a Mac to develop and test on. It is
   the largest single compatibility gap left.

### 4. No real plugin has ever been run on Windows or macOS

Every real-plugin walk (`tests/real_plugin_sessions.rs`,
`tools/real-plugin-sweep.sh`) has run only on this Linux machine. CI runs
the fixtures on all three systems, but fixtures are kind in ways real
plugins are not, and this whole report was found only by running real ones.

**Recommendation** (**M**): a CI job, on demand and weekly, that installs a
few free cross-platform instruments (Surge XT: CLAP and VST 3 on all
three systems; Dexed; Vital, if its licence allows a CI download) on the
Windows and macOS runners and runs the session walks with
`FONTELLE_REAL_ONLY`. It needs no new tests, only the installs.

---

## Tier 2: specific plugins or formats that misbehave

### 5. Windows plugins through yabridge (Serum) are untested

The reporter uses Serum, which on Linux runs through a Wine bridge such as
yabridge. yabridge presents a Windows plugin as an ordinary Linux VST 3 or
CLAP, so Fontelle's native hosting is what loads it, but the timing is
different: every call crosses into Wine, and state loads land later. That
is exactly where v0.23.1's settle logic matters.

**Recommendation** (**S**): install yabridge as a user (its release is a
tarball under `~/.local/share/yabridge`, no sudo needed; Wine is already
here) plus a free Windows instrument (Vital's Windows build, or TAL-NoiseMaker),
and run the walks against them. Any failure found there is real for every
Windows-plugin user on Linux.

### 6. Official Vital is untested

Vitalium (Vital's engine, LV2) passes every walk. Vital itself ships
VST 3 (JUCE) and its own `.vital` preset files.

**Recommendation** (**S**): Ty downloads the `.deb` from vital.audio (it needs
an account); extract it into the user plugin folders and run the walks.

### 7. LV2 parameters declared as `patch:` properties have no knobs

Of 339 LV2 bundles installed here, 42 declare some parameters as
`patch:writable` properties rather than control ports (Ultramaster KR-106 has
all of its parameters this way; LSP, Cardinal, drumkv1, AIDA-X and others use
them for file paths and settings). Fontelle shows only control ports. Those
parameters do not appear in the generated panel, cannot be automated, and
are kept only inside the plugin's saved state. Presets and saving still work
(the walks check that).

**Recommendation** (**M**): read `patch:writable` and `patch:readable` from
the Turtle, offer the number-typed ones (`atom:Float`, `atom:Int`,
`atom:Bool`) as parameters, set them with `patch:Set` on the event input,
and track them from the `patch:Set` messages the plugin sends back. Show
path-typed ones as a file chooser in the generated panel. The atom
plumbing exists already (`atom.rs`, used for editors).

### 8. LV2 plugins' latency is ignored

41 of the installed LV2 bundles report latency (lookahead limiters,
linear-phase EQs, LSP's analysers). Fontelle reads latency for CLAP and VST 3
and compensates for it, but treats every LV2 plugin as zero. A
lookahead limiter on one track puts that track late against the others, by
up to tens of milliseconds, which sounds like flamming or phasey drums.

**Recommendation** (**S**): read the output control port designated
`lv2:latency` after activation (and after a parameter change, since it
can move), and hand it to `PluginNode::latency`. The graph already
compensates.

### 9. Samples loaded inside a plugin stay where they were on disk

A sampler (LSP, sfizz, drumkv1) that loads a file stores its path in its
saved state. Fontelle maps the path through LV2's `state:mapPath`, but does
not copy the file into the project bundle, so a project moved to another
computer, or sent to a collaborator, opens with the sampler empty. Fontelle's
own samples go through TDD §17.4's import prompt and relink dialog;
plugin-loaded ones do not.

**Recommendation** (**M**): on save, for LV2 `mapPath` files outside the
bundle, offer the same import prompt Fontelle's own samples get (copy into
`assets/`, store the relative path). CLAP and VST 3 offer no such hook, so for
those, at least list the missing files by name when a project opens.

### 10. Wayland desktops: plugin windows go through XWayland

Fedora and most new installs run Wayland. Plugin editors are X11 windows by
design (`gui.rs`): no plugin format offers a Wayland embedding. On
fractional scaling they can be blurry or the wrong size, and their position
relative to the studio is up to the compositor.

**Recommendation** (**S**, verify first): test GNOME and KDE at 125% and
150% scaling with Surge XT and Vital. If editors are blurry, pass the scale
to the plugin (CLAP `gui.set_scale`, VST 3 `IPlugViewContentScaleSupport`)
and size the window in physical pixels. Fix only what the tests show.

---

## Tier 3: polish

### 11. A render does not tell CLAP and VST 3 plugins it is offline

LV2 plugins are told now (`lv2:freeWheeling`, v0.23.1). Some CLAP and VST 3
plugins switch to higher-quality oversampling, or skip real-time-only
shortcuts, when told they are rendering offline (CLAP `render` extension,
VST 3 `kOffline` process mode).

**Recommendation** (**S** for CLAP: call `render.set` on the main thread in
`lend_for_render` and `end_render`. **M** for VST 3: the mode is fixed at
`setupProcessing`, so it means re-activating around a render. Do CLAP now
and VST 3 only if a plugin is found that sounds different).

### 12. Loading a preset into an LV2 plugin reopens it

LV2 lets a host restore state safely only as an instance is created, so a
preset, an undo, or a state change on an LV2 plugin reopens it
(`PluginRack::ensure`). That can cause a short gap in the sound and close
its editor window.

**Recommendation** (**S**, verify first): check whether the editor closes.
If it does, reopen it automatically on the new instance. Restoring in place
is allowed for plugins that declare `state:threadSafeRestore`; use it
for those.

### 13. Plugin-side faults

These are bugs in the plugins, confirmed against a bare host, under
AddressSanitizer or gdb: Calf Wavetable, JuceOPL, Odin2, padthv1 and sfizz
(exit-time only). Items 1 and 3 contain them; nothing else in the host can.

**Recommendation** (**S**): report each upstream with the reproduction from
`PROGRESS.md`, since a fix there helps every host.

---

## Suggested order

| # | Item | Size | Why this place |
|---|------|------|----------------|
| 1 | Crash recovery: untitled autosave, recovery prompt (1.1) | S | Lost work is the worst experience |
| 2 | Crash guard: reopen with the plugin bypassed (1.2) | S/M | Ends crash loops |
| 3 | Say what was silenced, one-click reload (2) | S | Turns "broken" into "this plugin" |
| 4 | macOS editor message (3.1) | S | Wrong text today |
| 5 | LV2 latency (8) | S | Small change, audible timing fix |
| 6 | yabridge and Vital walks (5, 6) | S | The reporter's own setup |
| 7 | Wayland scaling check (10) | S | Fedora's default desktop |
| 8 | CLAP offline render (11) | S | Cheap |
| 9 | LV2 preset reopen, editor check (12) | S | Verify, then fix if needed |
| 10 | Real plugins on Windows and macOS CI (4) | M | Finds what this machine can't |
| 11 | LV2 `patch:` parameters (7) | M | 1 in 8 LV2 bundles |
| 12 | Plugin-loaded samples travel with the project (9) | M | Collaboration and moving machines |
| 13 | macOS editors (3.2) | L | Needs a Mac |
| 14 | Plugins in their own process (1.3) | L | The complete answer to crashes |
| 15 | Upstream reports (13) | S | Anytime |

Items 1 to 9 are about two weeks of work together and would make a good
v0.24.0 before or alongside FocalLoid's Stage 3 vocal track. Items 13 and
14 are the two large ones and each deserves its own plan document first.
