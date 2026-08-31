//! The open studio, and everything that has to happen when it changes.
//!
//! `fontelle-app` is the one layer allowed to see the model, the engine and the
//! UI at once, so this is where the three meet: the window asks for an edit,
//! the edit becomes a `Command`, the command goes through `History`, and the
//! result is recompiled and published to the audio thread.
//!
//! Two invariants live or die here:
//!
//! - **INVARIANT 9** — every mutation is a command. There is no `&mut Project`
//!   reachable from the UI; [`Session::edit`] and the channel operations below
//!   are the whole surface.
//! - **INVARIANT 2** — the roll is a view. It sends [`RollEdit`] values and
//!   reads notes back; it never writes.
//!
//! # Two channels to the audio thread, not one
//!
//! Editing notes republishes the **timeline**. Choosing a soundfont, adding a
//! channel or muting one republishes the **graph** — a different thing, through
//! `fontelle_engine::graph_channel`, because the graph owns the instruments and
//! the timeline only points at them. Keeping them apart is what makes drawing a
//! note cost a timeline recompile and nothing else, and it is why adding a
//! channel no longer needs the audio device restarted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fontelle_engine::{GraphPublisher, TimelinePublisher};
use fontelle_model::{
    AddChannel, AddClip, AddNotes, Arena, Clip, ClipSource, Command, DuplicateClip, FlagTarget,
    History, Lane, MoveClip, MoveNotes, Note, NoteData, NumberTarget, Project, RemoveClip,
    RemoveNotes, ResizeClip, ResizeNotes, SetFlag, SetNoteProperty, SetNumber,
};
use fontelle_types::{
    ChannelId, ClipId, EventPayload, LaneId, MixerTrackId, NodeId, NoteId, PPQN, Sample, Tick,
    TimedEvent,
};
use fontelle_ui::canvas::{ArrangeEdit, InstrumentView, RollEdit};
use fontelle_ui::document::{
    ChannelInfo, ClipInfo, ClipKind, DocumentHost, GhostFilter, GhostNote, LaneInfo, LibraryEntry,
    MixerStrip, StudioHost,
};

use crate::bank::{BankRow, SoundfontBank, matches_names};
use crate::library::SampleLibrary;
use crate::projects::ProjectLibrary;
use crate::realise::{RealiseOptions, apply_mixer_controls, realise};
use crate::settings::Settings;

/// How long a clip a freshly added channel gets, in bars.
const NEW_CLIP_BARS: i64 = 8;

/// And how long a brand-new project is. The same eight: a new project is one
/// empty channel with one clip on it, and the two should agree.
const NEW_PROJECT_BARS: i64 = 8;

/// How much silence a bounce keeps past the last note, so its release is not
/// cut off mid-ring. Two bars, which covers a long pad at a slow tempo.
const RELEASE_TAIL: Tick = PPQN * 8;

/// The voice context every audition carries.
///
/// The same one live MIDI uses, and deliberately not a clip's: a sequenced
/// note-off must never cut a note the *player* is holding, whether the player
/// is a keyboard or a mouse (TDD §11.4).
const AUDITION_VOICE_CONTEXT: u32 = u32::MAX;

pub struct Session {
    project: Project,
    history: History,
    library: SampleLibrary,
    channel_nodes: HashMap<ChannelId, NodeId>,
    /// Every automatable parameter in the running graph, by its address, and
    /// the node that owns it — see [`crate::Realised::param_nodes`].
    ///
    /// Kept beside `channel_nodes` and for the same reason: the timeline is
    /// recompiled on every edit, and both maps are what turns a document into
    /// events addressed at the right nodes. Replaced whenever the graph is.
    param_nodes: HashMap<fontelle_types::ParamAddress, NodeId>,
    /// The live end of every mixer track's fader and meter, as the graph that
    /// is currently playing sees them.
    ///
    /// Replaced wholesale by [`Session::rebuild_graph`], because the graph that
    /// owned the old set has been handed back to be freed. Between rebuilds
    /// this is how a fader is *heard* — see `fontelle_engine::TrackControls`
    /// for why writing the document alone is not enough.
    track_controls: HashMap<MixerTrackId, std::sync::Arc<fontelle_engine::TrackControls>>,
    /// The live end of every insert in the running graph, keyed as the panel
    /// addresses one — see [`crate::Realised::effect_controls`]. Replaced
    /// wholesale on a rebuild, because a triple buffer's two ends cannot be
    /// re-paired; the document is the source of truth either way.
    effect_controls: HashMap<(MixerTrackId, usize), fontelle_engine::EffectControls>,
    /// The automation clip the editor has open, what it is called, and which
    /// of its points are selected.
    ///
    /// Session state, not document state: which clip you are looking at is not
    /// something a project sent to somebody else should arrive with, for the
    /// same reason the piano roll's open clip is not saved.
    automation_clip: Option<fontelle_types::ClipId>,
    automation_label: String,
    automation_selection: Vec<fontelle_types::PointId>,
    /// What each automated parameter is *called*, by address.
    ///
    /// The caption is the panel's — "Master — band1.gain" is a sentence only
    /// the side holding the strip names can write — and the document stores
    /// only the address. This is where the two are put back together for the
    /// arrangement's blocks and lane headers. Session state, not the
    /// document's: it is re-derivable, and a file is not the place for a
    /// window's wording.
    automation_names: HashMap<fontelle_types::ParamAddress, String>,
    /// Which mixer strip the track-options column is about.
    ///
    /// The mixer's own selection, kept apart from `selected` — several
    /// channels may share one track (§13.1), so "the selected channel" does
    /// not name a strip.
    selected_track: usize,
    /// Where live MIDI is pointed — the selected channel's node. See
    /// [`Session::with_live_target`].
    live_target: Option<std::sync::Arc<fontelle_midi::LiveTarget>>,
    /// The metronome the running graph is playing through. Kept across a
    /// rebuild — choosing a soundfont with the click on must not turn it off.
    metronome: Option<std::sync::Arc<fontelle_engine::Metronome>>,
    /// Where live input is mirrored while the transport is recording, and what
    /// has been mirrored so far.
    ///
    /// Drained in [`pump`](StudioHost::pump) rather than at the end: the ring
    /// is bounded, and a take longer than it holds would otherwise lose its
    /// beginning. `take` is the whole performance, and it is emptied when a
    /// take is kept or thrown away.
    capture: Option<fontelle_engine::CaptureReader>,
    take: Vec<TimedEvent>,
    publisher: TimelinePublisher,
    /// The other half of the pair — see this module's own documentation.
    graphs: Option<GraphPublisher>,
    options: RealiseOptions,
    /// The clip the piano roll is showing, and the channel it belongs to.
    clip: ClipId,
    selected: usize,
    bundle: Option<PathBuf>,
    dirty: bool,
    /// Bumped whenever anything the window's panels draw has changed. The
    /// window re-reads its lists on a change and not once a frame.
    revision: u64,

    // --- the soundfont bank (TDD §17.5) ---
    settings: Settings,
    /// Where the settings are read from and written back to. `None` is the
    /// user's own config directory; a path is how a test keeps its hands off
    /// it, which is not a nicety — the first run of this crate's own studio
    /// tests wrote a soundfont folder in `/tmp` into the developer's real
    /// `~/.config/fontelle/settings.json`.
    settings_path: Option<PathBuf>,
    bank: SoundfontBank,
    /// The projects folder and what is in it (TDD §17.1, §17.3). Live-session
    /// state, like the bank: where projects live is a setting, and the listing
    /// is a read of a folder that may change under us.
    projects: ProjectLibrary,
    query: String,
    /// Which soundfont is open, and what is inside it.
    ///
    /// The **path**, not a row number. The list under it moves — a search, a
    /// folder change, a rescan — and an index would name whatever happened to
    /// land in that slot afterwards. The row to highlight is worked out from
    /// the path when the panel asks (see `selected_file`).
    open_file: Option<PathBuf>,
    presets: Vec<fontelle_assets::PresetInfo>,
    /// What each channel is playing, as the file it came from and the preset's
    /// own index inside that file — which is what the browser needs to say
    /// *which* instrument is on the channel you are looking at.
    ///
    /// Live-session state, not document state: a `PatchData` records the audio
    /// a patch points at (INVARIANT 8) and not the row of a browser it was
    /// picked from. A project reopened from disk therefore has no highlight
    /// until a preset is chosen again — the channel's *name* is the half of the
    /// answer that does survive, which is why it is set from the preset.
    /// Clips taken by `Ctrl+C`/`Ctrl+X` on the arrangement, whole.
    ///
    /// Real `Clip` values rather than the ids they came from, and that is the
    /// point of holding them here: a cut removes the source, so an id would
    /// name nothing by the time paste ran. The canvas cannot hold them —
    /// INVARIANT 2 — so it emits [`ArrangeEdit::Copy`] and this keeps them.
    ///
    /// Live-session state on purpose, like `channel_presets` and
    /// `patch_cache`: a clipboard is not part of the document.
    clip_clipboard: Vec<Clip>,
    channel_presets: HashMap<ChannelId, (PathBuf, usize)>,
    /// The selected channel's patch, deserialised.
    ///
    /// A cache, and a load-bearing one: the instrument editor reads it on every
    /// revision and a knob drag writes it sixty times a second, and
    /// `Patch::from_data` parses a JSON tree of every zone in the preset. Keyed
    /// by the channel so selecting another one drops it.
    patch_cache: Option<(ChannelId, fontelle_core::Patch)>,
    message: Option<String>,

    /// Where an audition goes. `None` when the window was opened without a
    /// live-input channel, and then drawing a note is silent until playback
    /// reaches it.
    audition: Option<Box<dyn fontelle_types::EventSink>>,

