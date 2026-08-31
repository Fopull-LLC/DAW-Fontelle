//! The parametric EQ's DSP. Its parameters are `fontelle_types::EqConfig` —
//! the document owns those, this owns the filter memory.

use fontelle_dsp::{SvfCoeffs, SvfFilter, SvfMode};
use fontelle_types::{BANDS, BUTTERWORTH_Q, BandChannel, BandType, EqConfig};

/// The most 2-pole sections one band can need: a 48 dB/oct pass filter is
/// eighth-order, which is four of them in series.
const MAX_SECTIONS: usize = 4;

/// Left and right. A bus with more than two channels is not something the
/// mixer builds, and one with fewer is handled by processing what is there.
const MAX_CHANNELS: usize = 2;

/// The filter mode one section of `band_type` runs in.
///
/// Here rather than on `BandType` itself because `SvfMode` is a `fontelle-dsp`
/// type and the document may not name one: what a band *is* belongs to the
/// document, and what it is built out of belongs here.
fn mode_of(band_type: BandType) -> SvfMode {
    match band_type {
        BandType::Bell => SvfMode::Bell,
        BandType::LowShelf => SvfMode::LowShelf,
        BandType::HighShelf => SvfMode::HighShelf,
        BandType::LowPass12 | BandType::LowPass24 | BandType::LowPass48 => SvfMode::Lowpass,
        BandType::HighPass12 | BandType::HighPass24 | BandType::HighPass48 => SvfMode::Highpass,
        BandType::Notch => SvfMode::Notch,
        BandType::BandPass => SvfMode::Bandpass,
    }
}

/// The Q of each section of a Butterworth cascade, by order.
///
/// A steep filter is not one section with a high Q, and it is not several
/// identical sections either: cascading four copies of a Q=0.707 low-pass
/// gives a response 12 dB down at the corner rather than 3, with a droop that
/// starts an octave early. The poles of a Butterworth of order `2n` sit at
/// evenly spaced angles on a circle, and these are the Qs those angles imply —
/// one gentle section and one resonant one for fourth order, and so on. It is
/// the difference between a filter flat up to its corner and one that sags
/// into it.
fn butterworth_qs(sections: usize) -> &'static [f32] {
    match sections {
        2 => &[0.541_196_1, 1.306_562_9],
        4 => &[0.509_795_6, 0.601_344_9, 0.899_976_2, 2.562_915_4],
        // One section is the plain 2-pole case.
        _ => &[BUTTERWORTH_Q],
    }
}

/// One band that is going to run this block: whose filter memory it uses, how
/// many sections deep it is, and which channels it touches as a bit per
/// channel.
#[derive(Debug, Clone, Copy)]
struct Active {
    band: usize,
    sections: usize,
    channels: u8,
}

impl Active {
    const NONE: Self = Self {
        band: 0,
        sections: 0,
        channels: 0,
    };
}

/// A band-pass at unity gain in its own centre.
///
/// The SVF's own band-pass output is `v1`, which peaks at Q — the Cytomic
/// form's plain band-pass, and the right one for a voice's resonant filter,
/// where the resonance is meant to be loud. An EQ wants the other one: a
/// band-pass band at Q=4 that lifted its centre by 12 dB would be a bell
/// nobody asked for, and a soloed band would get louder the narrower it got.
/// Scaling the band-pass mix coefficient by `k` normalises it, at no per-sample
/// cost — the multiply happens once, here.
fn band_pass(freq_hz: f32, q: f32, sample_rate: f32) -> SvfCoeffs {
    let mut coeffs = SvfFilter::coeffs(SvfMode::Bandpass, freq_hz, q, 0.0, sample_rate);
    coeffs.m1 *= coeffs.k;
    coeffs
}

/// Filler for the unused tail of a block's coefficient array. Never reached —
/// `count` bounds every read — but an array has to be initialised with
/// something, and a pass-through is the one value that could not make noise if
/// that bound were ever wrong.
const FLAT: SvfCoeffs = SvfCoeffs {
    g: 0.0,
    k: 0.0,
    a1: 1.0,
    a2: 0.0,
    a3: 0.0,
    m0: 1.0,
    m1: 0.0,
    m2: 0.0,
};

/// 8 bands, TPT/SVF topology per band, mid/side capable (TDD §13.4).
///
/// The filter memory only — every parameter arrives with the block, in an
/// [`EqConfig`]. Coefficients are worked out once per block rather than once
/// per sample: an EQ's controls move when a hand moves them, and a block is
/// 2.7 ms, so per-sample recomputation would buy inaudible smoothness at eight
/// `tan` calls a sample.
pub struct ParametricEq {
    /// `sections[channel][band][section]`. One filter per section per band per
    /// channel, because two channels sharing a filter's state is a stereo EQ
    /// that collapses the image, and two sections sharing one is a slope that
    /// is not the slope asked for.
    sections: [[[SvfFilter; MAX_SECTIONS]; BANDS]; MAX_CHANNELS],
    sample_rate: f32,
}

