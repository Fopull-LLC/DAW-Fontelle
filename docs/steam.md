# Fontelle on Steam

Hub card 0429. Ty pays the Steam Direct fee on 2026-10-22; the target release
is Thursday 2026-11-19. Fontelle is free on Steam. A **Supporter Pack** DLC
carries content (soundfonts, presets, templates), never features.

This page covers the build side: how a release becomes three Steam depots, how
the program behaves when Steam installed it, the Supporter Pack's shape, the
system requirements, and what is still Ty's to do. The store page's
screenshots and trailer are not covered here.

---

## 1. The shape of it

```
tag vX.Y.Z ──release.yml──▶ GitHub release (4 archives + SHA256SUMS)
                                  │
                tools/steam/make-depots.sh <tag>    (also release.yml's `steam` job)
                                  ▼
              steam-X.Y.Z/depots/{linux,windows,macos}
              steam-X.Y.Z/scripts/app_build.vdf + depot_build_*.vdf
                                  │
     steamcmd +login <builder> +run_app_build …/scripts/app_build.vdf +quit   ← Ty, by hand
```

- **Nothing is rebuilt for Steam.** The depots hold the same bytes the
  release page offers, after the script checks them against the release's
  `SHA256SUMS`. A Steam build is therefore a release that CI has already
  passed (`release.yml`'s `ci` gate). It never becomes a second, untested
  binary.
- **No DRM wrapper and no `steam_api`.** The program never loads Steam's
  library and never needs the client running. Leave Steamworks' *DRM* tab
  untouched: SteamPipe applies no wrapper unless one is asked for there.
- **The only step done by hand is `steamcmd +run_app_build`.** It needs Ty's
  builder account.

## 2. How a copy knows Steam installed it

Steam owns its install folder and verifies it file by file. The start menu's
updater (`crates/fontelle-app/src/updates.rs`) does three things that would
fail verification:

- it downloads into the folder beside the program,
- it renames the running binary to `.old`,
- it installs `fontelle-asio.exe` beside the program.

So a Steam copy must not update itself.

**How it is detected: a marker file, `fontelle-steam.txt`.** The depot script
puts it beside the program. On macOS it goes in `Fontelle.app/Contents/Resources/`
instead, because codesign treats everything in `Contents/MacOS` as code.
`fontelle_app::distribution::Distribution::of_install` reads it once at launch.

A cargo feature `steam` was considered and turned down:

- A compile-time switch makes the Steam binary a **different build**. Every
  release would then build four more targets, and those binaries would have
  had no CI run of their own.
- The depot script could no longer use the published, checksummed archives.
- The macOS universal binary would need its own pair of builds as well.

The marker keeps one binary for every channel. Its cost is that the updater's
code is still compiled in, and the marker decides whether that code is
reachable. Deleting the marker from a Steam install turns the updater back on,
and Steam's next verification puts the marker back. That is acceptable.

**What a Steam copy does differently.** These are all the differences; a copy
without the marker behaves exactly as it did before:

| | Release-page copy | Steam copy |
|---|---|---|
| Start menu's update line | checks GitHub, offers *Install update* | "Updates come through Steam" (`UpdateStatus::ByStore`), no button, no network |
| `Updater::upgrade` | downloads, verifies, swaps the binary | does nothing (`Updater::managed_by`) |
| Settings ▸ Updates (heading, *Check at launch*, *Beta updates*) | shown | not shown |
| `.old` tidy at launch | removes `fontelle.old` / `fontelle-asio.old` beside the program | skipped (it writes nothing there) |
| Working directory | inherited | moved to Fontelle's data folder at launch (see §3) |

## 3. Where Fontelle writes (the audit)

A Steam install folder must stay exactly as the depot laid it out. This audit
covered every write the shipped binary makes, on 2026-10-19, at
0.29.0-beta.1 + this card.

- **Config folder** (`Settings::config_dir`: `$XDG_CONFIG_HOME/fontelle`,
  `~/.config/fontelle`, `%APPDATA%\fontelle`): `settings.json`, themes, the
  collab `identity.key`, `previews.json`, `plugin-scan.json`, the tour song.
- **Data folder** (`Settings::data_dir`: `$XDG_DATA_HOME/fontelle`,
  `~/.local/share/fontelle`, `%LOCALAPPDATA%\fontelle`):
  - session logs and crash logs (`logs/`)
  - `recovery/`
  - `hashes.json`
  - the default soundfont bank and user presets (unless the person set folders)
  - voicebanks and their downloads
  - extensions (`bridges/`; on macOS `~/Library/Application Support/fontelle/bridges`)
  - the Linux desktop entry and icon (`~/.local/share/applications`, `icons/`)
- **Cache folder** (`Settings::cache_dir`, and the UTAU cache's own rule:
  `~/Library/Caches/fontelle/utau` on macOS): analysis, sung takes, UTAU
  analysis, singer levels.
- **Places the person named:** projects, renders, exports, received
  collaboration files, and a set preset or soundfont folder.
- **The temporary folder:** a few fallbacks used when there is no config
  folder (themes, the tour, collab `.part` files, vocal export).
- **Beside the program:** only the updater (§2), which is off in a Steam copy.

This card fixed three gaps the audit found. Each could have resolved a folder
to a **relative** path, which means under the working directory, which is the
install folder when Steam starts a game:

1. A relative `HOME` passed through `Settings::own_dir` and
   `fontelle_host::fontelle_data_dir_from`. Now a relative `HOME` is no home.
2. The UTAU cache (`fontelle_utau::cache::default_cache_dir`) accepted an
   empty `HOME`, a relative `XDG_CACHE_HOME` and any `LOCALAPPDATA`. Now it
   gives an absolute folder or none (`default_cache_dir_from`).
3. With no data folder at all, voicebanks went to `./voicebanks`. Now they go
   to the temporary folder (`voicebanks_dir_or_scratch_from`).

Tests: `fontelle-app/tests/steam.rs`, `fontelle-utau/tests/cache_dir.rs`.

**Third-party plugins** inherit the working directory. Cardinal, for one,
writes `patch.json` into it. A Steam copy therefore moves its working
directory to the data folder at launch. A plugin then writes there, and a
relative path typed on the command line is read against that folder, not the
install folder. A release-page copy keeps the working directory it was given.

## 4. The depots

`tools/steam/make-depots.sh <tag> [out-dir]` needs `gh`, `tar`, `unzip`,
`sha256sum` (or `shasum`), and `lipo` (on macOS) or `llvm-lipo` (on Linux with
LLVM; this devbox has it).

```
steam-0.29.0-beta.1/
  depots/linux/     fontelle, fontelle-steam.txt, com.fopull.Fontelle.{desktop,png},
                    LICENSE-*, README.md, licenses/
  depots/windows/   fontelle.exe, fontelle-asio.exe, fontelle-asio-README.txt,
                    fontelle-steam.txt, LICENSE-*, README.md, licenses/ (incl. fontelle-asio, trademarks)
  depots/macos/     Fontelle.app/Contents/{Info.plist, MacOS/fontelle (x86_64 + arm64),
                    Resources/fontelle-steam.txt}, LICENSE-*, README.md, licenses/
  scripts/          app_build.vdf, depot_build_{linux,windows,macos}.vdf
  downloads/        the four archives and SHA256SUMS, as published
```

- Linux's `install.sh` is left out of the Linux depot. It is for a hand
  install from the tarball, and Steam makes its own shortcut.
- The macOS app is built by the script, because **the release has no `.app`
  today**: each macOS archive is a bare `fontelle` binary. The bundle is
  minimal:
  - `Info.plist` from `packaging/macos/Info.plist.in`: bundle id
    `com.fopull.Fontelle`, `LSMinimumSystemVersion` 11.0, and an
    `NSMicrophoneUsageDescription`. Without that entry macOS refuses the
    input stream.
  - the two architectures joined by `lipo`.

  It is not signed beyond what the linker did to each slice (ad-hoc), and it
  has no icon (§7).
- **Universal binary, and where it is made.** Steam has one macOS depot, so
  the binary must be universal. The workflow's `steam` job runs on
  `macos-latest` and uses Apple's `lipo`. The local script uses `llvm-lipo`
  where there is no `lipo`, so it works on this devbox too.

Dry run against v0.29.0-beta.1 (2026-10-19):

- every archive matched `SHA256SUMS`
- the depots came to linux 119 MB, windows 105 MB, macos 180 MB (universal)
- `llvm-lipo -info` reported `x86_64 arm64`

### The Steamworks ids

`scripts/*.vdf` come from `tools/steam/*.vdf.in`. The ids are read from the
environment:

| Variable | What |
|---|---|
| `STEAM_APP_ID` | the app |
| `STEAM_DEPOT_LINUX` | Linux depot (OS: Linux, 64-bit) |
| `STEAM_DEPOT_WINDOWS` | Windows depot (OS: Windows, 64-bit) |
| `STEAM_DEPOT_MACOS` | macOS depot (OS: macOS) |

Unset, each stays `FILL_ME_…`, which steamcmd refuses, so a build cannot go
up under a wrong id. In the workflow they are repository **variables**
(`vars.STEAM_*`), not secrets.

`SetLive` is empty: an uploaded build goes live on no branch until somebody
sets it on the Steamworks site.

### Launch options (Steamworks ▸ Installation ▸ General)

| OS | Executable | Working dir |
|---|---|---|
| Windows | `fontelle.exe` | (blank) |
| Linux | `fontelle` | (blank) |
| macOS | `Fontelle.app` | (blank) |

On Linux, **do not** pick a Steam Linux Runtime container (§6).

### The `steam` job in `release.yml`

Every `v*` tag builds the depots once `publish` has finished, which means only
after CI passed. The result is uploaded as the workflow artifact
`fontelle-steam-<version>` (depots + scripts). There is also an upload step
running steamcmd; it is `if: false` until Ty wants CI to upload (§7). A Steam
build is a tag away. Download the artifact and run steamcmd, or run the
script locally.

## 5. The Supporter Pack

**Where it is looked for** (`fontelle_app::supporter_pack::pack_places`):

1. `<install folder>/supporter-pack/`. The install folder is the program's
   folder, or on macOS the folder holding `Fontelle.app`. DLC depots install
   into the app's install folder, and the pack must never be inside the
   bundle, where it would break the bundle's signature.
2. `<data folder>/supporter-pack/`, e.g. `~/.local/share/fontelle/supporter-pack`
   or `%LOCALAPPDATA%\fontelle\supporter-pack`. Anybody can drop the pack
   here, from Steam or not.

Both are read. The same folder reached twice counts once.

**What a pack is:** a folder with a manifest.

```
supporter-pack/
  fontelle-pack.json      {"format": 1, "name": "Fontelle Supporter Pack", "version": "1.0.0"}
  soundfonts/             *.sf2, in folders if wanted
  presets/                (shape reserved; not read yet)
  templates/              (shape reserved; not read yet)
```

A folder without `fontelle-pack.json` is not a pack, and neither is one whose
`format` is newer than this build reads. Unknown manifest fields are ignored,
not obeyed.

**What it does:**

- Its `soundfonts/` folder becomes a **read-only shelf** of the soundfont bank
  (`FileBank::add_shelf`). It is a folder at the bank's top level, named after
  the manifest, and searched with everything else.
- It is never written into settings (its place is wherever Steam put it this
  time), and it is never the folder the browser's buttons act on.
- `is_read_only` answers true for its files.

It unlocks nothing, which is the whole point. `the_pack_is_content_never_features`
checks that the settings page and the update line are identical with and
without a pack.

Presets and templates are a shape only. Wiring them in waits on Ty's choice of
contents (§7).

## 6. System requirements

Derived from the v0.29.0-beta.1 binaries, not guessed:

- glibc and GLIBCXX: `objdump -T`
- shared libraries: `readelf -d` / `llvm-objdump -p`
- macOS floors: `LC_BUILD_VERSION`

### Windows

- **OS:** Windows 10 version 1903 or later, 64-bit (or Windows 11). Rust's
  MSVC target needs Windows 10. The binary imports `d3d12.dll` and
  **`DirectML.dll`**, and DirectML ships with Windows from 1903.
- **Runtime:** the Visual C++ 2015–2022 Redistributable (x64). The binary
  imports `MSVCP140.dll` and `VCRUNTIME140(_1).dll`. Tick it under
  Steamworks ▸ Installation ▸ Redistributables, so Steam installs it.
- **GPU:** Direct3D 12 or Vulkan 1.1 with compute shaders. The UI is vello on
  wgpu 29.
- **Audio:** WASAPI. ASIO through the bundled `fontelle-asio.exe`, if an ASIO
  driver is installed.

### macOS

- **OS:** the binaries declare macOS 11.0 (arm64) and 10.12 (x86_64).
  - **The store page should say macOS 11 Big Sur or later** (the Info.plist
    says 11.0), and that is still unverified on real 11/12 hardware.
  - On Apple silicon, DiffSinger links pyke's prebuilt ONNX Runtime 1.28
    statically, and ONNX Runtime's own macOS floor is 13.x. Singing with
    DiffSinger may need macOS 13. Ty should test on the oldest Mac he can.
  - On Intel Macs, DiffSinger loads ONNX Runtime at run time, so it needs a
    `libonnxruntime.dylib` the person provides. Everything else works.
- **CPU:** Apple silicon or Intel x86_64 (one universal app).
- **GPU:** Metal, which every Mac that runs macOS 11 has.

### Linux

- **OS:** x86_64 with **glibc 2.35+**. The binary asks for `GLIBC_2.35` and
  `GLIBCXX_3.4.30`, the floor CI's `linux-release-floor` job holds on Ubuntu
  22.04 (`old-glibc` provides ONNX Runtime's 2.38 symbols). That means
  Ubuntu 22.04, Debian 12, Fedora 36, SteamOS 3 (Arch-based), or anything
  newer.
- **Libraries** (`NEEDED`, from the host): `libasound.so.2` (ALSA),
  `libdbus-1.so.3`, **`liblilv-0.so.0`** (the `lilv` package, for LV2
  hosting), `libstdc++.so.6`, `libgcc_s.so.1`. Wayland, X11, xkbcommon, Vulkan
  and JACK are opened at run time. SteamOS 3's read-only image very likely
  has no lilv (not checked on a Deck). If it has none, the Linux build will
  not start there. This needs Ty's decision (§7).
- **Steam Linux Runtime:** choose **none** (Steamworks' default for a native
  title). *Steam Linux Runtime 3.0 (sniper)* has glibc 2.31 and would refuse
  the binary. A 4.0 runtime would have the glibc, but it has no lilv.
- **GPU:** Vulkan 1.1 with compute (Mesa RADV/ANV, or NVIDIA's driver).
- **Audio:** PipeWire, PulseAudio (through ALSA) or JACK.

### All three

- **Memory:** 4 GB minimum, 8 GB recommended. Soundfonts are fully resident
  until streaming lands (bank.rs), so a large General MIDI bank is hundreds
  of MB on its own. **Not measured** on a running session for this card; Ty
  should check Activity Monitor or Task Manager with the tour song open.
- **Disk:** the program is about 105 MB (Windows), 119 MB (Linux) or 180 MB
  (macOS, universal). The factory samples (Salamander grand, the drum kits)
  and presets are inside the binary. 500 MB keeps room for the data folder
  (logs, caches, presets). Soundfonts and voicebanks the person adds are on
  top of that.
- **Network:** none required. It is used for collaboration, Find singers and
  extensions, and on a release-page copy for the update check.

## 7. Ty's to do

1. **Steamworks:**
   - create the app and the three depots (Linux 64, Windows 64, macOS), and
     set `STEAM_APP_ID` and `STEAM_DEPOT_*` (env locally, repository
     variables in GitHub)
   - set the launch options as in §4
   - tick the VC++ 2022 x64 redistributable
   - choose no Linux runtime
   - leave DRM off
2. **A builder account** for steamcmd (a separate Steam account with only
   build rights is Valve's advice). If CI should upload:
   - add `STEAM_USERNAME` and `STEAM_CONFIG_VDF` secrets
   - turn on the disabled step in `release.yml`'s `steam` job

   Otherwise run steamcmd by hand from the artifact.
3. **macOS signing and notarisation:**
   - an Apple Developer ID
   - `codesign --deep --options runtime` of `Fontelle.app` with the
     microphone entitlement
   - `notarytool`, and stapling

   None of this is invented here. Steam does not require it, but an
   unsigned app can be blocked by Gatekeeper on some setups. Also an
   **icon**: there is no `.icns`. `packaging/linux/com.fopull.Fontelle.png`
   is the source, and `iconutil` on a Mac makes one; then add
   `CFBundleIconFile` to `packaging/macos/Info.plist.in`.
4. **The oldest macOS:** test on 11/12/13 and set the store page's minimum
   (§6).
5. **Linux and lilv:** decide between three options:
   - ship `liblilv-0.so.0` (+ serd, sord, sratom, zix) in the Linux depot,
     with an rpath
   - make LV2 hosting load lilv with `dlopen`
   - list `lilv` as a requirement

   SteamOS needs the first or the second.
6. **The Supporter Pack's contents** (by Oct 31): a SoundFont pack, a
   template pack, or both. Then:
   - a DLC app in Steamworks with its own depot holding `supporter-pack/`
     at its top
   - the pack's `fontelle-pack.json`
   - if it has presets or templates, a follow-up card to read those folders

   Soundfonts already work.
7. **The store page itself:** screenshots, trailer, copy, and the
   requirements from §6.