    /// Handed back when the roll asks for notes and the clip has gone.
    empty: Arena<NoteId, Note>,
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project: Project,
        library: SampleLibrary,
        channel_nodes: HashMap<ChannelId, NodeId>,
        publisher: TimelinePublisher,
        options: RealiseOptions,
        clip: ClipId,
        bundle: Option<PathBuf>,
    ) -> Self {
        let (settings, error) = Settings::load();
        let mut session = Self {
            project,
            history: History::new(),
            library,
            channel_nodes,
            param_nodes: HashMap::new(),
            track_controls: HashMap::new(),
            effect_controls: HashMap::new(),
            automation_clip: None,
            automation_label: String::new(),
            automation_selection: Vec::new(),
            automation_names: HashMap::new(),
            selected_track: 0,
            live_target: None,
            metronome: None,
            capture: None,
            take: Vec::new(),
            publisher,
            graphs: None,
            options,
            clip,
            selected: 0,
            bundle,
            dirty: false,
            revision: 1,
            settings,
            settings_path: None,
            bank: SoundfontBank::default(),
            projects: ProjectLibrary::default(),
            query: String::new(),
            open_file: None,
            presets: Vec::new(),
            clip_clipboard: Vec::new(),
            channel_presets: HashMap::new(),
            patch_cache: None,
            message: error.map(|e| e.to_string()),
            audition: None,
            empty: Arena::default(),
        };
        session.selected = session.channel_index_of_clip().unwrap_or(0);
        session
    }

    /// Points the session at a settings file other than the user's own.
    ///
    /// For tests, and for a `--config` flag when there is one. Reloads, because
    /// the settings that were read at construction came from somewhere else.
    pub fn with_settings_path(mut self, path: PathBuf) -> Self {
        let (settings, error) = Settings::load_from(&path);
        self.settings = settings;
        if let Some(e) = error {
            self.message = Some(e.to_string());
        }
        self.settings_path = Some(path);
        self
    }

    fn save_settings(&self) -> std::io::Result<()> {
        match &self.settings_path {
            Some(path) => self.settings.save_to(path),
            None => self.settings.save(),
        }
    }

    /// Gives the session the graph channel and the live end of the graph
    /// already in it, so instrument changes and fader moves reach a running
    /// stream. Without one it still edits — it just cannot be heard until the
    /// next launch, which is what the offline paths want.
    ///
    /// The two arrive together on purpose. `controls` belongs to the graph the
    /// caller realised and handed to `graph_channel`; a session given the
    /// channel but not the controls would have a mixer whose faders moved the
    /// document silently, which is the exact bug this pairing makes
    /// unwritable.
    pub fn with_graphs(
        mut self,
        graphs: GraphPublisher,
        controls: HashMap<MixerTrackId, std::sync::Arc<fontelle_engine::TrackControls>>,
    ) -> Self {
        self.graphs = Some(graphs);
        self.track_controls = controls;
        self
    }

    /// The parameter addresses the realised graph answers to, so automation
    /// can be compiled at the right nodes — see [`crate::Realised::param_nodes`].
    ///
    /// Separate from [`with_graphs`](Self::with_graphs) rather than bundled
    /// into it because a session that never rebuilds its graph still needs
    /// this, and a caller that forgets it gets automation that draws and does
    /// nothing — which is the failure this whole pass exists to avoid, so it
    /// is worth a named method rather than a third positional argument.
    pub fn with_param_nodes(
        mut self,
        param_nodes: HashMap<fontelle_types::ParamAddress, NodeId>,
    ) -> Self {
        self.param_nodes = param_nodes;
        self
    }

    /// Gives the session somewhere to send auditions (TDD §14.1's live path).
    pub fn with_audition(mut self, sink: Box<dyn fontelle_types::EventSink>) -> Self {
        self.set_audition(sink);
        self
    }

    /// [`with_audition`](Self::with_audition) in place, for a session that has
    /// already been built.
    pub fn set_audition(&mut self, sink: Box<dyn fontelle_types::EventSink>) {
        self.audition = Some(sink);
    }

    /// Scans the soundfont folders, creating the default one on a first run.
    ///
    /// Returns the folder it had to create, if it did — worth telling the user
    /// about exactly once, because it is where their soundfonts go.
    pub fn open_bank(&mut self) -> Option<PathBuf> {
        let dirs = self.settings.soundfont_dirs_or_default();
        self.bank = SoundfontBank::new(dirs.dirs);
        self.bank.rescan();
        // Remembered, so the default is only chosen once and any folder the
        // user adds with `--soundfonts` survives a restart.
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.revision += 1;
        dirs.created
    }

    /// Reads the projects folder the settings name, if they name one.
    ///
    /// Separate from [`Session::open_bank`] because the two folders answer
    /// unrelated questions, and because **there is no default projects
    /// folder**: INVARIANT 10 says Fontelle writes nothing outside places the
    /// user named, and `~/Documents` is not one of them. `None` means ask.
    pub fn open_projects(&mut self) {
        self.projects.set_dir(self.settings.projects_dir.clone());
        self.revision += 1;
    }

    /// Points the session at a projects folder and remembers it. For tests,
    /// and for a `--projects <dir>` flag when there is one.
    pub fn set_projects_dir(&mut self, dir: Option<PathBuf>) {
        self.settings.projects_dir = dir.clone();
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.projects.set_dir(dir);
        self.revision += 1;
    }

    /// Where the open project lives, if it has been saved anywhere.
    pub fn bundle_path(&self) -> Option<&Path> {
        self.bundle.as_deref()
    }

    /// Replaces everything that is open with `opened`, from `path`.
    ///
    /// The one place a whole document is swapped, so everything that has to be
    /// dropped with it is dropped in one place: the history (an undo into the
    /// *previous* project is not an undo), every cache keyed by an id the old
    /// document minted, and — the one that would be a real bug —
    /// `track_controls`. A slotmap's keys start over per document, so an id
    /// from the old project can name a different track in the new one, and a
    /// reused control surface would be a fader silently moving somebody
    /// else's track.
    fn adopt(&mut self, opened: crate::bundle::OpenedProject, path: PathBuf) {
        self.project = opened.project;
        // The **folder's** name is the project's name. They could drift apart
        // — `save_project` writes `meta.name` and never renames a bundle — so
        // opening `MySong.fontelle` used to put "Untitled" in the title bar,
        // in the projects list, and on every render it exported. The folder is
        // the name somebody typed, so the folder wins.
        if let Some(stem) = path.file_stem() {
            self.project.meta.name = stem.to_string_lossy().into_owned();
        }
        self.library = opened.library;
        self.history = History::new();
        self.clip = Session::first_clip(&self.project).unwrap_or_default();
        self.bundle = Some(path);
        self.dirty = false;
        self.patch_cache = None;
        self.channel_presets.clear();
        self.clip_clipboard.clear();
        self.track_controls.clear();
        self.selected = self.channel_index_of_clip().unwrap_or(0);
        for missing in &opened.missing {
            self.message = Some(format!(
                "{} could not be found",
                missing.file.path.display()
            ));
        }
        // Rebuilds the graph, recompiles the timeline and bumps the revision.
        self.rebuild_graph();
    }

    /// Adds a folder to the bank and remembers it (INVARIANT 10: it is only
    /// ever the user who says where).
    pub fn add_soundfont_dir(&mut self, dir: &Path) {
        let dir = dir.to_path_buf();
        if !self.settings.soundfont_dirs.contains(&dir) {
            self.settings.soundfont_dirs.push(dir);
        }
    }

    /// The folders the bank scans. The first is the one the browser's buttons
    /// act on.
    pub fn library_dirs(&self) -> Vec<PathBuf> {
        self.bank.dirs().to_vec()
    }

    /// Points the bank at `dirs`, rescans, and writes the choice down.
    ///
    /// `add` keeps the folders already configured; otherwise this replaces
    /// them, which is what somebody who clicked "Change" meant.
    pub fn set_library_dirs(&mut self, dirs: Vec<PathBuf>, add: bool) {
        let mut wanted = if add {
            self.bank.dirs().to_vec()
        } else {
            Vec::new()
        };
        for dir in dirs {
            if !wanted.contains(&dir) {
                wanted.push(dir);
            }
        }
        self.settings.soundfont_dirs = wanted.clone();
        // Remembered before the scan, so a folder that turns out to be empty is
        // still the folder Fontelle opens on next time — the user said so.
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.bank = SoundfontBank::new(wanted);
        self.bank.rescan();
        // The bank is a different collection now: the file that was open may
        // not be in any of these folders.
        self.open_file = None;
        self.presets.clear();
        self.revision += 1;
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn library(&self) -> &SampleLibrary {
        &self.library
    }

    /// The first clip in the project, which is the one a freshly opened
    /// project shows.
    pub fn first_clip(project: &Project) -> Option<ClipId> {
        project
            .clips
            .iter()
            .filter(|(_, clip)| matches!(clip.source, ClipSource::Notes(_)))
            .map(|(id, _)| id)
            .next()
    }

    /// The channels, in the order the rack lists them. One place, because
    /// "channel 2" has to mean the same thing to the rack, the roll and the
    /// node map.
    fn channel_ids(&self) -> Vec<ChannelId> {
        self.project.channels.keys().collect()
    }

    /// The mixer's tracks in the order the panel lays them out: the ordinary
    /// ones first, the master **last**.
    ///
    /// The same reason [`Session::channel_ids`] exists — "strip 3" has to mean
    /// one thing to the canvas and to the commands it produces — plus one
    /// more: the master is drawn apart from the rest, and a panel that decided
    /// which one it was by index would put whichever track happened to be last
    /// in the arena in its column.
    fn mixer_track_ids(&self) -> Vec<MixerTrackId> {
        let master = self.project.mixer.master;
        let mut ids: Vec<MixerTrackId> = self
            .project
            .mixer
            .tracks
            .keys()
            .filter(|id| Some(*id) != master)
            .collect();
        ids.extend(master.filter(|id| self.project.mixer.tracks.contains_key(*id)));
        ids
    }

    /// The lane an automation clip for `address` belongs on, making one if
    /// this parameter has none yet.
    ///
    /// A lane is *visual only* (TDD §10.3) and deliberately cheap, which is
    /// why this inserts one rather than going through a command — the same
    /// thing `add_channel_with` does for a new channel's lane, and with the
    /// same consequence: making a lane is not on the undo stack, though
    /// everything put on it is.
    fn automation_lane(
        &mut self,
        address: &fontelle_types::ParamAddress,
        label: &str,
    ) -> fontelle_types::LaneId {
        // A lane already carrying this parameter's automation is this
        // parameter's lane. Found by the clips on it rather than by its name,
        // because the name is a caption and the address is the identity.
        let existing = self.project.clips.values().find_map(|clip| match &clip.source {
            ClipSource::Automation(data) if data.target == *address => Some(clip.lane),
            _ => None,
        });
        if let Some(lane) = existing {
            return lane;
        }
        self.project.lanes.insert(Lane {
            name: label.to_string(),
            height: 32.0,
            // Dimmer than a note lane's, so the two kinds of strip are
            // tellable apart down the header column before either is read.
            color: [0x7a, 0x6f, 0x9a, 0xff],
            muted: false,
            locked: false,
        })
    }

    /// What an automation clip is called on the arrangement.
    ///
    /// The words came from the **panel** that made it — "Master — band1.gain",
    /// which is a sentence only the side holding the strip names could write.
    /// Remembered against the address so a clip made in one session is still
    /// captioned in the next, and falling back to the address itself, which is
    /// unlovely and never wrong.
    fn automation_name(&self, address: &fontelle_types::ParamAddress) -> String {
        self.automation_names
            .get(address)
            .cloned()
            .unwrap_or_else(|| address.to_string())
    }

    /// Keeps the mixer's selection inside the mixer.
    ///
    /// Called after a track is deleted. Not on every read: an index that is
    /// briefly stale is a panel drawing the wrong strip for one frame, and an
    /// index silently rewritten under a caller is a bug that shows up
    /// somewhere else entirely.
    fn clamp_track_selection(&mut self) {
        let last = self.mixer_track_ids().len().saturating_sub(1);
        self.selected_track = self.selected_track.min(last);
    }

    /// Tells the metronome where the beats are.
    ///
    /// From the tempo map, so a click follows the tempo box; a **constant**
    /// beat, which is exact for a song at one tempo and drifts across a tempo
    /// change — see `fontelle_engine::Metronome::set_beat` for why the RT side
    /// cannot be given the map itself (INVARIANT 3).
    fn publish_metronome(&self) {
        let Some(metronome) = &self.metronome else {
            return;
        };
        metronome.set_beat(
            crate::beat_samples(&self.project),
            self.project.beats_per_bar,
        );
    }

    /// Gives the session the click the running graph is playing through.
    ///
    /// **Without this the metronome button goes dead on the first rebuild.**
    /// `rebuild_graph` passes whatever it holds to `realise`, and `realise`
    /// mints a fresh `Metronome` when handed `None` — so a session that was
    /// never given one adopts a new node on every rebuild while the transport
    /// bar goes on holding the original `Arc`. Everything still compiles and
    /// nothing sounds. See `fontelle-app/tests/click.rs`.
    pub fn with_metronome(mut self, metronome: std::sync::Arc<fontelle_engine::Metronome>) -> Self {
        self.metronome = Some(metronome);
        self.publish_metronome();
        self
    }

    /// The click itself, so a caller can hand the same switch to the transport
    /// bar — the button and the node in the schedule have to be one thing.
    pub fn metronome(&self) -> std::sync::Arc<fontelle_engine::Metronome> {
        self.metronome
            .clone()
            .unwrap_or_else(|| std::sync::Arc::new(fontelle_engine::Metronome::new()))
    }

    /// Gives the session the cell live MIDI is routed through (TDD §14.3).
    ///
    /// Published straight away and again after every selection change and
    /// every graph rebuild: a rebuild mints new node ids, so a target set once
    /// and never again is aimed at a node that no longer exists — which is
    /// silence, and indistinguishable from never having wired it up.
    ///
    /// Optional, because every offline path builds a `Session` without one.
    pub fn with_live_target(mut self, target: std::sync::Arc<fontelle_midi::LiveTarget>) -> Self {
        self.live_target = Some(target);
        self.publish_live_target();
        self
    }

    /// Points live MIDI at the selected channel.
    fn publish_live_target(&self) {
        if let Some(target) = &self.live_target {
            target.set(self.audition_target());
        }
    }

    /// Gives the session the recording end of the live-event channel.
    ///
    /// Without one the studio still plays and still edits; it simply cannot
    /// keep a take, which is what the offline paths want.
    pub fn with_capture(mut self, capture: fontelle_engine::CaptureReader) -> Self {
        self.capture = Some(capture);
        self
    }

    /// Bounces the whole project to a WAV inside its own `renders/` folder,
    /// and says where it went.
    ///
    /// The last clause of §3's gate sentence. `render_offline` has existed
    /// since M0; what was missing was somewhere to put the result.
    ///
    /// Rendered at [`crate::RENDER_QUALITY`] rather than at the session's own
    /// playback quality — that is what those two constants are for (§7.6), and
    /// a bounce is the one time the extra cost does not have to fit in a
    /// callback.
    ///
    /// **A read.** It builds its own graph and its own transport rather than
    /// borrowing the running ones, so bouncing does not move the playhead,
    /// mark the document dirty, or disturb what you are listening to.
    pub fn export_wav(&mut self) -> Result<String, String> {
        // A render lives inside the bundle, so there has to be a bundle.
        // Guessing at somewhere else would be a write outside anywhere the
        // user named (INVARIANT 10). Every project *made in the window* is on
        // disk from the moment it is made, so this is the scratch session
        // `--blank` opens and nothing else.
        let bundle = self
            .bundle
            .clone()
            .ok_or_else(|| "save this project first — a render goes inside it".to_string())?;

        let options = RealiseOptions {
            quality: crate::RENDER_QUALITY,
            ..self.options
        };
        let mut realised =
            realise(&self.project, &self.library, options).map_err(|e| e.to_string())?;
        let timeline = fontelle_sequencer::compile(
            &self.project,
            &realised.channel_nodes,
            &Default::default(),
        );
        let samples = crate::project_duration_samples(&self.project, RELEASE_TAIL);
        let audio = crate::render_offline(&timeline, &mut realised.graph, samples);

        let renders = bundle.join("renders");
        std::fs::create_dir_all(&renders).map_err(|e| format!("{}: {e}", renders.display()))?;
        // A name nothing in the folder has: bouncing twice to compare them is
        // the ordinary thing to do, and a render that silently replaced the
        // one you were comparing against would be the worst possible moment to
        // find that out.
        let taken: Vec<String> = std::fs::read_dir(&renders)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                std::path::Path::new(&e.file_name())
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .collect();
        // Named after the **bundle**, not `meta.name`: the folder is the name
        // the user typed and the one they will look for in a file manager,
        // and the two can disagree on a project whose folder was renamed
        // outside Fontelle. `adopt` keeps them together on open; this is
        // right even when nothing has opened yet.
        let stem = crate::unique_name(
            &bundle
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.project.meta.name.clone()),
            &taken.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        let path = renders.join(format!("{stem}.wav"));

        let clipped = crate::write_wav16(&path, &audio, 2, self.options.sample_rate)
            .map_err(|e| format!("{}: {e}", path.display()))?;

        self.revision += 1;
        Ok(match clipped {
            // Surfaced rather than swallowed, which is what §11 of the plan
            // asks for: a bounce that clipped is one you want to know about
            // before you send it anywhere.
            0 => format!("exported {}", path.display()),
            1 => format!("exported {} \u{2014} 1 clipped sample", path.display()),
            n => format!("exported {} \u{2014} {n} clipped samples", path.display()),
        })
    }

    /// Writes a backup of the open project into its own `backups/` folder.
    ///
    /// Returns whether anything was written. **Nothing is, unless there are
    /// unsaved changes** — the window sleeps at idle (§16.3) and a timer that
    /// wrote an identical file every minute would be the one thing keeping it
    /// awake.
    ///
    /// A whole bundle rather than a loose `project.json`, so recovering one is
    /// opening it like any other project rather than renaming a file by hand.
    /// It costs a few kilobytes: `save_project` writes one JSON file and
    /// assets are referenced, not copied (§17.4).
    ///
    /// **It does not count as saving.** The document stays dirty and the title
    /// bar keeps its dot, or somebody quits believing their work is on disk
    /// where they put it.
    pub fn autosave(&mut self) -> bool {
        if !self.dirty {
            return false;
        }
        let Some(bundle) = self.bundle.clone() else {
            return false; // nowhere the user named to put it
        };
        let path = bundle.join("backups").join("autosave.fontelle");
        match crate::save_project(&self.project, &path) {
            Ok(()) => true,
            Err(e) => {
                self.message = Some(format!("could not write a backup: {e}"));
                false
            }
        }
    }

    /// Whether the click is on, and the switch for it. The window's, not the
    /// document's: a project sent to somebody else must not arrive with a
    /// woodblock on every beat.
    pub fn metronome_on(&self) -> bool {
        self.metronome.as_ref().is_some_and(|m| m.is_on())
    }

    pub fn set_metronome(&mut self, on: bool) {
        if let Some(metronome) = &self.metronome {
            metronome.set_on(on);
        }
        self.revision += 1;
    }

    /// Pushes the document's levels, pans and effective mutes at the graph
    /// that is already playing, and tells the panels to redraw.
    ///
    /// **Not** a graph rebuild, which is the point: `realise` deserialises
    /// every channel's patch, and a fader drag calls this sixty times a
    /// second. It is also why muting a track no longer reloads every
    /// soundfont in the project.
    fn publish_mixer(&mut self) {
        apply_mixer_controls(&self.project, &self.track_controls);
        self.revision += 1;
    }

    /// The live end of every mixer track's fader, in the order
    /// [`Session::mixer_track_ids`] gives.
    ///
    /// Public so a test can prove a fader was heard without a sound card, and
    /// prove it was heard *without* the graph being rebuilt.
    pub fn track_controls(&self) -> Vec<std::sync::Arc<fontelle_engine::TrackControls>> {
        self.mixer_track_ids()
            .into_iter()
            .filter_map(|id| self.track_controls.get(&id).cloned())
            .collect()
    }

    /// The lanes, in the order the arrangement stacks them. The same reason
    /// [`Session::channel_ids`] exists: "lane 3" has to mean one thing to the
    /// canvas and to the commands it produces.
    fn lane_ids(&self) -> Vec<LaneId> {
        self.project.lanes.keys().collect()
    }

    /// The channel a clip plays, for the block's caption.
    fn channel_of_clip(&self, clip: ClipId) -> Option<ChannelId> {
        match &self.project.clips.get(clip)?.source {
            ClipSource::Notes(data) => Some(data.channel),
            _ => None,
        }
    }

    fn channel_index_of_clip(&self) -> Option<usize> {
        let ClipSource::Notes(data) = &self.project.clips.get(self.clip)?.source else {
            return None;
        };
        self.channel_ids().iter().position(|id| *id == data.channel)
    }

    /// Recompiles the timeline and hands the result to the audio thread.
    ///
    /// A full recompile, not the segmented one §11.3 describes: at the sizes
    /// this opens today it is well under a frame, and an incremental path that
    /// is wrong is worse than a whole one that is slow. `DirtyBars` is where
    /// that goes when it is needed.
    /// The timeline this session's document compiles to, right now.
    ///
    /// Public so a test can prove a *sequencer* mute is one — that a muted
    /// channel puts no events on the timeline at all — without a sound card.
    pub fn compiled(&self) -> fontelle_types::CompiledTimeline {
        fontelle_sequencer::compile(&self.project, &self.channel_nodes, &self.param_nodes)
    }

    fn republish(&mut self) {
        let timeline =
            fontelle_sequencer::compile(&self.project, &self.channel_nodes, &self.param_nodes);
        self.publisher.publish(timeline);
    }

    /// Rebuilds the whole graph and publishes it — what an instrument change,
    /// a new channel or a mute needs.
    ///
    /// Note the order: the node map is rebuilt *first*, because adding a
    /// channel renumbers the nodes, and a timeline compiled against the old map
    /// would address notes to nodes that have moved.
    /// What an addressed parameter is worth right now, normalised.
    ///
    /// So a fresh automation clip starts by changing nothing: a lane that
    /// jumped the parameter the moment it was created is a lane nobody trusts.
    fn parameter_now(&self, address: &fontelle_types::ParamAddress) -> Option<f64> {
        use fontelle_types::ParamTarget;
        match ParamTarget::parse(address)? {
            ParamTarget::Tempo => None,
            ParamTarget::TrackGain(id) => {
                let db = self.project.mixer.tracks.get(id)?.gain_db;
                let span = fontelle_engine::GAIN_MAX_DB - fontelle_engine::GAIN_MIN_DB;
                Some(f64::from(
                    ((db - fontelle_engine::GAIN_MIN_DB) / span).clamp(0.0, 1.0),
                ))
            }
            ParamTarget::TrackPan(id) => {
                let pan = self.project.mixer.tracks.get(id)?.pan;
                Some(f64::from((pan + 1.0).clamp(0.0, 2.0) / 2.0))
            }
            ParamTarget::Insert { track, slot, param } => {
                let insert = self.project.mixer.tracks.get(track)?.inserts.get(slot)?;
                insert.config.normalised(&param).map(f64::from)
            }
        }
    }

    fn rebuild_graph(&mut self) {
        // Reusing the control surfaces, so a track's fader and meter outlive
        // the graph they were built with — see `realise::fader`.
        match crate::realise::realise_with(
            &self.project,
            &self.library,
            self.options,
            &self.track_controls,
            self.metronome.clone(),
        ) {
            Ok(realised) => {
                self.channel_nodes = realised.channel_nodes;
                self.param_nodes = realised.param_nodes;
                // The old set belonged to the graph that is being replaced.
                self.track_controls = realised.track_controls;
                self.effect_controls = realised.effect_controls;
                self.metronome = Some(realised.metronome);
                self.publish_metronome();
                // New graph, new node ids — including the one a plugged-in
                // keyboard is playing through.
                self.publish_live_target();
                for (_, missing) in &realised.unresolved {
                    self.message = Some(format!("layer {} has no audio", missing.layer));
                }
                if let Some(graphs) = &mut self.graphs {
                    graphs.publish(realised.graph);
                }
            }
            Err(e) => self.message = Some(e.to_string()),
        }
        self.republish();
        self.revision += 1;
    }

    fn run(&mut self, command: Box<dyn Command>) {
        match self.history.apply(command, &mut self.project) {
            Ok(()) => {
                self.dirty = true;
                self.republish();
            }
            // A refused edit is not a crash and not a history entry — a note
            // dragged past key 127 simply does not move.
            Err(e) => {
                eprintln!("Fontelle: {e}");
                self.message = Some(e.to_string());
            }
        }
    }

    fn notes_of_clip(&self) -> Option<&Arena<NoteId, Note>> {
        match &self.project.clips.get(self.clip)?.source {
            ClipSource::Notes(data) => Some(&data.notes),
            _ => None,
        }
    }

    /// The clip a channel writes into, creating one when it has none.
    fn clip_of_channel(&self, channel: ChannelId) -> Option<ClipId> {
        self.project
            .clips
            .iter()
            .find(|(_, clip)| match &clip.source {
                ClipSource::Notes(data) => data.channel == channel,
                _ => false,
            })
            .map(|(id, _)| id)
    }

    /// The paths the browser's first list is showing, row by row.
    ///
    /// `None` for a row that is not a soundfont — the way up, and a folder.
    /// One function so `open_file`, `selected_file` and the panel can never
    /// disagree about what row `n` is.
    fn file_rows(&self) -> Vec<Option<PathBuf>> {
        if !self.query.trim().is_empty() {
            return self
                .bank
                .search(&self.query)
                .into_iter()
                .map(|entry| Some(entry.path.clone()))
                .collect();
        }
        self.bank
            .rows()
            .iter()
            .map(|row| match row {
                BankRow::File(entry) => Some(entry.path.clone()),
                _ => None,
            })
            .collect()
    }

    /// Which projects the live search leaves, as indices into the library's
    /// own list. The same shape as [`Session::filtered_files`] and for the
    /// same reason: the panel indexes the filtered list, the commands need the
    /// real one.
    fn filtered_projects(&self) -> Vec<usize> {
        let names: Vec<&str> = self
            .projects
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        matches_names(&names, &self.query)
    }

    fn filtered_presets(&self) -> Vec<usize> {
        let names: Vec<&str> = self.presets.iter().map(|p| p.name.as_str()).collect();
        matches_names(&names, &self.query)
    }

    /// The node an audition should reach: the selected channel's.
    ///
    /// Public because it is also where **live MIDI** goes (§14.3). A keyboard
    /// and a clicked key on the roll play the same instrument, and having one
    /// answer to "which node is that" is what keeps them from drifting apart.
    pub fn audition_target(&self) -> NodeId {
        self.channel_ids()
            .get(self.selected)
            .and_then(|id| self.channel_nodes.get(id).copied())
            .unwrap_or_default()
    }

    fn send_live(&mut self, payload: EventPayload) {
        let target = self.audition_target();
        if let Some(sink) = &mut self.audition {
            // Sample zero: the live drain stamps events with the position the
            // audio thread is actually at, so a timestamp from here would only
            // be a guess about a clock this thread cannot read.
            sink.send(TimedEvent {
                sample: 0,
                target,
                payload,
            });
        }
    }

    /// Puts a preset onto a channel, through the command path like everything
    /// else, and rebuilds the graph so it can be heard.
    fn install_preset(&mut self, channel: ChannelId, preset: usize) -> Result<(), String> {
        let file = self.open_file_path().ok_or("no soundfont is open")?;
        let index = self
            .presets
            .get(preset)
            .map(|p| p.index)
            .ok_or("that preset is not in this soundfont")?;
        let patch = self
            .library
            .import_sf2(&file, index)
            .map_err(|e| format!("{}: {e}", file.display()))?;
        // Through the history, like every other document mutation
        // (INVARIANT 9). Putting the wrong soundfont on a channel is exactly
        // the kind of thing somebody presses Ctrl+Z on.
        let data = patch
            .to_data(self.library.provenance())
            .map_err(|e| e.to_string())?;
        // The patch and the channel's name are one thing a person did — they
        // chose an instrument — so they are one history entry. The rack says
        // what a channel is playing, and a channel that goes on saying what it
        // used to be is the loudest "nothing happened" a rack can give
        // somebody who has just changed its sound. There is no way to name a
        // channel by hand yet; when there is, this becomes "rename unless the
        // user named it".
        let mut parts: Vec<Box<dyn Command>> = vec![Box::new(
            fontelle_model::SetChannelPatch::new(channel, Some(data)),
        )];
        if let Some(name) = self.presets.get(preset).map(|p| p.name.clone())
            && !name.is_empty()
        {
            parts.push(Box::new(fontelle_model::RenameChannel::new(channel, name)));
        }
        self.history
            .apply(
                Box::new(fontelle_model::Compound::new("Choose instrument", parts)),
                &mut self.project,
            )
            .map_err(|e| e.to_string())?;
        self.history.break_gesture();

        self.channel_presets.insert(channel, (file, index));
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        Ok(())
    }

    /// The selected channel, if there is one.
    fn selected_channel_id(&self) -> Option<ChannelId> {
        self.channel_ids().get(self.selected).copied()
    }

    /// The selected channel's patch, from the cache or freshly parsed.
    ///
    /// `None` when the channel has no instrument on it, or when what it has
    /// cannot be read — a patch from a future format version is a message, not
    /// a panel of knobs writing over it.
    fn selected_patch(&self) -> Option<fontelle_core::Patch> {
        let channel = self.selected_channel_id()?;
        if let Some((cached, patch)) = &self.patch_cache
            && *cached == channel
        {
            return Some(patch.clone());
        }
        let data = self.project.channels.get(channel)?.patch_data.as_ref()?;
        fontelle_core::Patch::from_data(data, |file| self.library.resolve(file))
            .ok()
            .map(|loaded| loaded.patch)
    }

    /// Writes `patch` back onto `channel` through the history, coalescing with
    /// its own predecessor so a knob drag is one undo entry.
    fn store_patch(&mut self, channel: ChannelId, patch: fontelle_core::Patch) {
        let data = match patch.to_data(self.library.provenance()) {
            Ok(data) => data,
            Err(e) => {
                self.message = Some(e.to_string());
                return;
            }
        };
        if let Err(e) = self.history.apply(
            Box::new(fontelle_model::SetChannelPatch::new(channel, Some(data))),
            &mut self.project,
        ) {
            self.message = Some(e.to_string());
            return;
        }
        self.patch_cache = Some((channel, patch));
        self.dirty = true;
        // The graph carries the instrument, so a patch change is a graph
        // change — otherwise you cannot hear what you just turned.
        self.rebuild_graph();
    }

    fn open_file_path(&self) -> Option<PathBuf> {
        self.open_file.clone()
    }
}

