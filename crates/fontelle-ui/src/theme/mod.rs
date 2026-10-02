//! Token-based theming (TDD §16.6).
//!
//! A theme is a file. Fontelle ships a dark default and a light variant, and a
//! user theme is one of those written out, edited, and pointed at — there is no
//! theme editor in v1 and there does not need to be, because the format is
//! small enough to read.
//!
//! # The file format
//!
//! JSON, pretty-printed, for the same reasons §17.2 chose it for the document:
//! diffable, greppable, and fixable by hand when something goes wrong.
//!
//! ```json
//! {
//!   "format_version": 2,
//!   "name": "Fontelle Dark",
//!   "palette": {
//!     "window": "#0e0e11",
//!     "panel": "#17171c",
//!     ...
//!   },
//!   "metrics": { "panel_margin": 8.0, ... },
//!   "font": { "family": "sans-serif", "size": 13.0, "line_height": 1.35 }
//! }
//! ```
//!
//! Colours are `#rrggbb`, or `#rrggbbaa` when they are not opaque. Every field
//! is required: a half-specified theme would render half in someone else's
//! colours, which is worse than refusing to load. `format_version` is read
//! before the body, so a theme from a newer build is refused by version rather
//! than by whichever field happened to change shape first — the same rule the
//! project document follows.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod looks;

/// The revision of the theme format this build writes.
///
/// Its own number, separate from the project's and the patch's: a colour token
/// added to the chrome has nothing to do with either.
pub const THEME_FORMAT_VERSION: u32 = 11;

/// What a theme file is called: `Midnight.fontelletheme`. The same JSON as
/// ever — the name is so a file says what it is when it is sent to someone.
pub const THEME_EXTENSION: &str = "fontelletheme";

/// An 8-bit sRGB colour with alpha, written to file as hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color(pub [u8; 4]);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 0xff])
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    /// `#rrggbb` when opaque, `#rrggbbaa` when not.
    ///
    /// Dropping a redundant `ff` matters more than it sounds: nearly every
    /// token is opaque, and a file full of `#0e0e11ff` is a file people stop
    /// reading.
    pub fn to_hex(&self) -> String {
        let [r, g, b, a] = self.0;
        if a == 0xff {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    pub fn parse(s: &str) -> Result<Self, ColorParseError> {
        let hex = s
            .strip_prefix('#')
            .ok_or_else(|| ColorParseError(s.to_string()))?;
        if hex.len() != 6 && hex.len() != 8 {
            return Err(ColorParseError(s.to_string()));
        }
        let byte = |i: usize| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| ColorParseError(s.to_string()))
        };
        Ok(Self([
            byte(0)?,
            byte(2)?,
            byte(4)?,
            if hex.len() == 8 { byte(6)? } else { 0xff },
        ]))
    }

    /// The same colour at another opacity — what a tint over a row is.
    pub const fn with_alpha(self, alpha: u8) -> Self {
        let [r, g, b, _] = self.0;
        Self([r, g, b, alpha])
    }

    /// The form `vello` paints with.
    pub fn to_peniko(self) -> vello::peniko::Color {
        let [r, g, b, a] = self.0;
        vello::peniko::Color::from_rgba8(r, g, b, a)
    }
}

/// A string that was meant to be a colour and was not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorParseError(pub String);

impl std::fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} is not a colour — write #rrggbb, or #rrggbbaa for transparency",
            self.0
        )
    }
}

impl std::error::Error for ColorParseError {}

impl TryFrom<String> for Color {
    type Error = ColorParseError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<Color> for String {
    fn from(c: Color) -> Self {
        c.to_hex()
    }
}

/// Every colour the chrome draws with.
///
/// Deliberately a closed list rather than a map of arbitrary names: a theme
/// that can be missing a token is a theme that renders a hole, and the point of
/// naming them here is that adding one is a visible change to the format.
///
/// The set covers the panels named in §16.1 rather than only the one panel item
/// 6 draws — a token added later is a format revision, and the timeline,
/// piano roll and mixer already know which colours they will ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    /// Behind everything, where no panel reaches.
    pub window: Color,
    /// A panel's body.
    pub panel: Color,
    /// The strip along a panel's top carrying its name.
    pub panel_header: Color,
    /// Between panels and around controls.
    pub border: Color,
    pub text: Color,
    /// Labels, units, counts — quieter, still readable.
    pub text_muted: Color,
    /// The one colour that says "this is Fontelle": focus rings, active
    /// controls, the record button.
    pub accent: Color,
    /// The faintest of the roll's three grid levels: the lines *between*
    /// beats — sixteenths, eighths, whatever the snap is showing. Added in
    /// theme format v5.
    ///
    /// It exists because subdivisions and beats used to share
    /// [`grid_line`](Self::grid_line), which made the grid one
    /// undifferentiated comb that could not be counted by eye.
    pub grid_line_sub: Color,
    /// Beat lines in the timeline and piano roll — the middle level.
    pub grid_line: Color,
    /// Bar lines, which have to be tellable from beat lines at a glance.
    pub grid_line_strong: Color,
    pub playhead: Color,
    /// Fill over selected notes and clips; usually translucent.
    pub selection: Color,
    /// A meter below its warning point.
    pub meter: Color,
    /// A meter at or above it, and the limiter's gain-reduction readout.
    pub meter_peak: Color,

    // --- the piano roll and the timeline (added in theme format v2) ---
    /// A note block's default fill. Per-channel colours override it later.
    pub note: Color,
    pub note_selected: Color,
    /// The natural keys down the side of the piano roll.
    pub key_white: Color,
    pub key_black: Color,
    /// The roll's row behind a black key — a shade off the panel, so octaves
    /// are countable without drawing a line for every one.
    pub row_accidental: Color,

    // --- the key map (added in theme format v4) ---
    /// The roll's row for a key the instrument on the channel cannot play.
    ///
    /// Not the same ink as [`row_accidental`](Self::row_accidental), and it
    /// must not be: they are two different statements about a row, and a drum
    /// kit's dead keys land on naturals and accidentals alike.
    pub row_dead: Color,
    /// The same key, on the keyboard down the side.
    pub key_dead: Color,

    // --- the scale tool (added in theme format v9) ---
    /// A row outside the song's key, dimmed. It replaces the black-key
    /// stripes while a key is on: with a scale, *in or out* is what a row
    /// needs to say, not *black or white*.
    pub row_out_of_scale: Color,
    /// A row on the key's root, in every octave — tinted toward the accent
    /// so the key reads at a glance.
    pub row_scale_root: Color,
    /// A note written on a key nothing plays. It is still a note — it can be
    /// selected, moved, and heard the moment the instrument changes — so it
    /// is drawn quietly rather than not at all.
    pub note_silent: Color,

    // --- controls under automation (added in theme format v6) ---
    /// The groove of a knob an automation lane has taken over — TDD §12.2's
    /// "distinct ring colour".
    ///
    /// **Not one of the three ramps**, and for the same reason
    /// [`meter_peak`](Self::meter_peak) is not: it is a statement about *who
    /// is holding the control*, and a teal ring on teal chrome says nothing.
    /// Amber, which is the one hue left that neither the accent, the playhead,
    /// a note nor a clipping meter has already claimed.
    pub param_automated: Color,

    // --- modulation (added in theme format v7) ---
    /// The dashed arc round a control something in the mod matrix reaches, and
    /// the source badges it is dragged from (`docs/flopsynth-plan.md` §8.1
    /// rule 6).
    ///
    /// **A fifth ink, from outside the three ramps**, and it has to be: at
    /// three pixels the arc has to be tellable from the accent, the playhead,
    /// a note *and* the automation amber, and a modulation arc in any of those
    /// is a control whose owner you have to guess at. Violet is the hue none
    /// of them has claimed.
    pub modulation: Color,

