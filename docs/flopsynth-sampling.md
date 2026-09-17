# The sampled shelves — grains, kits and growls (2026-09-16)

> *"use flopsynths new sampling features to make a variety of new complex
> presets that can be experimental, synthy, modulating, instruments,
> percussion kits, growls, dubstep sounds, etc. ... if you think we can
> expand the features any way to make our synth even more awesome and
> genuinely stand up to other synths like omnisphere and serum we should do
> that."* — Ty, 2026-09-16

Built the evening `v0.6.0` shipped, tests-first, on top of that day's
sample oscillator (`docs/flopsynth-bridge.md` §1). **Shipped as `v0.7.0`**
on 2026-09-17, after the audit in §6 — Ty: *"these sound great release it
after you audit existing sounds for how you could make them better with
the new features ... make sure all the presets are up to par then you can
release."* Every claim below names its test.

---

## 1. What the sample source could not do, and can now

The v0.6.0 sample source played a recording once or round a forward loop —
a sampler. What makes a recording worth having *in a synthesiser* is the
ways of reading it a tape head has not got. Five now, on the card's `loop`
chooser (`fontelle_dsp::SampleLoop`, `tests/synth_sample_modes.rs`):

| mode | what it is |
| --- | --- |
| **Once** | as before: the recording's own decay is the note's. |
| **Loop** | round the loop points, with the 5 ms crossfade at the seam. |
| **Bounce** | back and forth between the loop points — the head turns round rather than jumping, so a loop that never lands on its own cycle has no seam at all (a ramp bounced has no step in it). |
| **Reverse** | backwards, once. The start knob counts from the *end*, so a knob at rest plays the whole recording backwards and a knob part way skips the tail — the tail of a grand is its quietest two seconds, and every reversed row starts part way. |
| **Grains** | a cloud: four raised-cosine grains in the air, each landing where the start knob points and read at the note. The recording never ends because grains keep coming, and **a route on the start knob scans through it** — the knob is read as it moves, not latched at the note's start like the other modes. |

Two knobs stand in for the loop points while the mode is Grains: **grain**
(5–500 ms, log) and **spray** (how far either side of the knob a grain may
land, 0–1; a sprayed grain past either end is folded back in rather than
piled on the end). The picture shows the spray as the region around the
start.

### The grains are landed in phase

The thing worth knowing. Four grains that all start at the same frame but a
hop apart in time read the recording a hop apart in *phase*, and at any
grain length where the hop is an odd number of half-cycles they cancel: at
50 ms on a 440 Hz tone the cloud rendered at −104 dB. That is the metallic
comb every granular freeze has, and the usual answer is spray, which
decorrelates it into a wash.

This one knows the recording's pitch — a `SampleData` carries `root_hz` —
so each new grain is landed a fraction of a period along from the knob:
the fraction the cloud's clock has advanced, modulo the period
(`SynthState::land`). Every grain then reads the same phase at the same
moment, whatever its length: a frozen note is a note (0.40 RMS steady at
50 ms, 80 ms and 8 ms in `evenly_staggered_grains_are_a_steady_level`),
and the spray is a choice rather than a necessity. A consequence for the
tests: short grains no longer smear a *periodic* recording, so what grain
length is heard as is how fast the cloud follows the knob
(`short_grains_follow_the_knob_at_once_and_long_ones_take_their_time`).

### A zone lock

`SampleSettings::zone` (the card's **zone** chooser, on a recording with
more than one zone): play *this* zone for every key, transposed from its
own root, instead of the zone whose range holds the key. What turns one hit
of a kit into an instrument. Zones have names now (`SampleZone::name` — a
hit's, or a dropped file's stem — left out of the file when empty), and the
chooser lists them.

Addresses: `synth/sample/grain`, `synth/sample/spray`, `synth/sample/zone`
(a chooser: *any*, then the zones), on every sample oscillator
(`tests/user_sample.rs`). The `synth/sample/loop` chooser has five positions
now instead of two, so a lane written against the old two reads
differently — nobody has one, the address is a day old.

## 2. The kits: two more factory sets

`FactorySampleSet::KitStudio` and `Kit808`: every General MIDI hit of the
drum machine's Studio and 808 kits, played once at full velocity **through
the kit's own bus** (rendered through `SamplerNode`, which is what runs a
patch's chain), trimmed to the hit, faded, 32 kHz mono, 1.7 MB for the
two. `fontelle-app/examples/kit_samples.rs` cuts them;
`assets/flopsynth/samples/kit/README.md` says how. One recording per key,
each its own key alone and named as the roll names it, so a Flopsynth
preset can be a kit and a zone lock can make one hit an instrument
(`tests/factory_samples.rs`). The kit's own balance is kept — one gain per
kit — so an 808's shaker sits well under its kick, as it does.

The card's right-click menu lists all four sets over the audio folder.

## 3. Four shelves, forty-eight presets

On shelves of their own for the reason the first expansion was: a piano
remade twelve ways would collide with the twelve pianos on Keys, and the
pairwise gate is a claim *within* a shelf. Bank at **388**; every gate in
`tests/flopsynth_presets.rs` passes, and four new ones in
`tests/flopsynth_shows_off.rs` hold that every read mode, every factory
set, a recording as somebody's FM/RM modulator and a zone lock are each in
some preset.

