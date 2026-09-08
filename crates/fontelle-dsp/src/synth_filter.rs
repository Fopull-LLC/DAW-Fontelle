//! Flopsynth's filter models (`docs/flopsynth-plan.md` §3.3).
//!
//! One slot, four models. The [`SvfFilter`](crate::SvfFilter) the sampler has
//! always had is one of them ([`FilterModel::Clean`]) and keeps its behaviour
//! exactly; the other three are what makes a synthesiser's filter a character
//! rather than a slope.
//!
//! # Why the models are here and not four effects
//!
//! Because a filter in a *voice* is per note. Two notes sounding together each
//! need their own filter memory, and an insert has one — see `Voice::filters`,
//! which has said so since the sampler had two filters and one model.
//!
//! # What one of these costs a voice
//!
//! [`SynthFilter`] is one per channel per slot, and it carries every model's
//! state whether or not that model is selected: a `Copy` struct with a fixed
//! delay line in it, sized once, so that switching model mid-note is a branch
//! rather than an allocation (INVARIANT 1 and 6). The delay line is what makes
//! it large — [`COMB_LEN`] samples — and it is why the comb's lowest reachable
//! frequency is what it is.

use crate::filter::MIN_Q;
use crate::{SvfCoeffs, SvfFilter, SvfMode, vowel_at};

/// How steep the clean filter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FilterSlope {
    /// One two-pole section — what the sampler has always had.
    #[default]
    Db12,
    /// Two of them, with the Q split so the pair is Butterworth at rest.
    Db24,
}

impl FilterSlope {
    pub const ALL: [Self; 2] = [Self::Db12, Self::Db24];

    pub fn label(self) -> &'static str {
        match self {
            Self::Db12 => "12 dB",
            Self::Db24 => "24 dB",
        }
    }
}

/// Which filter this slot is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FilterModel {
    /// The state-variable filter, in every mode it has. Transparent, exact,
    /// and the one to reach for when the filter is meant to be a slope.
    #[default]
    Clean,
    /// Four one-pole stages with a saturating feedback path — the transistor
    /// ladder. `character` is how hard the loop saturates; at full resonance
    /// it self-oscillates.
    Ladder,
    /// Three band-passes at a vowel's formants. `character` morphs
    /// A → E → I → O → U, `cutoff` scales all three at once (an octave up is
    /// a smaller throat) and `resonance` is their bandwidth.
    ///
    /// This is the choir and the talk-box, and it is why "Choir Ahh" can be
    /// built without a sample.
    Formant,
    /// A delay fed back on itself. `character` is the feedback, **signed
    /// around the middle** so the hollow one is reachable; `resonance` damps
    /// the loop.
    Comb,
}

impl FilterModel {
    pub const ALL: [Self; 4] = [Self::Clean, Self::Ladder, Self::Formant, Self::Comb];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clean => "Clean",
            Self::Ladder => "Ladder",
            Self::Formant => "Formant",
            Self::Comb => "Comb",
        }
    }

    /// What the `character` knob's caption says for this model — or `None`
    /// when the model has no use for it, in which case the panel hides it
    /// rather than drawing a knob that does nothing.
    pub fn character_label(self) -> Option<&'static str> {
        match self {
            Self::Clean => None,
            Self::Ladder => Some("saturation"),
            Self::Formant => Some("vowel"),
            Self::Comb => Some("feedback"),
        }
    }
}

/// The comb's delay line, in samples per channel per slot.
///
/// 1024 at 48 kHz puts the lowest comb frequency at 47 Hz, which is below the
/// bottom of a bass guitar and above nothing anybody combs. Fixed, because a
/// voice's cost has to be knowable before it sounds (INVARIANT 6).
pub const COMB_LEN: usize = 1024;

