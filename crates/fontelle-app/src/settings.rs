//! Where Fontelle keeps things, and how the user changes it (TDD §17.3, §18).
//!
//! # INVARIANT 10
//!
//! > Fontelle writes nothing outside locations the user has explicitly
//! > configured, except its own config directory.
//!
//! Everything here exists to keep that true and checkable. Two consequences
//! worth stating plainly, because both were decisions:
//!
//! - **Nothing defaults to `~/Documents`, `~/Music`, or anywhere else that
//!   belongs to the user.** The only paths this module invents are under the
//!   XDG config and data directories, which are Fontelle's own.
//! - **The soundfont bank defaults to `$XDG_DATA_HOME/fontelle/soundfonts`**,
//!   and Fontelle creates it — once, on first run, saying so. That is a write
//!   outside the *config* directory, so it is a deliberate reading of the
//!   invariant rather than a strict one: the XDG data directory is as much
//!   Fontelle's own as the config directory is, and "a folder you drop your
//!   soundfonts into" cannot exist without something creating it. Any other
//!   folder is added by the user, with `--soundfonts <dir>`, and is remembered
//!   here. **Flag this to the owner if it is the wrong reading** — it is one
//!   function and one line of `create_dir_all`.
//!
//! The full first-run wizard (§18) is M7. This is its smallest useful half: a
//! file the user can edit, with the paths in it, that Fontelle reads and never
//! silently rewrites.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Likewise: what a star on a menu row remembers.
pub use fontelle_types::Favorite;
/// Re-exported so the settings file's own vocabulary is in one place, while
/// the window — which may not depend on this crate — can still name it.
pub use fontelle_types::FolderKind;

/// The revision of the settings file this build writes. Its own number, like
/// the theme's and the project's — where Fontelle keeps things has nothing to
/// do with either.
///
/// Two since the settings file grew [`MidiInputSettings`], three since it grew
/// the two import folders, four since it grew the favourites, six since it
/// grew the recent projects and the update switch, seven since it grew the
/// three a shared song needs (`docs/collab-plan.md` §10.4), eight since it
/// grew how much a theme's backdrops may move (hub card 0366), nine since it
/// grew the audio output's backend, device and buffer, ten since it grew
/// Compatible plugin graphics. Every added field carries
/// `#[serde(default)]`, so an older file still reads — the bump is so that an
/// *older build* handed a newer file says "upgrade Fontelle" rather than
/// "unknown field `midi_dir`".
pub const SETTINGS_FORMAT_VERSION: u32 = 10;

/// How many projects the start menu remembers. A menu's worth: past this a
/// list stops being something you glance at and becomes something you search.
pub const RECENT_PROJECTS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub format_version: u32,
    /// Every folder the browser scans for `.sf2` files, in the order they are
    /// searched. §17.5's "one or more soundfont directories".
    pub soundfont_dirs: Vec<PathBuf>,
    /// Where the file dialogs start. `None` means "ask" — never a guess.
    pub projects_dir: Option<PathBuf>,
    /// A theme file, if the user has pointed at one (§16.6).
    pub theme: Option<PathBuf>,
    /// The look chosen on the settings page, by its name in the theme
    /// library (`themes.rs`). `None` is the default. `default` so a file
    /// written before there was a choice is not a broken one.
    #[serde(default)]
    pub theme_name: Option<String>,
    /// How live MIDI input is read (§14.3).
    ///
    /// `default` rather than required, because a settings file written before
    /// this existed is not a broken one — and because the default is the
    /// identity, so a person who never opens the tab is in exactly the state
    /// they were in before it did.
    #[serde(default)]
    pub midi_input: MidiInputSettings,
    /// Where `.mid` files are kept, for the roll's *Import MIDI*.
    ///
    /// `None` means "ask", and never a guess: INVARIANT 10 says Fontelle
    /// touches nothing the user has not named, and `~/Music` is as much
    /// somebody's own folder as any other. The window's answer to `None` is
    /// to send you here rather than to browse something you did not choose.
    #[serde(default)]
    pub midi_dir: Option<PathBuf>,
    /// Where FL Studio's `.fsc` score files are kept.
    ///
    /// Usually somewhere inside an FL Studio installation, which is exactly
    /// why it cannot be guessed: on this machine it is under a Wine prefix on
    /// a second disk, and on the next one it will be somewhere else again.
    #[serde(default)]
    pub score_dir: Option<PathBuf>,
    /// Where sounds and loops are kept, for the arrangement's *Import audio*
    /// (TDD §15).
    ///
    /// `None` means "ask", like the other two and for INVARIANT 10's reason:
    /// Fontelle touches nothing the user has not named.
    #[serde(default)]
    pub audio_dir: Option<PathBuf>,
    /// Where the user's own presets are kept (`docs/flopsynth-plan.md` §P.3).
    ///
    /// `None` is not "ask" here, unlike the three above, and the difference is
    /// which way the files move. Those three name a folder somebody *already
    /// filled* and Fontelle would be guessing to pick one; this is a folder
    /// Fontelle **writes**, and a program that cannot say where "Save as…"
    /// puts a file has no Save as…. So `None` means
    /// [`default_preset_dir`](Settings::default_preset_dir) — Fontelle's own
    /// data directory, which INVARIANT 10 already lets it own, the same
    /// position the soundfont bank's default takes.
    #[serde(default)]
    pub preset_dir: Option<PathBuf>,
    /// Folders to look in for plugins, **beyond** the ones the format
    /// nominates (TDD §8.4).
    ///
    /// The standard locations are searched without being listed here — CLAP
    /// names them, and a host that made somebody type them in would be asking
    /// them to know where their own installer put things. This is for the
    /// other case: a collection on a second disk, a build directory, a Wine
    /// prefix. The same reason `score_dir` exists.
    ///
    /// `default` so a settings file written before plugins could be hosted is
    /// not a broken one.
    #[serde(default)]
    pub plugin_dirs: Vec<PathBuf>,
    /// What has been starred: the effects, instruments and plugins the menus
    /// put first and draw lit. See [`fontelle_types::Favorite`] for why this
    /// is here and not in the project.
    ///
    /// In the order they were starred. A list rather than a set so the file
    /// is stable under a text editor, and short enough that it does not
    /// matter — see [`Settings::toggle_favorite`], which keeps it one of each.
    #[serde(default)]
    pub favorites: Vec<Favorite>,
    /// The keyboard shortcuts that differ from the defaults, by action id
    /// — `"play": "Ctrl+P"`, and `""` for one that has been unbound. Only
    /// the differences: the defaults are the window's (`fontelle-ui`'s
    /// `canvas::keymap`), and a file that copied them would go stale the
    /// day one moved. A `BTreeMap` so the file is stable under a text
    /// editor. `default` so a file written before shortcuts could be changed
    /// is not a broken one.
    #[serde(default)]
    pub keybinds: std::collections::BTreeMap<String, String>,
    /// The projects the start menu lists, newest first — every bundle this
    /// machine last opened, made or saved under a name. See
    /// [`Settings::remember_project`] for the shape of the list.
    ///
    /// Here rather than anywhere else because it is a fact about this
    /// machine: which files on *this* disk were touched, not a property of
    /// any one of them.
    #[serde(default)]
    pub recent_projects: Vec<PathBuf>,
    /// Whether the start menu asks GitHub for a newer release at launch.
    ///
    /// On by default: a person who never opens the settings tab should still
    /// hear about a new release, which is half of what a start menu is for.
    /// Off is a real choice — a DAW that talks to the network at every launch
    /// is something some people rightly want to switch off — and the menu
    /// says the check is off rather than drawing nothing.
    #[serde(default = "yes")]
    pub check_for_updates: bool,
    /// Whether the first-run offer to install an extension has been answered
    /// — either way, so it is shown once and then not again
    /// (`docs/vst-plan.md` §4.2).
    #[serde(default)]
    pub extensions_offered: bool,
    /// Flopsynth's window scale (`docs/flopsynth-next.md` §3.2), in
    /// percent: one of the window's `SCALES`, multiplying every design size,
    /// and the size the window opens at. A setting rather than window state
    /// because a person who chose 75 % for a small screen chose it for every
    /// session. A whole number so the file reads `125` and the struct stays
    /// `Eq`; skipped at 100 so a file that never chose says nothing.
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub flopsynth_scale_percent: u16,
    /// The **notepad's** theme: the last one chosen, which is what a new
    /// notepad insert opens in (`docs/effects-catalogue.md` §2.8).
    ///
    /// > *"it should have themes you can switch between and it should alwasy
    /// > save your default preferred theme as your last one selected."*
    ///
    /// A setting rather than document state, exactly as
    /// [`flopsynth_scale_percent`](Self::flopsynth_scale_percent) is:
    /// somebody who chose amber chose it for every session and every project,
    /// not for the one pad they happened to be looking at. The pad's *own*
    /// theme stays in the project, because two pads may differ.
    ///
    /// Written as the theme's **name** rather than its position, so that
    /// adding a theme in the middle of the list cannot silently change
    /// somebody's default; a name this build does not know reads as the first
    /// theme rather than as a file it refuses to open. Skipped when nothing
    /// has been chosen, so a file written by somebody who never opened a
    /// notepad says nothing about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notepad_theme: Option<String>,
    /// What the people this studio shares a song with see it called
    /// (`docs/collab-plan.md` §10.4, decision 7). `None` is the computer's
    /// own user name — see [`Settings::your_name`] — and the first Share or
    /// Join asks, since a login name is often not what anybody is called.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// A relay of somebody's own to share through, as `host:port`; `None` is
    /// Floptle Cloud (§9.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    /// This studio, as the record of what two copies last agreed on names it
    /// (§4.5). Minted the first time it is asked for and kept — see
    /// [`Settings::install_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<fontelle_types::PersistentId>,
    /// What a new song is routed as — rack-style or lane-style
    /// (`docs/ux-routing-and-learning-plan.md` §1). `None` until the first
    /// new song asks; the answer is kept here, and the settings page's
    /// Project section changes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_song_routing: Option<fontelle_model::RoutingMode>,
    /// Whether the start menu's first-launch offer of the tour has been
    /// answered — either way, so it is made once (`docs/ux-routing-and-
    /// learning-plan.md` §5: offered, *never starting by itself*).
    #[serde(default)]
    pub tour_offered: bool,
    /// Which backend, device and buffer the studio plays through
    /// ([`AudioOutputSettings`]). All automatic until chosen.
    #[serde(default, skip_serializing_if = "AudioOutputSettings::is_automatic")]
    pub audio_output: AudioOutputSettings,
    /// How much a theme's backdrops move: Moving, Still or Off. `None` until
    /// chosen, which follows the desktop's reduce-motion switch where one can
    /// be read ([`Settings::backdrop_motion`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrop_effects: Option<fontelle_ui::backdrop::Effects>,
    /// The most frames a second a moving backdrop draws.
    #[serde(default = "thirty")]
    pub backdrop_fps: u16,
    /// A backdrop's resolution, as a percentage of the window's.
    #[serde(default = "fifty")]
    pub backdrop_scale_percent: u16,
    /// Hold every backdrop still while the transport plays. Off by default:
    /// the music is the point.
    #[serde(default)]
    pub hold_backdrops_while_playing: bool,
    /// Keep backdrops moving while another window has the focus. On by
    /// default: Ty likes *"lots of moving windows"*.
    #[serde(default = "yes")]
    pub backdrops_when_unfocused: bool,
    /// Plugin windows that draw with EGL draw with Mesa's, from the next
    /// start (`fontelle_host::gui::egl_vendor_for`). Off by default, and
    /// Linux's alone: Vital's window aborts the studio on NVIDIA's EGL under
    /// X11 and draws on Mesa's (`fontelle_host::alpha_egl`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compatible_plugin_graphics: bool,
}