impl DocumentHost for Session {
    fn notes(&self) -> &Arena<NoteId, Note> {
        self.notes_of_clip().unwrap_or(&self.empty)
    }

    fn edit(&mut self, edit: RollEdit) -> Vec<NoteId> {
        let clip = self.clip;
        // `AddNotes` is the one command whose ids the caller needs back, so it
        // is applied through the history by hand rather than through `run`.
        // Everything else goes the ordinary way.
        match edit {
            // The roll hands over a whole note, properties and all: what you
            // draw is a copy of the last note you drew or clicked, which is
            // `PianoRoll::template`.
            RollEdit::Add { note } => self.insert(clip, vec![note]),
            RollEdit::Insert(notes) => self.insert(clip, notes),
            RollEdit::Remove(ids) => {
                self.run(Box::new(RemoveNotes::new(clip, ids)));
                Vec::new()
            }
            RollEdit::Move {
                ids,
                tick_delta,
                key_delta,
            } => {
                self.run(Box::new(MoveNotes::new(clip, ids, tick_delta, key_delta)));
                Vec::new()
            }
            RollEdit::Resize { ids, tick_delta } => {
                self.run(Box::new(ResizeNotes::new(clip, ids, tick_delta)));
                Vec::new()
            }
            RollEdit::Slice { cuts } => {
                self.run(Box::new(fontelle_model::SliceNotes::new(clip, cuts)));
                self.history.break_gesture();
                Vec::new()
            }
            RollEdit::SetSlide { ids, slide } => {
                self.run(Box::new(fontelle_model::SetNoteSlide::new(
                    clip, ids, slide,
                )));
                Vec::new()
            }
            RollEdit::SetProperty {
                ids,
                property,
                value,
            } => {
                self.run(Box::new(SetNoteProperty::new(
                    clip,
                    ids,
                    property.property(),
                    value,
                )));
                Vec::new()
            }
        }
    }