    // --- the rings per source (theme format v8; `docs/flopsynth-next.md`
    // §3.3) ---
    /// The five inks a modulation ring wears, by what kind of source made
    /// the route: envelopes, LFOs, macros, the note's own values, the
    /// performance's. Five because a knob with three rings has to say
    /// which is which at a glance; `modulation` stays the matrix's own ink.
    pub mod_envelope: Color,
    pub mod_lfo: Color,
    pub mod_macro: Color,
    pub mod_note: Color,
    pub mod_performance: Color,
}

impl Palette {
    /// The same inks with the grounds made solid: for whatever is drawn
    /// **over** other content — a page, a menu, a tip — where a see-through
    /// theme would put two layers of words on top of each other.
    pub fn solid(&self) -> Self {
        let mut p = self.clone();
        for c in [&mut p.window, &mut p.panel, &mut p.panel_header] {
            *c = c.with_alpha(0xff);
        }
        p
    }
}

/// Sizes and radii, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    /// Between the window edge and the panels inside it.
    pub panel_margin: f32,
    pub panel_header_height: f32,
    /// Between a panel's frame and its contents.
    pub panel_padding: f32,
    pub border_width: f32,
    pub corner_radius: f32,
    /// One line in a list: channels, presets, the browser.
    pub row_height: f32,
    /// The strip across the top of the window carrying play/stop and the
    /// playhead. Added in theme format v1.
    pub transport_bar_height: f32,
    /// How wide the left-hand column of docked panels — the channel rack and
    /// the soundfont browser — is. Added in theme format v3.
    ///
    /// Wide enough for a soundfont's name and narrow enough that the piano
    /// roll is still the thing the window is mostly made of.
    pub sidebar_width: f32,
}

/// The chrome's typeface.
///
/// `family` is a `cosmic-text` family name, so the generic names
/// (`sans-serif`, `monospace`) work and resolve to whatever the machine has —
/// which is what a theme file should say unless its author is shipping a font
/// alongside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontTokens {
    pub family: String,
    /// In logical pixels.
    pub size: f32,
    /// A multiple of `size`.
    pub line_height: f32,
    /// A face for titles — the editor panel's heading and the start menu's
    /// — where a theme wants one apart from its text face (v11). `None`
    /// draws titles in `family`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl FontTokens {
    /// The same tokens in the display face, for a title.
    pub fn for_titles(&self) -> Self {
        Self {
            family: self.display.clone().unwrap_or_else(|| self.family.clone()),
            display: None,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub format_version: u32,
    pub name: String,
    /// One line saying what the look is, shown in the picker (v11).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub palette: Palette,
    pub metrics: Metrics,
    pub font: FontTokens,
    /// Fonts carried inside the file (v11), named by [`FontTokens::family`]
    /// and [`FontTokens::display`] by their own family names — so a look
    /// with a face of its own is still one file to send.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<ThemeFont>,
    /// What is behind the panels, carried inside the file: since v10
    /// pictures, since v11 a stack of layers per section, moving ones among
    /// them. Optional and left out when empty, so a plain theme stays the
    /// short file it was.
    #[serde(default, skip_serializing_if = "Backdrops::is_empty")]
    pub backdrops: Backdrops,
}

/// Which part of the window a backdrop sits behind.
///
/// Ty: *"i want themes to be able to get detailed in the backgrounds they
/// want to make for each section"* — so every section has its own, and the
/// window's own is under all of them, showing through any panel a theme
/// makes see-through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropPanel {
    /// Under everything: the ground the panels sit on.
    Window,
    /// The bar along the top.
    Transport,
    /// The channel rack.
    Channels,
    /// The sidebar: import, sounds, presets, projects.
    Browser,
    /// Behind the arrangement's lanes.
    Arrangement,
    /// Behind the piano roll's notes.
    Roll,
    /// Behind the mixer's strips.
    Mixer,
}

impl BackdropPanel {
    pub const ALL: [Self; 7] = [
        Self::Window,
        Self::Transport,
        Self::Channels,
        Self::Browser,
        Self::Arrangement,
        Self::Roll,
        Self::Mixer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Window => "Window",
            Self::Transport => "Transport",
            Self::Channels => "Channels",
            Self::Browser => "Browser",
            Self::Arrangement => "Arrangement",
            Self::Roll => "Piano roll",
            Self::Mixer => "Mixer",
        }
    }
}

/// How a picture sits in its section.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackdropFit {
    /// Scaled to fill the section, cropped where it does not fit; the
    /// anchor says which part shows.
    #[default]
    Cover,
    /// Scaled to fit whole inside the section, placed by the anchor.
    Contain,
    /// Stretched to the section exactly.
    Stretch,
    /// Repeated at its own size from the section's corner — for a pattern.
    Tile,
    /// At its own size, a pixel to a point, placed by the anchor — Floptle's
    /// `Natural`, for a logo in a corner.
    Natural,
}

impl BackdropFit {
    pub const ALL: [Self; 5] = [
        Self::Cover,
        Self::Contain,
        Self::Stretch,
        Self::Tile,
        Self::Natural,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cover => "Cover",
            Self::Contain => "Contain",
            Self::Stretch => "Stretch",
            Self::Tile => "Tile",
            Self::Natural => "Natural size",
        }
    }
}

fn centre() -> [f32; 2] {
    [0.5, 0.5]
}

/// Where a `width` × `height` picture is drawn in `area`: one rectangle,
/// or one per tile. `anchor` is where it sits, 0 to 1 across and down —
/// `[1.0, 0.5]` puts a contained picture at the right, or shows a covering
/// one's right-hand side.
pub fn backdrop_tiles(
    area: crate::layout::Rect,
    width: f32,
    height: f32,
    fit: BackdropFit,
    anchor: [f32; 2],
) -> Vec<crate::layout::Rect> {
    backdrop_tiles_scaled(area, width, height, fit, anchor, 1.0, [0.0, 0.0])
}

/// The same, at `scale` times the size the fit gives (a tile's too), moved
/// by `offset` points after it is placed. Stretch is the area whatever the
/// scale: that is what it says.
pub fn backdrop_tiles_scaled(
    area: crate::layout::Rect,
    width: f32,
    height: f32,
    fit: BackdropFit,
    anchor: [f32; 2],
    scale: f32,
    offset: [f32; 2],
) -> Vec<crate::layout::Rect> {
    use crate::layout::Rect;
    if area.is_empty() || width <= 0.0 || height <= 0.0 || scale <= 0.0 {
        return Vec::new();
    }
    let [ax, ay] = [anchor[0].clamp(0.0, 1.0), anchor[1].clamp(0.0, 1.0)];
    let placed = |fitted: f32| {
        let (w, h) = (width * fitted * scale, height * fitted * scale);
        Rect::new(
            area.x + (area.width - w) * ax + offset[0],
            area.y + (area.height - h) * ay + offset[1],
            w,
            h,
        )
    };
    let cover = (area.width / width).max(area.height / height);
    let contain = (area.width / width).min(area.height / height);
    // A tile is the picture at its own size, scaled.
    let (width, height) = (width * scale, height * scale);
    match fit {
        BackdropFit::Cover => vec![placed(cover)],
        BackdropFit::Contain => vec![placed(contain)],
        BackdropFit::Natural => vec![placed(1.0)],
        BackdropFit::Stretch => vec![area],
        BackdropFit::Tile => {
            let across = (area.width / width).ceil() as usize;
            let down = (area.height / height).ceil() as usize;
            // A one-pixel picture over a big panel is a pattern nobody
            // meant; past this it is not drawn rather than drawn slowly.
            if across.saturating_mul(down) > 4096 {
                return Vec::new();
            }
            (0..down)
                .flat_map(|row| {
                    (0..across).map(move |col| {
                        Rect::new(
                            area.x + col as f32 * width,
                            area.y + row as f32 * height,
                            width,
                            height,
                        )
                    })
                })
                .collect()
        }
    }
}

