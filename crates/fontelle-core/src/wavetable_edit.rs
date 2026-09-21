//! The wavetable editor's operations (`docs/flopsynth-next.md` §4.3), as
//! pure edits on a patch's own table.
//!
//! The window turns a gesture into one [`WavetableEdit`] and the host
//! applies it through the undo stack; nothing here knows about pointers or
//! cards. Every edit works on a table laid out at [`WAVETABLE_LEN`] samples
//! a frame — a sound dropped from a file at some other length is laid out
//! that way by the first edit ([`UserWavetable::lay_out`]), with the same
//! read the oscillator does, so what is edited is what was heard.
//!
//! The harmonic bars are an edit on the frame's own analysis: setting one
//! partial leaves the others where the frame had them, so bars can be
//! pushed around on a drawn wave rather than only on a blank one. The
//! formula is a small expression language (`formula.rs`), applied over the
//! phase and normalised to full scale, because a formula that peaks at 0.3
//! is a quiet oscillator nobody asked for.

use crate::patch::UserWavetable;
use fontelle_dsp::{MAX_USER_FRAMES, WAVETABLE_LEN};

/// How many harmonic bars the editor shows, and the analysis returns.
pub const EDIT_HARMONICS: usize = 64;

pub use fontelle_types::WavetableEdit;

impl UserWavetable {
    /// One silent frame, at the table's own length, called `name`.
    pub fn blank(name: &str) -> Self {
        Self {
            name: name.to_string(),
            frames: 1,
            samples: vec![0.0; WAVETABLE_LEN],
        }
    }

    /// Whether the samples are laid out at [`WAVETABLE_LEN`] a frame.
    pub fn is_laid_out(&self) -> bool {
        self.frames >= 1 && self.samples.len() == self.frames * WAVETABLE_LEN
    }

    /// Lays the samples out at [`WAVETABLE_LEN`] a frame, reading each part
    /// the way `Wavetable::from_samples` does — linear, wrapping inside the
    /// part — so a dropped sound is edited as it was heard.
    pub fn lay_out(&mut self) {
        if self.is_laid_out() {
            return;
        }
        let frames = self.frames.clamp(1, MAX_USER_FRAMES);
        let mut out = vec![0.0f32; frames * WAVETABLE_LEN];
        if !self.samples.is_empty() {
            let per_frame = self.samples.len() as f64 / frames as f64;
            for frame in 0..frames {
                let from = ((frame as f64 * per_frame).round() as usize).min(self.samples.len());
                let to = (((frame + 1) as f64 * per_frame).round() as usize)
                    .min(self.samples.len())
                    .max(from);
                let part = &self.samples[from..to];
                if part.is_empty() {
                    continue;
                }
                for i in 0..WAVETABLE_LEN {
                    out[frame * WAVETABLE_LEN + i] = if part.len() == WAVETABLE_LEN {
                        part[i]
                    } else {
                        let x = i as f64 * part.len() as f64 / WAVETABLE_LEN as f64;
                        let a = part[(x as usize) % part.len()];
                        let b = part[(x as usize + 1) % part.len()];
                        a + (b - a) * (x - x.floor()) as f32
                    };
                }
            }
        }
        self.frames = frames;
        self.samples = out;
    }

    /// One frame's samples, laid out. A frame the table does not have is
    /// silence rather than a panic.
    pub fn frame(&self, frame: usize) -> Vec<f32> {
        let mut laid = self.clone();
        laid.lay_out();
        laid.samples
            .get(frame * WAVETABLE_LEN..(frame + 1) * WAVETABLE_LEN)
            .map_or_else(|| vec![0.0; WAVETABLE_LEN], <[f32]>::to_vec)
    }

    fn frame_mut(&mut self, frame: usize) -> Result<&mut [f32], String> {
        self.lay_out();
        if frame >= self.frames {
            return Err(format!(
                "frame {} of a table with {}",
                frame + 1,
                self.frames
            ));
        }
        Ok(&mut self.samples[frame * WAVETABLE_LEN..(frame + 1) * WAVETABLE_LEN])
    }

