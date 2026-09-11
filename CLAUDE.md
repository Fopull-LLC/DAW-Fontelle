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
commit, tag `vX.Y.Z`, push the tag: `.github/workflows/release.yml` builds the
four archives and `SHA256SUMS`, and the start menu's updater
(`crates/fontelle-app/src/updates.rs`) finds them by those exact names.
