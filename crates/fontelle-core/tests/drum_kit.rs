//! The drum machine as a `Patch`: a kit is layers, one per key.
//!
//! > *"a built in general purpose drum machine ... you can play them all in the
//! > piano roll all labeled and stuff should have lots of presets for different
//! > styles and genres of kits. should be encorperated like any other vst would
//! > be."*
//!
//! *"Like any other vst"* is the design constraint that decides everything
//! here. It is not a special path through the engine: a kit is an ordinary
//! `Patch` whose layers happen to carry `Source::Drum`, so it saves like a
//! patch, loads like a patch, is automated like a patch, and every mechanism
//! this program already has — the key map, the mod matrix, the two filters, the
//! amp envelope, the mixer routing — works on it without being told.
//!
//! The keys are **General MIDI's** drum map, which is what makes a drum MIDI
//! file dropped on the arrangement land on the right hits.

use fontelle_core::{DrumKitStyle, GM_DRUM_MAP, Patch, Source, drum_kit, drum_slots};
use fontelle_dsp::DrumModel;

// ------------------------------------------------------------- the map ---

#[test]
fn the_kit_is_laid_out_on_the_general_midi_drum_map() {
    // Not an arbitrary run of keys. GM is what every drum MIDI file, every
    // pad controller and every other drum plugin agrees on, so a part written
    // anywhere else plays here and a part written here plays anywhere else.
    let slots = drum_slots(DrumKitStyle::Studio);
    assert_eq!(slots.len(), GM_DRUM_MAP.len());
    // The three that matter, on the keys everybody expects.
    let key_of = |name: &str| {
        slots
            .iter()
            .find(|slot| slot.name == name)
            .unwrap_or_else(|| panic!("no {name} in the kit"))
            .key
    };
    assert_eq!(key_of("Kick"), 36);
    assert_eq!(key_of("Snare"), 38);
    assert_eq!(key_of("Closed Hat"), 42);
    assert_eq!(key_of("Open Hat"), 46);
    assert_eq!(key_of("Crash"), 49);
    assert_eq!(key_of("Ride"), 51);
}

#[test]
fn every_slot_has_a_key_of_its_own_and_they_run_in_order() {
    // Two hits on one key is one hit you can never play, and the key map
    // labels whichever came first — a bug that shows up as a name on the
    // wrong row rather than as an error.
    let slots = drum_slots(DrumKitStyle::Studio);
    for pair in slots.windows(2) {
        assert!(
            pair[1].key > pair[0].key,
            "{} and {} are out of order or share a key",
            pair[0].name,
            pair[1].name
        );
    }
}

#[test]
fn every_slot_is_named_because_the_naming_is_the_point() {
    // *"you can play them all in the piano roll all labeled"* — an unnamed
    // slot is a row in the roll that says nothing, which is the complaint the
    // key map was written for in the first place.
    for style in DrumKitStyle::ALL {
        for slot in drum_slots(style) {
            assert!(!slot.name.is_empty(), "{style:?} has an unnamed slot");
            assert!(
                slot.name.chars().next().unwrap().is_uppercase(),
                "{:?} is not written the way a label is",
                slot.name
            );
        }
    }
}

// ------------------------------------------------------------ the patch ---

#[test]
fn a_kit_is_an_ordinary_patch_of_one_layer_per_key() {
    let patch = drum_kit(DrumKitStyle::Studio);
    let slots = drum_slots(DrumKitStyle::Studio);
    assert_eq!(patch.layers.len(), slots.len());
    for (layer, slot) in patch.layers.iter().zip(&slots) {
        assert_eq!(
            layer.key_range,
            (slot.key, slot.key),
            "{} covers more than its own key",
            slot.name
        );
        assert_eq!(layer.vel_range, (0, 127), "{} is velocity split", slot.name);
        assert!(matches!(layer.source, Source::Drum(_)));
    }
}

#[test]
fn a_kits_layers_carry_the_voices_the_kit_asked_for() {
    let patch = drum_kit(DrumKitStyle::EightOhEight);
    let slots = drum_slots(DrumKitStyle::EightOhEight);
    for (layer, slot) in patch.layers.iter().zip(&slots) {
        let Source::Drum(voice) = &layer.source else {
            panic!("{} is not a drum", slot.name)
        };
        assert_eq!(*voice, slot.voice, "{} lost its settings", slot.name);
    }
}