    fn undo(&mut self) {
        if let Some(result) = self.history.undo(&mut self.project) {
            if let Err(e) = result {
                eprintln!("Fontelle: could not undo — {e}");
                return;
            }
            self.dirty = true;
            // An undo can put a different patch back, and the graph carries the
            // instrument, so both caches have to go and the graph has to be
            // rebuilt — a Ctrl+Z that changes what you see and not what you
            // hear is worse than one that does nothing.
            self.patch_cache = None;
            self.rebuild_graph();
        }
    }

    fn redo(&mut self) {
        if let Some(result) = self.history.redo(&mut self.project) {
            if let Err(e) = result {
                eprintln!("Fontelle: could not redo — {e}");
                return;
            }
            self.dirty = true;
            self.patch_cache = None;
            self.rebuild_graph();
        }
    }

    fn end_gesture(&mut self) {
        self.history.break_gesture();
    }

    fn beats_per_bar(&self) -> u32 {
        self.project.beats_per_bar
    }

    /// The tempo at the **start** of the piece. See the trait's own note: a
    /// song with a tempo change has no single BPM (INVARIANT 5), and this is
    /// the number the transport's box shows and moves.
    fn tempo(&self) -> f64 {
        self.project.tempo_map.tempo_at(0)
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.run(Box::new(SetNumber::new(NumberTarget::Tempo, bpm)));
        // The click counts in samples, so a new tempo is a new beat length.
        self.publish_metronome();
        // The tempo is the one document value the *timeline* depends on for
        // more than its contents: every tick becomes a different sample.
        // `run` has already republished it. The graph is untouched — nothing
        // in it knows about beats.
        self.revision += 1;
    }

    fn set_beats_per_bar(&mut self, beats: u32) {
        self.run(Box::new(SetNumber::new(
            NumberTarget::BeatsPerBar,
            f64::from(beats),
        )));
        // And a new signature is a new downbeat.
        self.publish_metronome();
        self.revision += 1;
    }

    /// From the song's own tempo map rather than from a BPM: a song with a
    /// tempo change has no single BPM (INVARIANT 5), and this is what makes a
    /// clicked note sound for as long as it *is*.
    ///
    /// Measured over a whole beat and divided down, so a map stored in samples
    /// answers to within a sample rather than to within a tick.
    fn seconds_per_tick(&self) -> f64 {
        let map = &self.project.tempo_map;
        let rate = map.sample_rate_hz();
        if rate <= 0.0 {
            return 0.5 / PPQN as f64;
        }
        let beat = map.tick_to_sample(PPQN) - map.tick_to_sample(0);
        if beat <= 0 {
            return 0.5 / PPQN as f64;
        }
        beat as f64 / rate / PPQN as f64
    }

    fn playhead_tick(&self, position_sample: Sample) -> Option<Tick> {
        let clip = self.project.clips.get(self.clip)?;
        let tick = self.project.tempo_map.sample_to_tick(position_sample);
        // Only while the playhead is actually over this clip: a roll that
        // draws a playhead parked at its left edge whenever the song is
        // elsewhere is lying about where you are.
        (tick >= clip.start && tick <= clip.start + clip.length).then(|| tick - clip.start)
    }

    fn sample_of_clip_tick(&self, tick: Tick) -> Sample {
        let start = self
            .project
            .clips
            .get(self.clip)
            .map_or(0, |clip| clip.start);
        self.project.tempo_map.tick_to_sample(start + tick.max(0))
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn save(&mut self) -> Result<(), String> {
        let bundle = self.bundle.clone().ok_or_else(|| {
            "this project has no file yet — open it with --save <path>".to_string()
        })?;
        crate::save_project(&self.project, &bundle).map_err(|e| e.to_string())?;
        self.dirty = false;
        self.revision += 1;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.project.meta.name
    }
}

impl Session {
    /// The half of [`DocumentHost::edit`] that has to hand ids back.
    ///
    /// The history owns the command from the moment it is applied, so the ids
    /// it minted are read back off the entry on top of the undo stack — see
    /// `History::last_applied`, which exists for exactly this.
    fn insert(&mut self, clip: ClipId, notes: Vec<Note>) -> Vec<NoteId> {
        if notes.is_empty() {
            return Vec::new();
        }
        if let Err(e) = self
            .history
            .apply(Box::new(AddNotes::new(clip, notes)), &mut self.project)
        {
            eprintln!("Fontelle: {e}");
            self.message = Some(e.to_string());
            return Vec::new();
        }
        self.dirty = true;
        self.republish();
        self.history
            .last_applied()
            .and_then(|command| command.as_any().downcast_ref::<AddNotes>())
            .map(|add| add.ids().to_vec())
            .unwrap_or_default()
    }

