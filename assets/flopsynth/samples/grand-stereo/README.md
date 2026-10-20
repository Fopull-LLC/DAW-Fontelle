# The factory Grand Piano's stereo recordings

Ninety recordings of a **Yamaha C5**: every key it was sampled at (the bottom
A and each minor third up to C8, thirty keys) at three strengths, `soft/`,
`medium/` and `hard/`, in stereo as its two microphones heard them, at
48 kHz, as Ogg Vorbis. They are what the factory **Grand Piano** preset plays
(`fontelle_core::factory_samples`, sets `grand-left` and `grand-right`); they
are compiled into the binary, so nothing here is read at run time.

## Credit and licence

These are cut from the **Salamander Grand Piano** by **Alexander Holm**,
published under the
[Creative Commons Attribution 3.0](http://creativecommons.org/licenses/by/3.0/)
licence, from the SFZ+FLAC edition (V3+20200602) assembled by Roberto
(zenvoid.org) for the
[FreePats project](https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html).
That package's own readme says: *"The original Salamander Grand Piano was
created by Alexander Holm. Published under the terms of the Creative Commons
Attribution 3.0 license"*.

Fontelle's changes: Salamander's velocity layers 3, 7 and 14 of sixteen were
kept; each note was trimmed from just before its onset to between 1.6 and 6
seconds with a fade over its last part; each key's two channels were levelled
to lean from three decibels left in the bass to three right at the top; the
soft and medium layers were scaled to the hard layer's loudness at each key,
and everything by one gain; and the result was encoded as Ogg Vorbis
(quality 6). The recordings stay under CC BY 3.0; the credit above is the
attribution the licence asks for, and it belongs wherever these sounds go.

## Re-cutting

`cargo run -p fontelle-app --example grand_stereo_samples --release -- <SalamanderGrandPiano-SFZ+FLAC folder>/samples assets/flopsynth/samples/grand-stereo`
rewrites all three folders from the FreePats SFZ+FLAC download (needs
`ffmpeg` with libvorbis).