#[test]
fn a_kit_arrives_able_to_play_rather_than_waiting_for_a_file() {
    // The one thing that separates it from the sampler and the soundfont
    // player: there is nothing to load. A kit on a fresh install with no bank
    // configured still makes a beat.
    let patch = drum_kit(DrumKitStyle::Studio);
    assert!(!patch.layers.is_empty());
    assert!(
        patch
            .layers
            .iter()
            .all(|l| matches!(l.source, Source::Drum(_))),
        "a kit must reference no files at all"
    );
}

#[test]
fn a_kits_amp_envelope_does_not_shorten_its_hits() {
    // The hit's own decay is the length of the sound. A patch envelope that
    // closed before it would cut every drum off, and it is the sort of thing
    // that reads as "the kick sounds wrong" rather than as an envelope.
    let patch = drum_kit(DrumKitStyle::Studio);
    let env = patch.envelopes.first().expect("an amp envelope");
    assert!(
        env.sustain_level >= 1.0,
        "the kit's hits are gated at {}",
        env.sustain_level
    );
    assert!(env.decay_s <= 0.001, "the amp envelope shapes the hits");
    assert!(env.attack_s <= 0.001, "the amp envelope softens the hits");
}

// ----------------------------------------------------------- the styles ---

#[test]
fn there_are_a_lot_of_them_and_every_one_is_named() {
    // *"lots of presets for different styles and genres of kits."*
    assert!(
        DrumKitStyle::ALL.len() >= 16,
        "only {} kits",
        DrumKitStyle::ALL.len()
    );
    let mut names: Vec<&str> = DrumKitStyle::ALL.iter().map(|s| s.label()).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "two kits share a name");
    for name in names {
        assert!(!name.is_empty());
    }
}

#[test]
fn every_style_fills_every_slot() {
    // A kit with a hole in it is a key that is labelled and silent, which is
    // worse than one that is neither.
    let studio = drum_slots(DrumKitStyle::Studio);
    for style in DrumKitStyle::ALL {
        let slots = drum_slots(style);
        assert_eq!(slots.len(), studio.len(), "{style:?} is missing hits");
        for (slot, reference) in slots.iter().zip(&studio) {
            assert_eq!(
                slot.key, reference.key,
                "{style:?} moved {}",
                reference.name
            );
            assert_eq!(slot.name, reference.name, "{style:?} renamed a hit");
        }
    }
}

