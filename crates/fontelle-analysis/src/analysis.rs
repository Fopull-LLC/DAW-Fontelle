//! Everything the Analyze Musically window shows about one piece of audio,
//! in one value: what the job computes and the cache keeps (plan §3.10).

use crate::chords::ChordSpan;
use crate::confidence::{Clarity, ExtractionEvidence};
use crate::key::KeyReading;
use crate::mono::MonoNote;
use crate::transcribe::NoteEvent;

/// Which engines and which version of their decoding made an analysis. A
/// cached analysis of another version is not read back.
pub const ENGINE_VERSION: &str = "basic-pitch-icassp2022.tidy1+pyin1+key1+chords1+extract1";

/// Melody (one voice: pitch-tracked notes, editable) or chords (polyphonic
/// notes, view and copy only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    Melody,
    Chords,
}

/// How well notes could be pulled out, as the badge shows it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Extraction {
    pub confidence: f32,
    pub clarity: Clarity,
    pub evidence: ExtractionEvidence,
}

/// One piece of audio, analysed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Analysis {
    pub engine: String,
    /// Seconds.
    pub duration: f64,
    pub mode: Mode,
    /// basic-pitch's notes, tidied (always present: chords are read from them).
    pub notes: Vec<NoteEvent>,
    /// In melody mode, the pitch-tracked notes with drift and vibrato.
    pub melody: Vec<MonoNote>,
    pub key: Option<KeyReading>,
    pub chords: Vec<ChordSpan>,
    pub extraction: Extraction,
    /// Lower thresholds were needed to find any notes: they are a guess.
    pub guessed: bool,
}

#[cfg(feature = "model")]
pub use run::analyse;

#[cfg(feature = "model")]
mod run {
    use super::*;

    /// Thresholds tried, in turn, when the default ones find nothing in audio
    /// that is not silent: a drum loop or noise still gets a guess, labelled as
    /// one (plan §2.6).
    const GUESS_THRESHOLDS: [(f32, f32, usize); 2] = [(0.3, 0.2, 11), (0.1, 0.05, 5)];
    /// Under this peak, audio is silence and gets no guess.
    const SILENCE_PEAK: f32 = 1e-3;
    /// Melody when at most one note sounds in this share of the frames where
    /// any does, and the pitch tracker hears a voice in enough of them.
    const MONO_SHARE: f32 = 0.9;
    const MONO_VOICING: f32 = 0.6;
    /// A guess is never more than a rough one.
    const GUESS_CEILING: f32 = 0.39;
    /// The chord lane's step without a tempo.
    const CHORD_STEP: f64 = 0.5;

    /// Analyses mono audio at any rate: notes, melody or chords, key, chord
    /// lane, extraction confidence.
    pub fn analyse(
        model: &crate::transcribe::basic_pitch::BasicPitch,
        audio: &[f32],
        sample_rate: u32,
    ) -> Result<Analysis, crate::transcribe::basic_pitch::ModelError> {
        use crate::chords::{TimedPitch, detect_chords};
        use crate::confidence::{extraction_confidence, extraction_evidence};
        use crate::key::{PitchWeight, detect_key};
        use crate::transcribe::basic_pitch::SAMPLE_RATE;
        use crate::transcribe::{NoteParams, notes_from_posteriorgrams, tidy_notes_with};

        let duration = audio.len() as f64 / f64::from(sample_rate.max(1));
        let at_model_rate = crate::resample::resample_mono(audio, sample_rate, SAMPLE_RATE);
        let post = model.posteriorgrams(&at_model_rate)?;
        let decode = |params: &NoteParams| {
            tidy_notes_with(notes_from_posteriorgrams(&post, params), &post, params)
        };

        let mut notes = decode(&NoteParams::default());
        let mut guessed = false;
        let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if notes.is_empty() && peak > SILENCE_PEAK {
            for (onset, frame, min) in GUESS_THRESHOLDS {
                notes = decode(&NoteParams {
                    onset_threshold: onset,
                    frame_threshold: frame,
                    min_note_frames: min,
                    ..NoteParams::default()
                });
                if !notes.is_empty() {
                    guessed = true;
                    break;
                }
            }
        }

        let evidence = extraction_evidence(&at_model_rate, SAMPLE_RATE, &post, &notes);
        let mut confidence = extraction_confidence(&evidence);
        if guessed {
            confidence = confidence.min(GUESS_CEILING);
        }

        let weights: Vec<PitchWeight> = notes
            .iter()
            .map(|n| PitchWeight {
                midi: n.midi,
                seconds: (n.end - n.start) as f32,
                weight: n.amplitude,
            })
            .collect();
        let key = detect_key(&weights);
        let timed: Vec<TimedPitch> = notes.iter().map(TimedPitch::from).collect();
        let chords = detect_chords(&timed, duration, CHORD_STEP);

        let (mode, melody) = melody_or_chords(&notes, post.frames, audio, sample_rate);
        Ok(Analysis {
            engine: ENGINE_VERSION.to_string(),
            duration,
            mode,
            notes,
            melody,
            key,
            chords,
            extraction: Extraction {
                confidence,
                clarity: Clarity::of(confidence),
                evidence,
            },
            guessed,
        })
    }

    /// Melody when one voice: at most one note at a time nearly everywhere,
    /// and the pitch tracker hearing a voice where the notes are. Then the
    /// melody's notes come from pYIN, with drift and vibrato.
    fn melody_or_chords(
        notes: &[NoteEvent],
        frames: usize,
        audio: &[f32],
        sample_rate: u32,
    ) -> (Mode, Vec<MonoNote>) {
        let mut sounding = vec![0u16; frames];
        for n in notes {
            for count in sounding
                .iter_mut()
                .take(n.end_frame.min(frames))
                .skip(n.start_frame)
            {
                *count = count.saturating_add(1);
            }
        }
        let active = sounding.iter().filter(|c| **c > 0).count();
        let single = sounding.iter().filter(|c| **c == 1).count();
        if active == 0 || (single as f32) < MONO_SHARE * active as f32 {
            return (Mode::Chords, Vec::new());
        }
        let track = crate::mono::pyin(audio, sample_rate, &crate::mono::PyinParams::default());
        let (mut voicing, mut count) = (0.0f32, 0usize);
        for n in notes {
            let first = (n.start / track.hop).round() as usize;
            let end = ((n.end / track.hop).round() as usize).min(track.len());
            for v in track.voicing.iter().take(end).skip(first) {
                voicing += v;
                count += 1;
            }
        }
        if count == 0 || voicing / (count as f32) < MONO_VOICING {
            return (Mode::Chords, Vec::new());
        }
        (Mode::Melody, crate::mono::segment(&track))
    }
}