fn thirty() -> u16 {
    30
}

fn fifty() -> u16 {
    50
}

/// The Backdrop rate row's steps, in frames a second.
pub const BACKDROP_RATES: [u16; 4] = [15, 24, 30, 60];

/// The Backdrop resolution row's steps: a percentage of the window's, and
/// what the row calls it.
pub const BACKDROP_RESOLUTIONS: [(u16, &str); 4] = [
    (25, "Quarter"),
    (50, "Half"),
    (75, "Three quarters"),
    (100, "Full"),
];

fn yes() -> bool {
    true
}

fn hundred() -> u16 {
    100
}

fn is_hundred(percent: &u16) -> bool {
    *percent == 100
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format_version: SETTINGS_FORMAT_VERSION,
            soundfont_dirs: Vec::new(),
            projects_dir: None,
            theme_name: None,
            theme: None,
            midi_input: MidiInputSettings::default(),
            midi_dir: None,
            score_dir: None,
            audio_dir: None,
            preset_dir: None,
            plugin_dirs: Vec::new(),
            favorites: Vec::new(),
            keybinds: std::collections::BTreeMap::new(),
            recent_projects: Vec::new(),
            check_for_updates: true,
            extensions_offered: false,
            flopsynth_scale_percent: 100,
            notepad_theme: None,
            display_name: None,
            relay: None,
            install: None,
            new_song_routing: None,
            tour_offered: false,
            audio_output: AudioOutputSettings::default(),
            backdrop_effects: None,
            backdrop_fps: thirty(),
            backdrop_scale_percent: fifty(),
            hold_backdrops_while_playing: false,
            backdrops_when_unfocused: true,
            compatible_plugin_graphics: false,
        }
    }
}

impl Settings {
    /// How much the window's backdrops may move. `reduce_motion` is the
    /// desktop's own switch (`desktop::reduce_motion`): with Effects never
    /// chosen, a desktop that asks for less motion starts Fontelle Still.
    pub fn backdrop_motion(&self, reduce_motion: bool) -> fontelle_ui::backdrop::Motion {
        use fontelle_ui::backdrop::{Effects, Motion};
        Motion {
            effects: self.backdrop_effects.unwrap_or(if reduce_motion {
                Effects::Still
            } else {
                Effects::Moving
            }),
            fps: f32::from(self.backdrop_fps.clamp(5, 60)),
            scale: f32::from(self.backdrop_scale_percent.clamp(25, 100)) / 100.0,
            hold_while_playing: self.hold_backdrops_while_playing,
            when_unfocused: self.backdrops_when_unfocused,
        }
    }

    /// The name the people this studio shares with see: the one typed on the
    /// settings page, or the computer's user name, or — with neither — a word
    /// rather than nothing.
    pub fn your_name(&self) -> String {
        self.display_name
            .clone()
            .or_else(|| {
                ["USER", "USERNAME", "LOGNAME"]
                    .iter()
                    .filter_map(|var| std::env::var(var).ok())
                    .map(|name| name.trim().to_string())
                    .find(|name| !name.is_empty())
            })
            .unwrap_or_else(|| "Someone".to_string())
    }

    /// This studio's id, minted the first time it is asked for. The caller
    /// saves the settings when this was `None` before.
    pub fn install_id(&mut self) -> fontelle_types::PersistentId {
        *self
            .install
            .get_or_insert_with(fontelle_types::PersistentId::new)
    }

    /// Puts `path` at the top of the recent list.
    ///
    /// One entry per path, so a project opened ten times is one row and not
    /// ten; newest first, so the row at the top is the one you were in last;
    /// and no longer than [`RECENT_PROJECTS`], the oldest falling off.
    pub fn remember_project(&mut self, path: &Path) {
        self.forget_project(path);
        self.recent_projects.insert(0, path.to_path_buf());
        self.recent_projects.truncate(RECENT_PROJECTS);
    }

    /// Takes `path` out of the recent list, if it is in it.
    pub fn forget_project(&mut self, path: &Path) {
        self.recent_projects.retain(|p| p != path);
    }

    /// Whether `favorite` has been starred.
    pub fn is_favorite(&self, favorite: &Favorite) -> bool {
        self.favorites.contains(favorite)
    }

    /// Stars `favorite` if it is not, and un-stars it if it is. Whether it is
    /// a favourite **afterwards** — what the status line says.
    ///
    /// A toggle rather than an add and a remove, because a star is one
    /// control pressed one way: the same press on the same star takes it back
    /// off. Every copy goes when it goes, so a list that somehow held two of
    /// something cannot leave one behind.
    pub fn toggle_favorite(&mut self, favorite: Favorite) -> bool {
        if self.is_favorite(&favorite) {
            self.favorites.retain(|f| *f != favorite);
            false
        } else {
            self.favorites.push(favorite);
            true
        }
    }
}

/// What a MIDI keyboard's notes are read as (TDD §14.3).
///
/// Reported from playing one: *"every single input device will register
/// differently, we need to expose options like this for users."* The engine's
/// own shape for this is [`fontelle_midi::InputSettings`], which is built to
/// cross onto a device callback thread in one atomic; this is the shape that
/// goes in the file, which is a different job — it is read by a person with a
/// text editor, so the curve is a name and not a tag byte, and `Fixed`'s value
/// is a field of its own rather than a payload that vanishes when the curve is
/// anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MidiInputSettings {
    pub velocity_curve: VelocityCurveSetting,
    /// The velocity window a note must fall inside to be played at all, both
    /// ends inclusive. `(0, 127)` lets everything through.
    pub velocity_min: u8,
    pub velocity_max: u8,
    /// What [`VelocityCurveSetting::Fixed`] plays at. Kept even while another
    /// curve is selected, so switching away and back does not lose it.
    pub fixed_velocity: u8,
    pub transpose_semitones: i8,
    /// Which MIDI channel to listen to, counted from 1 as every keyboard's
    /// own front panel counts it. `None` is all sixteen.
    pub channel_filter: Option<u8>,
}

impl Default for MidiInputSettings {
    /// The identity, per §14.3: per-device config is an "optional refinement,
    /// never required setup".
    fn default() -> Self {
        Self {
            velocity_curve: VelocityCurveSetting::Linear,
            velocity_min: 0,
            velocity_max: 127,
            fixed_velocity: 100,
            transpose_semitones: 0,
            channel_filter: None,
        }
    }
}

/// The audio output as the person chose it on the settings page. `None` is
/// "automatic" in each: the system's default backend and device, and
/// [`fontelle_engine::DEFAULT_OUTPUT_BUFFER`] frames.
///
/// Reported: *"audio drivers not configurable enough so pretty sure its
/// defaulting to default audio drivers for a lot of users causing things to
/// sound like failing audio drivers sometimes"*. Names rather than indices,
/// like an input: a device list is not in the same order twice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AudioOutputSettings {
    /// A backend by its name: "ALSA", "JACK", "PulseAudio", "WASAPI",
    /// "CoreAudio".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// A device of that backend, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Frames per buffer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_frames: Option<u32>,
}

impl AudioOutputSettings {
    pub fn is_automatic(&self) -> bool {
        *self == Self::default()
    }

    /// What the engine is asked to open.
    pub fn choice(&self) -> fontelle_engine::OutputChoice {
        fontelle_engine::OutputChoice {
            host: self.host.clone(),
            device: self.device.clone(),
            buffer_frames: self.buffer_frames,
        }
    }
}

/// The buffer sizes the settings page offers besides Automatic.
pub const OUTPUT_BUFFER_SIZES: [u32; 6] = [64, 128, 256, 512, 1024, 2048];

/// A buffer size as the settings page says it: "256 frames (5.3 ms)", or
/// "Automatic (512 frames, 10.7 ms)".
pub fn buffer_label(frames: Option<u32>) -> String {
    let rate = crate::SAMPLE_RATE;
    match frames {
        Some(frames) => format!(
            "{frames} frames ({})",
            fontelle_engine::latency_label(frames, rate)
        ),
        None => {
            let frames = fontelle_engine::DEFAULT_OUTPUT_BUFFER;
            format!(
                "Automatic ({frames} frames, {})",
                fontelle_engine::latency_label(frames, rate)
            )
        }
    }
}

/// Which shape a velocity is read through — the file's spelling of
/// [`fontelle_midi::VelocityCurve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VelocityCurveSetting {
    /// What the keyboard sent, unchanged.
    #[default]
    Linear,
    /// Quiet playing reads louder — the same effort reaches further up the
    /// range, which is what a light action wants.
    Soft,
    /// Quiet playing reads quieter, for a keyboard that is too eager.
    Hard,
    /// Every note at one velocity, whatever was played. What an organ patch
    /// wants, and what a keyboard with a broken sensor needs.
    Fixed,
}

impl VelocityCurveSetting {
    /// What the settings tab writes in the row's value column.
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Soft => "Soft",
            Self::Hard => "Hard",
            Self::Fixed => "Fixed",
        }
    }

    /// The four in the order the tab steps through them.
    pub const ALL: [Self; 4] = [Self::Linear, Self::Soft, Self::Hard, Self::Fixed];
}

