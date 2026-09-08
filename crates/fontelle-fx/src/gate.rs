//! The gate and the expander, which are one machine
//! (`docs/effects-catalogue.md` §2.1). Its parameters are
//! `fontelle_types::GateConfig` — the document owns those, this owns the
//! envelope and the look-ahead line.
//!
//! # The path
//!
//! ```text
//! key → (key high-pass) → peak, stereo-linked → open? ─┐
//!                                                       ↓
//!            out ← × gain ← attack / hold / release ← target
//!                    ↑
//!            audio, delayed by the look-ahead
//! ```
//!
//! # What makes a gate usable rather than merely correct
//!
//! Three things, and each one is a control here because each one is a defect
//! without it:
//!
//! - **Hysteresis.** A signal sitting at the threshold crosses it dozens of
//!   times a second, and a gate with one threshold opens and closes on every
//!   crossing. That is not gating, it is a stutter — and it is what a person
//!   hears as "the gate is chattering". The opening threshold and the closing
//!   one are held apart by `hysteresis_db`.
//! - **Hold.** A drum's decay dips under the threshold long before the drum
//!   has stopped. Without a hold, the release starts on that first dip and
//!   the tail is cut in half.
//! - **A key filter.** A gate on a hi-hat mic that hears the kick opens on
//!   the kick. The filter is on the *decision*, not on the audio, so nothing
//!   it does reaches the output.
//!
//! # What it is not
//!
//! Not a transient shaper — this only ever pulls the gain *down*. Not a
//! ducker: the key is the signal itself until sends reach `EffectNode`, and
//! the ducker is the same detector with the knobs relabelled
//! (`docs/effects-catalogue.md` §2.1).

use fontelle_dsp::{SvfCoeffs, SvfFilter, SvfMode};
use fontelle_types::{GATE_KEY_OFF_HZ, GateConfig, MAX_GATE_LOOKAHEAD_MS};

const MAX_CHANNELS: usize = 2;

/// The floor the detector reports for silence. Not negative infinity, because
/// every number downstream of it is arithmetic — the same floor and the same
/// reason as the compressor's.
const SILENCE_DB: f32 = -120.0;

/// One notch above the bottom of the key filter's knob is where "off" stops.
const OFF_MARGIN_HZ: f32 = 0.5;

/// How long the detector holds a peak before it starts letting go, and how
/// fast it lets go afterwards.
///
/// **The detector is not the rectifier.** A gate compares an *envelope* to
/// its threshold, and a bare `|x|` is not one: a steady sine's rectified
/// value visits zero twice a cycle, so a gate written on it opens and closes
/// at the tone's own frequency and every measurement of its threshold comes
/// out wrong. Holding the peak for one cycle of the lowest note anybody gates
/// on — 50 Hz, so 20 ms — makes a steady tone read as a steady level, which
/// is what a threshold is a threshold *of*.
///
/// The hold is the reason the gate takes about this long to notice that a
/// note has stopped. That is a property of every peak-hold detector, and it
/// is under the [`hold`](fontelle_types::GateConfig::hold_ms) knob rather
/// than over it: 20 ms is shorter than the shortest hold a person sets.
const DETECT_HOLD_MS: f32 = 20.0;
const DETECT_RELEASE_MS: f32 = 10.0;

/// A downward expander with a hold, a hysteresis and a look-ahead — which at
/// the top of its ratio and the bottom of its range is a gate
/// (`docs/effects-catalogue.md` §2.1).
///
/// **Stereo-linked**, like the compressor and the limiter: one decision from
/// whichever channel is louder, because a gate that closed one side of a
/// stereo overhead pair and not the other would move the kit.
pub struct Gate {
    /// Whether the detector is currently over the threshold. What the
    /// hysteresis switches between the two thresholds on.
    open: bool,
    /// Samples of hold left to run, counted down once the signal has fallen
    /// back under. Fractional so the knob is not quantised to blocks.
    hold_left: f32,
    /// The gain currently applied, in dB and **negative**, which is what
    /// attack and release smooth.
    reduction_db: f32,
    /// The detector's envelope: the key's peak, held and then let go — see
    /// [`DETECT_HOLD_MS`].
    envelope: f32,
    /// Samples of that hold left to run.
    detect_hold: f32,
    /// The key's high-pass, one per channel: the decision is stereo-linked,
    /// but a filter runs on a signal rather than on a magnitude, so each side
    /// is filtered before the two are compared.
    key_hp: [SvfFilter; MAX_CHANNELS],
    /// The look-ahead line: the audio, delayed so the decision arrives first.
    /// Interleaved, sized in `prepare` for the longest look-ahead the knob
    /// offers, and read at whatever offset the knob is asking for now.
    delay: Vec<f32>,
    /// How many frames the line holds.
    capacity: usize,
    write: usize,
    sample_rate: f32,
}

