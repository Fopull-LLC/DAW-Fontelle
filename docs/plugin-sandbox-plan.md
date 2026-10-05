# Plugins in a process of their own: the design

*Written 2026-10-05, the backlog's item 14 (`docs/plugin-experience-backlog.md`
§1.3). Nothing in it is built; the phases at the end say what comes first.*

> *"i want to give people a good experience"* — and the one thing v0.24.0's
> crash recovery cannot do is keep the studio up when a plugin crashes.

## 1. Why, and what it buys

Plugins run inside the studio's process. When one faults — Calf Wavetable in
its own `run`, Odin2 dereferencing null, padthv1's scheduler thread during its
own teardown (all in `PROGRESS.md`) — the studio goes with it. v0.24.0 made
that survivable: the song is recovered and the plugin held back once
(`fontelle_host::guard`, `crashlog::recovery`). It did not make it
*contained*. Containment means the plugin runs in another process, and its
crash costs one channel's sound for a moment — not the studio, the song or
the other plugins.

It costs: a context switch each way per block, a few tens of microseconds; a
copy of the audio each way; a process per plugin (memory); and editors
embedded across processes, which macOS does not allow.

## 2. The shape

**A sandbox is a bridge.** `fontelle-host` already hosts plugins it does not
run itself: a *bridge* (`bridge.rs`, `fontelle_bridge_abi`, ABI v3) is a
library that presents plugins through a table of functions — open, activate,
parameters, state, notes, editors — and `BridgedPlugin`/`BridgedProcessor`
turn that table into a `HostedPlugin` like any other. The sandbox is an
in-tree bridge whose table forwards every call to a child process. The rack,
the graph, the document and the editor strip see a bridged plugin, and
already know what to do with one.

**One child per plugin instance**: the `fontelle` binary with a flag
(`--fontelle-plugin-host`), as the scanner's probe already is
(`fontelle_host::probe`). Inside, the ordinary `PluginHost` opens the plugin
— CLAP, VST 3 or LV2, unchanged.

**Two channels:**

- **Control**, for the main-thread calls (open, activate, parameter list and
  text, state save and load, presets, editor open/close/resize, the plugin's
  main-thread callbacks): a Unix domain socket (named pipe on Windows),
  length-prefixed messages, request and answer. Not real-time.
- **Audio**, per block, real-time: a shared-memory region (`memfd` on Linux,
  `shm_open` on macOS, a pagefile mapping on Windows) holding the block's
  input and output buffers, its events (notes, controllers, parameter
  changes, transport) and the parameter table the plugin's own changes come
  back through. The audio thread writes the block in, wakes the child
  (futex / `os_unfair_lock`-free semaphore / Win32 event), and waits for it
  — **with a deadline** of a fraction of the block's period.

**Same-block, not a block late.** The child runs the block while the audio
thread waits, so no latency is added; the cost is the two wake-ups.

## 3. When the child crashes or hangs

- **Missed deadline:** the block is silence; the node counts it. A child that
  misses several in a row is treated as hung.
- **Crash** (the child exits, or the socket closes): the node plays silence
  from that block on — the same `PluginNode` path a NaN takes, said the same
  way (`Session::deal_with_silenced`): *"X crashed and was restarted"*. The
  rack starts a new child, hands it the state captured last (or the
  document's), and parks the new processor in the same bay. One restart per
  ten seconds, as with NaN; after that it stays down and says so.
- The crash report is the child's, named for the plugin (the guard runs in
  the child too).

## 4. Editors

- **Linux (X11):** the child embeds the plugin into the window id the studio
  hands it. Cross-process X11 embedding is ordinary (it is what XEmbed is
  for); the strip stays the studio's.
- **Windows:** a child `HWND` in another process's window works
  (`SetParent` across processes), with known focus quirks; the plugin's
  window is parented to the frame's embed `HWND` as now.
- **macOS:** an `NSView` cannot be embedded across processes with public
  API. The child shows the plugin's editor in a window of its own (CLAP's
  floating API, or a VST 3 view in a child-owned `NSWindow`), placed over
  the studio's frame and kept in step with it. Second best, and said in the
  frame's title.
- **LV2 editors with `instance-access`** need the instance in their own
  process: shown by the child, floating, everywhere.

## 5. What does not change

The document, presets, automation, the rack's settle logic (it talks to a
`HostedPlugin` either way), the render (a render borrows the processor; the
child runs offline blocks as fast as it can), and the crash guard.

## 6. Who runs sandboxed

A per-plugin switch in the plugin's strip menu, *"Run in its own process"*,
kept in the settings by plugin key (not in the song: it is about this
machine). **On by default for a plugin the crash guard has named**, so a
plugin that crashed once is contained from then on. A global default stays
"in process" until the sandbox has a release of use behind it, and then
flips.

## 7. Phases

1. **Audio and control on Linux**, one CLAP plugin in a child: the shared
   region, the futex handshake with its deadline, the control socket with
   open/activate/parameters/state. The engine's no-allocation tests extended
   to the audio thread's side. A fixture that crashes on a note proves the
   studio survives and the channel comes back.
2. **Every format, every main-thread call**, VST 3 and LV2 included, presets,
   the plugin's own parameter changes, offline renders.
3. **Windows and macOS** transports (pipe, mapping, event; `shm_open`,
   semaphores), in the real-plugin CI on all three.
4. **Editors**: X11 and Win32 embedded, macOS floating.
5. **The switch and the default**: per-plugin in the strip, automatic after a
   crash; measurement of the cost per block on each system, published in
   `PROGRESS.md` before the global default is considered.

Phases 1 and 2 are about two weeks; 3 to 5 about as much again.