/// One row of the settings tab (TDD §18).
///
/// The rows are an enum rather than an index into a table because what a step
/// *means* differs per row — a choice wraps and a number stops — and because
/// the window addresses them by position: `nudge_setting(3, +1)` has to reach
/// the same setting the third row drew, and an enum in a `const` array is the
/// one shape where those cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRow {
    /// Points Fontelle at a folder of plugins, on top of the ones CLAP
    /// nominates (TDD §8.4). A button: a click opens a picker.
    PluginFolder,
    /// The `n`th folder in [`Settings::plugin_dirs`], listed under the add
    /// button. A button: a click removes it. One row per folder, so a
    /// second folder is no longer invisible except as a count
    /// (`docs/vst-plan.md` §5).
    PluginDir(usize),
    /// Adds the folders FL Studio searches — read off its own settings,
    /// see [`crate::daw_folders`]. A button.
    ImportFlFolders,
    /// Where "Save as…" puts a preset, and where the bank reads the user's
    /// own back from (§P.3).
    PresetFolder,
    /// Plugin windows that draw with EGL use Mesa's, from the next start.
    /// A switch; Linux only, and a sentence where Mesa's EGL is missing.
    CompatibleGraphics,
    /// Walks the plugin folders again. Also a button.
    ///
    /// Its own row because a scan `dlopen`s every bundle it finds, so it does
    /// not happen on its own — and because installing a plugin while Fontelle
    /// is open is the ordinary case, not an unusual one.
    RescanPlugins,
    /// The name the people you share with see (`docs/collab-plan.md` §10.4).
    /// Typed.
    YourName,
    /// The relay to share through: blank for Floptle Cloud, or a
    /// `host:port` of your own (§9.3). Typed.
    Relay,
    /// Which audio system the studio plays through: Automatic, or one of
    /// the backends this build has. A choice; the session answers its list.
    AudioBackend,
    /// Which output device of that backend. A choice; the session lists
    /// them.
    AudioDevice,
    /// Frames per buffer, each with its latency. A choice.
    AudioBuffer,
    /// The studio's sample rate. Nothing to press; the value says it.
    SampleRate,
    /// What is open now, as the engine says it. A button: open it again.
    OutputNow,
    /// Dropouts the backend reported since the output opened. A button:
    /// start the count again.
    Dropouts,
    /// A section title. Nothing to set, and a click does nothing — it is what
    /// says which of these settings belong together. It is deliberately *not*
    /// the only thing saying so: see [`SettingRow::Transpose`]'s label.
    Heading(&'static str),
    VelocityCurve,
    FixedVelocity,
    VelocityMin,
    VelocityMax,
    Transpose,
    ChannelFilter,
    /// A folder to import from. Clicking it opens a picker rather than
    /// stepping a value — see [`SettingRow::folder`].
    Folder(FolderKind),
    /// One catalogue extension, by its index in
    /// [`crate::extensions::CATALOGUE`]. A button: install it, or remove it,
    /// depending on its state (`docs/vst-plan.md` §4.2).
    Extension(usize),
    /// Whether the start menu asks GitHub for a newer release at launch
    /// (`updates.rs`). A switch: a click flips it, either direction.
    CheckForUpdates,
    /// The open song's routing, rack-style or lane-style. A choice; the
    /// session answers its value, since it is the song's and not a setting.
    SongRouting,
    /// What a new song starts as: ask, rack-style or lane-style. A choice.
    NewSongRouting,
    /// Which look the window wears, from the theme library. A choice; the
    /// session answers its list, since the library is a folder.
    Theme,
    /// How round the corners are, 0 to 12 px. A slider; changing a built-in
    /// look saves a copy of it.
    CornerRounding,
    /// A picture behind one panel, carried inside the theme. A button:
    /// choose one, or take it off.
    Backdrop(fontelle_ui::theme::BackdropPanel),
    /// How strongly the pictures show. A slider.
    PictureStrength,
    /// How see-through the panels are, so the window's picture shows
    /// through them. A slider; the ground under everything stays solid.
    PanelSeeThrough,
    /// Adds a `.fontelletheme` file to the library and wears it. A button.
    ImportTheme,
    /// Writes the look in use to a `.fontelletheme` file, to send to
    /// someone. A button.
    SaveTheme,
    /// Moving, Still or Off: how much a theme's backdrops move. A choice.
    BackdropEffects,
    /// The most frames a second a moving backdrop draws. A choice.
    BackdropRate,
    /// A backdrop's resolution against the window's. A choice.
    BackdropResolution,
    /// Hold backdrops still while the transport plays. A switch.
    HoldBackdrops,
    /// Keep backdrops moving while another window has the focus. A switch.
    BackdropsUnfocused,
    /// How a section's picture fits it: cover, contain, stretch, tile,
    /// natural size. A choice; listed only under a section that has one.
    PictureFit(fontelle_ui::theme::BackdropPanel),
    /// How big it is against what the fit gives, 10–300 %. A slider.
    PictureSize(fontelle_ui::theme::BackdropPanel),
    /// Where it sits: one of nine places. A choice.
    PicturePlace(fontelle_ui::theme::BackdropPanel),
}

/// The Picture position row's nine places, and the anchor each is.
pub const PICTURE_PLACES: [(&str, [f32; 2]); 9] = [
    ("Top left", [0.0, 0.0]),
    ("Top", [0.5, 0.0]),
    ("Top right", [1.0, 0.0]),
    ("Left", [0.0, 0.5]),
    ("Centre", [0.5, 0.5]),
    ("Right", [1.0, 0.5]),
    ("Bottom left", [0.0, 1.0]),
    ("Bottom", [0.5, 1.0]),
    ("Bottom right", [1.0, 1.0]),
];

/// The Picture size row's range: a tenth to three times.
pub const PICTURE_SIZE_RANGE: (f32, f32) = (0.1, 3.0);

/// Every row the settings tab shows, in the order it shows them.
///
/// One section today. More belong here — the theme, the autosave interval —
/// and adding one is a variant, a `label`, a `value` and a `nudge`, with
/// nothing in `fontelle-ui` to change: the window draws names and values and
/// knows what none of them mean.
pub const SETTING_ROWS: [SettingRow; 52] = [
    // First: if the studio does not sound right, nothing under it matters.
    SettingRow::Heading("Audio output"),
    SettingRow::AudioBackend,
    SettingRow::AudioDevice,
    SettingRow::AudioBuffer,
    SettingRow::SampleRate,
    SettingRow::OutputNow,
    SettingRow::Dropouts,
    SettingRow::Heading("MIDI input"),
    SettingRow::VelocityCurve,
    SettingRow::FixedVelocity,
    SettingRow::VelocityMin,
    SettingRow::VelocityMax,
    SettingRow::Transpose,
    SettingRow::ChannelFilter,
    // Their own heading, and not "MIDI input"'s: a row called "MIDI files"
    // under a heading about the keyboard reads as a property of the keyboard
    // rather than as a place on disk.
    SettingRow::Heading("Import from"),
    SettingRow::Folder(FolderKind::Midi),
    SettingRow::Folder(FolderKind::Scores),
    SettingRow::Folder(FolderKind::Audio),
    // Its own heading: a plugin folder is not somewhere files are imported
    // *from*, it is somewhere instruments and effects are found (TDD §8.4).
    SettingRow::Heading("Plugins"),
    SettingRow::PluginFolder,
    // The folder rows go here — see `setting_rows`, which is the list the
    // tab actually draws.
    SettingRow::ImportFlFolders,
    SettingRow::RescanPlugins,
    // Its own heading too, and for the same reason: a preset folder is not a
    // place files are imported from, it is the one folder in this list
    // Fontelle *writes* to.
    SettingRow::Heading("Presets"),
    SettingRow::PresetFolder,
    // Its own heading: an extension is a thing installed beside the product,
    // not a folder or a keyboard setting (`docs/vst-plan.md` §4.2). The
    // catalogue's rows go here — see `setting_rows`.
    SettingRow::Heading("Extensions"),
    // Sharing a song: who you are to the others, and through where.
    SettingRow::Heading("Sharing"),
    SettingRow::YourName,
    SettingRow::Relay,
    // How the window looks (`themes.rs`): the theme, its two easy knobs, its
    // pictures, and the way in and out for a `.fontelletheme` file.
    SettingRow::Heading("Appearance"),
    SettingRow::Theme,
    SettingRow::CornerRounding,
    SettingRow::Backdrop(fontelle_ui::theme::BackdropPanel::Window),
    SettingRow::Backdrop(fontelle_ui::theme::BackdropPanel::Transport),
    SettingRow::Backdrop(fontelle_ui::theme::BackdropPanel::Channels),
    SettingRow::Backdrop(fontelle_ui::theme::BackdropPanel::Browser),
    SettingRow::Backdrop(fontelle_ui::theme::BackdropPanel::Arrangement),
    SettingRow::Backdrop(fontelle_ui::theme::BackdropPanel::Roll),
    SettingRow::Backdrop(fontelle_ui::theme::BackdropPanel::Mixer),
    SettingRow::PictureStrength,
    SettingRow::PanelSeeThrough,
    // How much the theme moves — under its pictures, which are what moves.
    SettingRow::BackdropEffects,
    SettingRow::BackdropRate,
    SettingRow::BackdropResolution,
    SettingRow::HoldBackdrops,
    SettingRow::BackdropsUnfocused,
    SettingRow::ImportTheme,
    SettingRow::SaveTheme,
    // Under its own heading, because it is about the network rather than
    // about a folder or a keyboard — and not "Sharing"'s, which is about
    // other people.
    SettingRow::Heading("Updates"),
    SettingRow::CheckForUpdates,
    // The song's own routing, and what a new one starts as — the one place
    // a per-project choice sits among settings that outlive projects, so it
    // says "this song" in its name (`docs/ux-routing-and-learning-plan.md`).
    SettingRow::Heading("Project"),
    SettingRow::SongRouting,
    SettingRow::NewSongRouting,
];

/// Every row the settings tab shows **for these settings**: the skeleton
/// above, with one [`SettingRow::PluginDir`] per plugin folder under the add
/// button.
///
/// The window addresses rows by position, and the host answers a press by
/// looking the position up in this same list — so the two cannot disagree
/// about which folder the third row removes.
pub fn setting_rows(settings: &Settings) -> Vec<SettingRow> {
    setting_rows_with(settings, |_| false)
}