impl Gate {
    pub fn new() -> Self {
        Self {
            open: false,
            hold_left: 0.0,
            reduction_db: 0.0,
            envelope: 0.0,
            detect_hold: 0.0,
            key_hp: Default::default(),
            delay: Vec::new(),
            capacity: 0,
            write: 0,
            sample_rate: 48_000.0,
        }
    }

    /// Off-RT: allocates the look-ahead line, sized for the top of the knob
    /// rather than for the current setting, so moving the knob is a change of
    /// read offset rather than a reallocation on the audio thread
    /// (INVARIANT 1).
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.capacity =
            ((MAX_GATE_LOOKAHEAD_MS / 1000.0 * self.sample_rate).ceil() as usize).max(1) + 1;
        self.delay.clear();
        self.delay.resize(self.capacity * MAX_CHANNELS, 0.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        // The *gain* starts open, not closed: a gate that started at its
        // range would swallow the first note after every transport stop while
        // its attack ran. The detector starts empty, which is the truth — so
        // a track that begins in silence closes the gate over its release,
        // exactly as it would mid-song.
        self.open = true;
        self.hold_left = 0.0;
        self.reduction_db = 0.0;
        self.envelope = 0.0;
        self.detect_hold = 0.0;
        for filter in &mut self.key_hp {
            filter.reset();
        }
        self.delay.fill(0.0);
        self.write = 0;
    }

    /// How much it is pulling down right now, in dB. Negative, and zero when
    /// the gate is open — what a gain-reduction meter draws.
    pub fn gain_reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// The look-ahead's cost, in samples, at the setting it was last run at.
    ///
    /// The graph compensates for it: `fontelle_app::realise` holds every
    /// other track back to meet this one, and `EffectNode` delays the dry
    /// path the mix control blends back in (TDD §5.5).
    pub fn latency_samples(&self, config: &GateConfig) -> u32 {
        self.lookahead_frames(config) as u32
    }

    fn lookahead_frames(&self, config: &GateConfig) -> usize {
        let asked = config.lookahead_ms.clamp(0.0, MAX_GATE_LOOKAHEAD_MS);
        ((asked / 1000.0 * self.sample_rate).round() as usize).min(self.capacity.saturating_sub(1))
    }