    /// Applies `command` through the history and hands back the entry, so a
    /// caller that needs the id the command minted can downcast for it.
    fn apply_for<T: Command + 'static>(&mut self, command: Box<dyn Command>) -> Result<&T, String> {
        self.history
            .apply(command, &mut self.project)
            .map_err(|e| e.to_string())?;
        self.dirty = true;
        self.history
            .last_applied()
            .and_then(|c| c.as_any().downcast_ref::<T>())
            .ok_or_else(|| "the command that was just applied is not on the history".to_string())
    }
}

impl StudioHost for Session {
    fn revision(&self) -> u64 {
        self.revision
    }

    fn channels(&self) -> Vec<ChannelInfo> {
        self.project
            .channels
            .values()
            .map(|channel| ChannelInfo {
                name: channel.name.clone(),
                // The channel's own switches, not its track's. Since a channel
                // plays through the master by default, a rack mute that
                // reached for the track would silence the whole song.
                muted: channel.muted,
                soloed: channel.soloed,
                has_instrument: channel.patch_data.is_some(),
                // What the row's route chip says. `None` is the master, which
                // the chip writes out by name rather than as a blank.
                route: channel
                    .mixer_track
                    .and_then(|id| self.mixer_track_ids().iter().position(|t| *t == id)),
            })
            .collect()
    }

    fn selected_channel(&self) -> usize {
        self.selected
    }

    fn select_channel(&mut self, index: usize) {
        let Some(channel) = self.channel_ids().get(index).copied() else {
            return;
        };
        self.selected = index;
        self.patch_cache = None;
        // A MIDI keyboard follows the rack, the same as an audition does.
        self.publish_live_target();
        // The roll follows the rack: selecting a channel opens its clip, which
        // is the whole reason a rack and a roll are next to each other.
        if let Some(clip) = self.clip_of_channel(channel) {
            self.clip = clip;
        }
        self.revision += 1;
    }

    /// The rack's mute. The **channel's**, not its mixer track's.
    ///
    /// A sequencer mute, so what has to be republished is the *timeline* — the
    /// compiler drops a muted channel's clips. `run` does that already, which
    /// is why there is nothing else here.
    fn toggle_mute(&mut self, index: usize) {
        let Some(channel) = self.channel_ids().get(index).copied() else {
            return;
        };
        let now = self.project.channels.get(channel).is_some_and(|c| c.muted);
        self.run(Box::new(SetFlag::new(
            FlagTarget::ChannelMuted(channel),
            !now,
        )));
        self.history.break_gesture();
        self.revision += 1;
    }

    fn toggle_solo(&mut self, index: usize) {
        let Some(channel) = self.channel_ids().get(index).copied() else {
            return;
        };
        let now = self.project.channels.get(channel).is_some_and(|c| c.soloed);
        self.run(Box::new(SetFlag::new(
            FlagTarget::ChannelSoloed(channel),
            !now,
        )));
        self.history.break_gesture();
        self.revision += 1;
    }

    // --- routing (TDD §13.1) ---

    fn route_names(&self) -> Vec<String> {
        self.mixer_track_ids()
            .into_iter()
            .filter_map(|id| self.project.mixer.tracks.get(id))
            .map(|track| track.name.clone())
            .collect()
    }

    fn set_channel_route(&mut self, channel: usize, route: Option<usize>) {
        let Some(id) = self.channel_ids().get(channel).copied() else {
            return;
        };
        // `None` from the panel and `None` in the document mean the same
        // thing — the master — and so does a route naming the master's own
        // strip, which is what the chip's first row is.
        let track = route
            .and_then(|strip| self.mixer_track_ids().get(strip).copied())
            .filter(|id| Some(*id) != self.project.mixer.master);
        self.run(Box::new(fontelle_model::SetChannelRoute::new(id, track)));
        self.history.break_gesture();
        // The channel's audio arrives on a different bus now, which is the
        // graph's shape and not a value in it.
        self.rebuild_graph();
    }

    fn add_mixer_track(&mut self) {
        // Named for where it lands, which is what every mixer does and what
        // makes "send the drums to 3" a sentence.
        let name = format!("Track {}", self.project.mixer.tracks.len());
        self.run(Box::new(fontelle_model::AddMixerTrack::new(name)));
        self.history.break_gesture();
        // The one you just made is the one you are about to put an effect on,
        // so the options column follows it. It lands before the master, which
        // `mixer_track_ids` keeps last.
        self.selected_track = self.mixer_track_ids().len().saturating_sub(2);
        // A new bus is a new pair of buffers, so this is the graph's shape
        // changing rather than a value in it.
        self.rebuild_graph();
    }

