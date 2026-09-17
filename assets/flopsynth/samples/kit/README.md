# The factory kits' recordings

Seventy-two short mono recordings, one per General MIDI hit of the drum
machine's **Studio** kit (`studio/`) and **808** kit (`808/`), named as the
piano roll names the row (`Kick.wav`, `Tom-Mid-2.wav`). They are what the
factory **Kits & Hits** presets play and what the growls modulate with
(`fontelle_core::factory_samples`); they are compiled into the binary, so
nothing here is read at run time.

## Provenance

These are Fontelle's own: each hit is the drum machine's voice
(`fontelle_core::drum_kit`) played once at full velocity through the kit's
bus, trimmed to the hit, faded over its last 30 ms, and written at 32 kHz.
One gain per kit, so the balance between the hits is the kit's own. No
outside material and no licence beyond the repository's.

## Re-cutting

`cargo run -p fontelle-app --example kit_samples --release -- assets/flopsynth/samples/kit`
rewrites both folders from the drum machine. The file list in
`factory_samples.rs` is in `GM_DRUM_MAP`'s order and must match.