/// What a theme puts behind its panels — Ty: *"maybe allowing users to
/// even put background images behind their arrangement or different
/// panels"* — as one stack of [`Layer`]s per section, drawn bottom to top
/// over the panel's ground and under everything on it (v11; a v10 file's
/// one picture per section reads as a stack of one).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backdrops {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub window: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transport: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub browser: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arrangement: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roll: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixer: Vec<Layer>,
}

impl Backdrops {
    pub fn is_empty(&self) -> bool {
        BackdropPanel::ALL
            .iter()
            .all(|p| self.layers(*p).is_empty())
    }

    /// `panel`'s stack, bottom first.
    pub fn layers(&self, panel: BackdropPanel) -> &[Layer] {
        match panel {
            BackdropPanel::Window => &self.window,
            BackdropPanel::Transport => &self.transport,
            BackdropPanel::Channels => &self.channels,
            BackdropPanel::Browser => &self.browser,
            BackdropPanel::Arrangement => &self.arrangement,
            BackdropPanel::Roll => &self.roll,
            BackdropPanel::Mixer => &self.mixer,
        }
    }

    pub fn layers_mut(&mut self, panel: BackdropPanel) -> &mut Vec<Layer> {
        match panel {
            BackdropPanel::Window => &mut self.window,
            BackdropPanel::Transport => &mut self.transport,
            BackdropPanel::Channels => &mut self.channels,
            BackdropPanel::Browser => &mut self.browser,
            BackdropPanel::Arrangement => &mut self.arrangement,
            BackdropPanel::Roll => &mut self.roll,
            BackdropPanel::Mixer => &mut self.mixer,
        }
    }

    /// `panel`'s picture: the topmost picture layer, the one the settings
    /// page's *Choose…* sets.
    pub fn get(&self, panel: BackdropPanel) -> Option<&Backdrop> {
        self.layers(panel).iter().rev().find_map(|l| match l {
            Layer::Image(b) => Some(b),
            _ => None,
        })
    }

    /// Sets `panel`'s picture, or takes its pictures off with `None`. The
    /// section's other layers stay where they are and the picture goes on
    /// top of them: a picture chosen over a moving theme keeps it moving
    /// under the picture.
    pub fn set(&mut self, panel: BackdropPanel, backdrop: Option<Backdrop>) {
        let layers = self.layers_mut(panel);
        layers.retain(|l| !matches!(l, Layer::Image(_)));
        if let Some(b) = backdrop {
            layers.push(Layer::Image(b));
        }
    }

    /// Every shader layer, by the section it is in.
    pub fn shaders(&self) -> impl Iterator<Item = (BackdropPanel, &ShaderLayer)> {
        BackdropPanel::ALL.into_iter().flat_map(move |panel| {
            self.layers(panel).iter().filter_map(move |l| match l {
                Layer::Shader(s) => Some((panel, s)),
                _ => None,
            })
        })
    }

    pub fn has_shaders(&self) -> bool {
        self.shaders().next().is_some()
    }

    /// Whether anything here moves: a shader with a speed. One at speed 0
    /// is drawn once and held, and holds nothing awake.
    pub fn is_animated(&self) -> bool {
        self.shaders()
            .any(|(_, s)| s.speed != 0.0 && s.opacity > 0.0)
    }
}

/// One layer of a section's stack.
///
/// The four kinds Floptle's themes have (`floptle-theme/src/model.rs`), so a
/// look moves between the two programs without a new idea in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Layer {
    /// A picture, carried in the file.
    Image(Backdrop),
    /// A moving picture drawn by a WGSL shader — see [`crate::backdrop`]
    /// for what it is handed.
    Shader(ShaderLayer),
    /// A wash from one colour to another.
    Gradient(GradientLayer),
    /// A flat colour over what is under it.
    Solid { color: Color },
}

/// How a layer goes over what is under it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Blend {
    /// Over, as a picture is.
    #[default]
    Normal,
    /// Added to what is under it: light, for a glow.
    Add,
}

impl Blend {
    fn is_normal(&self) -> bool {
        *self == Blend::Normal
    }
}

fn one() -> f32 {
    1.0
}

fn ninety() -> f32 {
    90.0
}

/// A shader layer. Its picture spans the whole window, so two sections
/// showing the same one read as one scene through both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderLayer {
    /// `builtin:<name>` for one Fontelle ships, or the WGSL itself — the
    /// one function `fn backdrop(uv, px) -> vec4<f32>` — so a theme with a
    /// shader of its own is still one file.
    pub shader: String,
    #[serde(default = "one")]
    pub opacity: f32,
    /// How fast it moves; 0 holds one frame.
    #[serde(default = "one")]
    pub speed: f32,
    /// A size the shader may scale its pattern by.
    #[serde(default = "one")]
    pub scale: f32,
    /// Up to four colours for it; unset ones are the theme's own — see
    /// [`ShaderLayer::colors_for`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<Color>,
    /// Up to eight numbers it may read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<f32>,
    /// A PNG or JPEG it may sample, base64 like a picture layer's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Blend::is_normal")]
    pub blend: Blend,
}

impl ShaderLayer {
    /// `shader` at full strength and speed, in the theme's colours.
    pub fn new(shader: &str) -> Self {
        Self {
            shader: shader.to_string(),
            opacity: 1.0,
            speed: 1.0,
            scale: 1.0,
            colors: Vec::new(),
            params: Vec::new(),
            image: None,
            blend: Blend::Normal,
        }
    }

    /// The four colours the shader is handed: the layer's, then the
    /// theme's accent, playhead, window and text for any it leaves out —
    /// Floptle's accent, accent_hi, ground and text, as near as Fontelle's
    /// palette has them.
    pub fn colors_for(&self, palette: &Palette) -> [Color; 4] {
        let fallback = [
            palette.accent,
            palette.playhead,
            palette.window,
            palette.text,
        ];
        std::array::from_fn(|i| self.colors.get(i).copied().unwrap_or(fallback[i]))
    }

    /// The eight numbers, zeros for any left out.
    pub fn params8(&self) -> [f32; 8] {
        std::array::from_fn(|i| self.params.get(i).copied().unwrap_or(0.0))
    }
}

/// A gradient layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientLayer {
    /// `[position 0–1, colour]`, in order.
    pub stops: Vec<(f32, Color)>,
    /// Degrees: 90 runs top to bottom, 0 left to right.
    #[serde(default = "ninety")]
    pub angle: f32,
    /// From the middle outward instead.
    #[serde(default)]
    pub radial: bool,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Blend::is_normal")]
    pub blend: Blend,
}

/// A font file carried inside a theme: TrueType or OpenType, base64.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFont {
    pub data: String,
}

impl ThemeFont {
    /// A font from a `.ttf` or `.otf` file's bytes. Refused here, when it is
    /// chosen, if it is not one.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ThemeError> {
        let magic = bytes.get(..4).unwrap_or_default();
        let sfnt = [&[0x00, 0x01, 0x00, 0x00][..], b"OTTO", b"true", b"ttcf"];
        if !sfnt.contains(&magic) {
            return Err(ThemeError::Format(
                "that is not a TrueType or OpenType font".to_string(),
            ));
        }
        use base64::Engine as _;
        Ok(Self {
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }

    /// The font file's bytes, as they were handed in.
    pub fn bytes(&self) -> Option<Vec<u8>> {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .ok()
    }
}

/// One picture: the image file's own bytes, base64 in the JSON so the theme
/// is one file to send, and how strongly it shows through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backdrop {
    /// A PNG or a JPEG, as the file was, in base64.
    pub image: String,
    /// 0 is not there, 1 is the picture as it is. Drawn over the panel's
    /// ground and under everything on it.
    pub opacity: f32,
    /// How it sits in its section. Cover when the file does not say.
    #[serde(default)]
    pub fit: BackdropFit,
    /// Where, 0 to 1 across and down ([`backdrop_tiles`]). The middle when
    /// the file does not say.
    #[serde(default = "centre")]
    pub anchor: [f32; 2],
    #[serde(default, skip_serializing_if = "Blend::is_normal")]
    pub blend: Blend,
    /// Times the size the fit gives it (v11).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f32,
    /// Points it is moved by once placed, across and down (v11).
    #[serde(default, skip_serializing_if = "is_zero2")]
    pub offset: [f32; 2],
    /// Sampled nearest-neighbour: for pixel art (v11).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pixelated: bool,
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
}