/// The same, with a section's three picture rows under its picture row
/// wherever `has_picture` says that section has one — the theme is the
/// session's, so the session says.
pub fn setting_rows_with(
    settings: &Settings,
    has_picture: impl Fn(fontelle_ui::theme::BackdropPanel) -> bool,
) -> Vec<SettingRow> {
    let mut rows = Vec::with_capacity(SETTING_ROWS.len() + settings.plugin_dirs.len());
    for row in SETTING_ROWS {
        rows.push(row);
        if let SettingRow::Backdrop(panel) = row
            && has_picture(panel)
        {
            rows.extend([
                SettingRow::PictureFit(panel),
                SettingRow::PictureSize(panel),
                SettingRow::PicturePlace(panel),
            ]);
        }
        if row == SettingRow::PluginFolder {
            rows.extend((0..settings.plugin_dirs.len()).map(SettingRow::PluginDir));
        }
        if row == SettingRow::Heading("Extensions") {
            rows.extend((0..crate::extensions::CATALOGUE.len()).map(SettingRow::Extension));
        }
        // Linux's alone: plugin windows are X11 there, and EGL's vendor is
        // glvnd's to choose.
        if row == SettingRow::RescanPlugins && cfg!(target_os = "linux") {
            rows.push(SettingRow::CompatibleGraphics);
        }
    }
    rows
}

/// What the Compatible plugin graphics row says: the setting (`on`), whether
/// it is in force in this process (`active`, `gui::compatible_graphics_active`)
/// and whether Mesa's EGL is installed (`mesa`). It changes the environment
/// the process starts with, so a flip applies after restart, and says so.
pub fn compatible_graphics_value(on: bool, active: bool, mesa: bool) -> String {
    match (mesa, on, active) {
        (false, _, false) => "Unavailable: Mesa's EGL is not installed",
        (_, true, true) => "On",
        (_, false, false) => "Off",
        (_, true, false) => "On after restart",
        (_, false, true) => "Off after restart",
    }
    .to_string()
}

/// How far transpose goes either way. Two octaves is as far as anybody moves a
/// keyboard to reach a part; past it you have chosen the wrong octave.
const MAX_TRANSPOSE: i8 = 24;

/// Which kind of control a settings row is drawn and driven as.
///
/// The window asks this so it can draw a real control and route a press —
/// a drag to a slider, a menu to a choice, a flip to a switch — rather than
/// the old click-that-steps-a-list. It is the row's to decide, like its
/// [`label`](SettingRow::label) and its [`value`](SettingRow::value): the
/// window knows nothing about what any of them mean (INVARIANT 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingControlKind {
    /// A section title — nothing to touch.
    Heading,
    /// A press acts: a folder picker, a rescan, an install/remove. The value
    /// column is a caption saying what the press does, not a value.
    Button,
    /// A number set by dragging a groove or nudging with the arrow keys.
    Slider,
    /// One of a fixed list, chosen from a drop-down.
    Choice,
    /// An on/off switch a press flips.
    Switch,
    /// Words, typed into the prompt a project is named in, seeded with what
    /// the row holds ([`SettingRow::text`]).
    Text,
}

impl SettingRow {
    /// The name in the row's left-hand column.
    ///
    /// **Short enough for a 248-pixel panel with a value beside it.** The
    /// section heading carries "MIDI", so most rows under it need not repeat
    /// it — with one exception, [`Self::Transpose`], because the piano roll has
    /// a transposer of its own and a row called "Transpose" in a settings list
    /// reads as that tool's missing half. It was reported as exactly that.
    pub fn label(self, settings: &Settings) -> String {
        match self {
            // The **end** of the path, like every folder value here.
            Self::PluginDir(index) => settings
                .plugin_dirs
                .get(index)
                .map(|dir| crate::desktop::elide_path(dir, 2))
                .unwrap_or_default(),
            Self::Extension(index) => crate::extensions::CATALOGUE
                .get(index)
                .map(|extension| extension.name.to_string())
                .unwrap_or_default(),
            other => other.static_label().to_string(),
        }
    }