#[test]
fn every_style_is_actually_a_different_kit() {
    // Twenty rows in a menu that all sound the same is twenty rows of
    // disappointment. Compared on the kick, the snare and the closed hat,
    // which is what anybody auditions first.
    let voice_of = |style: DrumKitStyle, name: &str| {
        drum_slots(style)
            .into_iter()
            .find(|slot| slot.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .voice
    };
    for (a, b) in DrumKitStyle::ALL
        .iter()
        .zip(DrumKitStyle::ALL.iter().skip(1))
    {
        let same = ["Kick", "Snare", "Closed Hat"]
            .iter()
            .all(|name| voice_of(*a, name) == voice_of(*b, name));
        assert!(!same, "{a:?} and {b:?} are the same kit");
    }
}

#[test]
fn every_hit_of_every_kit_is_settings_a_person_could_have_dialled_in() {
    // A style table is easy to get wrong by a factor of ten, and a tune of
    // 5000 Hz on a kick is a bug that sounds like a bug and is hard to trace
    // back to a row in a table.
    for style in DrumKitStyle::ALL {
        for slot in drum_slots(style) {
            let v = &slot.voice;
            let what = format!("{style:?}'s {}", slot.name);
            assert!(
                (20.0..=16_000.0).contains(&v.tune_hz),
                "{what}: {} Hz",
                v.tune_hz
            );
            assert!(
                (0.005..=8.0).contains(&v.decay_s),
                "{what}: {} s",
                v.decay_s
            );
            assert!(
                (20.0..=20_000.0).contains(&v.tone_hz),
                "{what}: {} Hz",
                v.tone_hz
            );
            assert!((0.0..=1.0).contains(&v.noise), "{what}: noise {}", v.noise);
            assert!((0.0..=1.0).contains(&v.snap), "{what}: snap {}", v.snap);
            assert!((0.0..=1.0).contains(&v.drive), "{what}: drive {}", v.drive);
            assert!(
                (-24.0..=12.0).contains(&v.gain_db),
                "{what}: {} dB",
                v.gain_db
            );
            assert!(
                (0.0..=48.0).contains(&v.bend_semitones),
                "{what}: bend {}",
                v.bend_semitones
            );
        }
    }
}

#[test]
fn the_models_are_the_ones_the_names_promise() {
    // A hat that is secretly a kick is a kit that cannot be edited, because
    // every knob on the panel would do the wrong thing.
    let slots = drum_slots(DrumKitStyle::Studio);
    let model_of = |name: &str| {
        slots
            .iter()
            .find(|slot| slot.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .voice
            .model
    };
    assert_eq!(model_of("Kick"), DrumModel::Kick);
    assert_eq!(model_of("Snare"), DrumModel::Snare);
    assert_eq!(model_of("Closed Hat"), DrumModel::ClosedHat);
    assert_eq!(model_of("Open Hat"), DrumModel::OpenHat);
    assert_eq!(model_of("Clap"), DrumModel::Clap);
    assert_eq!(model_of("Crash"), DrumModel::Cymbal);
    assert_eq!(model_of("Cowbell"), DrumModel::Cowbell);
}

// --------------------------------------------------------- round tripping ---

#[test]
fn a_kit_survives_being_saved_and_read_back() {
    // *"like any other vst"* means it is in `project.json` like anything else.
    // A drum layer references no file, so it needs no relinking and no
    // provenance — which is the one place it is *simpler* than a sampler.
    let patch = drum_kit(DrumKitStyle::Trap);
    let data = patch
        .to_data(&Default::default())
        .expect("a kit is writable");
    let back = Patch::from_data(&data, |_| None).expect("and readable");
    assert!(back.unresolved.is_empty(), "a kit asked for a file");
    assert_eq!(back.patch.layers.len(), patch.layers.len());
    for (a, b) in back.patch.layers.iter().zip(&patch.layers) {
        assert_eq!(a.source, b.source, "a hit changed on the way through");
        assert_eq!(a.key_range, b.key_range);
    }
}

#[test]
fn a_saved_kit_names_no_samples() {
    let patch = drum_kit(DrumKitStyle::Studio);
    let data = patch.to_data(&Default::default()).unwrap();
    assert!(
        fontelle_core::referenced_samples(&data)
            .expect("readable")
            .is_empty(),
        "a synthesised kit must reference nothing on disk"
    );
}

// ------------------------------------------------------ it actually sounds ---
//
// A kit that is stored and not sounded is exactly the state `Source::Oscillator`
// was in before `oscillator_layers.rs` was written, and it looks identical from
// the outside: the patch is right, the panel draws, and the keyboard is silent.
// So these are measured off the rendered block.

use fontelle_core::{NoteTrigger, PrepareContext, SampleStore, Sampler};

const SR: f32 = 48_000.0;

fn ready(patch: Patch) -> Sampler {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });
    sampler
}

fn render(sampler: &mut Sampler, store: &SampleStore, frames: usize) -> Vec<f32> {
    const BLOCK: usize = 128;
    let mut out = Vec::with_capacity(frames);
    let mut scratch = vec![0.0f32; BLOCK];
    let mut left = frames;
    while left > 0 {
        let n = left.min(BLOCK);
        scratch[..n].fill(0.0);
        sampler.render(store, &mut [&mut scratch[..n]]);
        out.extend_from_slice(&scratch[..n]);
        left -= n;
    }
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

/// One hit of `style` on `key`, half a second of it.
fn hit(style: DrumKitStyle, key: u8) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = ready(drum_kit(style));
    sampler.trigger(NoteTrigger::new(key, 100));
    render(&mut sampler, &store, (SR * 0.5) as usize)
}

