//! The modulation rate (`docs/flopsynth-next.md` §4.2): the mod envelopes
//! and the LFOs advance every `MOD_STEP` samples and the matrix is walked
//! at that rate, with the layer's gain, pitch, pan and position ramped
//! between steps — so a 4 ms gate is heard and a 20 Hz LFO on pitch is a
//! vibrato rather than a stair.
//!
//! Block rate was 375 Hz at the engine's 128-frame blocks: a source read
//! once a block and held. A 20 Hz sine held in 128-sample steps is a
//! staircase, and a staircase on pitch puts a comb of sidebands round the
//! note at multiples of 375 Hz; a 5 ms envelope read at 0, 2.7 and 5.3 ms
//! never reaches its peak.

use fontelle_core::flopsynth::{LayerRole, flopsynth_init};
use fontelle_core::{
    Curve, ModDest, ModRoute, ModSource, NoteTrigger, Patch, PrepareContext, SampleStore,
    Sampler, Source,
};
use fontelle_dsp::{SynthSource, WavetableId, fft_in_place};

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn route(source: ModSource, destination: ModDest, depth: f32) -> ModRoute {
    ModRoute {
        source,
        destination,
        depth,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass: false,
    }
}

/// The Init with only oscillator A up, a plain sine, no filter, an amp
/// envelope that opens at once and holds.
fn a_sine() -> Patch {
    let mut patch = flopsynth_init();
    for (index, layer) in patch.layers.iter_mut().enumerate() {
        let Source::Synth(osc) = &mut layer.source else {
            continue;
        };
        if index == LayerRole::OscA as usize {
            osc.source = SynthSource::Table(WavetableId::SubSine);
            osc.unison.voices = 1;
            layer.gain_db = -6.0;
        } else {
            layer.gain_db = -120.0;
        }
    }
    for slot in &mut patch.filters {
        slot.enabled = false;
    }
    patch.envelopes[0].attack_s = 0.0;
    patch.envelopes[0].decay_s = 0.0;
    patch.envelopes[0].sustain_level = 1.0;
    patch.mod_matrix.routes.clear();
    patch
}

/// `key` at velocity 100 for `seconds`, rendered in the engine's blocks,
/// the left channel.
fn render(patch: Patch, key: u8, seconds: f32) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let frames = (seconds * SR) as usize;
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        let mut l = [0.0f32; BLOCK];
        let mut r = [0.0f32; BLOCK];
        sampler.render(&store, &mut [&mut l, &mut r]);
        out.extend_from_slice(&l);
    }
    out.truncate(frames);
    out
}

/// The power spectrum of `samples` under a Blackman-Harris window, in dB
/// per bin, and the width of a bin.
fn spectrum(samples: &[f32]) -> (Vec<f32>, f32) {
    let n = samples.len();
    assert!(n.is_power_of_two());
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let t = std::f32::consts::TAU * i as f32 / n as f32;
            let w =
                0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos();
            s * w
        })
        .collect();
    let mut im = vec![0.0f32; n];
    fft_in_place(&mut re, &mut im);
    let db: Vec<f32> = re[..n / 2]
        .iter()
        .zip(&im[..n / 2])
        .map(|(r, i)| 10.0 * (r * r + i * i).max(1e-30).log10())
        .collect();
    (db, SR / n as f32)
}

/// Energy (linear, summed over bins) between `from` and `to` hertz.
fn energy(db: &[f32], bin_hz: f32, from: f32, to: f32) -> f32 {
    db.iter()
        .enumerate()
        .filter(|(i, _)| {
            let hz = *i as f32 * bin_hz;
            hz >= from && hz < to
        })
        .map(|(_, d)| 10f32.powf(d / 10.0))
        .sum()
}

/// A 20 Hz sine on pitch, a semitone deep, on a note at C5: the note's
/// energy is in the vibrato's sidebands, within ±200 Hz of it, and what is
/// left a block rate *below* it — 375 Hz down, where a source held per
/// block puts its stair's first image (below, because above it the table's
/// own second harmonic sits) — is at least 70 dB under. It measured −47.
#[test]
fn a_twenty_hertz_lfo_on_pitch_is_sidebands_not_a_comb() {
    let mut patch = a_sine();
    patch.lfos[0].rate_hz = 20.0;
    patch.lfos[0].sync = false;
    patch.lfos[0].wave = fontelle_types::LfoWave::Sine;
    patch.lfos[0].delay_s = 0.0;
    patch.lfos[0].fade_s = 0.0;
    // 9600 cents at full depth: 0.01 is 96 cents, about a semitone.
    patch.mod_matrix.routes.push(route(
        ModSource::Lfo(0),
        ModDest::LayerPitch(LayerRole::OscA as u8),
        0.01,
    ));
    let out = render(patch, 72, 2.0);
    let (db, bin) = spectrum(&out[out.len() - 65_536..]);
    let f0 = 440.0 * 2f32.powf(3.0 / 12.0);
    let vibrato = energy(&db, bin, f0 - 200.0, f0 + 200.0);
    let comb = energy(&db, bin, f0 - 450.0, f0 - 300.0);
    let ratio_db = 10.0 * (comb / vibrato).log10();
    assert!(
        ratio_db < -70.0,
        "the block rate's comb is {ratio_db:.1} dB against the vibrato"
    );
}

