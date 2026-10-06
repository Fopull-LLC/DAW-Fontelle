"""Reference outputs of Python basic-pitch for the analysis tests.

Run once, offline, in a throwaway environment with onnxruntime, numpy, scipy,
librosa, pretty_midi, mir_eval and resampy, and a checkout of
spotify/basic-pitch on PYTHONPATH (the commit is in licenses/MODELS.md):

    cargo run -p fontelle-analysis --example write_fixtures -- <raw-dir>
    PYTHONPATH=<basic-pitch checkout> python make_reference.py <raw-dir> <out-dir>

It runs basic-pitch's own windowing, ONNX model and note creation on the
samples the Rust fixtures make (no resampling: they are already 22 050 Hz),
and writes, per fixture:

- `<name>.post.bin`: u32 LE frame count, u32 LE stride, then for every
  `stride`-th frame the 88 onset, 88 note and 264 contour values, each as
  u16 LE `round(v * 65535)`.
- `<name>.notes.json`: the note events in frames, as basic-pitch made them.
"""

import json
import pathlib
import struct
import sys

import numpy as np

from basic_pitch import inference
from basic_pitch import note_creation
from basic_pitch.constants import AUDIO_N_SAMPLES, FFT_HOP

STRIDE = 4
NAMES = ["triad", "vibrato", "mix"]


def posteriorgrams(model, audio):
    n_overlapping_frames = inference.DEFAULT_OVERLAPPING_FRAMES
    overlap_len = n_overlapping_frames * FFT_HOP
    hop_size = AUDIO_N_SAMPLES - overlap_len
    padded = np.concatenate([np.zeros((overlap_len // 2,), dtype=np.float32), audio])
    out = {"note": [], "onset": [], "contour": []}
    for window, _ in inference.window_audio_file(padded, hop_size):
        for k, v in model.predict(np.expand_dims(window, axis=0)).items():
            out[k].append(v)
    return {
        k: inference.unwrap_output(np.concatenate(v), audio.shape[0], n_overlapping_frames, hop_size)
        for k, v in out.items()
    }


def main():
    raw_dir, out_dir = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    import basic_pitch

    model_path = pathlib.Path(basic_pitch.__file__).parent / "saved_models/icassp_2022/nmp.onnx"
    model = inference.Model(model_path)
    assert model.model_type == inference.Model.MODEL_TYPES.ONNX
    for name in NAMES:
        audio = np.fromfile(raw_dir / f"{name}.f32", dtype="<f4")
        post = posteriorgrams(model, audio)
        frames = post["note"].shape[0]
        with open(out_dir / f"{name}.post.bin", "wb") as f:
            f.write(struct.pack("<II", frames, STRIDE))
            for t in range(0, frames, STRIDE):
                row = np.concatenate([post["onset"][t], post["note"][t], post["contour"][t]])
                q = np.round(np.clip(row, 0.0, 1.0) * 65535.0).astype("<u2")
                f.write(q.tobytes())
        # Frame-level events, exactly as model_output_to_notes builds them
        # before it turns frames into seconds.
        min_note_len = int(np.round(inference.DEFAULT_MINIMUM_NOTE_LENGTH_MS / 1000 * (22050 / FFT_HOP)))
        events = note_creation.output_to_notes_polyphonic(
            post["note"].copy(),
            post["onset"].copy(),
            onset_thresh=inference.DEFAULT_ONSET_THRESHOLD,
            frame_thresh=inference.DEFAULT_FRAME_THRESHOLD,
            min_note_len=min_note_len,
            infer_onsets=True,
            max_freq=None,
            min_freq=None,
            melodia_trick=True,
        )
        with_bends = note_creation.get_pitch_bends(post["contour"], events)
        _, timed = note_creation.model_output_to_notes(
            post,
            onset_thresh=inference.DEFAULT_ONSET_THRESHOLD,
            frame_thresh=inference.DEFAULT_FRAME_THRESHOLD,
            min_note_len=min_note_len,
        )
        notes = sorted(
            [int(s), int(e), int(p), float(a), [int(b) for b in bends]] for s, e, p, a, bends in with_bends
        )
        times = sorted([float(s), float(e), int(p)] for s, e, p, _a, _b in timed)
        with open(out_dir / f"{name}.notes.json", "w") as f:
            json.dump({"frames": int(frames), "notes": notes, "seconds": times}, f)
        print(name, frames, "frames,", len(notes), "notes")


if __name__ == "__main__":
    main()