    /// Runs `main` through the gate in place.
    ///
    /// `sidechain`, when given, is what the detector listens to instead of
    /// the signal itself — the external key of `docs/effects-catalogue.md`
    /// §2.1. Routing one track's audio here is the graph's job; this is the
    /// half that is ready for it.
    pub fn process(
        &mut self,
        main: &mut [&mut [f32]],
        sidechain: Option<&[f32]>,
        config: &GateConfig,
    ) {
        if main.is_empty() || self.delay.is_empty() {
            return;
        }
        let used = main.len().min(MAX_CHANNELS);
        let frames = main.iter().take(used).map(|c| c.len()).min().unwrap_or(0);
        if frames == 0 {
            return;
        }

        // The two thresholds the hysteresis holds apart: the signal has to
        // reach the upper one to open the gate and fall under the lower one
        // to close it.
        let open_at = config.threshold_db;
        let close_at = open_at - config.hysteresis_db.max(0.0);
        let ratio = config.ratio.max(1.0);
        let range_db = config.range_db.min(0.0);

        let attack = coefficient(config.attack_ms, self.sample_rate);
        let release = coefficient(config.release_ms, self.sample_rate);
        let hold_samples = (config.hold_ms.max(0.0) / 1000.0) * self.sample_rate;

        let keying = config.key_hp_hz > GATE_KEY_OFF_HZ + OFF_MARGIN_HZ;
        let key_hp: SvfCoeffs = SvfFilter::coeffs(
            SvfMode::Highpass,
            config.key_hp_hz,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            self.sample_rate,
        );

        let detect_hold_samples = (DETECT_HOLD_MS / 1000.0) * self.sample_rate;
        let detect_release = coefficient(DETECT_RELEASE_MS, self.sample_rate);

        let lookahead = self.lookahead_frames(config);

        for frame in 0..frames {
            // What the detector is looking at: the key if there is one, and
            // otherwise the loudest channel of the signal itself. The filter
            // runs on the signal and the comparison on what comes out of it,
            // so the key's *shape* decides and never reaches the audio.
            let level = match sidechain {
                Some(key) => {
                    let sample = key.get(frame).copied().unwrap_or(0.0);
                    let sample = if keying {
                        self.key_hp[0].process(sample, &key_hp)
                    } else {
                        sample
                    };
                    sample.abs()
                }
                None => {
                    let mut peak = 0.0f32;
                    for channel in 0..used {
                        let sample = main[channel][frame];
                        let sample = if keying {
                            self.key_hp[channel].process(sample, &key_hp)
                        } else {
                            sample
                        };
                        peak = peak.max(sample.abs());
                    }
                    peak
                }
            };
            // The rectified key, turned into an envelope: up instantly, held,
            // and only then let go. See `DETECT_HOLD_MS` for why a bare
            // rectifier is not a detector.
            if level >= self.envelope {
                self.envelope = level;
                self.detect_hold = detect_hold_samples;
            } else if self.detect_hold > 0.0 {
                self.detect_hold -= 1.0;
            } else {
                self.envelope += detect_release * (level - self.envelope);
            }
            let level_db = to_db(self.envelope);

            // The decision. `below` is measured against whichever threshold is
            // live, which is what makes the hysteresis a *state* rather than a
            // second knob on the same comparison.
            let threshold_now = if self.open { close_at } else { open_at };
            let below = threshold_now - level_db;
            if below <= 0.0 {
                self.open = true;
                // The hold is recharged for as long as the signal is over,
                // and spent afterwards — so it measures time since the signal
                // left rather than time since the gate opened.
                self.hold_left = hold_samples;
            } else if self.hold_left > 0.0 {
                self.hold_left -= 1.0;
            } else {
                self.open = false;
            }

            let target_db = if self.open || self.hold_left > 0.0 {
                0.0
            } else {
                // A downward expander: every decibel under the threshold is
                // `ratio - 1` more decibels off, until the range stops it. At
                // a ratio of one this is zero at every level, which is why a
                // 1:1 gate is a wire.
                (-below * (ratio - 1.0)).max(range_db)
            };

            // Attack is the gate *opening* and release is it closing, which is
            // the opposite way round from a compressor's — and gets them
            // backwards if the comparison is written on the level rather than
            // on the gain.
            let coeff = if target_db > self.reduction_db {
                attack
            } else {
                release
            };
            self.reduction_db += coeff * (target_db - self.reduction_db);
            let gain = if self.reduction_db >= -1e-6 {
                // Exactly unity when the gate is open, so a gate nobody has
                // set is the wire it claims to be rather than the wire times
                // 0.99999.
                1.0
            } else {
                10f32.powf(self.reduction_db / 20.0)
            };

            // The audio, delayed by the look-ahead so the decision above —
            // made on the *undelayed* signal — is already in force when the
            // transient that caused it comes out.
            // Written every block whatever the knob says, so that turning the
            // look-ahead up mid-song reads history rather than the silence a
            // line nobody had been filling would hold. At zero the read slot
            // *is* the write slot, so this is the input back, exactly — no
            // branch, and no special case to get wrong.
            let read = (self.write + self.capacity - lookahead) % self.capacity;
            for channel in 0..used {
                self.delay[self.write * MAX_CHANNELS + channel] = main[channel][frame];
                main[channel][frame] = self.delay[read * MAX_CHANNELS + channel] * gain;
            }
            self.write = (self.write + 1) % self.capacity;

            // Channels past the pair the decision links are silenced rather
            // than passed: they would arrive ahead of the ones the look-ahead
            // delays.
            for channel in used..main.len() {
                main[channel][frame] = 0.0;
            }
        }
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

/// A one-pole smoother's coefficient for a time constant in milliseconds —
/// the time to cover about 63 % of the distance, which is the convention
/// every attack knob is calibrated in, and the same one the compressor uses.
fn coefficient(ms: f32, sample_rate: f32) -> f32 {
    let samples = (ms.max(0.001) / 1000.0) * sample_rate;
    if samples <= 1.0 {
        1.0
    } else {
        1.0 - (-1.0 / samples).exp()
    }
}

fn to_db(level: f32) -> f32 {
    if level <= 1e-6 {
        SILENCE_DB
    } else {
        20.0 * level.log10()
    }
}