/// Everything one filter slot is set to, after modulation.
///
/// A plain struct handed in per sample rather than fields on the filter: the
/// *settings* are the patch's and move with automation, the *state* is the
/// voice's. Keeping them apart is what lets the cutoff be ramped within a
/// block without the filter having to know that is happening.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthFilterSettings {
    pub model: FilterModel,
    /// Which response, for [`FilterModel::Clean`]. The other three models have
    /// one shape each and ignore it.
    pub mode: SvfMode,
    pub slope: FilterSlope,
    pub cutoff_hz: f32,
    /// A **Q** for `Clean`, and 0..1 across each other model's own useful
    /// span — see the model's docs.
    pub resonance: f32,
    /// 0..1 → 0..+24 dB into a `tanh` **before** the filter, with the make-up
    /// that keeps drive 0 a wire. The same argument the Filter insert makes
    /// (catalogue §3.6): distortion after a filter is a different effect.
    pub drive: f32,
    /// Per model. See [`FilterModel::character_label`].
    pub character: f32,
}

impl Default for SynthFilterSettings {
    fn default() -> Self {
        Self {
            model: FilterModel::Clean,
            mode: SvfMode::Lowpass,
            slope: FilterSlope::Db12,
            cutoff_hz: 20_000.0,
            resonance: 0.707,
            drive: 0.0,
            character: 0.0,
        }
    }
}

/// One filter slot's memory, for one channel of one voice.
#[derive(Debug, Clone, Copy)]
pub struct SynthFilter {
    /// Two sections, so `Db24` is a cascade and `Db12` uses the first alone.
    clean: [SvfFilter; 2],
    /// The ladder's four one-pole stages.
    ladder: [f32; 4],
    /// Three band-passes in parallel, for the formant model.
    formant: [SvfFilter; 3],
    comb: [f32; COMB_LEN],
    comb_write: usize,
    /// The comb's damping pole, so `resonance` is a loss in the loop rather
    /// than a second delay.
    comb_damp: f32,
    /// The last settings [`clean`](Self::clean) built coefficients from, with
    /// the rate and what it built.
    ///
    /// **A `tan` and a `powf` per sample, per section, was most of what a
    /// voice cost.** `SvfFilter::coeffs` pre-warps the corner, which is a
    /// transcendental, and a 24 dB slope asks for two of them; the settings
    /// only move every `FILTER_STEP` samples, because `voice.rs` ramps the
    /// cutoff in steps. Measured (`fontelle-core/benches/flopsynth.rs`): one
    /// Init voice at 1.0 % of a core with the rebuild per sample and 0.4 %
    /// with it cached — the filters were two thirds of the voice.
    ///
    /// Keyed on the settings themselves rather than on a dirty flag, so a
    /// caller that changes one has nothing to remember: `tests/filters.rs`
    /// holds both halves — that the cache is used, and that it is never
    /// stale.
    cached: Option<Cached>,
}

/// What one set of settings works out to, kept until they move.
///
/// The `model` is part of the key, so only the arm the model reads is ever
/// built and the other is whatever the last miss left behind — which nothing
/// can read, because reading it would mean the model had changed and the key
/// with it.
#[derive(Debug, Clone, Copy)]
struct Cached {
    settings: SynthFilterSettings,
    rate: f32,
    /// [`FilterModel::Clean`]'s two sections.
    clean: [crate::SvfCoeffs; 2],
    /// [`FilterModel::Formant`]'s three band-passes, their levels, and the
    /// normalisation that keeps a vowel from being a volume knob.
    formant: [(crate::SvfCoeffs, f32); 3],
    formant_gain: f32,
}

impl Default for SynthFilter {
    fn default() -> Self {
        Self {
            clean: [SvfFilter::new(); 2],
            ladder: [0.0; 4],
            formant: [SvfFilter::new(); 3],
            comb: [0.0; COMB_LEN],
            comb_write: 0,
            comb_damp: 0.0,
            cached: None,
        }
    }
}