fn is_zero2(v: &[f32; 2]) -> bool {
    *v == [0.0, 0.0]
}

impl Backdrop {
    /// A backdrop from an image file's bytes. Refused here, when it is
    /// chosen, if it is not a picture this build can draw — not later, as a
    /// blank panel nobody can explain.
    pub fn from_image_bytes(bytes: &[u8], opacity: f32) -> Result<Self, ThemeError> {
        decode_image(bytes).map_err(|why| {
            ThemeError::Format(format!("that is not a PNG or JPEG picture: {why}"))
        })?;
        use base64::Engine as _;
        Ok(Self {
            image: base64::engine::general_purpose::STANDARD.encode(bytes),
            opacity: opacity.clamp(0.0, 1.0),
            fit: BackdropFit::Cover,
            anchor: centre(),
            blend: Blend::Normal,
            scale: 1.0,
            offset: [0.0, 0.0],
            pixelated: false,
        })
    }

    /// The image file's bytes, as they were handed in.
    pub fn image_bytes(&self) -> Option<Vec<u8>> {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(&self.image)
            .ok()
    }

    /// The picture, ready to draw. `None` for a file somebody broke by hand.
    pub fn decode(&self) -> Option<vello::peniko::ImageData> {
        decode_image(&self.image_bytes()?).ok()
    }
}

/// A PNG or a JPEG, by its first bytes.
pub fn decode_image(bytes: &[u8]) -> Result<vello::peniko::ImageData, String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return crate::branding::decode_png(bytes);
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        use zune_core::colorspace::ColorSpace;
        use zune_core::options::DecoderOptions;
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(bytes, options);
        let rgba = decoder.decode().map_err(|e| format!("{e:?}"))?;
        let (width, height) = decoder
            .dimensions()
            .ok_or_else(|| "no dimensions".to_string())?;
        if width == 0 || height == 0 || rgba.len() != width * height * 4 {
            return Err("an empty image".to_string());
        }
        return Ok(vello::peniko::ImageData {
            data: vello::peniko::Blob::new(std::sync::Arc::new(rgba)),
            format: vello::peniko::ImageFormat::Rgba8,
            alpha_type: vello::peniko::ImageAlphaType::Alpha,
            width: width as u32,
            height: height as u32,
        });
    }
    Err("neither a PNG nor a JPEG".to_string())
}

impl Theme {
    /// Every look Fontelle ships, the default first. The library lists these
    /// before the user's own files, and a built-in is never written over:
    /// changing one saves a copy.
    ///
    /// Built once: the library lists them often, and between them they
    /// carry pictures and fonts worth a megabyte or two.
    pub fn builtins() -> Vec<Self> {
        static BUILTINS: std::sync::OnceLock<Vec<Theme>> = std::sync::OnceLock::new();
        BUILTINS
            .get_or_init(|| {
                vec![
                    // The five Fontelle has had since v0.20.0, in their order:
                    // a settings file remembers a look by name, and tests and
                    // people by place.
                    Self::dark_default(),
                    Self::light_default(),
                    Self::midnight(),
                    Self::ember(),
                    Self::paper(),
                    // Hub card 0366's, for different tastes.
                    looks::still_water(),
                    looks::sunroom(),
                    looks::control_room(),
                    looks::lofi_rain(),
                    looks::neon_grid(),
                    looks::phosphor(),
                    looks::bubblegum(),
                    looks::nebula(),
                    looks::aurora(),
                    looks::stage(),
                    looks::high_contrast(),
                    looks::high_contrast_light(),
                ]
            })
            .clone()
    }

    /// Square corners, hard rules, a cold blue: the rigid look. Ty: *"maybe
    /// a theme looks more rigid."*
    pub fn midnight() -> Self {
        let mut theme = Self::dark_default();
        theme.name = "Midnight".to_string();
        theme.description =
            "Square corners and hard rules in a cold blue, over a slow night mist.".to_string();
        let p = &mut theme.palette;
        p.window = Color::rgb(0x07, 0x09, 0x0f);
        p.panel = Color::rgb(0x0f, 0x13, 0x1c);
        p.panel_header = Color::rgb(0x15, 0x1b, 0x27);
        p.border = Color::rgb(0x2a, 0x34, 0x4e);
        p.text = Color::rgb(0xe2, 0xe7, 0xf2);
        p.text_muted = Color::rgb(0x86, 0x92, 0xae);
        p.accent = Color::rgb(0x5b, 0x8d, 0xef);
        p.grid_line_sub = Color::rgb(0x12, 0x17, 0x22);
        p.grid_line = Color::rgb(0x1d, 0x25, 0x36);
        p.grid_line_strong = Color::rgb(0x2a, 0x34, 0x4e);
        p.playhead = Color::rgb(0x6c, 0xd0, 0xff);
        p.selection = Color::rgba(0x5b, 0x8d, 0xef, 0x50);
        p.meter = Color::rgb(0x4f, 0xc3, 0xa1);
        p.note = Color::rgb(0x4a, 0x6f, 0xc4);
        p.note_selected = Color::rgb(0xb4, 0xc8, 0xf4);
        p.key_black = Color::rgb(0x15, 0x1b, 0x27);
        p.row_accidental = Color::rgb(0x0b, 0x0f, 0x17);
        p.row_dead = Color::rgb(0x05, 0x07, 0x0b);
        p.row_out_of_scale = Color::rgb(0x09, 0x0c, 0x13);
        p.row_scale_root = Color::rgb(0x18, 0x22, 0x3a);
        // The keys, in the look's own greys rather than the default's teal.
        p.key_white = Color::rgb(0xc8, 0xd0, 0xe0);
        p.key_dead = Color::rgb(0x4a, 0x54, 0x68);
        p.note_silent = Color::rgb(0x2c, 0x35, 0x4a);
        theme.metrics.corner_radius = 0.0;
        theme.metrics.border_width = 1.0;
        // Hub card 0366: a slow cold-blue backdrop, seen through panels
        // only a little see-through — the rigid look stays rigid.
        p.panel = p.panel.with_alpha(0xd4);
        p.panel_header = p.panel_header.with_alpha(0xe4);
        p.row_accidental = p.row_accidental.with_alpha(0xb0);
        p.row_out_of_scale = p.row_out_of_scale.with_alpha(0xb0);
        theme.backdrops.window = vec![Layer::Shader(ShaderLayer {
            colors: vec![
                looks::hex(0x5b8def),
                looks::hex(0x2a4a9a),
                looks::hex(0x07090f),
                looks::hex(0xc8d4f0),
            ],
            ..ShaderLayer::new("builtin:midnight")
        })];
        theme
    }

    /// Warm and dark, with softer corners, and a fire behind the glass:
    /// the built-in that shows what a theme's pictures can do — smoke and
    /// drifting embers under see-through panels, a line of heat under the
    /// transport, sparks climbing the side panels and the arrangement, a
    /// bed of coals under the mixer. Its pictures are drawn by
    /// `assets/themes/ember/make.py`. Built once: the library lists the
    /// built-ins often, and these carry two megabytes of pictures.
    pub fn ember() -> Self {
        static EMBER: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();
        EMBER.get_or_init(Self::ember_built).clone()
    }

