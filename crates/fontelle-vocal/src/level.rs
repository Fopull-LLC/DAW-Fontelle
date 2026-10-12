//! Every voicebank at a like loudness.
//!
//! Banks are recorded anywhere from near full scale to 25 dB under it
//! (walked end to end: Hitsuboku Kumi's loudest moments at -12 dBFS, Yoko's
//! at -30), so a song that changed singer jumped in level and a quiet bank
//! sounded broken. The Fontelle Singer's runtimes are wrapped in
//! [`leveled`]: the first time a voice sings, a short reference phrase
//! ([`reference_phrase`]) is sung at preview quality, its level measured,
//! and every take of that voice is scaled by the same gain from then on.
//! The gain depends only on the voice, so takes stay deterministic and the
//! render cache keeps working; the wrapper's version moves, so takes kept
//! from before are sung again.
//!
//! The reference is also what warming a voice sings
//! ([`crate::VocalRenderer::warm`]): chosen on a track, a voice's models are
//! loaded and its level found while the first notes are being written.
//!
//! External engines are not wrapped: what they sing is theirs.

use std::collections::HashMap;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Condvar, Mutex};

use fontelle_sing::{
    Backend, Capabilities, Phrase, PhraseNote, Quality, Session, SingError, Sung, VoiceInfo,
};

/// The seed the reference is always sung with (and a warming job asks
/// for, so it is handed the reference take itself).
pub const REFERENCE_SEED: u64 = 1;

/// Bumped when the levelling would change what a take sounds like.
const LEVEL_VERSION: u32 = 1;

/// Where a voice's voiced frames are brought to, dBFS (the mean power of
/// the frames within 30 dB of its loudest) — where the middle of the test
/// library already sat, leaving room for dynamics drawn louder.
const TARGET_DB: f32 = -18.0;

/// How far a voice is moved at most: a bank recorded near silence is not
/// blown up into its noise floor, and a hot one is only tamed.
const MOST_BOOST_DB: f32 = 15.0;
const MOST_CUT_DB: f32 = 9.0;

/// The voice's languages in the order a reference is sung in: a vowel
/// every phonemizer of these reads alike.
const REFERENCE: &[(&str, &str)] = &[("ja", "あ"), ("en", "ah"), ("zh", "a"), ("ko", "아")];

/// The phrase a voice is measured (and warmed) with: four half-second
/// notes on an open vowel, in the middle of its range, in the first of
/// Japanese, English, Chinese and Korean it sings (else its own first
/// language, on "a").
pub fn reference_phrase(voice: &VoiceInfo) -> Phrase {
    let (language, lyric) = REFERENCE
        .iter()
        .find(|(l, _)| voice.languages.iter().any(|v| v == l))
        .map(|(l, s)| (l.to_string(), s.to_string()))
        .unwrap_or_else(|| {
            (
                voice
                    .languages
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "en".into()),
                "a".to_string(),
            )
        });
    let (low, high) = if voice.lowest < voice.highest {
        (voice.lowest, voice.highest)
    } else {
        (55, 72)
    };
    let mid = f32::from(low.midpoint(high)).clamp(48.0, 76.0);
    let notes = [0.0, 2.0, 4.0, 2.0]
        .iter()
        .enumerate()
        .map(|(i, step)| PhraseNote::new(i as f64 * 0.5, 0.5, mid + step, lyric.clone()))
        .collect();
    Phrase::new(voice.id.clone(), language, notes)
}

/// The level of a take, dBFS: the mean power of its 20 ms frames within
/// 30 dB of the loudest. `None` when it is silent.
fn level_db(sung: &Sung) -> Option<f32> {
    let frame = (sung.sample_rate as usize / 50).max(1);
    let powers: Vec<f32> = sung
        .samples
        .chunks(frame)
        .map(|c| c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32)
        .filter(|p| p.is_finite())
        .collect();
    let loudest = powers.iter().copied().fold(0.0, f32::max);
    if loudest <= 1e-12 {
        return None;
    }
    let floor = loudest * 1e-3;
    let voiced: Vec<f32> = powers.into_iter().filter(|p| *p >= floor).collect();
    let mean = voiced.iter().sum::<f32>() / voiced.len() as f32;
    Some(10.0 * mean.log10())
}

/// The gain a take measured at `db` is given.
fn gain_for(db: Option<f32>) -> f32 {
    let Some(db) = db else {
        return 1.0;
    };
    // Very near silence: the reference did not really sing; leave it.
    if db < -60.0 {
        return 1.0;
    }
    let change = (TARGET_DB - db).clamp(-MOST_CUT_DB, MOST_BOOST_DB);
    10f32.powf(change / 20.0)
}

/// `inner`, every voice levelled — see the module doc.
pub fn leveled(inner: Arc<dyn Backend>) -> Arc<dyn Backend> {
    Arc::new(Leveled {
        inner,
        gains: Arc::default(),
    })
}

