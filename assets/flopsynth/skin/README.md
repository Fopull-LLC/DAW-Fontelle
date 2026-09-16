# Flopsynth skin

Textures for the bridge — Flopsynth's window drawn as the inside of a ship
(`crates/fontelle-ui/src/render/bridge.rs`). **Every surface is procedural
without them**; a file dropped in here is used in place of the procedural
one the next time the studio starts. Nothing is compiled in, so nothing
here needs a rebuild, and nothing here is required.

The window looks for this folder at `assets/flopsynth/skin/` under the
current directory (a `cargo run` from the repository), then at
`$XDG_DATA_HOME/fontelle/skin/` (`~/.local/share/fontelle/skin/`), and
`FONTELLE_SKIN_DIR=<dir>` points it anywhere else.

## Files

All PNG, any size; sRGB. Names are exact.

| file | used for | what it should be |
| --- | --- | --- |
| `hull.png` | the ground under the consoles | a **tileable** plate texture: brushed or blasted metal, dark, low contrast (the theme's tint and the bevels go over it). 256–512 px square tiles well. |
| `glass.png` | over the canopy | the windshield's own surface: faint scratches, grime at the edges, a lens flare if you like — **mostly transparent**, alpha carries it. Any aspect; stretched to the opening. |
| `knob.png` | every knob's cap | a knurled or chamfered cap seen from above, on a **transparent** background, centred, with **no pointer** — the pointer, the arc and the ring are drawn over it. 128 px square is plenty. |
| `console.png` | each card's face | a **tileable** dark panel texture — anodised, carbon, leather — under the console's bevel and nameplate. |

A file that is not a PNG, or does not decode, is skipped and the
procedural surface drawn instead; the studio's log says which.

## Where to find some

Free-to-use texture sets that fit: Poly Haven (CC0 — search *metal
plate*, *brushed metal*), ambientCG (CC0 — *Metal*, *MetalPlates*),
Kenney's UI packs (CC0) for knob caps. CC0 needs no credit; anything
CC-BY does, in `README.md`'s credits, if it ships.