    fn static_label(self) -> &'static str {
        match self {
            Self::Heading(title) => title,
            Self::PluginDir(_) => "",
            Self::Extension(_) => "",
            Self::AudioBackend => "Backend",
            Self::AudioDevice => "Output device",
            Self::AudioBuffer => "Buffer size",
            Self::SampleRate => "Sample rate",
            Self::OutputNow => "Playing through",
            Self::Dropouts => "Dropouts",
            Self::VelocityCurve => "Velocity curve",
            Self::FixedVelocity => "Fixed velocity",
            Self::VelocityMin => "Velocity min",
            Self::VelocityMax => "Velocity max",
            // Whose transpose it is, in the row rather than in the heading
            // above it. Reported from using the window: the roll has a
            // transposer of its own and this one read as its settings half,
            // which is a feature nobody could find because it does not exist.
            Self::Transpose => "Keyboard transpose",
            Self::ChannelFilter => "Channel",
            Self::Folder(kind) => kind.label(),
            // What it *does*, because it is a button and because it adds
            // rather than replaces: the standard CLAP locations are searched
            // whether or not anything is listed here.
            Self::PluginFolder => "Plugin folders",
            Self::ImportFlFolders => "Use FL Studio's folders",
            Self::PresetFolder => "My presets",
            Self::RescanPlugins => "Rescan plugins",
            Self::CompatibleGraphics => "Compatible plugin graphics",
            Self::CheckForUpdates => "Check at launch",
            Self::YourName => "Your name",
            Self::Relay => "Relay",
            Self::SongRouting => "Routing in this song",
            Self::NewSongRouting => "New songs start as",
            Self::Theme => "Theme",
            Self::CornerRounding => "Corner rounding",
            Self::Backdrop(panel) => match panel {
                fontelle_ui::theme::BackdropPanel::Window => "Window picture",
                fontelle_ui::theme::BackdropPanel::Transport => "Transport picture",
                fontelle_ui::theme::BackdropPanel::Channels => "Channels picture",
                fontelle_ui::theme::BackdropPanel::Browser => "Browser picture",
                fontelle_ui::theme::BackdropPanel::Arrangement => "Arrangement picture",
                fontelle_ui::theme::BackdropPanel::Roll => "Piano roll picture",
                fontelle_ui::theme::BackdropPanel::Mixer => "Mixer picture",
            },
            Self::PictureStrength => "Picture strength",
            Self::PanelSeeThrough => "Panel see-through",
            Self::ImportTheme => "Import a theme",
            Self::SaveTheme => "Save this theme",
            Self::BackdropEffects => "Effects",
            Self::BackdropRate => "Backdrop rate",
            Self::BackdropResolution => "Backdrop resolution",
            Self::HoldBackdrops => "Hold backdrops still while playing",
            Self::BackdropsUnfocused => "Keep backdrops moving when not focused",
            Self::PictureFit(panel) => picture_row_label(panel, "fit"),
            Self::PictureSize(panel) => picture_row_label(panel, "size"),
            Self::PicturePlace(panel) => picture_row_label(panel, "position"),
        }
    }

    /// Whether this row is one of the plugin buttons.
    ///
    /// Asked of the row rather than matched at the call site, for the reason
    /// [`folder`](Self::folder) is: the one place a settings row is pressed
    /// should not have to know which variants are which.
    pub fn is_plugin_row(self) -> bool {
        matches!(
            self,
            Self::PluginFolder | Self::PluginDir(_) | Self::ImportFlFolders | Self::RescanPlugins
        )
    }

    /// Which folder this row is about, for the rows that are about one.
    ///
    /// What tells the host that a click here opens a **picker** rather than
    /// stepping a number. It is asked of the row rather than matched at the
    /// call site so that adding a third folder is a variant and nothing else.
    pub fn folder(self) -> Option<FolderKind> {
        match self {
            Self::Folder(kind) => Some(kind),
            _ => None,
        }
    }

    /// What it is set to, in the row's right-hand column.
    ///
    /// Takes the whole of [`Settings`] rather than just the MIDI half,
    /// because the rows now span two sections of it — and a row that could
    /// only see one of them is a row that would have to be told about the
    /// other by its caller.
    pub fn value(self, settings: &Settings) -> String {
        let midi = &settings.midi_input;
        match self {
            Self::Heading(_) => String::new(),
            Self::AudioBackend => settings
                .audio_output
                .host
                .clone()
                .unwrap_or_else(|| "Automatic".to_string()),
            Self::AudioDevice => settings
                .audio_output
                .device
                .clone()
                .unwrap_or_else(|| "System default".to_string()),
            Self::AudioBuffer => buffer_label(settings.audio_output.buffer_frames),
            Self::SampleRate => format!("{} kHz", crate::SAMPLE_RATE / 1000),
            // The session's, which holds the output.
            Self::OutputNow => "Not open".to_string(),
            Self::Dropouts => "None".to_string(),
            Self::VelocityCurve => midi.velocity_curve.label().to_string(),
            Self::FixedVelocity => midi.fixed_velocity.to_string(),
            Self::VelocityMin => midi.velocity_min.to_string(),
            Self::VelocityMax => midi.velocity_max.to_string(),
            // With its sign, always: "+0" against "0" is the difference
            // between a number that can go either way and one that might not.
            Self::Transpose => format!("{:+} st", midi.transpose_semitones),
            Self::ChannelFilter => match midi.channel_filter {
                Some(channel) => channel.to_string(),
                None => "All".to_string(),
            },
            // The **end** of the path: `…/FL Studio/Scores` says where you
            // are and `/home/someone/Docum…` says nothing at all. The same
            // rule the bank's own status line follows.
            Self::Folder(kind) => match settings.folder(kind) {
                Some(path) => crate::desktop::elide_path(path, 2),
                // Never blank: an empty value reads as a bug, and "there is
                // nothing here" is the state this whole feature starts in.
                // What a press does is on the button beside it
                // ([`caption`](Self::caption)), not in the value.
                None => "Not set".to_string(),
            },
            // The folders are the rows under this one, each named by its
            // end; this one counts them.
            Self::PluginFolder => match settings.plugin_dirs.len() {
                0 => "Not set".to_string(),
                1 => "1 folder of your own".to_string(),
                n => format!("{n} folders of your own"),
            },
            Self::PluginDir(_) | Self::ImportFlFolders | Self::RescanPlugins => String::new(),
            // Never blank, and never "not set": this folder always has an
            // answer, because Fontelle writes to it (see the field).
            Self::PresetFolder => match settings.user_preset_dir() {
                Some(path) => crate::desktop::elide_path(&path, 2),
                None => "Nowhere".to_string(),
            },
            // What the extension's state offers: install, remove, or a
            // sentence when this build cannot load it.
            Self::Extension(index) => match crate::extensions::CATALOGUE.get(index) {
                Some(extension) => match crate::extensions::state_here(extension) {
                    crate::extensions::ExtensionState::NotInstalled => "Not installed".to_string(),
                    crate::extensions::ExtensionState::Installed => "Installed".to_string(),
                    // Out of date is said in words, and the button updates:
                    // *"make sure it lets users know that its out of date"*.
                    crate::extensions::ExtensionState::OutOfDate { needs } => {
                        format!("Out of date \u{2014} this Fontelle needs {needs}")
                    }
                    crate::extensions::ExtensionState::UpdateAvailable { to } => {
                        format!("Installed \u{2014} {to} is out")
                    }
                    crate::extensions::ExtensionState::NeedsNewerFontelle => {
                        "Needs a newer Fontelle".to_string()
                    }
                },
                None => String::new(),
            },
            // As this file has it; the session says when it takes effect.
            Self::CompatibleGraphics => if settings.compatible_plugin_graphics {
                "On"
            } else {
                "Off"
            }
            .to_string(),
            Self::CheckForUpdates => if settings.check_for_updates {
                "On"
            } else {
                "Off"
            }
            .to_string(),
            Self::YourName => settings.your_name(),
            // The song's, answered by the session; this is the default a row
            // shows with no song to ask.
            Self::SongRouting => routing_label(Some(fontelle_model::RoutingMode::Rack)).to_string(),
            Self::NewSongRouting => routing_label(settings.new_song_routing).to_string(),
            // Never blank: blank *is* the managed relay, so it says which.
            Self::Relay => settings
                .relay
                .clone()
                .unwrap_or_else(|| "Floptle Cloud".to_string()),
            // The theme's, answered by the session, which holds the theme;
            // these are what a row shows with no session to ask.
            Self::Theme => settings
                .theme_name
                .clone()
                .unwrap_or_else(|| fontelle_ui::Theme::dark_default().name),
            Self::CornerRounding => format!(
                "{} px",
                fontelle_ui::Theme::dark_default().metrics.corner_radius
            ),
            Self::Backdrop(_) => "None".to_string(),
            Self::PictureStrength => "None set".to_string(),
            Self::PanelSeeThrough => "0%".to_string(),
            // Pure actions: the button says what they do.
            Self::ImportTheme | Self::SaveTheme => String::new(),
            Self::BackdropEffects => settings.backdrop_motion(false).effects.label().to_string(),
            Self::BackdropRate => format!("{} fps", settings.backdrop_fps),
            Self::BackdropResolution => backdrop_resolution_label(settings.backdrop_scale_percent),
            Self::HoldBackdrops => if settings.hold_backdrops_while_playing {
                "On"
            } else {
                "Off"
            }
            .to_string(),
            Self::BackdropsUnfocused => if settings.backdrops_when_unfocused {
                "On"
            } else {
                "Off"
            }
            .to_string(),
            // The picture's, answered by the session, which holds the theme.
            Self::PictureFit(_) | Self::PictureSize(_) | Self::PicturePlace(_) => String::new(),
        }
    }

    /// What a button row's button says: what a press does, as a verb. Empty
    /// for every other row, and for an extension this build cannot load —
    /// nothing to press, and the value says why.
    ///
    /// Apart from [`value`](Self::value) because the settings page draws them
    /// apart (`docs/ux-routing-and-learning-plan.md` §6): *"everything looks
    /// like a button even when things are just labels"* was a value that said
    /// "Not set — click", which is a label and an instruction in one cell.
    // Takes the settings as `value` does, so a button whose verb depends on
    // them does not change the call; none does yet.
    pub fn caption(self, _settings: &Settings) -> String {
        match self {
            Self::Folder(_) | Self::PresetFolder => "Choose\u{2026}",
            Self::PluginFolder => "Add\u{2026}",
            Self::PluginDir(_) => "Remove",
            Self::ImportFlFolders => "Import",
            Self::RescanPlugins => "Rescan",
            Self::ImportTheme => "Import\u{2026}",
            Self::OutputNow => "Restart",
            Self::Dropouts => "Reset",
            // "Remove" when one is set; the session knows, and says so.
            Self::Backdrop(_) => "Choose\u{2026}",
            Self::SaveTheme => "Save\u{2026}",
            Self::Extension(index) => match crate::extensions::CATALOGUE.get(index) {
                Some(extension) => {
                    match crate::extensions::action_for(&crate::extensions::state_here(extension)) {
                        crate::extensions::ExtensionAction::Install => "Install",
                        crate::extensions::ExtensionAction::Remove => "Remove",
                        crate::extensions::ExtensionAction::Update => "Update",
                        crate::extensions::ExtensionAction::None => "",
                    }
                }
                None => "",
            },
            _ => "",
        }
        .to_string()
    }

    /// One line under the row's name saying what it is for — the page's
    /// answer to *"some things just have no or little feedback"*: a row
    /// nobody can explain is a row nobody touches. Empty for a heading.
    pub fn help(self) -> &'static str {
        match self {
            Self::Heading(_) => "",
            Self::AudioBackend => "The audio system the studio plays through",
            Self::AudioDevice => {
                "Which output plays the studio; System default follows your desktop"
            }
            Self::AudioBuffer => {
                "Larger stops crackles and dropouts; smaller answers your keys sooner"
            }
            Self::SampleRate => {
                "The studio runs at this rate; a device at another is converted by the system"
            }
            Self::OutputNow => "What is open now; Restart opens it again",
            Self::Dropouts => {
                "Times the sound card ran dry since the output opened; a larger buffer helps"
            }
            Self::VelocityCurve => "How hard you play maps to how loud a note is",
            Self::FixedVelocity => "Every note at this velocity, with the Fixed curve",
            Self::VelocityMin => "What your softest touch plays",
            Self::VelocityMax => "What your hardest touch plays",
            Self::Transpose => "Shifts what your MIDI keyboard plays, in semitones",
            Self::ChannelFilter => "Listen to one MIDI channel, or to all of them",
            Self::Folder(FolderKind::Midi) => "Where the Import tab looks for MIDI files",
            Self::Folder(FolderKind::Scores) => "Where the Import tab looks for FL Studio scores",
            Self::Folder(FolderKind::Audio) => "Where the Import tab looks for audio",
            Self::PluginFolder => "Searched as well as the standard plugin places",
            Self::PluginDir(_) => "A folder you added; plugins in it are found at launch",
            Self::ImportFlFolders => "Adds the plugin folders FL Studio searches",
            Self::RescanPlugins => "Looks again, after you install a plugin",
            // One line, so the rest is the README's: Mesa draws on the CPU
            // unless it has a GPU driver of its own here, and the studio's
            // own window is Vulkan and does not change.
            Self::CompatibleGraphics => {
                "Plugin windows using OpenGL/EGL draw with Mesa, slower but sure. Applies after restart"
            }
            Self::PresetFolder => "Where your saved presets go and are read from",
            Self::Extension(_) => "An optional part of Fontelle, downloaded on request",
            Self::YourName => "What the people you share a project with see",
            Self::Relay => "Blank for Floptle Cloud, or a host:port of your own",
            Self::CheckForUpdates => "The start menu asks for a newer Fontelle at launch",
            Self::SongRouting => {
                "Rack-style: instruments choose tracks. Lane-style: each lane owns a track"
            }
            Self::NewSongRouting => "Asked the first time you make a song, unless chosen here",
            Self::Theme => "The look of the whole window \u{2014} yours are in the themes folder",
            Self::CornerRounding => "Square and rigid, or soft and round",
            Self::Backdrop(_) => "A PNG or JPEG behind the panel, kept inside the theme",
            Self::PictureStrength => "How strongly the pictures show through",
            Self::PanelSeeThrough => "Let the window's picture show through the panels",
            Self::ImportTheme => "Add a .fontelletheme file someone sent you",
            Self::SaveTheme => "Write this look to a .fontelletheme file to share",
            Self::BackdropEffects => {
                "Moving pictures as the theme draws them, held still, or colours only"
            }
            Self::BackdropRate => "How often a moving backdrop is drawn; lower costs less",
            Self::BackdropResolution => "How sharp a moving backdrop is; lower costs less",
            Self::HoldBackdrops => "Nothing behind the panels moves while the song plays",
            Self::BackdropsUnfocused => "Off: only the window you are working in moves",
            Self::PictureFit(_) => "Cover, contain, stretch to fit, tile, or its own size",
            Self::PictureSize(_) => "Larger or smaller than the fit makes it",
            Self::PicturePlace(_) => "Which corner or edge it sits against",
        }
    }

    /// Steps this row's value one place in the direction `delta` says.
    ///
    /// **A choice wraps and a number stops**, and the difference is not a
    /// detail: four curves are a set with no ends, so stopping at one would
    /// mean knowing to go back through; 0 and 127 are the ends of a range, and
    /// running off one and arriving at the other is a control that cannot be
    /// trusted to a held press.
    pub fn nudge(self, settings: &mut MidiInputSettings, delta: i32) {
        if delta == 0 {
            return;
        }
        let step = delta.signum();
        match self {
            // None of these is a value to step. A folder row and the two
            // plugin rows are buttons, and their press is the host's — this
            // module may not open a dialog or `dlopen` anything. What matters
            // here is that they do not quietly step the row above instead.
            Self::Heading(_)
            | Self::AudioBackend
            | Self::AudioDevice
            | Self::AudioBuffer
            | Self::SampleRate
            | Self::OutputNow
            | Self::Dropouts
            | Self::Folder(_)
            | Self::PresetFolder
            | Self::PluginFolder
            | Self::PluginDir(_)
            | Self::ImportFlFolders
            | Self::RescanPlugins
            | Self::Extension(_)
            | Self::CompatibleGraphics
            | Self::CheckForUpdates
            | Self::YourName
            | Self::Relay
            | Self::SongRouting
            | Self::NewSongRouting
            | Self::Theme
            | Self::CornerRounding
            | Self::Backdrop(_)
            | Self::PictureStrength
            | Self::PanelSeeThrough
            | Self::ImportTheme
            | Self::SaveTheme
            | Self::BackdropEffects
            | Self::BackdropRate
            | Self::BackdropResolution
            | Self::HoldBackdrops
            | Self::BackdropsUnfocused
            | Self::PictureFit(_)
            | Self::PictureSize(_)
            | Self::PicturePlace(_) => {}
            Self::VelocityCurve => {
                let all = VelocityCurveSetting::ALL;
                let at = all
                    .iter()
                    .position(|c| *c == settings.velocity_curve)
                    .unwrap_or(0) as i32;
                let next = (at + step).rem_euclid(all.len() as i32) as usize;
                settings.velocity_curve = all[next];
            }
            // Never zero: a velocity of zero is a note-off by convention
            // everywhere in MIDI, so "every note at velocity 0" is "no notes".
            Self::FixedVelocity => {
                settings.fixed_velocity = clamped(settings.fixed_velocity, step, 1, 127);
            }
            Self::VelocityMin => {
                settings.velocity_min = clamped(settings.velocity_min, step, 0, 127);
                // The two ends push each other rather than crossing: a window
                // whose bottom is above its top lets nothing through at all,
                // which looks exactly like a broken keyboard.
                settings.velocity_max = settings.velocity_max.max(settings.velocity_min);
            }
            Self::VelocityMax => {
                settings.velocity_max = clamped(settings.velocity_max, step, 0, 127);
                settings.velocity_min = settings.velocity_min.min(settings.velocity_max);
            }
            Self::Transpose => {
                settings.transpose_semitones = (settings.transpose_semitones as i32 + step)
                    .clamp(-(MAX_TRANSPOSE as i32), MAX_TRANSPOSE as i32)
                    as i8;
            }
            // Seventeen states in a ring: every channel, then the sixteen a
            // keyboard prints on its own front panel.
            Self::ChannelFilter => {
                let at = settings.channel_filter.unwrap_or(0) as i32;
                let next = (at + step).rem_euclid(17);
                settings.channel_filter = (next > 0).then_some(next as u8);
            }
        }
    }

    /// What a text row's prompt opens holding: what is set, and blank for
    /// what is not — so Enter on an untouched relay leaves it on Floptle
    /// Cloud rather than writing "Floptle Cloud" in as an address.
    pub fn text(self, settings: &Settings) -> String {
        match self {
            Self::YourName => settings.your_name(),
            Self::Relay => settings.relay.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Sets a text row to what was typed. Trimmed; blank is the default.
    pub fn set_text(self, settings: &mut Settings, text: &str) {
        let text = text.trim();
        let typed = (!text.is_empty()).then(|| text.to_string());
        match self {
            Self::YourName => settings.display_name = typed,
            Self::Relay => settings.relay = typed,
            _ => {}
        }
    }

    /// Which kind of control this row is — see [`SettingControlKind`].
    ///
    /// One place the classification lives, so the window's rendering and its
    /// press handling cannot disagree about whether a row is a slider or a
    /// button. Everything not a number, a choice or the one switch is a button:
    /// a folder opens a picker, a plugin row and an extension act, a heading is
    /// a label.
    pub fn control_kind(self) -> SettingControlKind {
        match self {
            Self::Heading(_) => SettingControlKind::Heading,
            Self::AudioBackend | Self::AudioDevice | Self::AudioBuffer => {
                SettingControlKind::Choice
            }
            Self::SampleRate | Self::OutputNow | Self::Dropouts => SettingControlKind::Button,
            Self::VelocityCurve
            | Self::ChannelFilter
            | Self::SongRouting
            | Self::NewSongRouting
            | Self::Theme => SettingControlKind::Choice,
            Self::CornerRounding | Self::PictureStrength | Self::PanelSeeThrough => {
                SettingControlKind::Slider
            }
            Self::Backdrop(_) | Self::ImportTheme | Self::SaveTheme => SettingControlKind::Button,
            Self::BackdropEffects | Self::BackdropRate | Self::BackdropResolution => {
                SettingControlKind::Choice
            }
            Self::HoldBackdrops | Self::BackdropsUnfocused => SettingControlKind::Switch,
            Self::PictureFit(_) | Self::PicturePlace(_) => SettingControlKind::Choice,
            Self::PictureSize(_) => SettingControlKind::Slider,
            Self::FixedVelocity | Self::VelocityMin | Self::VelocityMax | Self::Transpose => {
                SettingControlKind::Slider
            }
            Self::CheckForUpdates | Self::CompatibleGraphics => SettingControlKind::Switch,
            Self::YourName | Self::Relay => SettingControlKind::Text,
            Self::PluginFolder
            | Self::PluginDir(_)
            | Self::ImportFlFolders
            | Self::RescanPlugins
            | Self::PresetFolder
            | Self::Folder(_)
            | Self::Extension(_) => SettingControlKind::Button,
        }
    }

    /// The value, its floor and its ceiling, for the rows that are a number —
    /// the one place the four ranges are written, so a fraction read out of one
    /// and a fraction dragged back into it use the same ends.
    fn numeric_range(self, settings: &MidiInputSettings) -> Option<(i32, i32, i32)> {
        Some(match self {
            // Never zero: velocity zero is a note-off everywhere in MIDI.
            Self::FixedVelocity => (settings.fixed_velocity as i32, 1, 127),
            Self::VelocityMin => (settings.velocity_min as i32, 0, 127),
            Self::VelocityMax => (settings.velocity_max as i32, 0, 127),
            Self::Transpose => (
                settings.transpose_semitones as i32,
                -(MAX_TRANSPOSE as i32),
                MAX_TRANSPOSE as i32,
            ),
            _ => return None,
        })
    }

    /// Where a slider row's handle sits, 0..=1, or `None` for a row that is not
    /// a slider. What draws the groove's fill.
    pub fn fraction(self, settings: &MidiInputSettings) -> Option<f32> {
        let (value, low, high) = self.numeric_range(settings)?;
        Some(((value - low) as f32 / (high - low) as f32).clamp(0.0, 1.0))
    }

    /// Sets a slider row from a fraction of its groove (0..=1). A no-op on a row
    /// that is not a slider, so the one press handler can call it for any row.
    ///
    /// The same coupling [`nudge`](Self::nudge) keeps: the velocity window's two
    /// ends push each other rather than crossing, because a window whose bottom
    /// is above its top lets nothing through — which looks like a broken
    /// keyboard.
    pub fn set_fraction(self, settings: &mut MidiInputSettings, fraction: f32) {
        let Some((_, low, high)) = self.numeric_range(settings) else {
            return;
        };
        let value = (low as f32 + fraction.clamp(0.0, 1.0) * (high - low) as f32).round() as i32;
        match self {
            Self::FixedVelocity => settings.fixed_velocity = value.clamp(1, 127) as u8,
            Self::VelocityMin => {
                settings.velocity_min = value.clamp(0, 127) as u8;
                settings.velocity_max = settings.velocity_max.max(settings.velocity_min);
            }
            Self::VelocityMax => {
                settings.velocity_max = value.clamp(0, 127) as u8;
                settings.velocity_min = settings.velocity_min.min(settings.velocity_max);
            }
            Self::Transpose => {
                settings.transpose_semitones =
                    value.clamp(-(MAX_TRANSPOSE as i32), MAX_TRANSPOSE as i32) as i8
            }
            _ => {}
        }
    }

    /// What a drop-down row lists and which entry it is on, or `None` for a row
    /// that is not a choice. In the order [`nudge`](Self::nudge) steps them, so
    /// a drag through the list and a menu pick agree on what "the next one" is.
    pub fn choices(self, settings: &MidiInputSettings) -> Option<(Vec<String>, usize)> {
        match self {
            Self::VelocityCurve => {
                let at = VelocityCurveSetting::ALL
                    .iter()
                    .position(|c| *c == settings.velocity_curve)
                    .unwrap_or(0);
                let options = VelocityCurveSetting::ALL
                    .iter()
                    .map(|c| c.label().to_string())
                    .collect();
                Some((options, at))
            }
            Self::ChannelFilter => {
                let mut options = Vec::with_capacity(17);
                options.push("All".to_string());
                options.extend((1..=16).map(|n| n.to_string()));
                // 0 is "All", 1..=16 the channels a keyboard prints on its own
                // front panel — the same numbering the value column shows.
                let at = settings.channel_filter.unwrap_or(0) as usize;
                Some((options, at))
            }
            _ => None,
        }
    }

    /// The three backdrop rows' choices: what each lists and which it is on.
    /// `None` for every other row.
    pub fn backdrop_choices(
        self,
        settings: &Settings,
        reduce_motion: bool,
    ) -> Option<(Vec<String>, usize)> {
        use fontelle_ui::backdrop::Effects;
        Some(match self {
            Self::BackdropEffects => {
                let now = settings.backdrop_motion(reduce_motion).effects;
                (
                    Effects::ALL.iter().map(|e| e.label().to_string()).collect(),
                    Effects::ALL.iter().position(|e| *e == now).unwrap_or(0),
                )
            }
            Self::BackdropRate => (
                BACKDROP_RATES.iter().map(|r| format!("{r} fps")).collect(),
                BACKDROP_RATES
                    .iter()
                    .position(|r| *r == settings.backdrop_fps)
                    .unwrap_or(2),
            ),
            Self::BackdropResolution => (
                BACKDROP_RESOLUTIONS
                    .iter()
                    .map(|(_, name)| name.to_string())
                    .collect(),
                BACKDROP_RESOLUTIONS
                    .iter()
                    .position(|(p, _)| *p == settings.backdrop_scale_percent)
                    .unwrap_or(1),
            ),
            _ => return None,
        })
    }

    /// Sets a backdrop row to its `option`th choice. Answers whether it was
    /// one.
    pub fn choose_backdrop(self, settings: &mut Settings, option: usize) -> bool {
        use fontelle_ui::backdrop::Effects;
        match self {
            Self::BackdropEffects => {
                if let Some(e) = Effects::ALL.get(option) {
                    settings.backdrop_effects = Some(*e);
                }
            }
            Self::BackdropRate => {
                if let Some(r) = BACKDROP_RATES.get(option) {
                    settings.backdrop_fps = *r;
                }
            }
            Self::BackdropResolution => {
                if let Some((p, _)) = BACKDROP_RESOLUTIONS.get(option) {
                    settings.backdrop_scale_percent = *p;
                }
            }
            _ => return false,
        }
        true
    }

    /// The two routing rows' choices: the song's mode (`song`), and what a
    /// new song starts as. `None` for every other row.
    pub fn routing_choices(
        self,
        settings: &Settings,
        song: fontelle_model::RoutingMode,
    ) -> Option<(Vec<String>, usize)> {
        use fontelle_model::RoutingMode;
        let label = |mode| routing_label(Some(mode)).to_string();
        match self {
            Self::SongRouting => Some((
                vec![label(RoutingMode::Rack), label(RoutingMode::Lane)],
                usize::from(song == RoutingMode::Lane),
            )),
            Self::NewSongRouting => Some((
                vec![
                    routing_label(None).to_string(),
                    label(RoutingMode::Rack),
                    label(RoutingMode::Lane),
                ],
                match settings.new_song_routing {
                    None => 0,
                    Some(RoutingMode::Rack) => 1,
                    Some(RoutingMode::Lane) => 2,
                },
            )),
            _ => None,
        }
    }

    /// The audio output's drop-downs: Automatic, then each of `hosts`; the
    /// system default, then each of `devices`; Automatic, then each of
    /// [`OUTPUT_BUFFER_SIZES`]. A choice that is not in the list any more —
    /// a backend not running, a device unplugged — is listed as it was
    /// chosen and marked, rather than shown as something else.
    pub fn audio_choices(
        self,
        settings: &Settings,
        default_host: &str,
        hosts: &[String],
        devices: &[String],
    ) -> Option<(Vec<String>, usize)> {
        let listed = |first: String, names: &[String], chosen: &Option<String>, gone: &str| {
            let mut options = vec![first];
            options.extend(names.iter().cloned());
            let at = match chosen {
                None => 0,
                Some(name) => match names.iter().position(|n| n == name) {
                    Some(at) => at + 1,
                    None => {
                        options.push(format!("{name} ({gone})"));
                        options.len() - 1
                    }
                },
            };
            (options, at)
        };
        let output = &settings.audio_output;
        match self {
            Self::AudioBackend => Some(listed(
                format!("Automatic ({default_host})"),
                hosts,
                &output.host,
                "not available",
            )),
            Self::AudioDevice => Some(listed(
                "System default".to_string(),
                devices,
                &output.device,
                "not found",
            )),
            Self::AudioBuffer => {
                let mut options = vec![buffer_label(None)];
                options.extend(OUTPUT_BUFFER_SIZES.iter().map(|f| buffer_label(Some(*f))));
                let at = match output.buffer_frames {
                    None => 0,
                    Some(frames) => match OUTPUT_BUFFER_SIZES.iter().position(|f| *f == frames) {
                        Some(at) => at + 1,
                        None => {
                            options.push(buffer_label(Some(frames)));
                            options.len() - 1
                        }
                    },
                };
                Some((options, at))
            }
            _ => None,
        }
    }

    /// Sets an audio output drop-down to its `option`th entry, in the lists
    /// [`audio_choices`](Self::audio_choices) made from the same `hosts` and
    /// `devices`. Whether anything changed; a backend chosen forgets the
    /// device, which was the last backend's.
    pub fn choose_audio(
        self,
        settings: &mut Settings,
        option: usize,
        hosts: &[String],
        devices: &[String],
    ) -> bool {
        let output = &mut settings.audio_output;
        let before = output.clone();
        let pick = |names: &[String], current: &Option<String>| match option {
            0 => Some(None),
            n => match names.get(n - 1) {
                Some(name) => Some(Some(name.clone())),
                // The marked entry past the list: what is already chosen.
                None if n == names.len() + 1 => Some(current.clone()),
                None => None,
            },
        };
        match self {
            Self::AudioBackend => {
                if let Some(host) = pick(hosts, &output.host)
                    && host != output.host
                {
                    output.host = host;
                    output.device = None;
                }
            }
            Self::AudioDevice => {
                if let Some(device) = pick(devices, &output.device) {
                    output.device = device;
                }
            }
            Self::AudioBuffer => match option {
                0 => output.buffer_frames = None,
                n => {
                    if let Some(frames) = OUTPUT_BUFFER_SIZES.get(n - 1) {
                        output.buffer_frames = Some(*frames);
                    }
                }
            },
            _ => return false,
        }
        *output != before
    }

    /// Sets a drop-down row to its `option`th entry. A no-op on a row that is
    /// not a choice, and on an option past the end of the list.
    pub fn choose(self, settings: &mut MidiInputSettings, option: usize) {
        match self {
            Self::VelocityCurve => {
                if let Some(curve) = VelocityCurveSetting::ALL.get(option) {
                    settings.velocity_curve = *curve;
                }
            }
            // Option 0 is "All" (no filter); 1..=16 are the channels.
            Self::ChannelFilter => {
                settings.channel_filter = (1..=16).contains(&option).then_some(option as u8);
            }
            _ => {}
        }
    }
}

/// A routing mode as the settings page writes it; `None` is "ask".
pub fn routing_label(mode: Option<fontelle_model::RoutingMode>) -> &'static str {
    match mode {
        None => "Ask me",
        Some(fontelle_model::RoutingMode::Rack) => "Rack-style",
        Some(fontelle_model::RoutingMode::Lane) => "Lane-style",
    }
}