    fn ember_built() -> Self {
        let mut theme = Self::dark_default();
        theme.name = "Ember".to_string();
        theme.description =
            "Warm and dark, with smoke and embers rising from a fire behind the glass.".to_string();
        let p = &mut theme.palette;
        p.window = Color::rgb(0x12, 0x0c, 0x0a);
        p.panel = Color::rgb(0x1d, 0x15, 0x12);
        p.panel_header = Color::rgb(0x26, 0x1c, 0x17);
        p.border = Color::rgb(0x45, 0x33, 0x2a);
        p.text = Color::rgb(0xf1, 0xe6, 0xdf);
        p.text_muted = Color::rgb(0xa8, 0x90, 0x82);
        p.accent = Color::rgb(0xd9, 0x82, 0x4a);
        p.grid_line_sub = Color::rgb(0x21, 0x18, 0x14);
        p.grid_line = Color::rgb(0x33, 0x26, 0x20);
        p.grid_line_strong = Color::rgb(0x45, 0x33, 0x2a);
        p.playhead = Color::rgb(0xe8, 0xb8, 0x5c);
        p.selection = Color::rgba(0xd9, 0x82, 0x4a, 0x48);
        p.meter = Color::rgb(0x9c, 0xbf, 0x5e);
        p.note = Color::rgb(0xc0, 0x6e, 0x3c);
        p.note_selected = Color::rgb(0xf4, 0xc8, 0xa2);
        p.key_black = Color::rgb(0x26, 0x1c, 0x17);
        p.row_accidental = Color::rgb(0x17, 0x10, 0x0d);
        p.row_dead = Color::rgb(0x0c, 0x08, 0x06);
        p.row_out_of_scale = Color::rgb(0x14, 0x0e, 0x0b);
        p.row_scale_root = Color::rgb(0x33, 0x22, 0x18);
        p.key_white = Color::rgb(0xe2, 0xd6, 0xcc);
        p.key_dead = Color::rgb(0x5e, 0x50, 0x49);
        p.note_silent = Color::rgb(0x48, 0x38, 0x30);
        // See-through, so the fire behind the window shows; the words on
        // them still sit on something.
        p.panel = p.panel.with_alpha(0xc4);
        p.panel_header = p.panel_header.with_alpha(0xd4);
        // Quieter text a shade lighter: it sits on moving light now.
        p.text_muted = Color::rgb(0xb0, 0x98, 0x88);
        p.row_accidental = p.row_accidental.with_alpha(0xa0);
        p.row_out_of_scale = p.row_out_of_scale.with_alpha(0xa0);
        let picture = |bytes: &[u8], opacity: f32, fit: BackdropFit, anchor: [f32; 2]| {
            let mut b = Backdrop::from_image_bytes(bytes, opacity)
                .expect("Ember's pictures are compiled in and decode");
            b.fit = fit;
            b.anchor = anchor;
            Layer::Image(b)
        };
        // Hub card 0366: the fire moves now. Smoke and rising embers are a
        // shader behind the whole window; the line of heat under the
        // transport and the bed of coals under the mixer stay pictures,
        // over it; the side panels and the arrangement are lit from below.
        let fire = looks::hex(0xff7a1a);
        let lit = |strength: u8| looks::glow(fire.with_alpha(0), fire.with_alpha(strength), 1.0);
        theme.backdrops = Backdrops {
            window: vec![Layer::Shader(ShaderLayer {
                colors: vec![
                    looks::hex(0xe0702a),
                    looks::hex(0xffa040),
                    looks::hex(0x120c0a),
                    looks::hex(0xffe2a8),
                ],
                ..ShaderLayer::new("builtin:embers")
            })],
            transport: vec![picture(
                include_bytes!("../../../../assets/themes/ember/transport.png"),
                1.0,
                BackdropFit::Stretch,
                centre(),
            )],
            channels: vec![lit(0x20)],
            browser: vec![lit(0x20)],
            arrangement: vec![lit(0x18)],
            roll: vec![lit(0x14)],
            mixer: vec![picture(
                include_bytes!("../../../../assets/themes/ember/mixer.png"),
                0.9,
                BackdropFit::Cover,
                [0.5, 1.0],
            )],
        };
        theme.metrics.corner_radius = 7.0;
        theme
    }

    /// Light and soft: warm paper, rounder corners.
    pub fn paper() -> Self {
        let mut theme = Self::light_default();
        theme.name = "Paper".to_string();
        theme.description =
            "Warm paper and dark ink, rounder corners, the sheet's grain behind.".to_string();
        let p = &mut theme.palette;
        p.window = Color::rgb(0xe6, 0xdf, 0xd2);
        p.panel = Color::rgb(0xf7, 0xf3, 0xea);
        p.panel_header = Color::rgb(0xee, 0xe7, 0xda);
        p.border = Color::rgb(0xd2, 0xc7, 0xb3);
        p.text = Color::rgb(0x2a, 0x25, 0x1d);
        p.text_muted = Color::rgb(0x6e, 0x63, 0x52);
        p.accent = Color::rgb(0x2f, 0x72, 0x63);
        p.grid_line_sub = Color::rgb(0xef, 0xea, 0xdf);
        p.grid_line = Color::rgb(0xdd, 0xd4, 0xc4);
        p.grid_line_strong = Color::rgb(0xc8, 0xbc, 0xa6);
        p.playhead = Color::rgb(0xb0, 0x5a, 0x2a);
        p.selection = Color::rgba(0x2f, 0x72, 0x63, 0x38);
        p.meter = Color::rgb(0x3d, 0x85, 0x5a);
        p.note = Color::rgb(0x3d, 0x6e, 0x8c);
        p.note_selected = Color::rgb(0x2a, 0x4e, 0x66);
        p.row_accidental = Color::rgb(0xee, 0xe8, 0xdc);
        p.row_dead = Color::rgb(0xdc, 0xd4, 0xc5);
        p.row_out_of_scale = Color::rgb(0xe9, 0xe2, 0xd5);
        p.row_scale_root = Color::rgb(0xd9, 0xea, 0xe2);
        p.key_white = Color::rgb(0xfb, 0xf8, 0xf2);
        p.key_black = Color::rgb(0x3a, 0x32, 0x26);
        p.key_dead = Color::rgb(0xd6, 0xcd, 0xbd);
        p.note_silent = Color::rgb(0xb3, 0xa8, 0x96);
        // Quieter text darkened to AA on the header too.
        p.text_muted = Color::rgb(0x5f, 0x55, 0x46);
        // Hub card 0366: the sheet's grain and fibres, drawn once — speed 0,
        // so it costs nothing after the first frame — and seen faintly
        // through the panels.
        p.panel = p.panel.with_alpha(0xf0);
        p.panel_header = p.panel_header.with_alpha(0xf4);
        theme.backdrops.window = vec![Layer::Shader(ShaderLayer {
            colors: vec![
                looks::hex(0x8a7a60),
                looks::hex(0x8a7a60),
                looks::hex(0xe6dfd2),
                looks::hex(0x2a251d),
            ],
            speed: 0.0,
            ..ShaderLayer::new("builtin:paper")
        })];
        theme.metrics.corner_radius = 8.0;
        theme
    }

    /// The theme Flopsynth's window — the bridge — is painted in, whatever
    /// the studio's: the dark palette, with this theme's metrics and font.
    ///
    /// Ty's decision (`docs/flopsynth-next.md` §9.2(a)): the window keeps
    /// its own palette, dark under both themes. Grabbed with `--light` at
    /// v0.9.0 it was a purple smear on grey, the *Synth* tab label white on
    /// white and the pictures grey on grey — the hull, the glass and the sky
    /// were drawn in inks chosen for a dark ground, mixed from a light one.
    /// Serum, Omnisphere and Vital are dark-only and it is not a defect; the
    /// skins in `assets/flopsynth/skin/` are the way to change it. The
    /// accent inks stay the dark theme's too: a light theme's accent is
    /// chosen against a light ground and nothing promises it reads on this
    /// one.
    pub fn for_bridge(&self) -> Self {
        Self {
            format_version: self.format_version,
            name: self.name.clone(),
            description: self.description.clone(),
            palette: Self::dark_default().palette,
            metrics: self.metrics,
            // The default's face too: every width the bridge was laid out
            // against was measured in it (`text::OPEN_SANS_REGULAR`), and a
            // theme's own face or a larger size pushes its captions out of
            // their cells.
            font: Self::dark_default().font,
            fonts: Vec::new(),
            // Its own window: the studio's pictures are not the bridge's.
            backdrops: Backdrops::default(),
        }
    }

