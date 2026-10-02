# Themes

A Fontelle theme is **one file**, `<name>.fontelletheme`: JSON, carrying its
colours, sizes, fonts, pictures and moving backdrops inside it, so one file
shares the whole look. Settings ▸ Appearance chooses, imports and saves them;
your own live in the `themes` folder beside `settings.json`.

The format is `crates/fontelle-ui/src/theme/mod.rs` (`THEME_FORMAT_VERSION`
11). Every palette token is required — a theme missing one is refused rather
than drawn with a hole in it — and an unknown key is refused by name. A file
from a newer Fontelle is refused by its version.

## The file

```json
{
  "format_version": 11,
  "name": "Tide",
  "description": "One line, shown with the name.",
  "palette": { "window": "#0c1018", "panel": "#141a26c4", "...": "..." },
  "metrics": { "corner_radius": 8.0, "border_width": 1.0, "...": 0 },
  "font": { "family": "Open Sans", "size": 13.0, "line_height": 1.35,
            "display": "Mochiy Pop One" },
  "fonts": [ { "data": "<base64 of a .ttf or .otf>" } ],
  "backdrops": {
    "window": [
      { "kind": "shader", "shader": "builtin:rain", "speed": 1.0 },
      { "kind": "image", "image": "<base64 PNG/JPEG>", "opacity": 0.4,
        "fit": "cover", "anchor": [1.0, 1.0] }
    ],
    "transport": [
      { "kind": "gradient", "stops": [[0.0, "#39c5bb38"], [1.0, "#ff4fa338"]],
        "angle": 0.0, "blend": "add" }
    ]
  }
}
```

- Colours are `#rrggbb` or `#rrggbbaa`. A panel colour with alpha is a
  see-through panel: the window's backdrop shows through it. Keep panels at
  `bf`–`f2` (75–95 %) so the words and the roll's grid stay readable.
- `font.family` and `font.display` name a face by its own family name — one
  the machine has, or one in `fonts`. `display` is for titles (the editor
  panel's heading, the start menu).
- `backdrops` has a stack of layers per section: `window` (under
  everything), `transport`, `channels`, `browser`, `arrangement`, `roll`,
  `mixer`. Layers draw bottom to top over the section's ground and under
  everything on it.

### Layers

| `kind` | Keys (defaults) |
|---|---|
| `image` | `image` (base64 PNG/JPEG), `opacity`, `fit` (`cover`, `contain`, `stretch`, `tile`, `natural`), `anchor` (`[0.5, 0.5]`), `scale` (1), `offset` (`[0, 0]` points), `pixelated` (false), `blend` (`normal`) |
| `shader` | `shader` (`builtin:<name>` or WGSL text), `opacity` (1), `speed` (1; 0 holds one frame), `scale` (1), `colors` (≤ 4), `params` (≤ 8 numbers), `image` (base64, for the shader to sample), `blend` (`normal`) |
| `gradient` | `stops` (`[position, colour]`), `angle` (90 = top to bottom), `radial` (false), `opacity` (1), `blend` |
| `solid` | `color` |

`blend` is `normal` or `add` (light added: a glow).

A picture's `fit` says how it meets its section: `cover` fills it and crops,
`contain` shows all of it, `stretch` is the section exactly, `tile` repeats
it, `natural` is its own size (a pixel to a point). `scale` multiplies what
the fit gives (and a tile's size; `stretch` ignores it), `anchor` is where
it sits (0–1 across and down: `[1, 1]` is the bottom-right corner), and
`offset` moves it after that, in points. A logo tucked in a corner is
`"fit": "natural", "anchor": [1, 1], "scale": 0.5, "offset": [-18, -14]`.
Settings ▸ Appearance shows *fit*, *size* and *position* under any section
that has a picture.

A version 10 file still reads: each section's one picture becomes one
`image` layer.

## Writing a shader

The contract is **Floptle's, field for field** (`Floptle/docs/themes.md`
§"Writing a shader"), so one shader runs in both programs. A shader layer's
WGSL defines one function:

```wgsl
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    // uv: 0 to 1 across the window. px: the same, in points.
    let wave = 0.5 + 0.5 * sin(uv.x * 8.0 + bd.time);
    return vec4<f32>(mix(bd.color2.rgb, bd.color0.rgb, wave * 0.3), 1.0);
}
```

It returns an **sRGB colour with straight alpha**. The picture spans the
whole window, so every section showing the same shader shows one continuous
scene through its panel.

It can read the uniform `bd`, in this order:

| Field | |
|---|---|
| `resolution: vec2<f32>` | the texture being drawn, in pixels |
| `window: vec2<f32>` | the window, in points |
| `time: f32` | seconds × the layer's `speed`; holds still when backdrops are held; wraps once an hour |
| `scale: f32` | the layer's `scale` |
| `density: f32` | texture pixels per point |
| `color0` … `color3: vec4<f32>` | the layer's `colors`; any left out are the theme's accent, playhead, window and text |
| `params0`, `params1: vec4<f32>` | the layer's eight `params`, zeros for any left out |
| `pointer: vec4<f32>` | the pointer, 0–1 across the window; `z` is 1 while it is over the window |
| `beat: f32` | **Fontelle only.** The song's position in beats while it plays; otherwise a free 120 BPM clock. `fract(bd.beat)` is where in the beat it is |
| `level: f32` | **Fontelle only.** The master meter, 0–1 |

Fontelle's two come after every shared field, so a Floptle shader compiles
here unchanged. A shader can also sample `bd_image` with `bd_sampler` (the
layer's `image`, or white), and call the helpers every shader is given:
`bd_hash(p) -> f32`, `bd_noise(p) -> f32` (quintic value noise),
`bd_fbm(p, octaves) -> f32` and `bd_rot(angle) -> mat2x2<f32>`.

**A shader that does not compile** is refused with the compiler's (naga's)
message, which Settings shows when the theme is worn; its section shows its
plain colour, and the window carries on drawing.

Built-in shaders, named `builtin:<name>`, are in
`crates/fontelle-ui/shaders/`: Floptle's `galaxy`, `aurora`, `grid`,
`scanlines`, `drift`, `waves`, `starfield`, and Fontelle's `stage`,
`stillwater`, `sunroom`, `controlroom`, `rain`, `neon`, `phosphor`,
`bubblegum`, `embers`, `midnight`, `paper`. Each file's header says what its
colours and numbers mean.

To look at one: `cargo run --release -p fontelle-ui --example backdrop_probe
-- "<theme name or file>" <out-dir> 2 90` writes a frame early and one late
in its loop (a shader that looks right at 2 s can saturate after an hour),
and `-- "<theme>" --time` says what it costs.

## What moving costs, and when it stops

Each distinct shader layer is drawn once per frame into one texture at half
the window's resolution (Settings ▸ Appearance ▸ *Backdrop resolution*), at
most *Backdrop rate* times a second (30 by default), only where a section
showing it is on screen. It runs on the window's thread; nothing about it
touches the audio.

It keeps moving while another window has the focus, unless *Keep backdrops
moving when not focused* is off. It stops — and the window goes back to
drawing nothing at all — when the window is minimised or covered, when *Effects* is *Still* (each shader drawn
once and held) or *Off* (colours only: no shaders and no pictures), and,
with *Hold backdrops still while playing* on, while the song plays. A desktop
that asks for reduced motion (KDE's animation speed at Instant, GNOME's
Reduce Animation, macOS's Reduce motion, Windows's animations off) starts
Fontelle at *Still* until *Effects* is chosen.

Only share pictures and fonts you have the right to share.