/// `value` moved one step and kept inside `low..=high`.
fn clamped(value: u8, step: i32, low: u8, high: u8) -> u8 {
    (value as i32 + step).clamp(low as i32, high as i32) as u8
}

impl From<MidiInputSettings> for fontelle_midi::InputSettings {
    /// **The one place the file's shape becomes the engine's**, and where
    /// everything the file could hold that the engine cannot is squared off:
    /// a velocity window given the wrong way round is put back in order, and
    /// a channel is converted from the 1..=16 a keyboard prints on its own
    /// front panel to the 0..=15 the wire carries.
    fn from(settings: MidiInputSettings) -> Self {
        let low = settings.velocity_min.min(127);
        let high = settings.velocity_max.min(127);
        Self {
            velocity_curve: match settings.velocity_curve {
                VelocityCurveSetting::Linear => fontelle_midi::VelocityCurve::Linear,
                VelocityCurveSetting::Soft => fontelle_midi::VelocityCurve::Soft,
                VelocityCurveSetting::Hard => fontelle_midi::VelocityCurve::Hard,
                VelocityCurveSetting::Fixed => {
                    fontelle_midi::VelocityCurve::Fixed(settings.fixed_velocity.clamp(1, 127))
                }
            },
            velocity_range: (low.min(high), low.max(high)),
            transpose_semitones: settings.transpose_semitones,
            channel_filter: settings
                .channel_filter
                .filter(|c| (1..=16).contains(c))
                .map(|c| c - 1),
        }
    }
}