    /// The theme the **notepad's** window is painted in: this theme's metrics
    /// and font, over the pad's own inks (`docs/effects-catalogue.md` §2.8).
    ///
    /// Only the tokens the window's chrome actually draws with are replaced —
    /// the ground, the panels, the rules and the two inks — so a control that
    /// reaches for a colour nothing here names still gets the studio's.
    pub fn for_notepad(&self, ink: &NotepadInk) -> Self {
        let mut palette = self.palette.clone();
        palette.window = ink.ground;
        palette.panel = ink.paper;
        palette.panel_header = ink.ground;
        palette.border = ink.edge;
        palette.text = ink.ink;
        palette.text_muted = ink.faint;
        palette.accent = ink.caret;
        palette.grid_line = ink.edge;
        Self {
            format_version: self.format_version,
            name: self.name.clone(),
            description: self.description.clone(),
            palette,
            metrics: self.metrics,
            font: self.font.clone(),
            fonts: self.fonts.clone(),
            backdrops: Backdrops::default(),
        }
    }

    /// The default. Chosen against WCAG contrast rather than by eye — the
    /// chrome is small and dense, and a DAW is looked at for hours.
    pub fn dark_default() -> Self {
        Self {
            format_version: THEME_FORMAT_VERSION,
            name: "Fontelle Dark".to_string(),
            description: "The default: near-neutral teal, sober and still.".to_string(),
            palette: Palette {
                // The primary ramp's dark end carries the structure. Kept
                // near-neutral on purpose: a fully saturated teal UI reads as
                // a skin, and the brief was FL Studio's newer look — neutral
                // surfaces, colour reserved for things that mean something.
                window: Color::rgb(0x06, 0x10, 0x13),
                panel: Color::rgb(0x0e, 0x1f, 0x26),
                panel_header: Color::rgb(0x11, 0x26, 0x2e),
                border: Color::rgb(0x1f, 0x40, 0x4c),
                text: Color::rgb(0xdc, 0xe8, 0xec),
                // Lifted from #6e93a0 (5.1:1) to 6.3:1 on the panel and
                // 5.8:1 on its header: the quiet text is read for hours too.
                text_muted: Color::rgb(0x7f, 0xa3, 0xb0),
                // The top of the primary ramp: the one colour that says
                // Fontelle.
                accent: Color::rgb(0x40, 0x85, 0x9c),
                grid_line_sub: Color::rgb(0x0d, 0x1c, 0x22),
                grid_line: Color::rgb(0x18, 0x33, 0x3d),
                grid_line_strong: Color::rgb(0x1f, 0x40, 0x4c),
                // Green, from the first secondary ramp, so the playhead never
                // competes with the teal chrome it travels over.
                playhead: Color::rgb(0x40, 0xa4, 0x88),
                // Blue, from the second, translucent over whatever it covers.
                selection: Color::rgba(0x49, 0x6f, 0xa4, 0x59),
                meter: Color::rgb(0x40, 0xa4, 0x88),
                // The one colour not in the three ramps, and deliberately so:
                // clipping is a signal, not a brand. Nothing in a teal, green
                // and blue palette can say "too loud", and a meter that cannot
                // is not a meter.
                meter_peak: Color::rgb(0xc4, 0x60, 0x5c),
                note: Color::rgb(0x49, 0x6f, 0xa4),
                note_selected: Color::rgb(0xa8, 0xc4, 0xe4),
                key_white: Color::rgb(0xc9, 0xd6, 0xda),
                key_black: Color::rgb(0x11, 0x26, 0x2e),
                row_accidental: Color::rgb(0x0a, 0x18, 0x1e),
                row_dead: Color::rgb(0x05, 0x0d, 0x11),
                key_dead: Color::rgb(0x53, 0x63, 0x68),
                row_out_of_scale: Color::rgb(0x07, 0x12, 0x17),
                row_scale_root: Color::rgb(0x14, 0x2e, 0x38),
                note_silent: Color::rgb(0x2b, 0x3b, 0x48),
                param_automated: Color::rgb(0xd0, 0x8a, 0x3c),
                modulation: Color::rgb(0x9a, 0x6f, 0xd0),
                mod_envelope: Color::rgb(0x9a, 0x6f, 0xd0),
                mod_lfo: Color::rgb(0x3f, 0xb8, 0xc4),
                mod_macro: Color::rgb(0xe0, 0xa4, 0x3a),
                mod_note: Color::rgb(0x5d, 0xc2, 0x7a),
                mod_performance: Color::rgb(0xe0, 0x6a, 0x9a),
            },
            metrics: METRICS,
            font: FontTokens {
                family: "sans-serif".to_string(),
                size: 13.0,
                line_height: 1.35,
                display: None,
            },
            fonts: Vec::new(),
            backdrops: Backdrops::default(),
        }
    }

    /// The same theme in daylight. Same metrics, same font, inverted ground —
    /// which is what keeps it a variant rather than a second design.
    pub fn light_default() -> Self {
        Self {
            format_version: THEME_FORMAT_VERSION,
            name: "Fontelle Light".to_string(),
            description: "The default in daylight: the same teal, from the other end.".to_string(),
            palette: Palette {
                // The same three ramps, read from the other end.
                window: Color::rgb(0xc9, 0xd6, 0xda),
                panel: Color::rgb(0xe4, 0xed, 0xef),
                panel_header: Color::rgb(0xd3, 0xe0, 0xe4),
                border: Color::rgb(0xa9, 0xc0, 0xc7),
                text: Color::rgb(0x06, 0x10, 0x13),
                // AA on the header as well as the panel (it was 4.98:1
                // there), and an accent of its own: it was the quiet text's
                // ink, so an active control read as a label.
                text_muted: Color::rgb(0x2b, 0x56, 0x64),
                accent: Color::rgb(0x1f, 0x6f, 0x8b),
                grid_line_sub: Color::rgb(0xdf, 0xe8, 0xea),
                grid_line: Color::rgb(0xc0, 0xd0, 0xd5),
                grid_line_strong: Color::rgb(0xa9, 0xc0, 0xc7),
                playhead: Color::rgb(0x1e, 0x4f, 0x42),
                selection: Color::rgba(0x49, 0x6f, 0xa4, 0x40),
                meter: Color::rgb(0x31, 0x77, 0x64),
                meter_peak: Color::rgb(0xa8, 0x43, 0x3f),
                note: Color::rgb(0x37, 0x52, 0x78),
                note_selected: Color::rgb(0x49, 0x6f, 0xa4),
                key_white: Color::rgb(0xf5, 0xf9, 0xfa),
                key_black: Color::rgb(0x23, 0x36, 0x50),
                row_accidental: Color::rgb(0xd8, 0xe4, 0xe7),
                row_dead: Color::rgb(0xc2, 0xcc, 0xcf),
                key_dead: Color::rgb(0xcf, 0xd8, 0xda),
                row_out_of_scale: Color::rgb(0xcf, 0xd9, 0xdc),
                row_scale_root: Color::rgb(0xd2, 0xe8, 0xf0),
                note_silent: Color::rgb(0x9c, 0xac, 0xbb),
                param_automated: Color::rgb(0x8a, 0x55, 0x14),
                // Darker, for the same reason every other ink is on a light
                // ground: a violet that reads on charcoal is a smudge on paper.
                modulation: Color::rgb(0x6b, 0x3f, 0xa8),
                mod_envelope: Color::rgb(0x6b, 0x3f, 0xa8),
                mod_lfo: Color::rgb(0x18, 0x6f, 0xa0),
                mod_macro: Color::rgb(0xa8, 0x6a, 0x10),
                mod_note: Color::rgb(0x2e, 0x8a, 0x3a),
                mod_performance: Color::rgb(0xb0, 0x3a, 0x6a),
            },
            metrics: METRICS,
            font: FontTokens {
                family: "sans-serif".to_string(),
                size: 13.0,
                line_height: 1.35,
                display: None,
            },
            fonts: Vec::new(),
            backdrops: Backdrops::default(),
        }
    }

