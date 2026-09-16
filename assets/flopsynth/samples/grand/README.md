# The factory Grand Piano's recordings

Forty-six short mono recordings of a **Yamaha C5**, two per key: `soft/`
(the piano played at MIDI velocity 40) and `hard/` (at 120), the bottom A
and then every four semitones from C1 to C8. They are what the factory
**Grand Piano** preset plays (`fontelle_core::factory_samples`), crossfading
between the two by velocity; they are compiled into the binary, so nothing
here is read at run time.

## Credit and licence

These are cut from the **Salamander Grand Piano** by **Alexander Holm**
(axeldenstore at gmail dot com), published under the
[Creative Commons Attribution 3.0](http://creativecommons.org/licenses/by/3.0/)
licence, in the SF2 build assembled by Roberto (zenvoid.org) for the
[FreePats project](https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html).
Fontelle's changes: each note was played through Fontelle's own sampler,
trimmed to between 1.3 and 4 seconds with a fade at the end, resampled to
24 or 32 kHz, summed to mono, and scaled once per layer. The recordings
stay under CC BY 3.0; the credit above is the attribution the licence asks
for, and it belongs wherever these sounds go.

## Re-cutting

`cargo run -p fontelle-app --example grand_samples --release -- <SalamanderGrandPiano.sf2> assets/flopsynth/samples/grand`
rewrites both folders from the soundfont.