- **Sampled Keys** — the grand as eleven other keyboards: Felt, Honky
  Tonk, Tack, Toy Grand (FM'd by a sine a nineteenth up), Grand Music Box
  (three octaves up), Piano Pad (the sustain looped), Bowed Grand
  (reversed), Prepared Piano (RM by a tritone), Cinema Piano, Pianotron,
  Harpsi Grand, Sub Piano.
- **Grains & Clouds** — Grand Cloud, Frozen Note (env 2 walks the knob
  through the note's own history), Scan Pad (a triangle LFO sweeps it),
  Cymbal Wash, 808 Freeze (the kick's tone frozen as a sub), Snare Sheet,
  Glass Grains (12 ms grains two octaves up), Reverse Grand, Bounce Piano
  (the strike bounced at ten a second), Tape Stop Cloud, Ghost Choir (the
  cloud through the formant filter), Bongo Drone.
- **Kits & Hits** — Studio Kit, 808 Kit, Crunch Kit (pitched a fifth down
  like a sampler from 1988), Reverse Kit, FM Kit, Gated Kit, Buzz Roll Kit
  (bounced); and the hits — 808 Sub, Tom Melody, Cowbell Keys, Snare
  Riser, Hat Arp, Conga Choir.
- **Growls & Screams** — the thing no other synth's growl has is a
  **recording as the modulator**: Piano Growl (the growl table FM'd by the
  grand, so the tearing follows the piano's decay), Snarl (a Reese RM'd by
  the frozen 808), Metal Throat (a saw FM'd by a frozen ride), Choir
  Growl, Kick Roar (the frozen 808 FM'd by a saw); and the growl's own
  vocabulary — Talk Grains, Wub, Yoi Scream, Robot Vox, Rip Bass, Tearout,
  Grain Scream.

## 4. What voicing them taught, in the order it cost time

- **A one-shot under a legato take-over keeps its head.** The second note
  of a slide on `808 Sub` was the kick's tail at the new pitch
  (`tests/legato_notes.rs` caught it). `Build::mono_retrig` is
  `RetriggerMode::Mono` with a glide: every note is a kick, the glide is
  still the slide. Grain clouds are fine under legato — they never end.
- **A hit is all peak.** Four hats struck on the same sample are four
  peaks on top of each other, twice full scale at velocity 127 — and a
  ladder's drive is a gain, not a limiter. `Random → OscPosition` on a
  recording is a start a few milliseconds different each note, which is
  both a hat player's and inside full scale.
- **The `bass()` archetype's sub goes around the filter** (the memory
  note said so): three growls read as their sub and nothing else until the
  sub came down or went. `Robot Vox` with Quantise at 0.85 was *silent* —
  two steps a cycle land on the Vowel table's zero crossings.
- **Reverse from the very end is silence for two seconds** on a grand;
  from a kit hit it is fine. The reversed rows start part way back.
- **Grains over a hit's tail are quiet.** Spray 1.0 over a snare mostly
  lands in its decay; the sheet is sprayed 0.2 around its body.

## 5. Open

- The chooser clips a single-cell option past about five characters
  ("Grains", "Reverse" — but "Ladder" and "Bypass" already did); the
  `WIDE_CHOICE` rule counts nine. A rule that measured the text would
  fix all of them.
- Ty has heard the shelves (*"these sound great"*); the twenty-two
  re-routed rows and the five rebuilt ones in §6 are measured, not yet
  listened to. `cargo run --release -- --play-flopsynth "<name>"`.
- Grain **density** and a modulatable grain length are the two granular
  knobs Omnisphere has that this does not; four grains is fixed, and the
  window shape is always a raised cosine.

## 6. The audit of the existing bank (2026-09-17)

> *"pan flute sounds very noisy right now it just sounds like noise and air
> and a faint wave in the background."*

`examples/preset_audit.rs` reads every preset at C3, C4 and C5: tone
against noise, level across the keyboard, and which oscillators sit on the
Init patch's **serial** route while Filter 2 is enabled. That last column
was the Pan Pipe: its sine went through Filter 1 *and then* Filter 2, and
Filter 2 was the breath's 1.8 kHz band-pass at resonance 1.2 — the
fundamental twenty-three decibels down at C3, the breath ringing in the
resonance. `tests/wind_breath.rs` measures at A4, where enough tone
survives a band-pass to pass. The organ shelf had the identical fault on
2026-09-13; this time it was **twenty-two rows**: the four flutes, Melodica,
Sitar, Slap (a bass through a 4 kHz high-pass — all thumb, no bass), Ice
Field (a pad through a 5 kHz one), and the whole `choir()` family, whose
fundamentals were thirty decibels down through the breath's 3 kHz band.

Fixed in the archetype and the rows (tone to F1, breath on F2), each
re-trimmed with the probe, and held by `tests/tone_route.rs`: no audible
oscillator on the serial route while Filter 2 is a band-pass or a high-pass
at 200 Hz or over, with an allow-list that names why (Muted Trumpet's mute,
Ukulele's body, Pianotron's tape band) and is itself checked. Four pairs
that had been telling apart by what the band-pass left then collided and
were separated on what they are: Shakuhachi is a triangle with more breath
and a slower start, Melodica a narrow pulse (a free reed is asymmetric),
Sitar's jawari layer louder, Pianotron a tape with a band and a fade. The
Flute's breath came up five decibels, because with its tone back it read
cleaner than a flute. Cliffs: Oboe's throat and Dusty Rhodes' corner are
keyed now (fifteen and eighteen decibels down at C5 before).

**Better with the new features.** Five rows whose instrument *is* a struck
or plucked string are the sampled grand now, where they were tables: Harp
and Pizzicato and Pizz Section (the recording from thirty milliseconds in —
past the hammer — is a plucked string), Celesta (the hard strike two
octaves up, high-passed to the plate), and the Electric Grand (real strings
under a pickup's band, driven a little, through the chorus every CP-70 had).