impl SynthFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Back to silence.
    ///
    /// What a voice coming out of the pool needs: left alone, a filter's
    /// memory discharges into the new note as a transient belonging to one
    /// that already ended — see `Voice::trigger_note`, which has reset the
    /// SVFs for exactly this reason since before there were four models.
    pub fn reset(&mut self) {
        for filter in &mut self.clean {
            filter.reset();
        }
        self.ladder = [0.0; 4];
        for filter in &mut self.formant {
            filter.reset();
        }
        self.comb = [0.0; COMB_LEN];
        self.comb_write = 0;
        self.comb_damp = 0.0;
    }

    /// One sample through the slot.
    ///
    /// RT-safe: arithmetic, a couple of `tan`s and a `tanh`, no allocation.
    /// Cheap enough to call per sample with the settings changing under it,
    /// which is what the ramped cutoff of §3.3 does.
    pub fn process(&mut self, input: f32, config: &SynthFilterSettings, sample_rate: f32) -> f32 {
        let input = drive(input, config.drive);
        match config.model {
            FilterModel::Clean => self.clean(input, config, sample_rate),
            FilterModel::Ladder => self.ladder(input, config, sample_rate),
            FilterModel::Formant => self.formant(input, config, sample_rate),
            FilterModel::Comb => self.comb(input, config, sample_rate),
        }
    }

    fn clean(&mut self, input: f32, config: &SynthFilterSettings, sample_rate: f32) -> f32 {
        let coeffs = self.coeffs(config, sample_rate).clean;
        match config.slope {
            FilterSlope::Db12 => self.clean[0].process(input, &coeffs[0]),
            FilterSlope::Db24 => {
                let once = self.clean[0].process(input, &coeffs[0]);
                self.clean[1].process(once, &coeffs[1])
            }
        }
    }

    /// Everything the settings work out to, built only when they move.
    ///
    /// See [`Cached`] for why this is not simply recomputed: the pre-warp is a
    /// `tan` and the shelf gain is a `powf`, and a voice was paying for both
    /// on every sample of every filter — three times over on a formant.
    fn coeffs(&mut self, config: &SynthFilterSettings, sample_rate: f32) -> Cached {
        if let Some(cached) = &self.cached
            && cached.settings == *config
            && cached.rate == sample_rate
        {
            return *cached;
        }
        let clean = self.build_clean(config, sample_rate);
        let (formant, formant_gain) = build_formant(config, sample_rate);
        let cached = Cached {
            settings: *config,
            rate: sample_rate,
            clean,
            formant,
            formant_gain,
        };
        self.cached = Some(cached);
        cached
    }

    fn build_clean(&self, config: &SynthFilterSettings, sample_rate: f32) -> [crate::SvfCoeffs; 2] {
        match config.slope {
            FilterSlope::Db12 => {
                let one = SvfFilter::coeffs(
                    config.mode,
                    config.cutoff_hz,
                    config.resonance,
                    0.0,
                    sample_rate,
                );
                [one, one]
            }
            FilterSlope::Db24 => {
                // Two sections whose Qs multiply out to a fourth-order
                // Butterworth at rest — 0.5412 and 1.3066 — scaled together by
                // whatever the resonance knob adds above that, so the corner
                // peaks the way one section would but twice as steeply.
                let lift = (config.resonance / std::f32::consts::FRAC_1_SQRT_2).max(0.01);
                [
                    SvfFilter::coeffs(
                        config.mode,
                        config.cutoff_hz,
                        0.541_2 * lift,
                        0.0,
                        sample_rate,
                    ),
                    SvfFilter::coeffs(
                        config.mode,
                        config.cutoff_hz,
                        1.306_6 * lift,
                        0.0,
                        sample_rate,
                    ),
                ]
            }
        }
    }

    /// Four cascaded one-pole TPT stages with a saturating feedback path —
    /// the transistor ladder.
    ///
    /// # The feedback is solved, not delayed
    ///
    /// The obvious implementation takes the feedback from the previous
    /// sample. It is one line shorter and it is **wrong at the top of the
    /// band**: a sample of delay is a phase lag proportional to frequency, so
    /// near Nyquist the loop reaches −180° while the four stages have barely
    /// attenuated anything, and the filter breaks into oscillation at a
    /// fraction of the resonance it should need. Measured, before this was
    /// fixed: a cutoff at 18 kHz rang on its own at resonance 0.35, and any
    /// preset whose LFO swept a ladder upward screeched.
    ///
    /// So the loop is solved algebraically instead, the way the SVF's is.
    /// Each TPT one-pole is `y = G·x + (1−G)·s`, so a cascade of four is
    /// `y₄ = G⁴·u + S` for a state term `S` that does not depend on `u`; with
    /// `u = x − k·y₄` that rearranges to one division. The result is stable at
    /// every cutoff, and the resonance knob means the same thing across the
    /// keyboard.
    ///
    /// The `tanh` is applied to the solved feedback rather than solved
    /// *through* — that would need a Newton iteration per sample for a
    /// difference nobody has ever heard.
    fn ladder(&mut self, input: f32, config: &SynthFilterSettings, sample_rate: f32) -> f32 {
        let cutoff = config.cutoff_hz.clamp(10.0, sample_rate * 0.45);
        let g = (std::f32::consts::PI * cutoff / sample_rate).tan();
        let big_g = g / (1.0 + g);
        // 0..1 on the knob is 0..4.5 in the loop. The textbook ladder
        // oscillates at exactly 4; the last half is the margin the `tanh` in
        // the loop takes back, so that the top of the knob really is
        // self-oscillation and nothing below it is.
        let feedback = config.resonance.clamp(0.0, 1.0) * 4.5;
        // The bass compensation goes **into** the loop, not onto the output.
        // A ladder at high resonance loses its low end, and the fix is to feed
        // proportionally more in — which is what the circuit does. A quarter
        // of the feedback rather than a half: at a half, the top of the knob
        // was +10 dB of make-up on top of the resonant peak itself, which made
        // turning resonance up a volume knob.
        let compensation = 1.0 + feedback * 0.25;

        // What the cascade would output for an input of zero — the part of
        // `y₄` that is already in the integrators.
        let leak = 1.0 - big_g;
        let s = leak
            * (big_g * big_g * big_g * self.ladder[0]
                + big_g * big_g * self.ladder[1]
                + big_g * self.ladder[2]
                + self.ladder[3]);
        let g4 = big_g * big_g * big_g * big_g;
        let y4 = (g4 * input * compensation + s) / (1.0 + feedback * g4);

        // The saturation in the loop is what stops the oscillation growing
        // without bound; `character` is how hard it is driven into it.
        let drive_in_loop = 1.0 + config.character.clamp(0.0, 1.0) * 3.0;
        let saturated = (y4 * drive_in_loop).tanh() / drive_in_loop;
        let u = input * compensation - feedback * saturated;

        // Now run the stages forward with the input the solve implies, so the
        // states are where the next sample needs them.
        let mut x = u;
        for stage in &mut self.ladder {
            let y = big_g * x + leak * *stage;
            *stage = y + big_g * (x - *stage);
            x = y;
        }
        x
    }

    /// Three band-passes in parallel at the vowel's formants.
    fn formant(&mut self, input: f32, config: &SynthFilterSettings, sample_rate: f32) -> f32 {
        let cached = self.coeffs(config, sample_rate);
        let mut out = 0.0;
        for (index, (coeffs, level)) in cached.formant.iter().enumerate() {
            out += self.formant[index].process(input, coeffs) * level;
        }
        out * cached.formant_gain
    }

    fn comb(&mut self, input: f32, config: &SynthFilterSettings, sample_rate: f32) -> f32 {
        let delay = (sample_rate / config.cutoff_hz.max(1.0)).clamp(2.0, (COMB_LEN - 2) as f32);
        // **Signed around the middle**: below 0.5 the feedback is negative,
        // which is the hollow, stopped-pipe comb; above it is the resonant,
        // plucked-string one. A knob whose middle is "off" and whose two ends
        // are two different filters.
        let feedback = (config.character.clamp(0.0, 1.0) - 0.5) * 2.0 * 0.98;
        let damping = config.resonance.clamp(0.0, 1.0) * 0.9;

        let read = self.comb_write as f32 - delay;
        let read = if read < 0.0 {
            read + COMB_LEN as f32
        } else {
            read
        };
        let index = read as usize % COMB_LEN;
        let frac = read - read.floor();
        let a = self.comb[index];
        let b = self.comb[(index + 1) % COMB_LEN];
        let delayed = a + (b - a) * frac;

        // A one-pole in the loop, so `resonance` is a loss that grows with
        // frequency — which is what makes a resonant comb sound like a string
        // rather than like a ringing tube.
        self.comb_damp += (delayed - self.comb_damp) * (1.0 - damping);
        let damped = self.comb_damp;

        self.comb[self.comb_write] = input + damped * feedback;
        self.comb_write = (self.comb_write + 1) % COMB_LEN;
        // Feed-forward as well as back: the nulls come from the sum, and a
        // pure feedback comb has peaks but no notches.
        (input + delayed) * 0.5
    }
}