#[test]
fn every_hit_of_every_kit_makes_a_sound() {
    // The whole of *"you can play them all in the piano roll"*: thirty-six
    // keys times twenty-two kits, and every one of them has to sound. A hit
    // that is silent is a labelled row that does nothing, which is worse than
    // no label at all.
    for style in DrumKitStyle::ALL {
        for slot in drum_slots(style) {
            let out = hit(style, slot.key);
            assert!(
                peak(&out) > 0.005,
                "{style:?}'s {} on key {} is silent",
                slot.name,
                slot.key
            );
            assert!(
                out.iter().all(|s| s.is_finite()),
                "{style:?}'s {} produced a NaN",
                slot.name
            );
        }
    }
}

#[test]
fn a_key_outside_the_kit_is_silent_rather_than_wrong() {
    // The keys above and below the map have no layer covering them, so the
    // voice starts with nothing in it — which is the fact the key map greys
    // those rows out for.
    let store = SampleStore::new();
    let mut sampler = ready(drum_kit(DrumKitStyle::Studio));
    sampler.trigger(NoteTrigger::new(20, 100));
    let out = render(&mut sampler, &store, 4096);
    assert_eq!(
        peak(&out),
        0.0,
        "key 20 is not in the kit and must be silent"
    );
}

#[test]
fn a_hit_ends_on_its_own_rather_than_waiting_for_the_note_to_be_let_go() {
    // The drum machine's one departure from every other instrument here: a
    // hit's length is its own decay. The patch's amp envelope is held open
    // behind it, so a note held for a whole bar still gives one kick and then
    // silence rather than a kick that hums until it is released.
    let store = SampleStore::new();
    let mut sampler = ready(drum_kit(DrumKitStyle::Studio));
    // Key 42 is the closed hat: about fifty milliseconds of it.
    sampler.trigger(NoteTrigger::new(42, 127));
    let out = render(&mut sampler, &store, (SR * 2.0) as usize);
    let tail = &out[(SR * 1.0) as usize..];
    assert!(
        peak(tail) < 1.0e-3,
        "the hat was still sounding a second later at {}",
        peak(tail)
    );
}

#[test]
fn a_kit_does_not_clip_when_a_whole_bar_of_it_lands_at_once() {
    // A drum part is a chord: the kick, the snare and the hat on the same
    // tick. Seven at once is more than any real part, and the sum still has
    // to stay inside full scale — the master limiter catching every downbeat
    // is a mix that sounds squashed with nothing to point at.
    let store = SampleStore::new();
    let mut sampler = ready(drum_kit(DrumKitStyle::NineOhNine));
    for key in [36, 38, 42, 46, 49, 51, 56] {
        sampler.trigger(NoteTrigger::new(key, 127));
    }
    let out = render(&mut sampler, &store, (SR * 0.5) as usize);
    assert!(
        peak(&out) <= 1.0,
        "seven hits at once peaked at {}",
        peak(&out)
    );
    assert!(peak(&out) > 0.1, "seven hits at once are inaudible");
}

#[test]
fn velocity_still_means_what_it_means_everywhere_else() {
    // A drum layer goes through the same `velocity_gain` every other source
    // does, so a ghost note is quiet without the drum machine having to know
    // what a ghost note is.
    let store = SampleStore::new();
    let loud = {
        let mut sampler = ready(drum_kit(DrumKitStyle::Studio));
        sampler.trigger(NoteTrigger::new(38, 127));
        render(&mut sampler, &store, 8192)
    };
    let soft = {
        let mut sampler = ready(drum_kit(DrumKitStyle::Studio));
        sampler.trigger(NoteTrigger::new(38, 30));
        render(&mut sampler, &store, 8192)
    };
    assert!(
        peak(&soft) < peak(&loud) * 0.6,
        "a ghost note is not quieter: {} against {}",
        peak(&soft),
        peak(&loud)
    );
}

