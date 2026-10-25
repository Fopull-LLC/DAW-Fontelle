# LV2 on Windows and macOS — the plan

Ty, 2026-10-21: *"yes absolutely we should have lv2 support on windows and
macos lets work on that"*, after a friend on Windows installed an LV2 plugin,
added `Common Files\LV2` to her folders, rescanned, and never saw it.

Status (2026-10-23): **Phases 1 and 2 built on `main`; Phase 3 half done.**
Ty's answers to §6: one built-in thing rather than an add-on (*"id want to
keep the app small but i dont want to make developing too much of a hassle a
single thing may be better"*) — D1 and D2 as recommended; hold the release
until all open work is done, then a beta his brother tests on a Mac (D4).

What was found building it, beyond §2:

- **`lv2-sys`** (livi's other dependency) carries generated bindings for Linux
  and Windows only and refuses macOS outright. Patched like `lilv`
  (`vendor/lv2-sys`): macOS reads Linux's bindings for its architecture —
  the same 64-bit layout; the one difference, `va_list` in the log
  callback on Apple arm64, is called by nothing here.
- **A bundle's URI** on Windows: `canonicalize` gives `\\?\C:\…`, which the
  Unix rule percent-encoded into a URI lilv could not open
  (`lv2::file_uri_of`, `path_of_file_uri`).
- **The editors** needed only the UI class: `PluginWindow::id` is already the
  `HWND` on Windows and the `NSView` on macOS, which is what `ui:WindowsUI` and
  `ui:CocoaUI` take as their parent (`lv2_ui::NATIVE_UI`).
- **The preflight** lints macOS without its SDK, so the lilv build has a
  lint-only switch (`FONTELLE_LILV_LINT_ONLY`; clippy never links).
- **MSVC and Apple clang** each refused the C once: MSVC under `/std:c11`
  rejects zix's compound-literal static strings (C2099), so MSVC gets no
  standard flag, as in zix's own build; macOS lacks `posix_fadvise`, so
  `HAVE_POSIX_FADVISE=0`. CI green on all three since `21c7503`.
- **lilv's library count is not thread-safe**, and Windows really unloads a
  library when it reaches nothing (Linux mostly does not): two hosts sharing
  a parked world on two threads crashed 1 run in ~120. Instances are made and
  freed under one lock (`lv2::LIBRARIES`).

Proved here: the whole host suite's LV2 tests (67, one Linux-only by its
`/proc` harness), the scans and the presets, built for Windows and **run under
Wine**; Dragonfly Room Reverb's real **Windows** LV2 build scanned, opened and
run over audio under Wine (`real_plugins::an_effect_at_a_path_processes_a_
burst_of_sound`); its Windows editor loaded as `ui:WindowsUI` and got as far as
drawing — then failed for want of OpenGL under Wine on the nested display,
exactly as the same plugin's **CLAP** editor does there, so that is Wine, not
LV2. Proved by CI: the build and the whole suite under MSVC and on macOS
(arm64), and Dragonfly's LV2 run over audio on all three (the real-plugins
workflow). Not proved: an LV2 editor on a real Windows or Mac screen — the
beta's testers.

## 1. Why it does not work today

LV2 is compiled out everywhere but Linux (`fontelle-host/src/lib.rs`:
`#[cfg(target_os = "linux")] pub mod lv2;`, `lv2_stub.rs` elsewhere). The
reason is the C library every LV2 host uses, **lilv** (with serd, sord,
sratom, zix and the LV2 headers, all ISC): on Linux it is a system package,
and the `lilv` Rust crate livi depends on finds it with `pkg-config`. Windows
and macOS have no such package, so the build cannot link it.

Everything above that is portable already: `lv2.rs` (1950 lines) is plain
lilv calls and has Windows and macOS search paths written
(`%APPDATA%\LV2`, `%COMMONPROGRAMFILES%\LV2`, `~/Library/Audio/Plug-Ins/LV2`,
`/Library/Audio/Plug-Ins/LV2`) but never compiled. The editor code
(`lv2_ui.rs`) is X11-only: it looks for `ui:X11UI` and hands the plugin an
X11 window id.

## 2. What the spike showed (2026-10-21)

In a scratch crate, not the tree:

- **lilv 0.28.0, serd 0.32.10, sord 0.16.22, sratom 0.6.22, zix 0.8.2** —
  the versions Ty's Linux system runs — compiled by the `cc` crate straight
  from their release tarballs, no meson, into one static `lilv-0` archive.
  Their config headers detect the platform themselves. Three build facts
  learnt: C11 (GCC's default C23 turns zix's `constexpr` on and breaks the
  Windows file), `_GNU_SOURCE` on Linux (`copy_file_range`), and the
  platform's own zix files (`posix/`, `win32/`, `darwin/` semaphores).
