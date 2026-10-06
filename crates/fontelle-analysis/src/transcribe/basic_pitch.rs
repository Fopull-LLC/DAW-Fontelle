//! Spotify's basic-pitch (ICASSP 2022 "NMP" model, Apache-2.0,
//! licenses/basic-pitch and licenses/MODELS.md), run with tract.
//!
//! The windowing is `basic_pitch/inference.py`'s: mono 22 050 Hz audio,
//! 2 s windows (43 844 samples) overlapping by 30 frames, half the overlap
//! cut from each side of every window's output.

use super::notes::{NoteEvent, NoteParams, Posteriorgrams};

/// The rate the model was trained at.
pub const SAMPLE_RATE: u32 = 22_050;

/// The reviewed model file's SHA-256 (licenses/MODELS.md).
pub const MODEL_SHA256: &str = "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec";

/// `nmp.onnx` from spotify/basic-pitch, unchanged.
pub static MODEL_BYTES: &[u8] = include_bytes!("../../models/basic-pitch-icassp2022-nmp.onnx");

/// Why the model could not be loaded or run.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelError(pub String);

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "basic-pitch: {}", self.0)
    }
}

impl std::error::Error for ModelError {}

/// One window of audio the model takes: 2 s less one hop.
pub const WINDOW_SAMPLES: usize = 43_844;
/// The model's frame hop, in samples.
pub const FRAME_HOP: usize = 256;
/// Frames per window, and how many of them overlap the next window.
pub const WINDOW_FRAMES: usize = 172;
pub const OVERLAP_FRAMES: usize = 30;
/// Frames each window contributes once half the overlap is cut from each side.
pub const KEPT_FRAMES: usize = WINDOW_FRAMES - OVERLAP_FRAMES;
/// Samples between windows.
pub const WINDOW_HOP: usize = WINDOW_SAMPLES - OVERLAP_FRAMES * FRAME_HOP;

type Plan = std::sync::Arc<tract_onnx::prelude::TypedRunnableModel>;

/// The loaded, optimised model. Load once; it is reusable and `Send + Sync`.
pub struct BasicPitch {
    plan: Plan,
    /// Where onset, note and contour are among the model's outputs.
    order: [usize; 3],
}

fn err(e: impl std::fmt::Display) -> ModelError {
    ModelError(format!("{e:#}"))
}

impl BasicPitch {
    pub fn load() -> Result<Self, ModelError> {
        use tract_onnx::prelude::*;
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(MODEL_BYTES))
            .map_err(err)?;
        // The exported graph names its outputs after the TensorFlow model's:
        // contour is `:0`, note `:1`, onset `:2` (inference.py, `Model`).
        let names: Vec<String> = model
            .output_outlets()
            .map_err(err)?
            .iter()
            .map(|o| model.node(o.node).name.clone())
            .collect();
        let find = |suffix: &str| {
            names
                .iter()
                .position(|n| n.ends_with(suffix))
                .ok_or_else(|| ModelError(format!("no output {suffix} among {names:?}")))
        };
        let order = [find(":2")?, find(":1")?, find(":0")?];
        let plan = model
            .with_input_fact(0, f32::fact([1, WINDOW_SAMPLES, 1]).into())
            .map_err(err)?
            .into_optimized()
            .map_err(err)?
            .into_runnable()
            .map_err(err)?;
        Ok(Self { plan, order })
    }

    /// Posteriorgrams for mono audio already at [`SAMPLE_RATE`]: the
    /// windows of `inference.py`'s `run_inference`, unwrapped the same way.
    pub fn posteriorgrams(&self, audio: &[f32]) -> Result<Posteriorgrams, ModelError> {
        use super::notes::{N_CONTOUR_BINS, N_KEYS};
        use tract_onnx::prelude::*;
        let half_overlap = OVERLAP_FRAMES / 2;
        // The audio is preceded by half the overlap, so the first window's
        // cut-off frames fall in silence before the start.
        let lead = half_overlap * FRAME_HOP;
        let padded_len = lead + audio.len();
        // `int(len / hop * kept)`, in floating point, exactly as Python has it.
        let frames = (audio.len() as f64 / WINDOW_HOP as f64 * KEPT_FRAMES as f64) as usize;
        let windows = padded_len.div_ceil(WINDOW_HOP);
        let mut post = Posteriorgrams {
            frames,
            onset: Vec::with_capacity(windows * KEPT_FRAMES * N_KEYS),
            note: Vec::with_capacity(windows * KEPT_FRAMES * N_KEYS),
            contour: Vec::with_capacity(windows * KEPT_FRAMES * N_CONTOUR_BINS),
        };
        let mut state = self.plan.spawn().map_err(err)?;
        let mut window = vec![0.0f32; WINDOW_SAMPLES];
        for w in 0..windows {
            let start = w * WINDOW_HOP;
            window.fill(0.0);
            for (i, slot) in window.iter_mut().enumerate() {
                let at = start + i;
                if at >= padded_len {
                    break;
                }
                if at >= lead {
                    *slot = audio[at - lead];
                }
            }
            let input = Tensor::from_shape(&[1, WINDOW_SAMPLES, 1], &window).map_err(err)?;
            let outputs = state.run(tvec!(input.into())).map_err(err)?;
            for (which, dest, width) in [
                (self.order[0], &mut post.onset, N_KEYS),
                (self.order[1], &mut post.note, N_KEYS),
                (self.order[2], &mut post.contour, N_CONTOUR_BINS),
            ] {
                // Logical (row-major) order, whatever layout tract left it in.
                let out = outputs[which].to_plain_array_view::<f32>().map_err(err)?;
                if out.shape() != [1, WINDOW_FRAMES, width] {
                    return Err(ModelError(format!(
                        "output of shape {:?}, expected [1, {WINDOW_FRAMES}, {width}]",
                        out.shape()
                    )));
                }
                dest.extend(
                    out.iter()
                        .skip(half_overlap * width)
                        .take(KEPT_FRAMES * width)
                        .copied(),
                );
            }
        }
        post.onset.truncate(frames * N_KEYS);
        post.note.truncate(frames * N_KEYS);
        post.contour.truncate(frames * N_CONTOUR_BINS);
        Ok(post)
    }

    /// Notes for mono audio at any rate: resampled, decoded with `params`
    /// and tidied ([`super::notes::tidy_notes`]).
    pub fn transcribe(
        &self,
        audio: &[f32],
        sample_rate: u32,
        params: &NoteParams,
    ) -> Result<Vec<NoteEvent>, ModelError> {
        let audio = crate::resample::resample_mono(audio, sample_rate, SAMPLE_RATE);
        let post = self.posteriorgrams(&audio)?;
        let notes = super::notes::notes_from_posteriorgrams(&post, params);
        Ok(super::notes::tidy_notes_with(notes, &post, params))
    }
}
