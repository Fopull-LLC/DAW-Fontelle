# Model weights shipped in Fontelle

`cargo deny` checks crates, not files. Every weight file the release build
carries is listed here with where it came from, under what terms, and the
SHA-256 it was reviewed at; a test pins each hash, so a changed file fails
the build until this entry is reviewed again (docs/analyze-musically-plan.md
§0, §2.2).

## basic-pitch, ICASSP 2022 "NMP" model

| | |
|---|---|
| File | `crates/fontelle-analysis/models/basic-pitch-icassp2022-nmp.onnx` (embedded with `include_bytes!` behind the `model` feature, on by default) |
| Upstream | [spotify/basic-pitch](https://github.com/spotify/basic-pitch), `basic_pitch/saved_models/icassp_2022/nmp.onnx`, unchanged |
| Checked out at | commit `fa5997af0a8210982619003269994a1be25eddf3` (2025-11-13) |
| Size | 230 444 bytes |
| SHA-256 | `2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec` |
| Licence | Apache-2.0 — the repository's `LICENSE` (standard Apache 2.0 text, "Copyright 2022 Spotify AB"); copied to [`basic-pitch/LICENSE`](basic-pitch/LICENSE) |
| Notice | the repository's `NOTICE`, copied to [`basic-pitch/NOTICE`](basic-pitch/NOTICE); Apache-2.0 §4(d) asks that it ship with the work |
| Pinned by | `crates/fontelle-analysis/tests/transcribe_model.rs`, `model_file_hash_is_pinned` |

**What was checked (2026-10-06).** The weights sit in the repository beside
the code with no licence of their own: no `LICENSE` or terms file in
`saved_models/`, nothing about the weights in the README, `NOTICE`,
`pyproject.toml` (`license = { file = "LICENSE" }`) or `MANIFEST.in`. The
repository's Apache-2.0 therefore covers them. The `NOTICE` names the
Python dependencies (librosa ISC, mir_eval MIT, numpy BSD, pretty-midi MIT,
resampy ISC, scipy BSD, tensorflow Apache-2.0) and the Vocadito audio
(CC BY 4.0) used by basic-pitch's *tests*; none of that is shipped here.

**Still open.** The model was trained on datasets with their own terms
(basic-pitch's training code, `constants.py`, names MAESTRO, GuitarSet, MedleyDB-Pitch, iKala
and Slakh). Spotify published the trained weights under Apache-2.0, and no
upstream statement restricts them; asking upstream for an explicit word is
the cautious step before a release that ships them (plan §5, R2).

The note decoding in `crates/fontelle-analysis/src/transcribe/notes.rs` is a
port of basic-pitch's `note_creation.py` and the windowing in
`transcribe/basic_pitch.rs` of `inference.py`, both Apache-2.0, credited in
those files.