/// `tanh(x · gain)` with the make-up that keeps drive 0 a wire.
///
/// Before the filter, not after: the same argument `docs/effects-catalogue.md`
/// §3.6 makes for the Filter insert, and the test that holds it here is the
/// one that holds it there.
/// The three band-passes a vowel is, and the gain that keeps it from being a
/// volume knob.
///
/// The cutoff **scales** the formants rather than replacing them: a throat is
/// a throat and a bigger one is the same vowel lower down. 1 kHz is the pivot,
/// so a cutoff there is the table's own frequencies.
fn build_formant(
    config: &SynthFilterSettings,
    sample_rate: f32,
) -> ([(crate::SvfCoeffs, f32); 3], f32) {
    let scale = (config.cutoff_hz / 1_000.0).clamp(0.1, 8.0);
    let formants = vowel_at(config.character);
    // Resonance is bandwidth: wide open at 0, a whistle at 1.
    let q = 1.5 + config.resonance.clamp(0.0, 1.0) * 12.0;
    let bands = std::array::from_fn(|index| {
        let (hz, level) = formants[index];
        (
            SvfFilter::coeffs(
                SvfMode::Bandpass,
                (hz * scale).clamp(20.0, sample_rate * 0.45),
                q,
                0.0,
                sample_rate,
            ),
            level,
        )
    });
    // A band-pass at Q 12 is 12 times as loud at its peak as the signal going
    // in; without this a vowel would be a volume knob.
    (bands, 1.0 / q.sqrt())
}