- **The `lilv` crate's build script** is what refuses: it demands
  `pkg-config`. A copy of that crate (MIT, ~1 kloc of Rust) with its build
  script replaced by the C build, put in with `[patch.crates-io]`, is all
  livi needs. `lilv-sys`'s bindings are unchanged.
- **Linux**: linked the vendored copy, no system lilv (`ldd`), loaded and
  instantiated drumkv1 and DrumSynth.
- **Windows**: cross-built (`x86_64-pc-windows-gnu`), run under Wine with our
  own test plugin built as a Windows DLL: all six fixture plugins found and
  instantiated, from a `Z:\...` bundle path.
- **macOS**: not tried; no Mac here. CI's macOS runners are where it is
  proved (they run the tests).
- **MSVC** (what the Windows release is built with): not tried here either;
  CI's Windows runner proves it. zix's Windows code is written for MSVC.

## 3. The work, in phases

**Phase 1 — lilv everywhere.** A crate `vendor/lilv` (the `lilv` crate's
Rust, our build script) and `vendor/lilv/c/` holding the five libraries'
`src/` and `include/`, the LV2 headers, and each one's `COPYING` — ~1.1 MB.
`[patch.crates-io] lilv = { path = "vendor/lilv" }` in the workspace.
`fontelle-host`'s `lv2` module and its tests ungated on every platform;
`lv2_stub.rs` and `LV2_ELSEWHERE` go. The test fixture's bundle names its
binary per platform (`fontelle_testlv2.dll`/`.dylib`/`.so`). Bundle paths to
`file://` URIs checked for drive letters and backslashes. Tests: the
existing LV2 suite, running on CI's Windows and macOS runners for the first
time; a new one that a bundle in a Windows-style path loads. Release workflow:
`liblilv-dev` no longer needed.

**Phase 2 — editors on Windows and macOS.** `lv2_ui.rs` picks the UI type
the platform embeds — `ui:WindowsUI` (an `HWND` parent) and `ui:CocoaUI` (an
`NSView`) — from the same `PluginWindow` CLAP and VST 3 editors already use
(`gui.rs` has both windows). A plugin with no editor of that kind gets
Fontelle's own panel, as today. Tested by the fixture's editor built for each
platform (it draws through the parent it is given), under Wine for Windows
and by CI for macOS.

**Phase 3 — the real thing.** On Windows under Wine: an LV2 plugin that ships
Windows builds (LSP, Dragonfly, x42's free ones) scanned, played, its state
saved and reopened, its editor opened, closed, reopened. macOS: the same on a
Mac — which this machine is not, so a tester (§6 D4).

Then a beta (`0.32.0-beta.1`), Ty's word on the tag.

## 4. What does not change

The LV2 code itself: one implementation, the one Linux has used and tested
since September. A song made on Linux with an LV2 plugin opens on Windows
with the same plugin (same URI = same `PluginKey`).

## 5. Risks, plainly

- **We own a C build.** A lilv update is a re-vendor by hand. Low churn
  (lilv's API has been stable for a decade), but it is ours.
- **MSVC and Apple clang are unproved** until CI runs them. If either refuses
  the C, Phase 1 stops until it is fixed — not the release of anything else.
- **Few LV2 plugins ship for Windows and macOS.** Most LV2 plugins are
  Linux-first. Worth it for the ones that do, and so a song made on Linux
  opens anywhere — but it will not light up a Windows user's whole library.
- **Qt and GTK editors** (drumkv1, Calf) are not embeddable on Windows or
  macOS by any host without suil; those plugins get Fontelle's panel.
- **The macOS build's binary** gains lilv statically; nothing to sign or
  notarise separately.

## 6. Decisions — Ty's

- **D1. Vendored lilv on Linux too?** *Recommended: yes.* One lilv, one
  version, on all three; the Linux download stops needing lilv installed
  (Fedora's `/usr/lib64` report goes away for good). Against: a distribution's
  lilv fixes reach us only when we re-vendor.
- **D2. Sources in the tree, or fetched at build time?** *Recommended: in
  the tree* (~1.1 MB). A build that downloads is a build that fails offline
  and can be tampered with.
- **D3. Editors in the same release as loading?** *Recommended: yes* —
  Phase 1 alone gives Windows/macOS LV2 plugins only Fontelle's panel, which
  for most is a wall of sliders.
- **D4. A macOS tester?** CI proves it builds and the fixture plays; nobody
  here has opened a real LV2 plugin on a Mac. Ship macOS LV2 in the beta
  marked as untested on real plugins, or wait for someone with a Mac?