    /// The first [`EDIT_HARMONICS`] partials of `frame`, each `(amplitude,
    /// phase)` — the amplitude as the sine's peak, the phase in radians.
    pub fn harmonics(&self, frame: usize) -> Vec<(f32, f32)> {
        analyse(&self.frame(frame))
    }

    /// Applies one edit. An edit that cannot be applied says why and
    /// changes nothing.
    pub fn apply(&mut self, edit: &WavetableEdit) -> Result<(), String> {
        match edit {
            WavetableEdit::Draw { frame, from, to } => {
                let samples = self.frame_mut(*frame)?;
                draw_segment(samples, *from, *to);
                Ok(())
            }
            WavetableEdit::Harmonic {
                frame,
                index,
                amplitude,
            } => {
                if *index >= EDIT_HARMONICS {
                    return Err(format!("harmonic {} is past the bars", index + 1));
                }
                let samples = self.frame_mut(*frame)?;
                let mut partials = analyse(samples);
                partials[*index].0 = amplitude.clamp(0.0, 1.0);
                synthesise(samples, &partials);
                Ok(())
            }
            WavetableEdit::Formula { frame, text } => {
                let expression = crate::formula::parse(text)?;
                let samples = self.frame_mut(*frame)?;
                for (i, sample) in samples.iter_mut().enumerate() {
                    let x = i as f32 / WAVETABLE_LEN as f32;
                    let v = expression.eval(x);
                    *sample = if v.is_finite() { v } else { 0.0 };
                }
                normalise_frame(samples);
                Ok(())
            }
            WavetableEdit::AddFrame { after } => {
                self.insert_frame(*after, vec![0.0; WAVETABLE_LEN])
            }
            WavetableEdit::CopyFrame { frame } => {
                let copy = self.frame_mut(*frame)?.to_vec();
                self.insert_frame(*frame, copy)
            }
            WavetableEdit::RemoveFrame { frame } => {
                self.lay_out();
                if self.frames <= 1 {
                    return Err("a table is at least one frame".to_string());
                }
                if *frame >= self.frames {
                    return Err(format!("no frame {}", frame + 1));
                }
                self.samples
                    .drain(frame * WAVETABLE_LEN..(frame + 1) * WAVETABLE_LEN);
                self.frames -= 1;
                Ok(())
            }
            WavetableEdit::Morph { from, to, spectral } => {
                self.lay_out();
                let (a, b) = (*from.min(to), *from.max(to));
                if b >= self.frames {
                    return Err(format!("no frame {}", b + 1));
                }
                if b - a < 2 {
                    return Err("nothing between the two frames to fill".to_string());
                }
                let (start, end) = (self.frame(a), self.frame(b));
                let (start_h, end_h) = (analyse(&start), analyse(&end));
                for frame in a + 1..b {
                    let t = (frame - a) as f32 / (b - a) as f32;
                    let samples = self.frame_mut(frame)?;
                    if *spectral {
                        let partials: Vec<(f32, f32)> = start_h
                            .iter()
                            .zip(&end_h)
                            .map(|((aa, ap), (ba, bp))| {
                                // The shorter way round for the phase.
                                let mut dp = bp - ap;
                                while dp > std::f32::consts::PI {
                                    dp -= std::f32::consts::TAU;
                                }
                                while dp < -std::f32::consts::PI {
                                    dp += std::f32::consts::TAU;
                                }
                                (aa + (ba - aa) * t, ap + dp * t)
                            })
                            .collect();
                        synthesise(samples, &partials);
                    } else {
                        for (i, sample) in samples.iter_mut().enumerate() {
                            *sample = start[i] + (end[i] - start[i]) * t;
                        }
                    }
                }
                Ok(())
            }
            WavetableEdit::Normalise { frame } => {
                let samples = self.frame_mut(*frame)?;
                normalise_frame(samples);
                Ok(())
            }
        }
    }

