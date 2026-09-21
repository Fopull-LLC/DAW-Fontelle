//! MPE (`docs/flopsynth-next.md` §4.2, phase 5): per-note pressure, slide
//! and bend arriving after the note as `NoteMod`s. A voice holding a
//! note's own pressure reads it where `Aftertouch` used to read the
//! channel's; a note's bend moves that note alone, over the patch's MPE
//! range; a slide is the note's `NoteModY`, live now rather than fixed at
//! the note-on.

use fontelle_core::flopsynth::{LayerRole, flopsynth_init};
use fontelle_core::{
    Curve, ModDest, ModRoute, ModSource, NoteMod, NoteTrigger, Patch, PrepareContext, SampleStore,
    Sampler, Source, patch_params,
};
use fontelle_dsp::{SynthSource, WavetableId};

const SR: f32 = 48_000.0;

fn a_sine() -> Patch {
    let mut patch = flopsynth_init();
    for (index, layer) in patch.layers.iter_mut().enumerate() {
        let Source::Synth(osc) = &mut layer.source else {
            continue;
        };
        if index == LayerRole::OscA as usize {
            osc.source = SynthSource::Table(WavetableId::SubSine);
            osc.unison.voices = 1;
            osc.random_phase = false;
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
    patch.envelopes[0].release_s = 0.01;
    patch.mod_matrix.routes.clear();
    patch
}

fn route(source: ModSource, destination: ModDest, depth: f32) -> ModRoute {
    ModRoute {
        source,
        destination,
        depth,
        curve: Curve::Linear,
        via: None,
        bypass: false,
        invert: false,
    }
}

fn sampler(patch: Patch) -> Sampler {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler
}

/// `frames` of the left channel, in 512-frame blocks, with the channel's
/// pressure at `aftertouch`.
fn render(sampler: &mut Sampler, frames: usize, aftertouch: f32) -> Vec<f32> {
    let store = SampleStore::new();
    sampler.set_aftertouch(aftertouch);
    let mut out = Vec::with_capacity(frames);
    let mut done = 0;
    while done < frames {
        let n = 512.min(frames - done);
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        sampler.render(&store, &mut [&mut l, &mut r]);
        out.extend_from_slice(&l);
        done += n;
    }
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// The strongest line in `samples`, in hertz, by zero crossings of a
/// single sine.
fn pitch_hz(samples: &[f32]) -> f32 {
    let crossings = samples
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    crossings as f32 * SR / samples.len() as f32
}

#[test]
fn a_notes_own_pressure_is_read_where_the_channels_used_to_be() {
    let mut patch = a_sine();
    // Aftertouch to the layer's gain: pressure is loudness.
    patch.mod_matrix.routes.push(route(
        ModSource::Aftertouch,
        ModDest::LayerGain(LayerRole::OscA as u8),
        // A quarter of the destination's 96 dB: +24 at full pressure.
        0.25,
    ));
    let mut sampler = sampler(patch);
    sampler.trigger(NoteTrigger::new(60, 100));
    // Nothing pressed: the channel's nought.
    let quiet = peak(&render(&mut sampler, 4_800, 0.0)[2_400..]);
    // The note's own pressure, full.
    sampler.note_mod(60, 0, NoteMod::pressure(127));
    let pressed = peak(&render(&mut sampler, 4_800, 0.0)[2_400..]);
    assert!(pressed > quiet * 4.0, "{quiet} → {pressed}");
    // The channel's pressure, with the note's own present, is not read:
    // the note's wins.
    sampler.note_mod(60, 0, NoteMod::pressure(0));
    let own = peak(&render(&mut sampler, 4_800, 1.0)[2_400..]);
    assert!(
        (own - quiet).abs() < quiet * 0.2,
        "the note's nought wins: {own} vs {quiet}"
    );
    // And a note that never had one reads the channel's, as ever.
    sampler.note_off(60, 0);
    render(&mut sampler, 4_800, 0.0);
    sampler.trigger(NoteTrigger::new(60, 100));
    let channels = peak(&render(&mut sampler, 4_800, 1.0)[2_400..]);
    assert!(channels > quiet * 4.0, "{channels}");
}

#[test]
fn two_notes_carry_two_pressures() {
    let mut patch = a_sine();
    patch.mod_matrix.routes.push(route(
        ModSource::Aftertouch,
        ModDest::LayerGain(LayerRole::OscA as u8),
        -0.625,
    ));
    let mut sampler = sampler(patch);
    sampler.trigger(NoteTrigger::new(48, 100));
    sampler.trigger(NoteTrigger::new(72, 100));
    // Press the low one flat, leave the high one.
    sampler.note_mod(48, 0, NoteMod::pressure(127));
    let out = render(&mut sampler, 9_600, 0.0);
    let tail = &out[4_800..];
    // What is left is the high note: its pitch, not the low one's.
    let hz = pitch_hz(tail);
    let high = 440.0 * 2f32.powf((72.0 - 69.0) / 12.0);
    assert!(
        (hz / high - 1.0).abs() < 0.05,
        "{hz} Hz, the high note alone"
    );
}

#[test]
fn a_notes_bend_moves_that_note_alone_over_the_mpe_range() {
    let patch = a_sine();
    assert_eq!(
        patch.voice_config.mpe_bend_semitones, 48.0,
        "MPE's default range"
    );
    let mut sampler = sampler(patch);
    sampler.trigger(NoteTrigger::new(60, 100));
    // A quarter of the wheel up: twelve semitones of forty-eight.
    sampler.note_mod(60, 0, NoteMod::bend(2_048));
    let out = render(&mut sampler, 9_600, 0.0);
    let hz = pitch_hz(&out[4_800..]);
    let expected = 440.0 * 2f32.powf((72.0 - 69.0) / 12.0);
    assert!(
        (hz / expected - 1.0).abs() < 0.03,
        "{hz} Hz against {expected}"
    );
    // Two notes, one bent: the other stays.
    let mut sampler = self::sampler(a_sine());
    sampler.trigger(NoteTrigger::new(48, 100));
    sampler.trigger(NoteTrigger::new(72, 100).in_context(9));
    sampler.note_mod(48, 0, NoteMod::bend(-8_192));
    let out = render(&mut sampler, 9_600, 0.0);
    // The low one has gone four octaves down, under the measure; the high
    // one is still there at its own pitch.
    let tail = &out[4_800..];
    let hz = pitch_hz(tail);
    let high = 440.0 * 2f32.powf((72.0 - 69.0) / 12.0);
    // Zero crossings count the high note's and the (now very slow) low
    // one's together; within a few percent of the high note's.
    assert!((hz / high - 1.0).abs() < 0.08, "{hz} Hz against {high}");
}

#[test]
fn a_slide_is_the_notes_mod_y_live() {
    let mut patch = a_sine();
    patch.mod_matrix.routes.push(route(
        ModSource::NoteModY,
        ModDest::LayerGain(LayerRole::OscA as u8),
        // A quarter of the destination's 96 dB: +24 at full pressure.
        0.25,
    ));
    let mut sampler = sampler(patch);
    sampler.trigger(NoteTrigger::new(60, 100).with_mod_y(0));
    let low = peak(&render(&mut sampler, 4_800, 0.0)[2_400..]);
    sampler.note_mod(60, 0, NoteMod::slide(127));
    let high = peak(&render(&mut sampler, 4_800, 0.0)[2_400..]);
    assert!(high > low * 4.0, "{low} → {high}");
    // And X, for whatever sends it.
    let mut patch = a_sine();
    patch.mod_matrix.routes.push(route(
        ModSource::NoteModX,
        ModDest::LayerGain(LayerRole::OscA as u8),
        // A quarter of the destination's 96 dB: +24 at full pressure.
        0.25,
    ));
    let mut sampler = self::sampler(patch);
    sampler.trigger(NoteTrigger::new(60, 100));
    let low = peak(&render(&mut sampler, 4_800, 0.0)[2_400..]);
    sampler.note_mod(60, 0, NoteMod::mod_x(127));
    let high = peak(&render(&mut sampler, 4_800, 0.0)[2_400..]);
    assert!(high > low * 4.0, "{low} → {high}");
}

#[test]
fn a_mod_for_a_note_nobody_is_playing_does_nothing() {
    let mut sampler = sampler(a_sine());
    sampler.trigger(NoteTrigger::new(60, 100));
    sampler.note_mod(64, 0, NoteMod::bend(8_191));
    sampler.note_mod(60, 5, NoteMod::bend(8_191));
    let out = render(&mut sampler, 9_600, 0.0);
    let hz = pitch_hz(&out[4_800..]);
    let expected = 440.0 * 2f32.powf((60.0 - 69.0) / 12.0);
    assert!(
        (hz / expected - 1.0).abs() < 0.03,
        "{hz} Hz: another key's bend, another context's"
    );
}

/// The range is the patch's, addressable, and absent from the file at its
/// default (§0 rule 7).
#[test]
fn the_mpe_range_is_an_address_and_writes_nothing_at_its_default() {
    let mut patch = a_sine();
    let offered = fontelle_core::flopsynth::addresses(&patch);
    assert!(offered.iter().any(|a| a == "patch/voice/mpe_bend"));
    let data = patch.to_data(&Default::default()).unwrap();
    assert!(!data.body.to_string().contains("mpe_bend"));
    assert!(patch_params::set(&mut patch, "patch/voice/mpe_bend", 0.5));
    assert!(
        (patch.voice_config.mpe_bend_semitones - 24.0).abs() < 1e-3,
        "{}",
        patch.voice_config.mpe_bend_semitones
    );
    assert!((patch_params::value(&patch, "patch/voice/mpe_bend").unwrap() - 0.5).abs() < 1e-3);
    let data = patch.to_data(&Default::default()).unwrap();
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert!((back.voice_config.mpe_bend_semitones - 24.0).abs() < 1e-3);
}