#[test]
fn a_hat_retriggered_over_and_over_stays_the_same_size() {
    // Sixteen hats a bar is one voice reused, and a retrigger that added to
    // what was still ringing would grow without bound — the failure that
    // shows up as a drum part getting louder through a track.
    let store = SampleStore::new();
    let mut sampler = ready(drum_kit(DrumKitStyle::Studio));
    let mut peaks = Vec::new();
    for _ in 0..16 {
        sampler.trigger(NoteTrigger::new(42, 100));
        peaks.push(peak(&render(&mut sampler, &store, 1024)));
    }
    let first = peaks[0];
    for (n, p) in peaks.iter().enumerate() {
        assert!(
            (*p - first).abs() < first * 0.5 + 0.01,
            "hit {n} was {p} against a first hit of {first}"
        );
    }
}

// --------------------------------------------- they are different kits ---
//
// > *"the drumkits in the drum machine kind of all sound very similar"*
//
// Measured before this existed: every kit's closed hat had its spectral centre
// between 10 and 13 kHz and the same 25 ms length, every kick's between 80
// and 110 Hz, and LinnDrum against Rock differed by fifteen percent on one
// number. `every_style_is_actually_a_different_kit` above could not see it,
// because two tables of numbers that differ in the third decimal are "two
// kits" to `!=` and one kit to an ear. This section listens instead.

/// What an ear reads off one hit, as three logs: how long it lasts (seconds
/// to fall thirty decibels), where its weight sits (a spectral centroid in
/// hertz), and how spiky it is (peak over RMS). Logs, so that a difference is
/// a ratio — twice as long is the same distance at 50 ms as at 500.
fn ear(voice: &fontelle_dsp::DrumVoice) -> [f32; 3] {
    const SR: f32 = 48_000.0;
    let mut synth = fontelle_dsp::DrumSynth::new();
    synth.trigger(voice, SR);
    let mut out = Vec::new();
    while !synth.is_done() && out.len() < (3.0 * SR) as usize {
        out.push(synth.next_sample(voice, SR));
    }
    assert!(!out.is_empty(), "{voice:?} made no sound");
    let peak = out.iter().fold(0f32, |m, s| m.max(s.abs())).max(1e-9);

    // Thirty decibels under the peak, on five-millisecond windows.
    let win = 240;
    let mut t30 = out.len() as f32 / SR;
    let mut seen = false;
    for (i, chunk) in out.chunks(win).enumerate() {
        let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
        if rms > peak * 0.3 {
            seen = true;
        }
        if seen && rms < peak * 0.0316 {
            t30 = (i * win) as f32 / SR;
            break;
        }
    }

    // Sixty-four bins, a sixth of an octave apart, over the first 85 ms.
    let n = out.len().min(4096);
    let (mut num, mut den) = (0.0f32, 0.0f32);
    for b in 0..64 {
        let f = 40.0 * 2f32.powf(b as f32 / 7.0);
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, s) in out[..n].iter().enumerate() {
            let phase = 2.0 * std::f32::consts::PI * f * i as f32 / SR;
            re += s * phase.cos();
            im += s * phase.sin();
        }
        let mag = (re * re + im * im).sqrt();
        num += mag * f.ln();
        den += mag;
    }
    let centroid = if den > 0.0 { num / den } else { 0.0 };
    let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
    [t30.max(0.001).ln(), centroid, (peak / rms.max(1e-9)).ln()]
}

/// The three hits anybody auditions first, as the ear hears them.
fn ears_of(style: DrumKitStyle) -> Vec<[f32; 3]> {
    let slots = drum_slots(style);
    ["Kick", "Snare", "Closed Hat"]
        .iter()
        .map(|name| ear(&slots.iter().find(|s| s.name == *name).unwrap().voice))
        .collect()
}

/// How far apart two kits are: the largest ratio, over the three hits and
/// the three readings, between them. `0.35` is about forty percent — a kick
/// that lasts forty percent longer, or a hat whose centre is forty percent
/// higher, is a difference nobody has to be told about.
const APART: f32 = 0.35;

