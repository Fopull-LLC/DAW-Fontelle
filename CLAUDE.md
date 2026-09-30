# Working in this repository

Read `PROGRESS.md`'s top two sections first, then `docs/handoff.md`. The design
is `FONTELLE_TDD.md`; its INVARIANTs are hard.

## Process

- **Tests first, confirmed failing**, then the implementation. Never both in
  one edit. `PROGRESS.md` says why at the top.
- Loop on the crate you touched (`cargo test -p <crate> --test <file>`); run
  the workspace suite **once, at the end, in the background**, never two at
  once. `cargo clippy --workspace --all-targets -- -D warnings` is part of the
  bar.
- **Look at anything visual** before believing it: the headless dump
  (`FONTELLE_UI_DUMP=<dir> cargo test -p fontelle-ui --test render_headless`)
  for a scene, the real binary on a nested `Xwayland :99` for the studio.
- Never `cargo fmt --all` here; format only the files you edited with
  `rustfmt --edition 2024 <file>`.
- Update `PROGRESS.md` when you finish a chunk. Comments carry the reasoning,
  not the mechanics; quote the report a fix came from.

## Cross-repo coordination

This project coordinates with the other Fopull-LLC agents via the
`floptle-platform` hub repo (cloned alongside this one at
`/mnt/disks/3tb/GithubRepositories/floptle-platform`). On session start:
`git pull` it, read its `PROTOCOL.md`, and scan `tasks/` **recursively** for
open items addressed to you (`to: D`). Follow that protocol for anything
spanning repos — the website's product page for Fontelle is task `0234`,
addressed to agent W. **Your agent id here is `D`.**

Publishing anything public — a release tag, the repository going public, an
announcement — is Ty's call (PROTOCOL §5). Prepare it, stop at needs-review.

## Releases

One version for the whole workspace (`[workspace.package] version`). Bump it,
commit, push `main`, tag `vX.Y.Z`, push the tag: `.github/workflows/release.yml`
builds the four archives and `SHA256SUMS`, and the start menu's updater
(`crates/fontelle-app/src/updates.rs`) finds them by those exact names. It
publishes only once CI has passed on that commit
(`.github/scripts/ci-passed.sh`), so a red CI is a release that does not
happen — look at `gh run list --workflow CI` before tagging.

Nearly every release went red on its first CI run, always on something this
machine never checked. So:

- `.github/scripts/preflight.sh` runs fmt and clippy for Linux, Windows and
  macOS (clippy never links, so it cross-checks from here). The pre-push hook
  runs it. macOS skips `fontelle-app` and `fontelle-net`, which need the
  macOS SDK for `ring`.
- **Push `main` when a chunk is done**, not only at release, so CI's tests on
  the Windows and macOS runners see the work days before the version bump.