/// A 5 ms envelope — 2 ms up, 3 ms down to nothing — on the voice's amp
/// at −24 dB: the dip is heard. Read once a block it never reaches its
/// peak, because 2 ms falls between the reads at 0 and 2.7 ms.
#[test]
fn a_five_millisecond_envelope_on_a_gain_is_heard() {
    let mut patch = a_sine();
    patch.envelopes[1].delay_s = 0.0;
    patch.envelopes[1].attack_s = 0.002;
    patch.envelopes[1].hold_s = 0.0;
    patch.envelopes[1].decay_s = 0.003;
    patch.envelopes[1].sustain_level = 0.0;
    patch.envelopes[1].attack_shape = 0.0;
    patch.envelopes[1].decay_shape = 0.0;
    patch
        .mod_matrix
        .routes
        .push(route(ModSource::Envelope(1), ModDest::Amp, -1.0));
    // A high note, so the peak per half-millisecond is the amplitude.
    let out = render(patch, 108, 0.05);
    let window = (SR * 0.0005) as usize;
    let peaks: Vec<f32> = out
        .chunks(window)
        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect();
    let steady = peaks[peaks.len() - 10..]
        .iter()
        .fold(0.0f32, |m, p| m.max(*p));
    assert!(steady > 0.01, "the note sounds: {steady}");
    // At 2 ms the envelope is at its peak and the gain is 24 dB down.
    let at_2ms = peaks[4] / steady;
    let dip_db = 20.0 * at_2ms.log10();
    assert!(
        dip_db < -20.0,
        "at 2 ms the gate is {dip_db:.1} dB down; the envelope's peak was missed"
    );
    // And it has let go again by 6 ms.
    let at_6ms = peaks[12] / steady;
    assert!(
        at_6ms > 0.7,
        "at 6 ms the gate has let go: {at_6ms:.2} of steady"
    );
}

/// A macro stepped between blocks: the layer's gain arrives over the
/// modulation step, not in one sample. A gain that jumps 24 dB between
/// two samples is a click on every knob turn and every automation point.
#[test]
fn a_gain_change_ramps_between_steps() {
    let mut patch = a_sine();
    patch.mod_matrix.routes.push(route(
        ModSource::Macro(0),
        ModDest::LayerGain(LayerRole::OscA as u8),
        0.25, // 24 dB at full
    ));
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    // A note high enough that a sample-to-sample amplitude ratio can be
    // read off the waveform: 16 kHz is three samples a cycle.
    sampler.trigger(NoteTrigger::new(127, 100).with_fine_pitch(400));
    let mut l = [0.0f32; BLOCK];
    let mut r = [0.0f32; BLOCK];
    for _ in 0..8 {
        sampler.render(&store, &mut [&mut l, &mut r]);
    }
    let before = l.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    sampler.set_patch_param("patch/macro[0]", 1.0);
    sampler.render(&store, &mut [&mut l, &mut r]);
    let after = l[BLOCK - 16..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        after > before * 10.0,
        "the macro is heard: {before} -> {after}"
    );
    // The first samples of the block are on the way there: none of the
    // first four is the full new level, and each is at least where the
    // last one was.
    let peak_in = |from: usize, to: usize| l[from..to].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let first = peak_in(0, 3);
    assert!(
        first < after * 0.6 && first > before * 0.9,
        "the first samples ramp: {before} -> {first} -> {after}"
    );
}

/// The one thing the rate must not change: a patch with no route that
/// moves within a block renders as it did, sample for sample.
#[test]
fn an_unmodulated_patch_renders_as_it_did() {
    let patch = a_sine();
    let out = render(patch.clone(), 60, 0.1);
    let again = render(patch, 60, 0.1);
    assert_eq!(out, again);
    assert!(out.iter().any(|s| s.abs() > 0.1));
}