#[test]
fn every_pair_of_kits_is_audibly_apart() {
    let ears: Vec<(DrumKitStyle, Vec<[f32; 3]>)> = DrumKitStyle::ALL
        .iter()
        .map(|s| (*s, ears_of(*s)))
        .collect();
    let mut close = Vec::new();
    for (i, (a, ea)) in ears.iter().enumerate() {
        for (b, eb) in &ears[i + 1..] {
            let apart = ea
                .iter()
                .zip(eb)
                .flat_map(|(x, y)| x.iter().zip(y).map(|(p, q)| (p - q).abs()))
                .fold(0.0f32, f32::max);
            if apart < APART {
                close.push(format!("{a:?} and {b:?} are {apart:.2} apart"));
            }
        }
    }
    assert!(
        close.is_empty(),
        "kits that sound the same:\n{}",
        close.join("\n")
    );
}

#[test]
fn the_machines_have_metal_in_their_hats_and_the_acoustic_kits_do_not() {
    // The 808 hat is six square oscillators and everybody knows the sound; a
    // brushed jazz hat is not. This is the axis the report was missing.
    let hat_metal = |style: DrumKitStyle| {
        drum_slots(style)
            .into_iter()
            .find(|s| s.name == "Closed Hat")
            .unwrap()
            .voice
            .metal
    };
    for machine in [
        DrumKitStyle::EightOhEight,
        DrumKitStyle::SixOhSix,
        DrumKitStyle::SevenOhSeven,
    ] {
        assert!(
            hat_metal(machine) >= 0.5,
            "{machine:?}'s hat is {}",
            hat_metal(machine)
        );
    }
    for acoustic in [
        DrumKitStyle::Studio,
        DrumKitStyle::JazzBrushes,
        DrumKitStyle::Rock,
    ] {
        assert_eq!(hat_metal(acoustic), 0.0, "{acoustic:?}'s hat is metal");
    }
}

#[test]
fn the_eight_bit_kits_are_crushed_and_the_studio_kit_is_not() {
    let kick_crush = |style: DrumKitStyle| {
        drum_slots(style)
            .into_iter()
            .find(|s| s.name == "Kick")
            .unwrap()
            .voice
            .crush
    };
    for crushed in [
        DrumKitStyle::LinnDrum,
        DrumKitStyle::LoFi,
        DrumKitStyle::Chiptune,
    ] {
        assert!(kick_crush(crushed) > 0.0, "{crushed:?} is clean");
    }
    assert_eq!(kick_crush(DrumKitStyle::Studio), 0.0);
    assert_eq!(kick_crush(DrumKitStyle::Rock), 0.0);
}

#[test]
fn the_new_knobs_are_inside_their_ranges_in_every_kit() {
    for style in DrumKitStyle::ALL {
        for slot in drum_slots(style) {
            let v = &slot.voice;
            let what = format!("{style:?}'s {}", slot.name);
            assert!((0.0..=1.0).contains(&v.metal), "{what}: metal {}", v.metal);
            assert!((0.0..=1.0).contains(&v.crush), "{what}: crush {}", v.crush);
        }
    }
}

// ------------------------------------------------- which kit is this ---
//
// > *"not noticing much feedback for when i actually change a selection of
// > kit visually"*
//
// A preset writes the knobs and then has nothing further to say (rule 10), so
// nothing *remembers* which kit a channel holds. But the panel can *look*: a
// patch whose thirty-six hits are exactly the 808's is the 808, and the chip
// can say so. The moment one hit is touched it is nobody's kit, and no chip
// lights — which is the truth, and the same rule an effect's presets follow.

#[test]
fn every_kit_recognises_itself() {
    for style in DrumKitStyle::ALL {
        assert_eq!(DrumKitStyle::matching(&drum_kit(style)), Some(style));
    }
}

#[test]
fn a_kit_with_one_hit_touched_is_nobodys_kit() {
    let mut patch = drum_kit(DrumKitStyle::NineOhNine);
    let Source::Drum(voice) = &mut patch.layers[1].source else {
        panic!("not a drum")
    };
    voice.tune_hz += 1.0;
    assert_eq!(DrumKitStyle::matching(&patch), None);
}

#[test]
fn a_synth_is_not_a_kit_and_neither_is_an_empty_patch() {
    assert_eq!(DrumKitStyle::matching(&Patch::basic_synth()), None);
    let empty = Patch {
        layers: Vec::new(),
        ..Patch::basic_synth()
    };
    assert_eq!(DrumKitStyle::matching(&empty), None);
}
