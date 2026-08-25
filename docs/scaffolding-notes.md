# Scaffolding notes

Judgment calls made while scaffolding the workspace (2026-08-23) that aren't spelled out
in `FONTELLE_TDD.md`. Each is a decision that could reasonably have gone the other way;
they're documented here specifically so they're easy to revisit, not because they're
assumed correct.

## `fontelle-types`: a crate not in the TDD's crate list

TDD §4.1 states the dependency rule as a diagram: `model ──> (nothing in this workspace
except shared id types)`. But no crate in §4's layout is named as the home for those
shared id types, and two other things need the same kind of home:

- `fontelle-core::Patch::Layer::Source::Sf2Zone` embeds an `AssetRef` (§7.2), and
  `AssetRef` is specified in §17.4, under the *project storage* section — i.e. as part of
  `fontelle-model`'s territory. But INVARIANT 4 forbids `fontelle-core` from depending on
  `fontelle-model`.
- The RT thread reads a `CompiledTimeline` (§11.1) produced by `fontelle-sequencer`, but
  §4.1's permitted-dependency list does not have `fontelle-engine` depending on
  `fontelle-sequencer` (or vice versa) — only `sequencer ──> model`. Something has to
  define `CompiledTimeline`/`TimedEvent` somewhere both crates can reach.

Resolution: added `fontelle-types`, a dependency-free (aside from `serde`/`slotmap`/
`uuid`) crate below everything else, holding:

- The `slotmap` key newtypes (`ChannelId`, `ClipId`, `PrefabId`, ...) and `PersistentId`
  (the UUIDv7 on-disk counterpart from §10.2).
- `ParamAddress` (§8.2).
- `Tick`/`Sample`/`PPQN` (§6.1).
- `AssetRef`/`AssetKind` (§17.4).
- `TimedEvent`/`EventPayload`/`CompiledTimeline` (§11.1).

Updated dependency graph: `core ──> types, dsp`; `model ──> types`; `engine ──> types,
core, fx, dsp`; `sequencer ──> types, model`; `midi ──> types`; `assets ──> types`;
`ui ──> types`; `plugin ──> types, core, dsp, ui`. This keeps INVARIANT 4 intact (core
still never touches model, engine, or the DAW) while giving every crate that needs one of
these primitives a legal path to it.

## Other calls made without an explicit TDD answer

- **`fontelle-model::Channel` stores a serialised `Vec<u8>` patch, not a live
  `fontelle_core::Patch`.** The model crate cannot depend on `fontelle-core` (only
  `fontelle-types`, per INVARIANT 4's model-side counterpart), so it can't hold the live
  type. `fontelle-engine`'s `SamplerNode` is where the serialised form gets deserialised
  into a real `Sampler`. Worth revisiting once the serialisation format (§17.2) is
  designed — a dedicated `PatchData` intermediate type in `fontelle-types` might be
  cleaner than an opaque byte blob.
- **CI's Linux job installs ALSA + Wayland/X11/XKB dev headers** (`libasound2-dev`,
  `libudev-dev`, `libxkbcommon-dev`, `libwayland-dev`, `libxcb*-dev`) since `cpal`,
  `winit`, and `baseview` all need them to build. Not stated in the TDD; inferred from
  what the dependency table (§3.1) actually requires to compile on Linux.
- **Workspace `resolver = "3"`, `edition = "2024"`** — resolver 3 is the one that ships
  with 2024-edition workspaces; not worth a separate note beyond this one.