/// Each voice's gain, shared by every session: measured by one of them
/// while the others wait (a DiffSinger bank sings one phrase on a model at
/// a time, so a second reference would only be a wait).
#[derive(Default)]
struct Gains {
    known: Mutex<HashMap<String, Gain>>,
    changed: Condvar,
}

#[derive(Clone, Copy)]
enum Gain {
    Measuring,
    Known(f32),
}

struct Leveled {
    inner: Arc<dyn Backend>,
    gains: Arc<Gains>,
}

impl Backend for Leveled {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn version(&self) -> String {
        format!("{}+level{LEVEL_VERSION}", self.inner.version())
    }

    fn voices(&self) -> Vec<VoiceInfo> {
        self.inner.voices()
    }

    fn capabilities(&self, voice: &str) -> Capabilities {
        self.inner.capabilities(voice)
    }

    fn session(&self) -> Result<Box<dyn Session>, String> {
        Ok(Box::new(LeveledSession {
            inner: self.inner.session()?,
            backend: Arc::clone(&self.inner),
            gains: Arc::clone(&self.gains),
        }))
    }
}

/// The inner session, asked to sing a phrase at a quality with a seed.
type SingInner<'a> =
    dyn FnMut(&mut dyn Session, &Phrase, Quality, u64) -> Result<Sung, SingError> + 'a;

struct LeveledSession {
    inner: Box<dyn Session>,
    backend: Arc<dyn Backend>,
    gains: Arc<Gains>,
}

impl LeveledSession {
    /// The voice's gain when known; `None` when this session is to measure
    /// it (it is marked as being measured). Waits while another measures.
    fn known(&self, voice: &str) -> Option<f32> {
        let Ok(mut known) = self.gains.known.lock() else {
            return Some(1.0);
        };
        loop {
            match known.get(voice) {
                Some(Gain::Known(g)) => return Some(*g),
                Some(Gain::Measuring) => {
                    known = match self.gains.changed.wait(known) {
                        Ok(k) => k,
                        Err(_) => return Some(1.0),
                    };
                }
                None => {
                    known.insert(voice.to_string(), Gain::Measuring);
                    return None;
                }
            }
        }
    }

    /// Says what was measured (`None`: nothing, so another may try).
    fn settle(&self, voice: &str, gain: Option<f32>) {
        if let Ok(mut known) = self.gains.known.lock() {
            match gain {
                Some(g) => known.insert(voice.to_string(), Gain::Known(g)),
                None => known.remove(voice),
            };
        }
        self.gains.changed.notify_all();
    }

    /// Sings `phrase` scaled by its voice's gain, measuring the gain first
    /// when this is the voice's first phrase. `sing` is the inner session,
    /// asked with the caller's own cancel.
    fn sing_leveled(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        sing: &mut SingInner<'_>,
    ) -> Result<Sung, SingError> {
        let gain = match self.known(&phrase.voice) {
            Some(gain) => gain,
            None => {
                let reference = self
                    .backend
                    .voices()
                    .into_iter()
                    .find(|v| v.id == phrase.voice)
                    .map(|v| reference_phrase(&v));
                let (gain, take) = match &reference {
                    // Always the same seed: the gain is the voice's alone.
                    Some(r) => match sing(self.inner.as_mut(), r, Quality::Preview, REFERENCE_SEED)
                    {
                        Ok(take) => (gain_for(level_db(&take)), Some(take)),
                        Err(SingError::Cancelled) => {
                            self.settle(&phrase.voice, None);
                            return Err(SingError::Cancelled);
                        }
                        Err(SingError::Failed(_)) => (1.0, None),
                    },
                    None => (1.0, None),
                };
                self.settle(&phrase.voice, Some(gain));
                // Asked for the reference itself (warming): it is sung.
                if let (Some(mut take), true) = (take, reference.as_ref() == Some(phrase))
                    && quality == Quality::Preview
                    && seed == REFERENCE_SEED
                {
                    scale(&mut take, gain);
                    return Ok(take);
                }
                gain
            }
        };
        let mut take = sing(self.inner.as_mut(), phrase, quality, seed)?;
        scale(&mut take, gain);
        Ok(take)
    }
}

fn scale(take: &mut Sung, gain: f32) {
    if gain != 1.0 {
        for s in &mut take.samples {
            *s *= gain;
        }
    }
}

impl Session for LeveledSession {
    fn sing(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        self.sing_leveled(phrase, quality, seed, &mut |s, p, q, seed| {
            s.sing(p, q, seed, cancelled)
        })
    }

    fn sing_flagged(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        cancel: &AtomicU32,
    ) -> Result<Sung, SingError> {
        self.sing_leveled(phrase, quality, seed, &mut |s, p, q, seed| {
            s.sing_flagged(p, q, seed, cancel)
        })
    }
}