/// Which rules a folder is chosen by — a parameter rather than a `cfg!`, so
/// the Windows rules are tested on the machine this is built on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Linux and macOS: `HOME` and the XDG variables.
    Unix,
    /// `%APPDATA%` and `%LOCALAPPDATA%`; no `HOME` to be had.
    Windows,
}

impl Platform {
    /// The one this build runs on.
    pub fn here() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }

    /// Whether `path` is absolute *on this platform* — asked of a string,
    /// because `Path::is_absolute` answers for the machine running the test.
    fn is_absolute(self, path: &str) -> bool {
        match self {
            Self::Unix => path.starts_with('/'),
            Self::Windows => {
                let bytes = path.as_bytes();
                let drive = bytes.len() >= 3
                    && bytes[0].is_ascii_alphabetic()
                    && bytes[1] == b':'
                    && matches!(bytes[2], b'\\' | b'/');
                drive || path.starts_with(r"\\") || path.starts_with('/')
            }
        }
    }
}

impl Settings {
    /// `$XDG_CONFIG_HOME/fontelle`, or `$HOME/.config/fontelle` — or on
    /// Windows `%APPDATA%\fontelle` (see [`config_dir_on`](Self::config_dir_on)).
    ///
    /// The environment is injected so the answer is testable without one —
    /// this is the one function in the workspace that decides where Fontelle is
    /// allowed to write, and it should not need a particular machine to check.
    pub fn config_dir_from(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
        Self::config_dir_on(env, Platform::here(), &|path| path.is_dir())
    }

    /// `$XDG_DATA_HOME/fontelle`, or `$HOME/.local/share/fontelle` — or on
    /// Windows `%LOCALAPPDATA%\fontelle`.
    ///
    /// Fontelle's own data directory — the parent of the soundfont bank and
    /// the user presets, and where [`crate::crashlog`] writes. Its own
    /// function rather than `default_soundfont_dir().parent()`, which would
    /// make the crash log's home a consequence of where the soundfonts live.
    pub fn data_dir_from(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
        Self::data_dir_on(env, Platform::here(), &|path| path.is_dir())
    }

    /// The config folder on `platform`, with `exists` asked about folders
    /// already on disk.
    ///
    /// > *"could not write settings: there is n…"*
    ///
    /// **Windows sets no `HOME`**, so the XDG rule alone answered `None`
    /// there, and every Windows install ran with no settings file, no preset
    /// folder, no soundfont bank and no crash reports. A Windows program's
    /// own folders are `%APPDATA%` for what it is set to and `%LOCALAPPDATA%`
    /// for what it keeps — the second is where the bridges already went
    /// (`fontelle_host::bridge_search_paths`). One exception, for somebody
    /// who ran Fontelle from a shell that does set `HOME` (Git Bash, MSYS):
    /// a Fontelle folder that **already exists** under it is kept, because
    /// moving somebody's settings out from under them is worse than an
    /// unusual place for them.
    pub fn config_dir_on(
        env: &dyn Fn(&str) -> Option<String>,
        platform: Platform,
        exists: &dyn Fn(&Path) -> bool,
    ) -> Option<PathBuf> {
        Self::own_dir(
            env,
            platform,
            exists,
            "XDG_CONFIG_HOME",
            ".config",
            "APPDATA",
        )
    }

    /// The data folder on `platform` — [`config_dir_on`](Self::config_dir_on)'s
    /// rule, with `%LOCALAPPDATA%` for Windows.
    pub fn data_dir_on(
        env: &dyn Fn(&str) -> Option<String>,
        platform: Platform,
        exists: &dyn Fn(&Path) -> bool,
    ) -> Option<PathBuf> {
        Self::own_dir(
            env,
            platform,
            exists,
            "XDG_DATA_HOME",
            ".local/share",
            "LOCALAPPDATA",
        )
    }

    /// The soundfont bank inside [`data_dir_on`](Self::data_dir_on).
    pub fn default_soundfont_dir_on(
        env: &dyn Fn(&str) -> Option<String>,
        platform: Platform,
        exists: &dyn Fn(&Path) -> bool,
    ) -> Option<PathBuf> {
        Self::data_dir_on(env, platform, exists).map(|dir| dir.join("soundfonts"))
    }

    /// The user presets inside [`data_dir_on`](Self::data_dir_on).
    pub fn default_preset_dir_on(
        env: &dyn Fn(&str) -> Option<String>,
        platform: Platform,
        exists: &dyn Fn(&Path) -> bool,
    ) -> Option<PathBuf> {
        Self::data_dir_on(env, platform, exists).map(|dir| dir.join("presets"))
    }

    pub fn data_dir() -> Option<PathBuf> {
        Self::data_dir_from(&|key| std::env::var(key).ok())
    }

    /// `$XDG_DATA_HOME/fontelle/soundfonts`, or `$HOME/.local/share/...`.
    pub fn default_soundfont_dir_from(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
        Self::data_dir_from(env).map(|dir| dir.join("soundfonts"))
    }

    /// `$XDG_DATA_HOME/fontelle/presets`, or `$HOME/.local/share/...`.
    pub fn default_preset_dir_from(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
        Self::data_dir_from(env).map(|dir| dir.join("presets"))
    }

    pub fn default_preset_dir() -> Option<PathBuf> {
        Self::default_preset_dir_from(&|key| std::env::var(key).ok())
    }