    fn remove_mixer_track(&mut self, strip: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::RemoveMixerTrack::new(id)));
        self.history.break_gesture();
        // The list is shorter than the selection now, if the last strip went.
        self.clamp_track_selection();
        self.rebuild_graph();
    }

    fn selected_mixer_track(&self) -> usize {
        self.selected_track
    }

    fn select_mixer_track(&mut self, strip: usize) {
        // Ignored rather than clamped when it is past the end: a panel and a
        // document disagree for a frame every time a track is deleted, and a
        // selection that followed the panel off the end would be an index
        // nothing else could use.
        if strip >= self.mixer_track_ids().len() {
            return;
        }
        self.selected_track = strip;
        self.revision += 1;
    }

    fn track_output(&self, strip: usize) -> Option<usize> {
        let ids = self.mixer_track_ids();
        let output = self.project.mixer.tracks.get(*ids.get(strip)?)?.output?;
        // The master has two spellings in the document — `None`, and its own
        // id, which `AddMixerTrack` writes because it is "the only destination
        // that is always there". The panel has one, so both come back as
        // `None` here.
        if Some(output) == self.project.mixer.master {
            return None;
        }
        ids.iter().position(|id| *id == output)
    }

    fn set_track_output(&mut self, strip: usize, target: Option<usize>) {
        let ids = self.mixer_track_ids();
        let Some(id) = ids.get(strip).copied() else {
            return;
        };
        // `None` from the panel and `None` in the document mean the same
        // thing, and so does a target naming the master's own strip — which is
        // what the menu's first row is. The same normalisation
        // `set_channel_route` does.
        let output = target
            .and_then(|strip| ids.get(strip).copied())
            .filter(|id| Some(*id) != self.project.mixer.master);
        // A loop is refused by the command and reported by `run`, which is
        // what puts the message in front of the user (§13.2).
        self.run(Box::new(fontelle_model::SetTrackOutput::new(id, output)));
        self.history.break_gesture();
        // The signal arrives on a different bus now, which is the graph's
        // shape rather than a value in it.
        self.rebuild_graph();
    }

    fn move_insert(&mut self, strip: usize, from: usize, to: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        let len = self
            .project
            .mixer
            .tracks
            .get(id)
            .map_or(0, |track| track.inserts.len());
        // Guarded here rather than left to the command, which refuses both of
        // these with an error. A drag that ended where it started is not a
        // mistake worth telling anybody about — see `MoveInsert::apply`.
        if from >= len || to >= len || from == to {
            return;
        }
        self.run(Box::new(fontelle_model::MoveInsert::new(id, from, to)));
        self.history.break_gesture();
        // Order is what the chain *is*, so the schedule changes.
        self.rebuild_graph();
    }

    fn rename_mixer_track(&mut self, strip: usize, name: &str) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::RenameMixerTrack::new(id, name)));
        self.revision += 1;
    }

    /// What the browser's first list shows: the folder you are standing in,
    /// or — the moment anything is typed — the whole collection.
    ///
    /// **The search box decides which.** Browsing is a question about
    /// structure and searching is a question about names, and a search that
    /// only looked in the folder you happened to be standing in would not be
    /// §17.5's "instant fuzzy search over a large collection".
    fn library_files(&self) -> Vec<LibraryEntry> {
        if !self.query.trim().is_empty() {
            return self
                .bank
                .search(&self.query)
                .into_iter()
                .map(|entry| LibraryEntry {
                    name: entry.name.clone(),
                    // Where it is, not how big: two files called `Kit` in
                    // different folders is the commonest thing in a
                    // collection, and a flat list of names cannot tell them
                    // apart. The size is on the row once you are in the folder.
                    detail: match self.bank.folder_of(entry) {
                        folder if folder.is_empty() => human_size(entry.size_bytes),
                        folder => folder,
                    },
                    kind: fontelle_ui::document::LibraryKind::File,
                })
                .collect();
        }

        self.bank
            .rows()
            .iter()
            .map(|row| match row {
                BankRow::Up { .. } => LibraryEntry {
                    name: "..".to_string(),
                    detail: String::new(),
                    kind: fontelle_ui::document::LibraryKind::Up,
                },
                BankRow::Folder {
                    name, soundfonts, ..
                } => LibraryEntry {
                    name: name.clone(),
                    detail: match soundfonts {
                        0 => String::new(),
                        1 => "1 sf2".to_string(),
                        n => format!("{n} sf2"),
                    },
                    kind: fontelle_ui::document::LibraryKind::Folder,
                },
                BankRow::File(entry) => LibraryEntry {
                    name: entry.name.clone(),
                    detail: human_size(entry.size_bytes),
                    kind: fontelle_ui::document::LibraryKind::File,
                },
            })
            .collect()
    }

    fn library_presets(&self) -> Vec<LibraryEntry> {
        self.filtered_presets()
            .into_iter()
            .filter_map(|index| self.presets.get(index))
            .map(|preset| {
                // Bank and program, because a General MIDI soundfont has three
                // presets called "Piano" and the numbers are what tell them
                // apart.
                LibraryEntry::file(
                    preset.name.clone(),
                    format!("{}:{}", preset.bank, preset.program),
                )
            })
            .collect()
    }

    fn query(&self) -> &str {
        &self.query
    }

    fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        // The list is a different list now — the whole collection rather than
        // one folder, or the other way round. The file that was open is still
        // open; only which row it is has changed, and `selected_file` works
        // that out from its path.
        self.revision += 1;
    }

    fn open_file(&mut self, index: usize) -> Result<(), String> {
        // A folder row moves the browser and opens nothing. Only while
        // *browsing*: a search lists files wherever they are, and a hit is
        // always a file.
        if self.query.trim().is_empty() && self.bank.open_row(index) {
            // The rows under the pointer are different ones now, and the file
            // that was open is not in this folder.
            self.open_file = None;
            self.presets.clear();
            self.revision += 1;
            return Ok(());
        }

        let path = self
            .file_rows()
            .get(index)
            .cloned()
            .flatten()
            .ok_or("that soundfont is not in the bank any more")?;
        // Presets only: this reads the file's headers and decodes no audio, so
        // clicking through a collection costs nothing (TDD §17.5's "audition
        // on click without loading into a channel" is the next step past it).
        let presets =
            fontelle_assets::list_presets(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.presets = presets;
        self.open_file = Some(path);
        self.revision += 1;
        Ok(())
    }

    fn selected_file(&self) -> Option<usize> {
        let open = self.open_file.as_ref()?;
        // Worked out from the path each time, so a folder change or a search
        // moves the highlight to wherever the file now is — or removes it,
        // which is true when the file is not in the list at all.
        self.file_rows()
            .into_iter()
            .position(|row| row.as_ref() == Some(open))
    }

    fn selected_preset(&self) -> Option<usize> {
        let channel = self.channel_ids().get(self.selected).copied()?;
        let (file, index) = self.channel_presets.get(&channel)?;
        // Only when the browser is showing the file it came from: a preset
        // number means nothing across two different soundfonts, and a stale
        // highlight is worse than none.
        if self.open_file_path().as_ref() != Some(file) {
            return None;
        }
        // Into the *filtered* list, because that is what the panel draws — a
        // search that has hidden the chosen preset shows no highlight, which is
        // true.
        self.filtered_presets()
            .into_iter()
            .position(|p| self.presets.get(p).is_some_and(|info| info.index == *index))
    }

    fn add_channel_with(&mut self, preset: usize) -> Result<(), String> {
        let name = self
            .presets
            .get(preset)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("Channel {}", self.project.channels.len() + 1));

        // Through the history like everything else (INVARIANT 9), so adding an
        // instrument by mistake is one Ctrl+Z away. It is three entries — the
        // channel, its clip, its patch — rather than one; a compound command
        // is what makes it one, and there is no need for one yet.
        let channel = self
            .apply_for::<AddChannel>(Box::new(AddChannel::new(name, None)))?
            .channel()
            .ok_or("the channel was not created")?;
        self.history.break_gesture();

        // A channel with nowhere to write notes is a channel the roll cannot
        // open, so it gets a lane and an empty clip of its own, as long as the
        // longest one already there.
        let lane = self.project.lanes.insert(Lane {
            name: format!("Lane {}", self.project.lanes.len() + 1),
            height: 32.0,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            muted: false,
            locked: false,
        });
        let length = self
            .project
            .clips
            .values()
            .map(|c| c.length)
            .max()
            .unwrap_or(0)
            .max(PPQN * 4 * NEW_CLIP_BARS);
        self.history
            .apply(
                Box::new(AddClip::new(Clip {
                    lane,
                    start: 0,
                    length,
                    source: ClipSource::Notes(NoteData {
                        channel,
                        notes: Arena::default(),
                    }),
                    prefab_link: None,
                    color: None,
                    muted: false,
                    loop_length: None,
                })),
                &mut self.project,
            )
            .map_err(|e| e.to_string())?;
        self.history.break_gesture();

        self.dirty = true;
        self.selected = self.project.channels.len().saturating_sub(1);
        if let Some(id) = self.clip_of_channel(channel) {
            self.clip = id;
        }
        self.install_preset(channel, preset)
    }

    fn set_channel_instrument(&mut self, preset: usize) -> Result<(), String> {
        let channel = self
            .channel_ids()
            .get(self.selected)
            .copied()
            .ok_or("there is no channel selected")?;
        self.install_preset(channel, preset)
    }

    // --- the projects folder (TDD §17.1, §17.3) ---

    fn projects(&self) -> Vec<LibraryEntry> {
        self.filtered_projects()
            .into_iter()
            .filter_map(|index| self.projects.entries().get(index))
            .map(|entry| LibraryEntry::file(entry.name.clone(), entry.modified.clone()))
            .collect()
    }

    fn project_status(&self) -> String {
        self.projects.status().to_string()
    }

    fn export_wav(&mut self) -> Result<String, String> {
        Session::export_wav(self)
    }

    fn autosave(&mut self) -> bool {
        Session::autosave(self)
    }

    fn new_project(&mut self) -> Result<(), String> {
        let path = self
            .projects
            .new_project_path("Untitled")
            .ok_or_else(|| "no projects folder yet \u{2014} \"Change...\" picks one".to_string())?;
        // Written to disk before it is opened, so the thing in the list and
        // the thing on screen are the same thing from the first moment. A new
        // project that only exists in memory until somebody remembers to save
        // is the commonest way to lose one.
        let project = crate::blank_project(NEW_PROJECT_BARS, 120.0, self.options.sample_rate);
        crate::save_project(&project, &path).map_err(|e| e.to_string())?;
        self.projects.rescan();
        let opened = crate::open_project(&path).map_err(|e| e.to_string())?;
        self.adopt(opened, path);
        Ok(())
    }

    fn open_project(&mut self, index: usize) -> Result<(), String> {
        let path = self
            .filtered_projects()
            .get(index)
            .and_then(|i| self.projects.entries().get(*i))
            .map(|entry| entry.path.clone())
            .ok_or_else(|| "that project is not in the list any more".to_string())?;
        let opened = crate::open_project(&path).map_err(|e| e.to_string())?;
        self.adopt(opened, path);
        Ok(())
    }

    fn choose_projects_dir(&mut self) {
        // One projects folder, not a list: "where do my projects live" has one
        // answer, unlike a soundfont bank, which is genuinely several places.
        match crate::desktop::choose_folder(self.projects.dir()) {
            Ok(Some(dir)) => self.set_projects_dir(Some(dir)),
            Ok(None) => {}
            Err(e) => self.message = Some(e),
        }
    }

    fn reveal_projects_dir(&mut self) {
        let Some(dir) = self.projects.dir().map(Path::to_path_buf) else {
            self.message =
                Some("no projects folder yet \u{2014} \"Change...\" picks one".to_string());
            return;
        };
        if let Err(e) = crate::desktop::reveal(&dir) {
            self.message = Some(e);
        }
        // The folder may have grown a project while the file manager was open.
        self.projects.rescan();
        self.revision += 1;
    }

    /// One short line naming the bank folder.
    ///
    /// **Short is a requirement, not a preference.** The browser is 248 pixels
    /// wide, and the first version of this said "no .sf2 files yet — put them
    /// in /home/…" and ran off the end of the panel at exactly the word that
    /// mattered. The path is elided from the left, because the end of a path is
    /// the half that says where you are.
    fn library_count(&self) -> usize {
        self.bank.entries().len()
    }

    fn library_status(&self) -> String {
        let folders = self.bank.dirs();
        if folders.is_empty() {
            return "no soundfont folder set".to_string();
        }
        // While searching, say what is being searched — the whole collection,
        // which is the thing that is surprising about it.
        if !self.query.trim().is_empty() {
            let hits = self.bank.search(&self.query).len();
            return match hits {
                0 => format!("nothing matching \u{201c}{}\u{201d}", self.query.trim()),
                1 => "1 match in the whole collection".to_string(),
                n => format!("{n} matches in the whole collection"),
            };
        }
        // Otherwise say **where you are**, which is the question a browser you
        // can walk into raises and the flat list never did.
        let Some(at) = self.bank.at() else {
            return match folders.len() {
                1 => crate::desktop::elide_path(&folders[0], 2),
                n => format!("{n} soundfont folders"),
            };
        };
        let here = crate::desktop::elide_path(at, 2);
        match folders.len() {
            1 => here,
            n => format!("{here} \u{2014} 1 of {n} folders"),
        }
    }

    fn rescan_library(&mut self) {
        self.bank.rescan();
        self.revision += 1;
    }

    fn reveal_library_dir(&mut self) {
        let Some(dir) = self.bank.dirs().first().cloned() else {
            self.message = Some("no soundfont folder is configured".to_string());
            return;
        };
        if let Err(e) = crate::desktop::reveal(&dir) {
            self.message = Some(e);
        }
        // Whatever was dropped in while the file manager was open is picked up
        // the next time the panel is looked at — which is very often the point
        // of having opened it.
        self.bank.rescan();
        self.revision += 1;
    }

    fn choose_library_dir(&mut self, add: bool) {
        let start = self.bank.dirs().first().cloned();
        match crate::desktop::choose_folder(start.as_deref()) {
            Ok(Some(dir)) => {
                self.set_library_dirs(vec![dir], add);
                self.message = Some(match self.bank.entries().len() {
                    0 => "no .sf2 files in there".to_string(),
                    n => format!("found {n} soundfont(s)"),
                });
            }
            // A cancel is not an event.
            Ok(None) => {}
            Err(e) => self.message = Some(e),
        }
    }

    fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    fn audition_on(&mut self, key: u8, velocity: u8, pan: i8) {
        self.send_live(EventPayload::NoteOn {
            key,
            velocity,
            pan,
            // Auditioning plays the *instrument*, not a note off the score,
            // so the four score-only properties are at their written defaults.
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: AUDITION_VOICE_CONTEXT,
        });
    }

    fn audition_off(&mut self, key: u8) {
        self.send_live(EventPayload::NoteOff {
            key,
            voice_context: AUDITION_VOICE_CONTEXT,
        });
    }

    // -------------------------------------------------- the instrument editor ---

    fn instrument(&self) -> Option<InstrumentView> {
        let channel_id = self.selected_channel_id()?;
        let channel = self.project.channels.get(channel_id)?;
        let patch = self.selected_patch()?;
        // The fader lives on the mixer track, not on the patch — see
        // `crate::instrument::describe`.
        let track = channel
            .mixer_track
            .or(self.project.mixer.master)
            .and_then(|id| self.project.mixer.tracks.get(id));
        Some(crate::instrument::describe(
            &channel.name,
            &patch,
            track.map_or(0.0, |t| t.gain_db),
            track.map_or(0.0, |t| t.pan),
        ))
    }

    fn clip_clipboard_len(&self) -> usize {
        self.clip_clipboard.len()
    }

    fn key_map(&self) -> fontelle_ui::document::KeyMap {
        self.selected_patch()
            .map_or_else(fontelle_ui::document::KeyMap::unknown, |patch| {
                crate::key_map(&patch, &self.library)
            })
    }

    fn set_instrument_param(&mut self, address: &fontelle_types::ParamAddress, value: f32) {
        let Some(channel_id) = self.selected_channel_id() else {
            return;
        };
        // The two that are the mixer's rather than the patch's. A channel on
        // the master edits the master's, which is what the panel is showing.
        let track = self
            .project
            .channels
            .get(channel_id)
            .and_then(|c| c.mixer_track.or(self.project.mixer.master));
        match address.as_str() {
            crate::instrument::MIXER_GAIN => {
                let Some(track) = track else { return };
                let db = crate::instrument::GAIN_MIN_DB
                    + value.clamp(0.0, 1.0)
                        * (crate::instrument::GAIN_MAX_DB - crate::instrument::GAIN_MIN_DB);
                self.run(Box::new(fontelle_model::SetNumber::new(
                    fontelle_model::NumberTarget::TrackGainDb(track),
                    f64::from(db),
                )));
                self.rebuild_graph();
                return;
            }
            crate::instrument::MIXER_PAN => {
                let Some(track) = track else { return };
                self.run(Box::new(fontelle_model::SetNumber::new(
                    fontelle_model::NumberTarget::TrackPan(track),
                    f64::from(value.clamp(0.0, 1.0) * 2.0 - 1.0),
                )));
                self.rebuild_graph();
                return;
            }
            _ => {}
        }

        let Some(mut patch) = self.selected_patch() else {
            return;
        };
        // An address this build does not recognise changes nothing and is not
        // an error (INVARIANT 7): a project naming a parameter a later build
        // dropped has to open rather than refuse.
        if !crate::instrument::set(&mut patch, address, value) {
            return;
        }
        self.store_patch(channel_id, patch);
    }

    // --- the mixer (TDD §13) ---

    fn mixer_strips(&self) -> Vec<MixerStrip> {
        self.mixer_track_ids()
            .into_iter()
            .filter_map(|id| {
                let track = self.project.mixer.tracks.get(id)?;
                Some(MixerStrip {
                    name: track.name.clone(),
                    gain_db: track.gain_db,
                    pan: track.pan,
                    mute: track.mute,
                    solo: track.solo,
                    is_master: Some(id) == self.project.mixer.master,
                    color: track.color,
                    inserts: track
                        .inserts
                        .iter()
                        .map(|slot| fontelle_ui::canvas::InsertInfo {
                            label: slot.kind().label().to_string(),
                            bypassed: slot.bypassed,
                        })
                        .collect(),
                })
            })
            .collect()
    }

    fn mixer_peaks(&mut self) -> Vec<[f32; 2]> {
        self.mixer_track_ids()
            .into_iter()
            .map(|id| {
                // A track whose graph has been replaced since the last frame
                // reads silence rather than the last graph's levels.
                self.track_controls
                    .get(&id)
                    .map_or([0.0, 0.0], |controls| controls.take_peaks())
            })
            .collect()
    }

    fn add_insert(&mut self, strip: usize, kind: fontelle_types::EffectKind) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::AddInsert::new(id, kind)));
        self.history.break_gesture();
        // A new node in the chain is the graph's *shape*, not a value in it,
        // so this one does need the rebuild that tuning a band does not.
        self.rebuild_graph();
    }

    fn remove_insert(&mut self, strip: usize, slot: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::RemoveInsert::new(id, slot)));
        self.history.break_gesture();
        self.rebuild_graph();
    }

    fn toggle_insert_bypass(&mut self, strip: usize, slot: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        let Some(bypassed) = self
            .project
            .mixer
            .tracks
            .get(id)
            .and_then(|track| track.inserts.get(slot))
            .map(|insert| insert.bypassed)
        else {
            return;
        };
        self.run(Box::new(fontelle_model::SetInsertBypassed::new(
            id, slot, !bypassed,
        )));
        self.history.break_gesture();
        // Straight to the running graph: a bypass is a switch somebody flicks
        // while listening, and rebuilding the graph to flick it would reload
        // every soundfont in the project.
        if let Some(controls) = self.effect_controls.get_mut(&(id, slot)) {
            controls.set_bypassed(!bypassed);
        }
    }

    fn mixer_track_id(&self, strip: usize) -> Option<MixerTrackId> {
        self.mixer_track_ids().get(strip).copied()
    }

    fn create_automation(&mut self, address: &fontelle_types::ParamAddress, label: &str, at: Tick) {
        // **On a lane of its own**, at the playhead, one bar long. §12.4 says
        // "the current lane", and that reading put the curve on top of the
        // notes: an automation clip and a note clip in the same pixels is one
        // of them drawn over the other, which is the *"lane of empty clips"*
        // this was reported as.
        //
        // Reused when this parameter already has one, so a lane belongs to a
        // parameter rather than to a gesture — otherwise every right-click on
        // the same fader grows the arrangement another strip.
        let lane = self.automation_lane(address, label);
        let start = at.max(0);
        let bar = PPQN * i64::from(self.project.beats_per_bar.max(1));

        // Two points at the value the control is at now, so the clip starts by
        // changing nothing: an automation lane that jumped the parameter the
        // moment it was created would be a lane nobody trusts.
        let value = self.parameter_now(address).unwrap_or(0.5);
        let mut points = fontelle_model::Arena::default();
        for tick in [0, bar] {
            points.insert(fontelle_model::AutomationPoint {
                tick,
                value,
                curve: fontelle_model::CurveShape::Linear,
                tension: 0.0,
            });
        }

        let clip = Clip {
            lane,
            start,
            length: bar,
            source: ClipSource::Automation(fontelle_model::AutomationData {
                target: address.clone(),
                points,
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        };
        if let Ok(command) = self.apply_for::<AddClip>(Box::new(AddClip::new(clip)))
            && let Some(id) = command.id()
        {
            // It opens straight away: you made it to draw in it, which is the
            // same handshake a drawn note clip has.
            self.automation_clip = Some(id);
            self.automation_label = label.to_string();
            self.automation_names.insert(address.clone(), label.to_string());
            self.automation_selection.clear();
        }
        self.history.break_gesture();
        self.republish();
        self.revision += 1;
    }

    fn automation(&self) -> Option<fontelle_ui::canvas::AutomationView> {
        let clip_id = self.automation_clip?;
        let clip = self.project.clips.get(clip_id)?;
        let fontelle_model::ClipSource::Automation(data) = &clip.source else {
            return None;
        };
        let mut points: Vec<fontelle_ui::canvas::PointInfo> = data
            .points
            .iter()
            .map(|(id, point)| fontelle_ui::canvas::PointInfo {
                id,
                tick: point.tick,
                value: point.value,
                selected: self.automation_selection.contains(&id),
                curve: point.curve,
            })
            .collect();
        // In time order, so the handle drawn last is the rightmost rather than
        // whichever was made last.
        points.sort_by_key(|point| point.tick);
        Some(fontelle_ui::canvas::AutomationView {
            title: self.automation_label.clone(),
            length: clip.length,
            points,
        })
    }

    fn automation_data(&self) -> Option<fontelle_model::AutomationData> {
        let clip = self.project.clips.get(self.automation_clip?)?;
        match &clip.source {
            fontelle_model::ClipSource::Automation(data) => Some(data.clone()),
            _ => None,
        }
    }

    fn edit_automation(&mut self, edit: fontelle_ui::canvas::AutomationEdit) {
        use fontelle_ui::canvas::AutomationEdit;
        let Some(clip) = self.automation_clip else {
            return;
        };
        match edit {
            AutomationEdit::Add { tick, value } => {
                let point = fontelle_model::AutomationPoint {
                    tick,
                    value,
                    curve: fontelle_model::CurveShape::Linear,
                    tension: 0.0,
                };
                if let Ok(command) = self.apply_for::<fontelle_model::AddAutomationPoint>(Box::new(
                    fontelle_model::AddAutomationPoint::new(clip, point),
                )) {
                    // The point the click just made is the one the drag that
                    // follows moves — the same handshake drawing a note has.
                    self.automation_selection = command.id().into_iter().collect();
                }
                self.history.break_gesture();
            }
            AutomationEdit::Move {
                ids,
                tick_delta,
                value_delta,
            } => self.run(Box::new(fontelle_model::MoveAutomationPoints::new(
                clip,
                ids,
                tick_delta,
                value_delta,
            ))),
            AutomationEdit::Remove(ids) => {
                self.automation_selection.retain(|id| !ids.contains(id));
                self.run(Box::new(fontelle_model::RemoveAutomationPoints::new(
                    clip, ids,
                )));
                self.history.break_gesture();
            }
            AutomationEdit::SetCurve { ids, curve } => {
                self.run(Box::new(fontelle_model::SetPointCurve::new(
                    clip, ids, curve,
                )));
                self.history.break_gesture();
            }
        }
        self.republish();
    }

    fn is_automated(&self, address: &fontelle_types::ParamAddress) -> bool {
        fontelle_model::automated_targets(&self.project).contains(address)
    }

    fn eq_config(&self, strip: usize, slot: usize) -> Option<fontelle_types::EqConfig> {
        let id = self.mixer_track_ids().get(strip).copied()?;
        let insert = self.project.mixer.tracks.get(id)?.inserts.get(slot)?;
        let fontelle_types::EffectConfig::Eq(eq) = insert.config else {
            return None; // this slot holds something else
        };
        Some(eq)
    }

    fn set_eq_band(
        &mut self,
        strip: usize,
        slot: usize,
        band: usize,
        value: fontelle_types::EqBand,
    ) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::SetEqBand::new(
            id, slot, band, value,
        )));
        // Both ends, exactly as a fader does: the command for undo and for the
        // file, and the live channel for the sound between now and the next
        // rebuild. See `fontelle_engine::effect_channel`.
        if let Some(config) = self.eq_config(strip, slot)
            && let Some(controls) = self.effect_controls.get_mut(&(id, slot))
        {
            controls.publish(fontelle_types::EffectConfig::Eq(config));
        }
    }

    fn set_track_gain_db(&mut self, strip: usize, gain_db: f32) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(SetNumber::new(
            NumberTarget::TrackGainDb(id),
            f64::from(gain_db),
        )));
        self.publish_mixer();
    }

    fn set_track_pan(&mut self, strip: usize, pan: f32) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(SetNumber::new(
            NumberTarget::TrackPan(id),
            f64::from(pan),
        )));
        self.publish_mixer();
    }

    fn toggle_track_mute(&mut self, strip: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        let now = self.project.mixer.tracks.get(id).is_some_and(|t| t.mute);
        self.run(Box::new(SetFlag::new(FlagTarget::TrackMute(id), !now)));
        self.history.break_gesture();
        self.publish_mixer();
    }

    fn toggle_track_solo(&mut self, strip: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        let now = self.project.mixer.tracks.get(id).is_some_and(|t| t.solo);
        self.run(Box::new(SetFlag::new(FlagTarget::TrackSolo(id), !now)));
        self.history.break_gesture();
        self.publish_mixer();
    }

    fn ghost_notes(&self, filter: GhostFilter) -> Vec<GhostNote> {
        if filter == GhostFilter::Off {
            return Vec::new();
        }
        // The clip being edited is the frame of reference: a ghost's tick is
        // where it falls *in this clip*, so a part that starts two bars later
        // in the song is drawn two bars in.
        let Some(open) = self.project.clips.get(self.clip) else {
            return Vec::new();
        };
        let wanted = match filter {
            GhostFilter::Channel(index) => match self.channel_ids().get(index).copied() {
                Some(id) => Some(id),
                // A filter naming a channel that is gone shows nothing, which
                // is honest — the chip steps past it on the next press.
                None => return Vec::new(),
            },
            _ => None,
        };

        let mut ghosts = Vec::new();
        for (id, clip) in self.project.clips.iter() {
            if id == self.clip {
                continue;
            }
            let ClipSource::Notes(data) = &clip.source else {
                continue;
            };
            // Never the channel being edited, whatever the filter says: its own
            // notes are already drawn solid, and a ghost under every one of
            // them is a smear.
            let editing = self.channel_of_clip(self.clip);
            if Some(data.channel) == editing {
                continue;
            }
            if let Some(wanted) = wanted
                && data.channel != wanted
            {
                continue;
            }
            let color = clip.color.unwrap_or_else(|| {
                self.project
                    .lanes
                    .get(clip.lane)
                    .map_or([0x4f, 0x8f, 0xd0, 0xff], |lane| lane.color)
            });
            let offset = clip.start - open.start;
            ghosts.extend(data.notes.values().map(|note| GhostNote {
                start: note.start + offset,
                length: note.length,
                key: note.key,
                color,
            }));
        }
        ghosts
    }

    // ------------------------------------------------------ the arrangement ---

    fn lanes(&self) -> Vec<LaneInfo> {
        self.project
            .lanes
            .values()
            .map(|lane| LaneInfo {
                name: lane.name.clone(),
                muted: lane.muted,
            })
            .collect()
    }

    fn clips(&self) -> Vec<ClipInfo> {
        let lanes = self.lane_ids();
        self.project
            .clips
            .iter()
            // **Audio clips only** are left out, and only because §15 has not
            // built them: a `ClipSource::Audio` carries nothing to draw yet.
            // Automation used to be filtered out here too, which is what made
            // it *"play and open"* while a lane of it looked like a lane of
            // empty clips.
            .filter(|(_, clip)| !matches!(clip.source, ClipSource::Audio(_)))
            .map(|(id, clip)| {
                let (kind, name, curve) = match &clip.source {
                    // A note block is captioned with the channel it plays, not
                    // with a clip name — a clip has none, and "what instrument
                    // is this" is what somebody scanning an arrangement asks.
                    ClipSource::Notes(_) => (
                        ClipKind::Notes,
                        self.channel_of_clip(id)
                            .and_then(|channel| self.project.channels.get(channel))
                            .map(|channel| channel.name.clone())
                            .unwrap_or_else(|| "Clip".to_string()),
                        Vec::new(),
                    ),
                    // And an automation block with the parameter it moves,
                    // which is the same question asked of the other kind.
                    ClipSource::Automation(data) => {
                        let mut points: Vec<(Tick, f64)> = data
                            .points
                            .values()
                            .map(|point| (point.tick, point.value))
                            .collect();
                        // In time order, so the canvas draws a polyline
                        // without having to sort a copy every frame.
                        points.sort_by_key(|(tick, _)| *tick);
                        (
                            ClipKind::Automation,
                            self.automation_name(&data.target),
                            points,
                        )
                    }
                    ClipSource::Audio(_) => unreachable!("filtered above"),
                };
                ClipInfo {
                    id,
                    lane: lanes.iter().position(|l| *l == clip.lane).unwrap_or(0),
                    start: clip.start,
                    length: clip.length,
                    name,
                    muted: clip.muted,
                    // Either editor's open clip: the roll's, or the curve
                    // editor's. One mark, because there is one thing you are
                    // editing.
                    open: id == self.clip || Some(id) == self.automation_clip,
                    loop_length: clip.loop_length,
                    color: clip.color.unwrap_or_else(|| {
                        self.project
                            .lanes
                            .get(clip.lane)
                            .map_or([0x4f, 0x8f, 0xd0, 0xff], |lane| lane.color)
                    }),
                    kind,
                    curve,
                }
            })
            .collect()
    }

    fn arrange(&mut self, edit: ArrangeEdit) -> Vec<ClipId> {
        let lanes = self.lane_ids();
        // Filled in by the two edits that make clips. Every other arm leaves
        // it empty, which is what "created none" means to the canvas.
        let mut created = Vec::new();
        match edit {
            ArrangeEdit::Move {
                ids,
                tick_delta,
                lane_delta,
            } => {
                // One command per clip, because `MoveClip` names one — and each
                // coalesces with its own predecessor, so a drag of a four-clip
                // selection is still one undo.
                for id in ids {
                    let lane = (lane_delta != 0)
                        .then(|| {
                            let now = self.project.clips.get(id)?.lane;
                            let index = lanes.iter().position(|l| *l == now)?;
                            let wanted = (index as i64 + i64::from(lane_delta)).max(0) as usize;
                            lanes
                                .get(wanted.min(lanes.len().saturating_sub(1)))
                                .copied()
                        })
                        .flatten();
                    self.run(Box::new(MoveClip::new(id, tick_delta, lane)));
                }
            }
            ArrangeEdit::Resize { ids, tick_delta } => {
                for id in ids {
                    self.run(Box::new(ResizeClip::new(id, tick_delta)));
                }
            }
            ArrangeEdit::Duplicate { ids, tick_offset } => {
                for id in ids {
                    // Through `apply_for` rather than `run`, because the id of
                    // the clip it mints exists only on the command itself.
                    match self
                        .apply_for::<DuplicateClip>(Box::new(DuplicateClip::new(id, tick_offset)))
                    {
                        Ok(command) => created.extend(command.id()),
                        Err(e) => {
                            eprintln!("Fontelle: {e}");
                            self.message = Some(e);
                        }
                    }
                }
                self.republish();
                self.history.break_gesture();
            }
            ArrangeEdit::Remove(ids) => {
                let opened = ids.contains(&self.clip);
                for id in ids {
                    self.run(Box::new(RemoveClip::new(id)));
                }
                self.history.break_gesture();
                // The roll cannot go on showing a clip that is not there.
                if opened && let Some(next) = Self::first_clip(&self.project) {
                    self.clip = next;
                    self.selected = self.channel_index_of_clip().unwrap_or(0);
                }
                self.revision += 1;
            }
            ArrangeEdit::SetMuted { ids, muted } => {
                for id in ids {
                    self.run(Box::new(SetFlag::new(FlagTarget::ClipMuted(id), muted)));
                }
                self.history.break_gesture();
            }
            ArrangeEdit::Add { lane, start } => {
                // Which lane, clamped: clicking below the last one means the
                // last one rather than nothing, because a press that lands in
                // the empty space under an arrangement is somebody aiming at
                // the row above it.
                let lanes = self.lane_ids();
                let Some(lane_id) = lanes.get(lane).or_else(|| lanes.last()).copied() else {
                    self.message = Some("this project has no lanes to draw on".to_string());
                    return created;
                };
                // What the clip plays: whatever else is already on this lane,
                // because that is what a lane *means* to somebody looking at
                // it — and the rack's selection otherwise, which is the
                // channel they were last working on.
                let Some(channel) = self
                    .project
                    .clips
                    .values()
                    .filter(|clip| clip.lane == lane_id)
                    .find_map(|clip| match &clip.source {
                        ClipSource::Notes(data) => Some(data.channel),
                        _ => None,
                    })
                    .or_else(|| self.selected_channel_id())
                    .or_else(|| self.project.channels.keys().next())
                else {
                    self.message =
                        Some("add an instrument first — a clip has to play something".to_string());
                    return created;
                };

                let bar = PPQN * i64::from(self.project.beats_per_bar.max(1));
                let clip = Clip {
                    lane: lane_id,
                    start,
                    length: bar,
                    source: ClipSource::Notes(NoteData {
                        channel,
                        notes: Arena::default(),
                    }),
                    prefab_link: None,
                    color: None,
                    muted: false,
                    loop_length: None,
                };
                if let Ok(command) = self.apply_for::<AddClip>(Box::new(AddClip::new(clip)))
                    && let Some(id) = command.id()
                {
                    created.push(id);
                    // Drawn clips open in the roll, the same way clicked ones
                    // do: you made it to put notes in it.
                    self.clip = id;
                    self.selected = self.channel_index_of_clip().unwrap_or(self.selected);
                }
                self.history.break_gesture();
                self.republish();
                self.revision += 1;
            }
            ArrangeEdit::SetLoop { ids, loop_length } => {
                // No `break_gesture`: this arrives as the first step of a
                // Shift-drag on a clip's edge, and the resize steps that
                // follow it belong to the same gesture and the same undo
                // entry.
                for id in ids {
                    self.run(Box::new(fontelle_model::SetClipLoop::new(id, loop_length)));
                }
            }
            ArrangeEdit::Copy(ids) => {
                // In document order, so the shape of a multi-clip copy is the
                // shape it had — and taken *now*, whole, because a cut is
                // about to remove the originals.
                let mut taken: Vec<Clip> = ids
                    .iter()
                    .filter_map(|id| self.project.clips.get(*id).cloned())
                    .collect();
                taken.sort_by_key(|c| c.start);
                if !taken.is_empty() {
                    self.clip_clipboard = taken;
                }
                // A copy changes no document state, so no command and no undo
                // entry — and deliberately no `revision` bump either.
                return created;
            }
            ArrangeEdit::Paste { at } => {
                if self.clip_clipboard.is_empty() {
                    return created;
                }
                let earliest = self.clip_clipboard.first().map_or(0, |c| c.start);
                // A lane the project no longer has would make every paste
                // fail; falling back to the first is the only other honest
                // answer, and it puts the clip somewhere the person can see.
                let lanes = self.lane_ids();
                let clips: Vec<Clip> = self
                    .clip_clipboard
                    .iter()
                    .map(|clip| {
                        let mut copy = clip.clone();
                        copy.start = (at + (clip.start - earliest)).max(0);
                        if self.project.lanes.get(copy.lane).is_none()
                            && let Some(first) = lanes.first()
                        {
                            copy.lane = *first;
                        }
                        copy
                    })
                    .collect();
                for clip in clips {
                    match self.apply_for::<AddClip>(Box::new(AddClip::new(clip))) {
                        Ok(command) => created.extend(command.id()),
                        Err(e) => {
                            eprintln!("Fontelle: {e}");
                            self.message = Some(e);
                        }
                    }
                }
                self.republish();
                self.history.break_gesture();
            }
        }
        self.revision += 1;
        created
    }

    fn open_clip(&mut self, clip: ClipId) {
        let Some(open) = self.project.clips.get(clip) else {
            return;
        };
        // A block opens what is in it, whichever kind it is — the same
        // handshake a note clip has always had, for the other kind of clip.
        if let ClipSource::Automation(data) = &open.source {
            self.automation_label = self.automation_name(&data.target);
            self.automation_clip = Some(clip);
            self.automation_selection.clear();
            self.revision += 1;
            return;
        }
        self.clip = clip;
        // The rack follows: the arrangement, the rack and the roll are three
        // views of one selection, and two of them disagreeing is how a note
        // ends up drawn on the wrong instrument.
        if let Some(index) = self.channel_index_of_clip() {
            self.selected = index;
        }
        self.revision += 1;
    }

    fn song_length(&self) -> Tick {
        self.project
            .clips
            .values()
            .map(|clip| clip.start + clip.length)
            .max()
            .unwrap_or(0)
            .max(PPQN * i64::from(self.project.beats_per_bar) * NEW_CLIP_BARS)
    }

    fn playhead_song_tick(&self, position_sample: Sample) -> Tick {
        self.project
            .tempo_map
            .sample_to_tick(position_sample.max(0))
    }

    fn sample_of_song_tick(&self, tick: Tick) -> Sample {
        self.project.tempo_map.tick_to_sample(tick.max(0))
    }

    fn toggle_lane_mute(&mut self, lane: usize) {
        let Some(id) = self.lane_ids().get(lane).copied() else {
            return;
        };
        let now = self.project.lanes.get(id).is_some_and(|l| l.muted);
        self.run(Box::new(SetFlag::new(FlagTarget::LaneMuted(id), !now)));
        self.history.break_gesture();
        self.revision += 1;
    }

    fn keep_take(&mut self, end_sample: Sample) -> usize {
        // One last drain: the events between the previous pump and the stop
        // are the end of the performance, and they are the ones somebody just
        // played.
        if let Some(capture) = &mut self.capture {
            capture.drain_into(&mut self.take);
        }
        let events = std::mem::take(&mut self.take);
        if events.is_empty() {
            return 0;
        }

        let start = self
            .project
            .clips
            .get(self.clip)
            .map_or(0, |clip| clip.start);
        let source =
            fontelle_model::notes_from_capture(&events, &self.project.tempo_map, start, end_sample);
        let ClipSource::Notes(data) = source else {
            return 0; // a capture is always a note clip
        };
        let notes: Vec<Note> = data.notes.values().cloned().collect();
        if notes.is_empty() {
            // Played nothing, or only note-offs from a key that was already
            // down when recording began. Not a failure, and not an empty clip.
            return 0;
        }
        let count = notes.len();
        // Straight into the clip that is open, through `AddNotes` like every
        // other way notes arrive — so a take is one undo entry and can be
        // taken back like anything else.
        self.insert(self.clip, notes);
        self.revision += 1;
        count
    }

    fn discard_take(&mut self) {
        if let Some(capture) = &mut self.capture {
            capture.drain_into(&mut self.take);
        }
        self.take.clear();
    }

    fn pump(&mut self) {
        // Emptied every pass, so the ring never fills while a long take is
        // being played — the same rule the CLI's own recording loop follows.
        if let Some(capture) = &mut self.capture {
            capture.drain_into(&mut self.take);
        }
        if let Some(graphs) = &mut self.graphs {
            // Where a `CompiledGraph` the audio thread stopped using is freed:
            // on this thread, never on that one (INVARIANT 1).
            graphs.pump();
        }
    }
}

/// A file size a person can read at a glance.
///
/// Rounded hard and never more than one decimal: the browser is a list, and the
/// question it answers is "is this the 4 MB one or the 300 MB one".
fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb < 1.0 {
        format!("{} kB", (bytes as f64 / 1024.0).round() as u64)
    } else if mb < 10.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{} MB", mb.round() as u64)
    }
}