fn drive(input: f32, amount: f32) -> f32 {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 {
        // **Exactly** the input, not `tanh(x)/1` — a drive knob at the bottom
        // of its travel has to be the absence of an effect, and `tanh` is not
        // an identity anywhere but zero.
        return input;
    }
    // 0..+24 dB in.
    let gain = 10f32.powf(amount * 24.0 / 20.0);
    (input * gain).tanh() / gain.tanh()
}

/// A cutoff that follows the keyboard.
///
/// Middle C is the pivot, so a patch tuned there does not move when key
/// tracking is turned on — it only starts moving either side of it. At 1.0 the
/// corner follows the key exactly, which is what makes a comb "tuned to the
/// note" (the Guitar-ish preset) and a filter sound the same at every pitch.
pub fn key_tracked_cutoff(cutoff_hz: f32, key: u8, amount: f32) -> f32 {
    let semitones = (f32::from(key) - 60.0) * amount.clamp(0.0, 1.0);
    cutoff_hz * 2f32.powf(semitones / 12.0)
}

/// The coefficients [`FilterModel::Clean`] would use, for whatever wants to
/// draw the response rather than run it.
pub fn clean_coeffs(config: &SynthFilterSettings, sample_rate: f32) -> SvfCoeffs {
    SvfFilter::coeffs(
        config.mode,
        config.cutoff_hz,
        config.resonance,
        0.0,
        sample_rate,
    )
}

// -------------------------------------------------------- the picture ---