    fn insert_frame(&mut self, after: usize, samples: Vec<f32>) -> Result<(), String> {
        self.lay_out();
        if self.frames >= MAX_USER_FRAMES {
            return Err(format!("a table holds {MAX_USER_FRAMES} frames at most"));
        }
        let at = (after + 1).min(self.frames);
        let index = at * WAVETABLE_LEN;
        self.samples.splice(index..index, samples);
        self.frames += 1;
        Ok(())
    }

    /// The table as a 16-bit mono WAV, `frames × 2048` samples at 44.1 kHz
    /// — the layout Serum reads a table from, and the one
    /// `load_wavetable` reads back frame for frame.
    pub fn export_wav(&self) -> Vec<u8> {
        let mut laid = self.clone();
        laid.lay_out();
        // One gain over the whole table, the way it is played: a frame
        // built from partials may sit over full scale.
        let peak = laid.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if peak > 1.0 {
            for sample in &mut laid.samples {
                *sample /= peak;
            }
        }
        let data_len = (laid.samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        let rate = 44_100u32;
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in &laid.samples {
            let v = (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// The samples under a segment take the line between its ends.
fn draw_segment(samples: &mut [f32], from: (f32, f32), to: (f32, f32)) {
    let (mut a, mut b) = (from, to);
    if a.0 > b.0 {
        std::mem::swap(&mut a, &mut b);
    }
    let len = samples.len() as f32;
    let first = (a.0.clamp(0.0, 1.0) * len).round() as usize;
    let last = ((b.0.clamp(0.0, 1.0) * len).round() as usize).min(samples.len());
    if first >= last {
        // A point: the one sample under it.
        if let Some(sample) = samples.get_mut(first.min(samples.len() - 1)) {
            *sample = a.1.clamp(-1.0, 1.0);
        }
        return;
    }
    let span = (last - first) as f32;
    for (i, sample) in samples[first..last].iter_mut().enumerate() {
        let t = i as f32 / span;
        *sample = (a.1 + (b.1 - a.1) * t).clamp(-1.0, 1.0);
    }
}

/// The first [`EDIT_HARMONICS`] partials of one cycle: amplitude as the
/// sine's peak, phase in radians.
fn analyse(samples: &[f32]) -> Vec<(f32, f32)> {
    let n = samples.len() as f32;
    (1..=EDIT_HARMONICS)
        .map(|h| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in samples.iter().enumerate() {
                let phase = std::f32::consts::TAU * h as f32 * i as f32 / n;
                re += s * phase.cos();
                im += s * phase.sin();
            }
            let amplitude = (re * re + im * im).sqrt() * 2.0 / n;
            // The phase of a sine: `sin(θ + φ)` has `re = −sin φ`... so the
            // angle is taken so that `synthesise` gives it back.
            (amplitude, im.atan2(re))
        })
        .collect()
}

/// One cycle from its partials — the inverse of [`analyse`].
///
/// **Not clamped**: a fundamental at one with a third at a half peaks at
/// 1.2, and clipping it there would change the very partials that were
/// just set. The table's level is one gain over the whole table when it is
/// played (`Wavetable::from_samples`) and when it is exported, so a frame
/// over full scale in memory costs nothing but headroom.
fn synthesise(samples: &mut [f32], partials: &[(f32, f32)]) {
    let n = samples.len() as f32;
    for (i, sample) in samples.iter_mut().enumerate() {
        let mut v = 0.0f32;
        for (h, (amplitude, phase)) in partials.iter().enumerate() {
            if *amplitude == 0.0 {
                continue;
            }
            let angle = std::f32::consts::TAU * (h + 1) as f32 * i as f32 / n;
            v += amplitude * (angle - phase).cos();
        }
        *sample = v;
    }
}

/// Scaled to a peak of one; a silent frame stays silent.
fn normalise_frame(samples: &mut [f32]) {
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 1e-6 {
        for sample in samples.iter_mut() {
            *sample /= peak;
        }
    }
}