    pub fn to_json(&self) -> String {
        let mut theme = self.clone();
        theme.format_version = THEME_FORMAT_VERSION;
        // Pretty, because the format exists to be edited.
        serde_json::to_string_pretty(&theme).expect("a Theme is always serialisable")
    }

    /// Reads a theme, checking its version before its body.
    pub fn from_json(text: &str) -> Result<Self, ThemeError> {
        let json: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| ThemeError::Format(format!("this is not readable JSON: {e}")))?;

        let found = json
            .get("format_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                ThemeError::Format(
                    "this file has no format_version — it is not a Fontelle theme".to_string(),
                )
            })? as u32;
        if found > THEME_FORMAT_VERSION {
            return Err(ThemeError::FromTheFuture {
                found,
                newest: THEME_FORMAT_VERSION,
            });
        }
        let json = migrate(json, found)?;

        let theme: Self = serde_json::from_value(json)
            .map_err(|e| ThemeError::Format(format!("this theme could not be read: {e}")))?;
        // What serde cannot say: a shader is handed four colours and eight
        // numbers, and a fifth would be silently dropped.
        for (panel, shader) in theme.backdrops.shaders() {
            if shader.colors.len() > 4 {
                return Err(ThemeError::Format(format!(
                    "the {} section's shader has {} colours; a shader takes at most four",
                    panel.label(),
                    shader.colors.len()
                )));
            }
            if shader.params.len() > 8 {
                return Err(ThemeError::Format(format!(
                    "the {} section's shader has {} numbers; a shader takes at most eight",
                    panel.label(),
                    shader.params.len()
                )));
            }
        }
        Ok(theme)
    }

    /// Each shader in the theme that will not compile, by its section, with
    /// the compiler's reason. Its section shows its plain colour instead;
    /// this is so the person wearing it can be told why.
    pub fn shader_problems(&self) -> Vec<(BackdropPanel, String)> {
        self.backdrops
            .shaders()
            .filter_map(|(panel, layer)| {
                let why = match crate::backdrop::source_of(&layer.shader) {
                    Some(body) => crate::backdrop::validate(&body).err()?,
                    None => format!("there is no built-in shader called {}", layer.shader),
                };
                Some((panel, why))
            })
            .collect()
    }

    pub fn load_from_file(path: &Path) -> Result<Self, ThemeError> {
        let text = std::fs::read_to_string(path).map_err(|source| ThemeError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_json(&text).map_err(|e| match e {
            // Say which file, since a theme is loaded by path from settings and
            // the path is the only way to tell two of them apart.
            ThemeError::Format(why) => ThemeError::Format(format!("{}: {why}", path.display())),
            other => other,
        })
    }
}

/// The one set of metrics both defaults share.
const METRICS: Metrics = Metrics {
    panel_margin: 8.0,
    panel_header_height: 26.0,
    panel_padding: 8.0,
    border_width: 1.0,
    corner_radius: 4.0,
    row_height: 22.0,
    transport_bar_height: 34.0,
    sidebar_width: 248.0,
};

/// Brings a theme written by an older build up to [`THEME_FORMAT_VERSION`],
/// one step per revision, each rewriting the document from `N` to `N + 1`.
///
/// The rule this follows, and the reason it is not just `serde(default)`: a
/// missing token is only allowed to be filled in *by a migration that says
/// which version it is filling it in for*. `deny_unknown_fields` plus required
/// fields is what makes a typo in a hand-edited theme an error rather than a
/// silently ignored line, and defaulting everything would give that up
/// everywhere to solve it in one place. See `fontelle_model::storage`, which
/// does the same for the document.
fn migrate(mut json: serde_json::Value, mut from: u32) -> Result<serde_json::Value, ThemeError> {
    if from == 0 {
        // v1 added the transport bar (item 7 of `docs/first-usable-plan.md`).
        // A v0 file predates it and cannot have an opinion about its height.
        if let Some(metrics) = json.get_mut("metrics").and_then(|m| m.as_object_mut()) {
            metrics
                .entry("transport_bar_height")
                .or_insert_with(|| serde_json::json!(METRICS.transport_bar_height));
        }
        from = 1;
    }

    if from == 1 {
        // v2 added the piano roll's own colours (item 8). A v1 file was
        // written before there was a roll to have an opinion about.
        let dark = Theme::dark_default().palette;
        if let Some(palette) = json.get_mut("palette").and_then(|p| p.as_object_mut()) {
            for (key, value) in [
                ("note", dark.note),
                ("note_selected", dark.note_selected),
                ("key_white", dark.key_white),
                ("key_black", dark.key_black),
                ("row_accidental", dark.row_accidental),
            ] {
                palette
                    .entry(key)
                    .or_insert_with(|| serde_json::json!(value.to_hex()));
            }
        }
        from = 2;
    }

    if from == 2 {
        // v3 added the docked sidebar (item 9). A v2 file was written when the
        // window was a transport bar and one panel.
        if let Some(metrics) = json.get_mut("metrics").and_then(|m| m.as_object_mut()) {
            metrics
                .entry("sidebar_width")
                .or_insert_with(|| serde_json::json!(METRICS.sidebar_width));
        }
        from = 3;
    }

    if from == 3 {
        // v4 added the key map's colours: the roll now greys the keys the
        // instrument on the channel cannot play, and names the ones a drum
        // kit can. A v3 file was written when every row looked alike.
        let dark = Theme::dark_default().palette;
        if let Some(palette) = json.get_mut("palette").and_then(|p| p.as_object_mut()) {
            for (key, value) in [
                ("row_dead", dark.row_dead),
                ("key_dead", dark.key_dead),
                ("note_silent", dark.note_silent),
            ] {
                palette
                    .entry(key)
                    .or_insert_with(|| serde_json::json!(value.to_hex()));
            }
        }
        from = 4;
    }

    if from == 4 {
        // v5 split the grid into three levels. A v4 file had two, and its
        // `grid_line` meant "beat *and* subdivision" — so the new token is
        // filled in from the default and the old one keeps whatever the file
        // said, which is the level it was mostly describing.
        let dark = Theme::dark_default().palette;
        if let Some(palette) = json.get_mut("palette").and_then(|p| p.as_object_mut()) {
            palette
                .entry("grid_line_sub")
                .or_insert_with(|| serde_json::json!(dark.grid_line_sub.to_hex()));
        }
        from = 5;
    }

    if from == 5 {
        // v6 gave a control under automation a ring of its own (§12.2). A v5
        // file was written when every knob looked alike whether a lane owned
        // it or not, so it cannot have an opinion about the colour.
        let dark = Theme::dark_default().palette;
        if let Some(palette) = json.get_mut("palette").and_then(|p| p.as_object_mut()) {
            palette
                .entry("param_automated")
                .or_insert_with(|| serde_json::json!(dark.param_automated.to_hex()));
        }
        from = 6;
    }

    if from == 6 {
        // v7 gave a modulated control an arc of its own (§8.1 rule 6). A v6
        // file was written before there was a matrix to draw one from.
        let dark = Theme::dark_default().palette;
        if let Some(palette) = json.get_mut("palette").and_then(|p| p.as_object_mut()) {
            palette
                .entry("modulation")
                .or_insert_with(|| serde_json::json!(dark.modulation.to_hex()));
        }
        from = 7;
    }

    if from == 7 {
        // v8 gave every modulation source its own ring ink
        // (`docs/flopsynth-next.md` §3.3). A v7 file was written when every
        // ring was the matrix's violet, and cannot have an opinion about
        // the other four.
        let dark = Theme::dark_default().palette;
        if let Some(palette) = json.get_mut("palette").and_then(|p| p.as_object_mut()) {
            for (key, value) in [
                ("mod_envelope", dark.mod_envelope),
                ("mod_lfo", dark.mod_lfo),
                ("mod_macro", dark.mod_macro),
                ("mod_note", dark.mod_note),
                ("mod_performance", dark.mod_performance),
            ] {
                palette
                    .entry(key)
                    .or_insert_with(|| serde_json::json!(value.to_hex()));
            }
        }
        from = 8;
    }

    if from == 8 {
        // v9 added the scale tool's row inks. A v8 file was written before
        // a song had a key.
        let dark = Theme::dark_default().palette;
        if let Some(palette) = json.get_mut("palette").and_then(|p| p.as_object_mut()) {
            for (key, value) in [
                ("row_out_of_scale", dark.row_out_of_scale),
                ("row_scale_root", dark.row_scale_root),
            ] {
                palette
                    .entry(key)
                    .or_insert_with(|| serde_json::json!(value.to_hex()));
            }
        }
        from = 9;
    }

    if from == 9 {
        // v10 added backdrops, which are optional: a v9 file has none.
        from = 10;
    }

    if from == 10 {
        // v11 made each section a stack of layers. A v10 section was one
        // picture, which is a stack of one picture layer.
        if let Some(backdrops) = json.get_mut("backdrops").and_then(|b| b.as_object_mut()) {
            for (_, section) in backdrops.iter_mut() {
                if let Some(picture) = section.as_object_mut() {
                    let mut layer = serde_json::Map::new();
                    layer.insert("kind".to_string(), serde_json::json!("image"));
                    layer.extend(std::mem::take(picture));
                    *section = serde_json::Value::Array(vec![serde_json::Value::Object(layer)]);
                }
            }
        }
        from = 11;
    }

    if from != THEME_FORMAT_VERSION {
        return Err(ThemeError::Format(format!(
            "no migration from theme format version {from} to {THEME_FORMAT_VERSION}"
        )));
    }
    // A migrated theme *is* a current-version theme. Leaving the old stamp on
    // it would make the loaded value disagree with what it now contains, and
    // the next thing to write it out would silently claim a version it had
    // already left.
    if let Some(object) = json.as_object_mut() {
        object.insert(
            "format_version".to_string(),
            serde_json::json!(THEME_FORMAT_VERSION),
        );
    }
    Ok(json)
}