/// This filter's magnitude response at `hz`, in decibels.
///
/// # Why a closed form and not a measurement
///
/// §8.1 rule 5: *a picture that lies is believed*. The safest way to draw a
/// filter's response would be to run an impulse through the real thing and
/// transform it — but that is thousands of samples per redraw, and the window
/// redraws whenever anything moves.
///
/// So each model's response is written out in closed form here, **beside the
/// model it describes**, and `tests/filters.rs` holds every one of them
/// against the response the filter actually produces. A picture drawn from a
/// second description of a filter is a picture that will drift from it; a
/// picture drawn from a formula the same file's tests check against the sound
/// is one that cannot.
///
/// The drive is deliberately not in it: a `tanh` has no magnitude response,
/// because what it does depends on how loud the signal is.
pub fn response_db(config: &SynthFilterSettings, hz: f32, sample_rate: f32) -> f32 {
    let magnitude = match config.model {
        FilterModel::Clean => clean_magnitude(config, hz, sample_rate),
        FilterModel::Ladder => ladder_magnitude(config, hz, sample_rate),
        FilterModel::Formant => formant_magnitude(config, hz, sample_rate),
        FilterModel::Comb => comb_magnitude(config, hz, sample_rate),
    };
    20.0 * magnitude.max(1e-5).log10()
}

/// The analogue prototype's frequency ratio, pre-warped the way the filter's
/// own coefficients are — so the curve bends towards Nyquist exactly where the
/// filter does.
fn warped_ratio(hz: f32, cutoff: f32, sample_rate: f32) -> f32 {
    let nyquist = sample_rate * 0.5;
    let cutoff = cutoff.clamp(1.0, nyquist * 0.99);
    let hz = hz.clamp(1.0, nyquist * 0.999);
    let warp = |f: f32| (std::f32::consts::PI * f / sample_rate).tan();
    warp(hz) / warp(cutoff).max(1e-9)
}

fn clean_magnitude(config: &SynthFilterSettings, hz: f32, sample_rate: f32) -> f32 {
    let one = svf_magnitude(
        config.mode,
        config.resonance.max(MIN_Q),
        hz,
        config.cutoff_hz,
        sample_rate,
    );
    match config.slope {
        FilterSlope::Db12 => one,
        // Two sections in series is their product, with the Qs the cascade
        // actually uses.
        FilterSlope::Db24 => {
            let lift = (config.resonance / std::f32::consts::FRAC_1_SQRT_2).max(0.01);
            svf_magnitude(
                config.mode,
                0.541_2 * lift,
                hz,
                config.cutoff_hz,
                sample_rate,
            ) * svf_magnitude(
                config.mode,
                1.306_6 * lift,
                hz,
                config.cutoff_hz,
                sample_rate,
            )
        }
    }
}

/// One two-pole section's transfer at `hz`, as a complex number.
///
/// Complex rather than a magnitude, because the formant filter **sums three of
/// these** and signals add as complex numbers: between two formants the two
/// band-passes are most of a half-cycle apart and partly cancel, and adding
/// their magnitudes instead drew a curve twelve decibels above the filter in
/// exactly the trough that makes a vowel a vowel.
fn svf_transfer(mode: SvfMode, q: f32, hz: f32, cutoff: f32, sample_rate: f32) -> (f32, f32) {
    let w = warped_ratio(hz, cutoff, sample_rate);
    let k = 1.0 / q.max(MIN_Q);
    // `1 − w² + jkw` is the shared denominator of every mode; each mode is a
    // different numerator over it.
    let (dr, di) = (1.0 - w * w, k * w);
    let (nr, ni) = match mode {
        SvfMode::Lowpass => (1.0, 0.0),
        // `(jw)²`.
        SvfMode::Highpass => (-w * w, 0.0),
        // `jw`, whose magnitude at the corner is `1/k` — which **is** Q, and
        // is what makes `SvfFilter::coeffs` say resonance is checkable.
        SvfMode::Bandpass => (0.0, w),
        SvfMode::Notch => (1.0 - w * w, 0.0),
        // The three that take a gain are drawn flat: this window has no gain
        // control for them, and a curve for a number nobody can set would be
        // a picture of nothing.
        SvfMode::Bell | SvfMode::LowShelf | SvfMode::HighShelf => (dr, di),
    };
    // `(nr + j·ni) / (dr + j·di)`.
    let magnitude = (dr * dr + di * di).max(1e-12);
    (
        (nr * dr + ni * di) / magnitude,
        (ni * dr - nr * di) / magnitude,
    )
}