    /// Where user presets are written and read.
    ///
    /// The setting if there is one, Fontelle's own data directory otherwise —
    /// see the field for why this one has a default at all when the import
    /// folders do not.
    pub fn user_preset_dir(&self) -> Option<PathBuf> {
        self.preset_dir.clone().or_else(Self::default_preset_dir)
    }

    /// One of Fontelle's own folders: the XDG variable, then — on Windows —
    /// an existing folder under `HOME` or else the Windows variable, then
    /// the XDG fallback under `HOME`.
    fn own_dir(
        env: &dyn Fn(&str) -> Option<String>,
        platform: Platform,
        exists: &dyn Fn(&Path) -> bool,
        variable: &str,
        fallback: &str,
        windows: &str,
    ) -> Option<PathBuf> {
        // An XDG variable that is set but empty or relative is, per the spec,
        // to be ignored rather than honoured — and honouring a relative one
        // would put Fontelle's config wherever it happened to be launched from.
        if let Some(value) = env(variable)
            && platform.is_absolute(&value)
        {
            return Some(PathBuf::from(value).join("fontelle"));
        }
        let under_home = env("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(fallback).join("fontelle"));
        if platform == Platform::Windows {
            if let Some(old) = under_home.as_ref().filter(|dir| exists(dir)) {
                return Some(old.clone());
            }
            if let Some(base) = env(windows).filter(|value| platform.is_absolute(value)) {
                return Some(PathBuf::from(base).join("fontelle"));
            }
        }
        under_home
    }

    pub fn config_dir() -> Option<PathBuf> {
        Self::config_dir_from(&|key| std::env::var(key).ok())
    }

    pub fn config_path() -> Option<PathBuf> {
        Self::config_dir().map(|dir| dir.join("settings.json"))
    }

    pub fn default_soundfont_dir() -> Option<PathBuf> {
        Self::default_soundfont_dir_from(&|key| std::env::var(key).ok())
    }

    /// Which folder is set for `kind`, if one is.
    ///
    /// One accessor pair rather than a match at every call site — which is
    /// how a *"change my projects folder"* button in this very program came
    /// to replace somebody's soundfont bank.
    pub fn folder(&self, kind: FolderKind) -> Option<&Path> {
        match kind {
            FolderKind::Midi => self.midi_dir.as_deref(),
            FolderKind::Scores => self.score_dir.as_deref(),
            FolderKind::Audio => self.audio_dir.as_deref(),
        }
    }

    pub fn set_folder(&mut self, kind: FolderKind, dir: Option<PathBuf>) {
        match kind {
            FolderKind::Midi => self.midi_dir = dir,
            FolderKind::Scores => self.score_dir = dir,
            FolderKind::Audio => self.audio_dir = dir,
        }
    }

    pub fn to_json(&self) -> String {
        let mut settings = self.clone();
        settings.format_version = SETTINGS_FORMAT_VERSION;
        serde_json::to_string_pretty(&settings).expect("Settings is always serialisable")
    }

    /// Reads settings, checking the version before the body — the same rule the
    /// project document and the theme follow, for the same reason: a file from
    /// a newer build should be refused by version rather than by whichever
    /// field happened to change shape first.
    pub fn from_json(text: &str) -> Result<Self, SettingsError> {
        let json: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| SettingsError::Format(format!("this is not readable JSON: {e}")))?;
        let found = json
            .get("format_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                SettingsError::Format(
                    "this file has no format_version — it is not Fontelle's settings file"
                        .to_string(),
                )
            })? as u32;
        if found > SETTINGS_FORMAT_VERSION {
            return Err(SettingsError::FromTheFuture {
                found,
                newest: SETTINGS_FORMAT_VERSION,
            });
        }
        serde_json::from_value(json)
            .map_err(|e| SettingsError::Format(format!("these settings could not be read: {e}")))
    }

    /// Reads `path`, **always producing usable settings**.
    ///
    /// A missing file is a first run, which is not an error. A damaged one is:
    /// the error comes back alongside the defaults so the caller can say so and
    /// carry on, and — this is the part that matters — the broken file is left
    /// exactly where it is. Overwriting it with defaults would silently lose
    /// the list of folders somebody spent an afternoon assembling.
    pub fn load_from(path: &Path) -> (Self, Option<SettingsError>) {
        let text = match read_while_saved(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(source) => {
                return (
                    Self::default(),
                    Some(SettingsError::Io {
                        path: path.to_path_buf(),
                        source,
                    }),
                );
            }
        };
        match Self::from_json(&text) {
            Ok(settings) => (settings, None),
            Err(SettingsError::Format(why)) => (
                Self::default(),
                Some(SettingsError::Format(format!("{}: {why}", path.display()))),
            ),
            Err(other) => (Self::default(), Some(other)),
        }
    }

    /// Writes settings out, creating the directory that holds them.
    ///
    /// Temp-file-then-rename, like the project bundle (§17.1): a settings file
    /// truncated by a crash mid-write is one the next launch cannot read.
    ///
    /// **The temp file's name is unique per writer**, which is not decoration.
    /// One fixed `settings.json.tmp` is a shared mutable file: two writers
    /// interleave into it and both rename, and what lands is a document with an
    /// extra brace on the end — or nothing at all, because the second rename
    /// found the file already moved. That is not a hypothetical either; it is
    /// what this project's own test suite did to a real config directory the
    /// first time this function existed.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&temp, self.to_json())?;
        // A rename over an existing file is atomic on every filesystem this
        // targets, so a reader sees the old settings or the new ones and never
        // half of either.
        let result = while_windows_denies(|| std::fs::rename(&temp, path));
        if result.is_err() {
            std::fs::remove_file(&temp).ok();
        }
        result
    }

    /// Settings from the usual place, with whatever went wrong reading them.
    pub fn load() -> (Self, Option<SettingsError>) {
        match Self::config_path() {
            Some(path) => Self::load_from(&path),
            None => (Self::default(), None),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        match Self::config_path() {
            Some(path) => self.save_to(&path),
            None => Err(std::io::Error::other(
                "there is no home directory to keep settings in",
            )),
        }
    }

    /// The folders to scan, filling in the default bank on a first run.
    ///
    /// Creating it is the one write outside the config directory — see this
    /// module's own documentation for why, and `created` is `true` exactly when
    /// it happened, so the caller can tell the user where their soundfonts go.
    pub fn soundfont_dirs_or_default(&mut self) -> BankDirs {
        if !self.soundfont_dirs.is_empty() {
            return BankDirs {
                dirs: self.soundfont_dirs.clone(),
                created: None,
            };
        }
        let Some(default) = Self::default_soundfont_dir() else {
            return BankDirs {
                dirs: Vec::new(),
                created: None,
            };
        };
        let created = !default.exists() && std::fs::create_dir_all(&default).is_ok();
        self.soundfont_dirs.push(default.clone());
        BankDirs {
            dirs: self.soundfont_dirs.clone(),
            created: created.then_some(default),
        }
    }
}

/// What [`Settings::soundfont_dirs_or_default`] settled on.
pub struct BankDirs {
    pub dirs: Vec<PathBuf>,
    /// The folder that had to be created, if one did — worth telling the user
    /// about exactly once.
    pub created: Option<PathBuf>,
}

#[derive(Debug)]
pub enum SettingsError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    FromTheFuture {
        found: u32,
        newest: u32,
    },
    Format(String),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::FromTheFuture { found, newest } => write!(
                f,
                "these settings are in format version {found}, and this build of \
                 Fontelle understands up to version {newest} — upgrade Fontelle, \
                 or move the file aside to start again"
            ),
            Self::Format(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Reads the settings file even while another save is renaming a new one
/// over it.
///
/// On Windows a file open for reading without delete-sharing makes a rename
/// over it fail, and a rename in flight makes an open fail — both as "Access
/// is denied". CI's Windows runner hit it in `concurrent_saves_never_leave_a_damaged_file`,
/// and a user's settings being saved while they are read is the same race.
/// Opened sharing read, write and delete, and retried briefly on a denial.
fn read_while_saved(path: &Path) -> std::io::Result<String> {
    while_windows_denies(|| {
        use std::io::Read;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE.
            options.share_mode(0x1 | 0x2 | 0x4);
        }
        let mut text = String::new();
        options.open(path)?.read_to_string(&mut text)?;
        Ok(text)
    })
}

/// Runs `attempt` again for a moment while Windows answers "Access is
/// denied" — a rename landing, another reader, a virus scanner holding the
/// file. Once everywhere else: nowhere else denies for that reason.
fn while_windows_denies<T>(mut attempt: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let tries = if cfg!(windows) { 50 } else { 1 };
    let mut result = attempt();
    for _ in 1..tries {
        match &result {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                std::thread::sleep(std::time::Duration::from_millis(2));
                result = attempt();
            }
            _ => break,
        }
    }
    result
}

/// What the Backdrop resolution row calls `percent`.
fn backdrop_resolution_label(percent: u16) -> String {
    BACKDROP_RESOLUTIONS
        .iter()
        .find(|(p, _)| *p == percent)
        .map_or_else(|| format!("{percent}%"), |(_, name)| name.to_string())
}

/// "Piano roll picture fit", and the rest: a section's picture rows' names.
fn picture_row_label(panel: fontelle_ui::theme::BackdropPanel, what: &str) -> &'static str {
    use fontelle_ui::theme::BackdropPanel as P;
    match (panel, what) {
        (P::Window, "fit") => "Window picture fit",
        (P::Window, "size") => "Window picture size",
        (P::Window, _) => "Window picture position",
        (P::Transport, "fit") => "Transport picture fit",
        (P::Transport, "size") => "Transport picture size",
        (P::Transport, _) => "Transport picture position",
        (P::Channels, "fit") => "Channels picture fit",
        (P::Channels, "size") => "Channels picture size",
        (P::Channels, _) => "Channels picture position",
        (P::Browser, "fit") => "Browser picture fit",
        (P::Browser, "size") => "Browser picture size",
        (P::Browser, _) => "Browser picture position",
        (P::Arrangement, "fit") => "Arrangement picture fit",
        (P::Arrangement, "size") => "Arrangement picture size",
        (P::Arrangement, _) => "Arrangement picture position",
        (P::Roll, "fit") => "Piano roll picture fit",
        (P::Roll, "size") => "Piano roll picture size",
        (P::Roll, _) => "Piano roll picture position",
        (P::Mixer, "fit") => "Mixer picture fit",
        (P::Mixer, "size") => "Mixer picture size",
        (P::Mixer, _) => "Mixer picture position",
    }
}