#[derive(Debug)]
pub enum ThemeError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Written by a build newer than this one.
    FromTheFuture {
        found: u32,
        newest: u32,
    },
    Format(String),
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::FromTheFuture { found, newest } => write!(
                f,
                "this theme is in format version {found}, and this build of Fontelle \
                 understands up to version {newest} — upgrade Fontelle to use it"
            ),
            Self::Format(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ThemeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

// -------------------------------------------------------- the notepad's inks

/// What one notepad theme is painted in (`docs/effects-catalogue.md` §2.8).
///
/// Six colours rather than a whole [`Palette`]: a pad is a ground, a page,
/// words, quiet words, a caret and a rule — and a device that invented
/// twenty-eight tokens of its own would be a second theme system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotepadInk {
    /// Behind the sheet, out to the window's edges.
    pub ground: Color,
    /// The sheet itself.
    pub paper: Color,
    /// The words on it.
    pub ink: Color,
    /// The quiet ones: the footer's captions, the page counter, the rules.
    pub faint: Color,
    /// The caret, and the wash a selection is drawn in.
    pub caret: Color,
    /// The line round the sheet.
    pub edge: Color,
}

/// The seven looks.
///
/// Fixed rather than derived from the studio's palette — except *studio*,
/// which is exactly the case for somebody who wants the pad to disappear into
/// the window around it. The other six are the terminals they are named
/// after, and each is held to WCAG's 4.5:1 for its words by
/// `fontelle-ui/tests/notepad.rs`: a lyric sheet is read from further away
/// than the rest of the chrome, while its author is singing.
pub fn notepad_ink(theme: fontelle_types::NotepadTheme, palette: &Palette) -> NotepadInk {
    use fontelle_types::NotepadTheme as T;
    match theme {
        // The VT220 everybody pictures. The ground is a shade off the page so
        // the sheet reads as a sheet rather than as the whole window.
        T::Phosphor => NotepadInk {
            ground: Color::rgb(0x04, 0x0a, 0x06),
            paper: Color::rgb(0x07, 0x14, 0x0b),
            ink: Color::rgb(0x62, 0xf5, 0x92),
            faint: Color::rgb(0x2f, 0x7d, 0x4e),
            caret: Color::rgb(0x9d, 0xff, 0xc2),
            edge: Color::rgb(0x18, 0x3d, 0x28),
        },
        // The other CRT: amber on a warm black, which is the one people who
        // stare at a terminal all night tend to end up on.
        T::Amber => NotepadInk {
            ground: Color::rgb(0x0b, 0x07, 0x02),
            paper: Color::rgb(0x18, 0x0f, 0x04),
            ink: Color::rgb(0xff, 0xb7, 0x4d),
            faint: Color::rgb(0x8a, 0x5c, 0x21),
            caret: Color::rgb(0xff, 0xd9, 0x9b),
            edge: Color::rgb(0x45, 0x2d, 0x0e),
        },
        // Lights on: dark ink on warm paper. Not white — a full-brightness
        // page beside a dark studio is a torch.
        T::Paper => NotepadInk {
            ground: Color::rgb(0xd8, 0xd2, 0xc4),
            paper: Color::rgb(0xf2, 0xed, 0xe0),
            ink: Color::rgb(0x2a, 0x26, 0x1e),
            faint: Color::rgb(0x6d, 0x66, 0x57),
            caret: Color::rgb(0x4a, 0x6b, 0x8a),
            edge: Color::rgb(0xbe, 0xb6, 0xa4),
        },
        T::Ice => NotepadInk {
            ground: Color::rgb(0x03, 0x08, 0x12),
            paper: Color::rgb(0x08, 0x12, 0x20),
            ink: Color::rgb(0x9c, 0xd4, 0xff),
            faint: Color::rgb(0x4a, 0x74, 0x9c),
            caret: Color::rgb(0xd6, 0xec, 0xff),
            edge: Color::rgb(0x1b, 0x35, 0x4e),
        },
        T::Rose => NotepadInk {
            ground: Color::rgb(0x0e, 0x04, 0x0d),
            paper: Color::rgb(0x1a, 0x08, 0x18),
            ink: Color::rgb(0xff, 0x9e, 0xd8),
            faint: Color::rgb(0x96, 0x4e, 0x7e),
            caret: Color::rgb(0xff, 0xd2, 0xee),
            edge: Color::rgb(0x46, 0x1c, 0x3b),
        },
        // No colour at all, for somebody who wants the words and nothing
        // else.
        T::Slate => NotepadInk {
            ground: Color::rgb(0x08, 0x08, 0x09),
            paper: Color::rgb(0x13, 0x13, 0x15),
            ink: Color::rgb(0xe4, 0xe4, 0xe7),
            faint: Color::rgb(0x7b, 0x7b, 0x82),
            caret: Color::rgb(0xff, 0xff, 0xff),
            edge: Color::rgb(0x34, 0x34, 0x39),
        },
        // The studio's own inks, so the pad matches the window around it —
        // and follows it into a light theme, which is the one look here that
        // is not a fixed set of colours.
        T::Studio => NotepadInk {
            ground: palette.window,
            paper: palette.panel,
            ink: palette.text,
            faint: palette.text_muted,
            caret: palette.accent,
            edge: palette.border,
        },
    }
}