fn svf_magnitude(mode: SvfMode, q: f32, hz: f32, cutoff: f32, sample_rate: f32) -> f32 {
    let (re, im) = svf_transfer(mode, q, hz, cutoff, sample_rate);
    (re * re + im * im).sqrt()
}

/// Four one-poles inside a feedback loop: `G⁴ / (1 + k·G⁴)` with
/// `G = 1/(1 + jw)`. The same transfer the solved loop implements.
fn ladder_magnitude(config: &SynthFilterSettings, hz: f32, sample_rate: f32) -> f32 {
    let w = warped_ratio(hz, config.cutoff_hz, sample_rate);
    // One pole: 1/(1 + jw). Its fourth power in polar form.
    let pole = 1.0 / (1.0 + w * w).sqrt();
    let angle = -w.atan();
    let (magnitude, phase) = (pole.powi(4), angle * 4.0);
    let feedback = config.resonance.clamp(0.0, 1.0) * 4.5;
    // 1 + k·G⁴, as a complex number.
    let (dr, di) = (
        1.0 + feedback * magnitude * phase.cos(),
        feedback * magnitude * phase.sin(),
    );
    let compensation = 1.0 + feedback * 0.25;
    magnitude * compensation / (dr * dr + di * di).sqrt().max(1e-9)
}

fn formant_magnitude(config: &SynthFilterSettings, hz: f32, sample_rate: f32) -> f32 {
    let scale = (config.cutoff_hz / 1_000.0).clamp(0.1, 8.0);
    let q = 1.5 + config.resonance.clamp(0.0, 1.0) * 12.0;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (centre, level) in vowel_at(config.character) {
        let (r, i) = svf_transfer(
            SvfMode::Bandpass,
            q,
            hz,
            (centre * scale).clamp(20.0, sample_rate * 0.45),
            sample_rate,
        );
        re += r * level;
        im += i * level;
    }
    (re * re + im * im).sqrt() / q.sqrt()
}

/// A feed-forward and a feedback comb over one delay, **with the damping pole
/// that sits inside the loop**.
///
/// The loop is `w[n] = x[n] + f·L(w[n−D])` for a one-pole `L`, and the output
/// is `(x[n] + w[n−D])/2`. Leaving `L` out — which the first version of this
/// did — drew a comb eighteen decibels above the real one wherever the
/// damping was doing its job, which is most of the band.
fn comb_magnitude(config: &SynthFilterSettings, hz: f32, sample_rate: f32) -> f32 {
    let delay = (sample_rate / config.cutoff_hz.max(1.0)).clamp(2.0, (COMB_LEN - 2) as f32);
    let feedback = (config.character.clamp(0.0, 1.0) - 0.5) * 2.0 * 0.98;
    let damping = config.resonance.clamp(0.0, 1.0) * 0.9;

    // `z⁻ᴰ` at this frequency.
    let theta = -std::f32::consts::TAU * hz * delay / sample_rate;
    let (zr, zi) = (theta.cos(), theta.sin());
    // The damping one-pole `L(z) = a / (1 − (1−a)z⁻¹)`.
    let a = 1.0 - damping;
    let w = -std::f32::consts::TAU * hz / sample_rate;
    let (pr, pi) = (1.0 - (1.0 - a) * w.cos(), -(1.0 - a) * w.sin());
    let pole = (pr * pr + pi * pi).max(1e-12);
    let (lr, li) = (a * pr / pole, -a * pi / pole);

    // `f·L·z⁻ᴰ`.
    let (fr, fi) = (
        feedback * (lr * zr - li * zi),
        feedback * (lr * zi + li * zr),
    );
    // `1 / (1 − f·L·z⁻ᴰ)`.
    let (dr, di) = (1.0 - fr, -fi);
    let denominator = (dr * dr + di * di).max(1e-12);
    let (wr, wi) = (dr / denominator, -di / denominator);
    // `(1 + z⁻ᴰ·W) / 2`.
    let (yr, yi) = ((1.0 + zr * wr - zi * wi) * 0.5, (zr * wi + zi * wr) * 0.5);
    (yr * yr + yi * yi).sqrt()
}