impl ParametricEq {
    pub fn new() -> Self {
        Self {
            sections: [[[SvfFilter::new(); MAX_SECTIONS]; BANDS]; MAX_CHANNELS],
            sample_rate: 48_000.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reset();
    }

    pub fn reset(&mut self) {
        for channel in &mut self.sections {
            for band in channel {
                for section in band {
                    section.reset();
                }
            }
        }
    }

    /// Runs `channels` through the EQ in place.
    ///
    /// Accepts one channel as readily as two: a mono bus is a bus, and an
    /// effect that silently passes it through is the kind of gap this project
    /// keeps finding. On one channel there is nothing but mid, so a side band
    /// has nothing to work on and does not run.
    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &EqConfig) {
        if channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels
            .iter()
            .take(used)
            .map(|c| c.len())
            .min()
            .unwrap_or(0);
        if frames == 0 {
            return;
        }

        // Mid/side only if some band asked for it, and only with two channels
        // to rotate. While it is on, channel 0 carries the mid and channel 1
        // the side, which is what makes a band's target a channel index.
        let rotate = used >= 2 && config.needs_mid_side();

        // Coefficients resolved once per block, so the sample loop is
        // arithmetic. Each entry names the band slot it belongs to (its filter
        // memory), how many sections it runs, and which channels it touches —
        // so a band that is off, or is pointed at the side of a mono bus,
        // costs nothing per sample rather than a branch per sample.
        let soloing = config.soloing();
        let mut active: [Active; BANDS] = [Active::NONE; BANDS];
        let mut coeffs = [[FLAT; MAX_SECTIONS]; BANDS];
        let mut count = 0;

        for (index, band) in config.bands.iter().enumerate() {
            let channels_mask = match (band.channel, rotate) {
                // Not rotating: every band is on both sides, because filtering
                // mid and side alike is the same thing as filtering left and
                // right alike — see `BandChannel`.
                (_, false) if used >= 2 => 0b11,
                (BandChannel::Side, false) => 0b00,
                (_, false) => 0b01,
                (BandChannel::Stereo, true) => 0b11,
                (BandChannel::Mid, true) => 0b01,
                (BandChannel::Side, true) => 0b10,
            };
            if channels_mask == 0 {
                continue;
            }

            if soloing {
                // Listen: whatever kind of band it is, what a person wants to
                // hear is the region it works on, which is a band-pass at its
                // own frequency and Q. A soloed low-pass auditioned as a
                // low-pass would just be the mix again.
                if !(band.enabled && band.solo) {
                    continue;
                }
                coeffs[count][0] = band_pass(band.freq_hz, band.q, self.sample_rate);
                active[count] = Active {
                    band: index,
                    sections: 1,
                    channels: channels_mask,
                };
                count += 1;
                continue;
            }

            if !band.is_audible() {
                continue;
            }
            let mode = mode_of(band.band_type);
            let sections = band.band_type.sections();
            // A cascade divides its gain between its sections, so two shelves
            // in series make the shelf that was asked for rather than twice it.
            let gain = if band.band_type.uses_gain() {
                band.gain_db / sections as f32
            } else {
                0.0
            };
            let table = butterworth_qs(sections);
            for section in 0..sections {
                // A pass filter's sections take their Q from the Butterworth
                // table, scaled by what was asked for so the corner can still
                // be made to resonate. Everything else is one section at the
                // band's own Q.
                let q = if band.band_type.is_pass() {
                    table[section] * (band.q / BUTTERWORTH_Q)
                } else {
                    band.q
                };
                coeffs[count][section] = if mode == SvfMode::Bandpass {
                    band_pass(band.freq_hz, q, self.sample_rate)
                } else {
                    SvfFilter::coeffs(mode, band.freq_hz, q, gain, self.sample_rate)
                };
            }
            active[count] = Active {
                band: index,
                sections,
                channels: channels_mask,
            };
            count += 1;
        }

        if count == 0 {
            // Nothing switched on. The rotation is its own inverse, so there
            // is no work here either — and skipping it keeps an EQ nobody has
            // touched bit-for-bit a wire.
            return;
        }

        // Indexed rather than iterated: the loop reaches into two channels at
        // once for the mid/side rotation, which is exactly what an iterator
        // over one of them cannot do.
        #[allow(clippy::needless_range_loop)]
        for frame in 0..frames {
            // Mid/side is a rotation into another pair of axes and back out of
            // them again; everything between is the same filtering.
            if rotate {
                let (l, r) = (channels[0][frame], channels[1][frame]);
                channels[0][frame] = (l + r) * 0.5;
                channels[1][frame] = (l - r) * 0.5;
            }

            for (channel, filters) in self.sections.iter_mut().take(used).enumerate() {
                let mut sample = channels[channel][frame];
                for (slot, entry) in active.iter().take(count).enumerate() {
                    if entry.channels & (1 << channel) == 0 {
                        continue;
                    }
                    for section in 0..entry.sections {
                        sample =
                            filters[entry.band][section].process(sample, &coeffs[slot][section]);
                    }
                }
                channels[channel][frame] = sample;
            }

            if rotate {
                let (m, s) = (channels[0][frame], channels[1][frame]);
                channels[0][frame] = m + s;
                channels[1][frame] = m - s;
            }
        }
    }
}

impl Default for ParametricEq {
    fn default() -> Self {
        Self::new()
    }
}
