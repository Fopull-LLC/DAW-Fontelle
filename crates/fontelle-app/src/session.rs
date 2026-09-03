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
use fontelle_assets::{MidiChannels, import_fsc, import_midi, survey_midi};
use fontelle_model::{
    AddChannel, AddClip, AddNotes, Arena, Clip, ClipSource, Command, DuplicateClip, FlagTarget,
    History, ImportPart, ImportParts, Lane, MoveClip, MoveNotes, Note, NoteData, NumberTarget,
    Project, RemoveClip, RemoveNotes, ResizeClip, ResizeNotes, SetFlag, SetNoteProperty, SetNumber,
};
use fontelle_types::{
    ChannelId, ClipId, EventPayload, LaneId, MixerTrackId, NodeId, NoteId, PPQN, Sample, Tick,
    TimedEvent,
};
use fontelle_ui::canvas::{ArrangeEdit, InstrumentView, RollEdit};
use fontelle_ui::document::{
    ChannelInfo, ClipInfo, ClipKind, Created, CurvePoint, DocumentHost, GhostFilter, GhostNote,
    LaneInfo, LibraryEntry, MixerStrip, PlayMode, StudioHost,
};

use crate::bank::{BankFilter, BankRow, FileBank, SoundfontBank, matches_names};
use crate::library::SampleLibrary;
use crate::projects::ProjectLibrary;
use crate::realise::{RealiseOptions, apply_mixer_controls, apply_send_controls, realise};
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
    /// Which node plays the audio clips routed to each mixer track — see
    /// [`crate::Realised::audio_nodes`]. Beside the two above and replaced with
    /// them, for the same reason.
    audio_nodes: HashMap<Option<MixerTrackId>, NodeId>,
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
    /// One analyser tap per insert. **Kept across a rebuild** — see
    /// [`crate::Realised::spectrum_taps`].
    spectrum_taps: HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::SpectrumTap>>,
    /// The transform behind the EQ's graph, and the scratch it reads into.
    ///
    /// One of each rather than one per insert: only one EQ window is open at a
    /// time, and an analyser holds a few kilobytes of window and scratch that
    /// would otherwise be allocated per insert and used by none of them.
    analyser: fontelle_dsp::SpectrumAnalyser,
    spectrum_scratch: Vec<f32>,
    /// The automation clip last opened from the arrangement, so its block can
    /// be marked as the one in hand.
    ///
    /// Session state, not document state: which clip you are looking at is not
    /// something a project sent to somebody else should arrive with, for the
    /// same reason the piano roll's open clip is not saved.
    automation_clip: Option<fontelle_types::ClipId>,
    /// The live end of every send, keyed the way `realise` keys it. Kept for
    /// the reason `track_controls` is: a send level is dragged, and a drag has
    /// to be audible before the mouse comes up.
    send_controls: HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::SendControls>>,
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
    /// The cell every open MIDI device reads its input settings out of
    /// (TDD §14.3), when there is a hub to read it. `None` on every offline
    /// path — the settings are still edited and still written down, there is
    /// simply no keyboard listening.
    live_input: Option<std::sync::Arc<fontelle_midi::LiveMapping>>,
    /// The keys every open MIDI device is holding down, for the roll to light
    /// (TDD §14.1). `None` on every offline path, which has no keyboard to
    /// light — see [`Session::with_live_keys`].
    live_keys: Option<std::sync::Arc<fontelle_midi::LiveKeys>>,
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
    /// The transport the window is driving, when there is one.
    ///
    /// Held so the **time selection** can reach it: a loop is document state
    /// in ticks (§6.3) and the RT thread needs samples, and the conversion
    /// has to happen here because only this side has the tempo map. `None` on
    /// every offline path, where a loop is still edited and saved and there
    /// is simply nothing playing.
    transport: Option<std::sync::Arc<fontelle_engine::Transport>>,
    /// What pressing play plays. Session state, not the document's: it is how
    /// you are listening, not what the song is.
    play_mode: PlayMode,
    /// The tempo map the song actually plays by — the document's own, bent by
    /// whatever automation aims at the tempo (§12.3).
    ///
    /// Cached rather than derived per call, because **every** tick-to-sample
    /// conversion in the window goes through it — the playhead, both rulers,
    /// a loop range — and building it walks every clip in the project.
    /// Rebuilt in [`Session::republish`], which is the one thing every
    /// command already goes through.
    effective_tempo: fontelle_model::TempoMap,
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
    /// The folder the Import tab is browsing, and what is in it.
    ///
    /// One bank rather than two, rebuilt when the kind changes: only one of
    /// them is ever on screen, and two would mean two folder walks on every
    /// launch for a tab most sessions never open.
    import_bank: FileBank,
    /// Which kind the Import tab is showing.
    import_kind: fontelle_types::FolderKind,
    /// What the Import tab's search box holds. Its own, not the bank's: a
    /// query typed against soundfonts means nothing against MIDI files.
    import_query: String,
    /// A MIDI file waiting on the question *"all of it, or one part?"*.
    ///
    /// Session state and not the document's: it is a question in flight, and
    /// a project saved while one is open should not arrive with it.
    pending_import: Option<PendingImport>,
    /// Which of the browser's lists is on screen, so the search box knows
    /// which query it is editing. Mirrored from the window — see
    /// `StudioHost::set_browser_mode`.
    browser_mode: fontelle_ui::canvas::BrowserMode,
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
    /// Every preset in the whole collection, for searching across soundfonts
    /// rather than only inside the open one. Built lazily and in the
    /// background — see [`PresetIndex`].
    preset_index: PresetIndex,
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

/// How many buckets an audio clip's block preview holds.
///
/// A summary of a summary: the peak file already holds the loudest and
/// quietest sample per 64 frames, and this resamples that onto a fixed number
/// of columns covering the clip's own trimmed range. Fixed rather than
/// per-block-width because it is built per document revision and the block is
/// resized per frame — a preview rebuilt on every zoom is the cost §15.3 exists
/// to avoid. 512 is more columns than a clip is usually wide, so the picture
/// is smooth at any width a lane has room for.
const PREVIEW_BUCKETS: usize = 512;

impl Session {
    /// The waveform an audio clip's block draws (TDD §15.3).
    ///
    /// See [`PREVIEW_BUCKETS`] for how much of it there is.
    ///
    /// Resampled onto a fixed number of buckets covering **this clip's own
    /// trimmed range**, and in **play order** — so a clip trimmed to the middle
    /// of a file draws the middle of it, and a reversed one draws backwards,
    /// which is what makes the picture the sound rather than a decoration
    /// beside it.
    ///
    /// Built per document revision rather than per frame, and from the peak
    /// summary rather than the samples: §15.3's whole point.
    fn audio_preview(&self, data: &fontelle_types::AudioClipData) -> fontelle_ui::document::AudioPreview {
        let mut preview = fontelle_ui::document::AudioPreview {
            peaks: Vec::new(),
            fade_in: 0.0,
            fade_out: 0.0,
        };
        let frames = data.source_frames();
        if frames > 0 {
            // As a fraction of the clip, which is what the canvas draws
            // against. Clamped, because a fade longer than the clip is a real
            // thing to ask for by dragging and must not run off the block.
            preview.fade_in = (data.fade_in.frames as f32 / frames as f32).clamp(0.0, 1.0);
            preview.fade_out = (data.fade_out.frames as f32 / frames as f32).clamp(0.0, 1.0);
        }
        let Some(peaks) = self.library.audio_peaks(data.asset.id) else {
            // §15.3: draw what exists. Nothing yet is nothing drawn — never a
            // slab, which would say the take is loud all the way through.
            return preview;
        };
        // The finest level that is not more detail than the buckets can hold.
        let level = &peaks.levels[peaks.level_for(frames as usize, PREVIEW_BUCKETS)];
        if level.is_empty() {
            return preview;
        }
        let per_bucket = frames as f64 / PREVIEW_BUCKETS as f64;
        preview.peaks = (0..PREVIEW_BUCKETS)
            .map(|bucket| {
                // Which slice of the *file* this bucket covers. Through
                // `source_position`, so trim, speed, reverse and looping are
                // all obeyed by one function rather than four.
                let from = data.source_position(bucket as f64 * per_bucket);
                let to = data.source_position((bucket + 1) as f64 * per_bucket);
                let (from, to) = if from <= to { (from, to) } else { (to, from) };
                let scale = level.len() as f64 / peaks.frames.max(1) as f64;
                let a = ((from * scale) as usize).min(level.len() - 1);
                let b = ((to * scale) as usize).min(level.len() - 1);
                level[a..=b]
                    .iter()
                    .fold((0.0f32, 0.0f32), |acc, (lo, hi)| (acc.0.min(*lo), acc.1.max(*hi)))
            })
            .collect();
        preview
    }

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
            audio_nodes: HashMap::new(),
            track_controls: HashMap::new(),
            effect_controls: HashMap::new(),
            spectrum_taps: HashMap::new(),
            analyser: fontelle_dsp::SpectrumAnalyser::new(),
            spectrum_scratch: Vec::new(),
            automation_clip: None,
            send_controls: HashMap::new(),
            automation_names: HashMap::new(),
            selected_track: 0,
            live_target: None,
            live_input: None,
            live_keys: None,
            metronome: None,
            capture: None,
            take: Vec::new(),
            publisher,
            graphs: None,
            transport: None,
            play_mode: PlayMode::Song,
            effective_tempo: fontelle_model::TempoMap::default(),
            options,
            clip,
            selected: 0,
            bundle,
            dirty: false,
            revision: 1,
            settings,
            settings_path: None,
            import_bank: FileBank::default(),
            import_kind: fontelle_types::FolderKind::Midi,
            import_query: String::new(),
            pending_import: None,
            browser_mode: fontelle_ui::canvas::BrowserMode::Sounds,
            bank: SoundfontBank::default(),
            projects: ProjectLibrary::default(),
            query: String::new(),
            open_file: None,
            presets: Vec::new(),
            preset_index: PresetIndex::default(),
            clip_clipboard: Vec::new(),
            channel_presets: HashMap::new(),
            patch_cache: None,
            message: error.map(|e| e.to_string()),
            audition: None,
            empty: Arena::default(),
        };
        session.selected = session.channel_index_of_clip().unwrap_or(0);
        session.effective_tempo = session.tempo_for_scope();
        session
    }

    /// Gives the session the transport the window is driving, so a time
    /// selection and the play mode can reach it — see [`Session::transport`].
    pub fn with_transport(mut self, transport: std::sync::Arc<fontelle_engine::Transport>) -> Self {
        self.transport = Some(transport);
        self.publish_loop();
        self
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

    /// Keeps the analyser taps the first graph was built with.
    ///
    /// Without this a session only learns about them on its first *rebuild*,
    /// so a project opened with an EQ already on a track would draw no
    /// spectrum until something else changed the graph — which is the kind of
    /// bug that looks like the feature is broken rather than unwired. See
    /// [`crate::Realised::spectrum_taps`].
    pub fn with_spectrum_taps(
        mut self,
        taps: HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::SpectrumTap>>,
    ) -> Self {
        self.spectrum_taps = taps;
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
        self.automation_clip = None;
        // A different document is a different tempo curve and a different
        // loop; both are read off the project that has just arrived.
        self.effective_tempo = self.tempo_for_scope();
        self.publish_loop();
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

    /// A lane for an automation clip to go on.
    ///
    /// Always a new one, and the caller is why: [`create_automation`] hands
    /// back the clip a parameter already has rather than making a second, so
    /// by the time this is called there is no clip for this address and
    /// therefore no lane carrying one. (It used to search for one, by the
    /// clips on it rather than by its name — the name is a caption and the
    /// address is the identity — and that search can no longer find
    /// anything.)
    ///
    /// A lane is *visual only* (TDD §10.3) and deliberately cheap, which is
    /// why this inserts one rather than going through a command — the same
    /// thing `add_channel_with` does for a new channel's lane, and with the
    /// same consequence: making a lane is not on the undo stack, though
    /// everything put on it is.
    ///
    /// [`create_automation`]: fontelle_ui::document::StudioHost::create_automation
    fn automation_lane(
        &mut self,
        _address: &fontelle_types::ParamAddress,
        label: &str,
    ) -> fontelle_types::LaneId {
        self.project.lanes.insert(Lane {
            name: label.to_string(),
            height: 32.0,
            // A light lavender: a different hue from a note lane's blue, so
            // the two kinds of strip are tellable apart down the header
            // column, and **bright**, because on an automation block this
            // colour is the curve rather than the fill. The first attempt was
            // a dim `0x7a6f9a` and the curve was invisible on the block — see
            // `render::draw_automation_curve`.
            color: [0xb4, 0xa2, 0xe8, 0xff],
            muted: false,
            locked: false,
            order: next_lane_order(&self.project),
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

    /// How many sends `id` has.
    fn sends_of(&self, id: MixerTrackId) -> usize {
        self.project
            .mixer
            .tracks
            .get(id)
            .map_or(0, |track| track.sends.len())
    }

    /// The live end of every send, in the order `realise` built them.
    ///
    /// Public so a test can prove a send level was heard without a sound card,
    /// and prove it was heard *without* the graph being rebuilt.
    pub fn send_controls(
        &self,
    ) -> HashMap<(usize, usize), std::sync::Arc<fontelle_engine::SendControls>> {
        let ids = self.mixer_track_ids();
        self.send_controls
            .iter()
            .filter_map(|((track, index), live)| {
                let strip = ids.iter().position(|id| id == track)?;
                Some(((strip, *index), std::sync::Arc::clone(live)))
            })
            .collect()
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

    /// Gives the session the cell the MIDI hub's routers read their input
    /// settings out of, and publishes what the settings file says into it.
    ///
    /// The same shape as [`Session::with_live_target`] and for the same
    /// reason: a device callback may not take a lock, and reopening a port to
    /// change one number would drop whatever was being played across it.
    pub fn with_input_settings(
        mut self,
        cell: std::sync::Arc<fontelle_midi::LiveMapping>,
    ) -> Self {
        self.live_input = Some(cell);
        self.publish_input_settings();
        self
    }

    /// Gives the session the cell every open device lights its keys in, so the
    /// roll can show what is being played (TDD §14.1).
    ///
    /// Read-only from here: the routers write it, the window draws it, and the
    /// session is only the road between them. Optional, like the two cells
    /// above, because every offline path builds a `Session` without one.
    pub fn with_live_keys(mut self, keys: std::sync::Arc<fontelle_midi::LiveKeys>) -> Self {
        self.live_keys = Some(keys);
        self
    }

    /// Puts what the settings file says onto the shared cell, so a keyboard
    /// already plugged in plays by it.
    fn publish_input_settings(&self) {
        if let Some(cell) = &self.live_input {
            cell.set(self.settings.midi_input.into());
        }
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
        let timeline = fontelle_sequencer::compile_with(
            &self.project,
            &fontelle_sequencer::NodeMaps {
                channels: &realised.channel_nodes,
                params: &realised.param_nodes,
                audio: &realised.audio_nodes,
            },
            fontelle_sequencer::CompileScope::Song,
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
        apply_send_controls(&self.project, &self.send_controls);
        self.revision += 1;
    }

    /// What the document says one insert is set to, whatever kind it holds.
    ///
    /// [`eq_config`](StudioHost::eq_config)'s general case: the curve editor
    /// wants the EQ specifically, and publishing to the live end wants
    /// whatever is there.
    fn insert_config(&self, strip: usize, slot: usize) -> Option<fontelle_types::EffectConfig> {
        let id = self.mixer_track_ids().get(strip).copied()?;
        Some(self.project.mixer.tracks.get(id)?.inserts.get(slot)?.config)
    }

    /// What one insert's **live end** is set to right now — the config the
    /// audio thread is reading, rather than the one the document holds.
    ///
    /// Public for the reason [`Session::track_controls`] is: a test has to be
    /// able to prove a knob was heard without a sound card, and prove it was
    /// heard *without* the graph being rebuilt.
    pub fn live_effect_config(
        &self,
        strip: usize,
        slot: usize,
    ) -> Option<fontelle_types::EffectConfig> {
        let id = self.mixer_track_ids().get(strip).copied()?;
        Some(self.effect_controls.get(&(id, slot))?.config())
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
        // One answer to "what is row 3", and it is the document's — see
        // `Project::lane_ids`. The arena's own order was this until rows could
        // be moved.
        self.project.lane_ids()
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
        fontelle_sequencer::compile_with(
            &self.project,
            &fontelle_sequencer::NodeMaps {
                channels: &self.channel_nodes,
                params: &self.param_nodes,
                audio: &self.audio_nodes,
            },
            self.scope(),
        )
    }

    /// How much of the document the timeline carries: everything, or the one
    /// clip being edited (§6.3's clip mode).
    fn scope(&self) -> fontelle_sequencer::CompileScope {
        match self.play_mode {
            PlayMode::Song => fontelle_sequencer::CompileScope::Song,
            PlayMode::Clip => fontelle_sequencer::CompileScope::Clip(self.clip),
        }
    }

    fn republish(&mut self) {
        // The tempo first: a tempo lane changes what sample every tick in the
        // pass lands on, and the loop the transport is running is in samples.
        //
        // **Against the same scope the timeline is compiled with**, which is
        // what keeps the window's clock and the song it is playing the same
        // clock. Clip mode compiles one clip and leaves every other clip out,
        // a tempo lane on another row included — so the window has to leave
        // it out too, or the playhead is drawn in a bar the notes are not in.
        self.effective_tempo = self.tempo_for_scope();
        self.publish_loop();
        let timeline = self.compiled();
        self.publisher.publish(timeline);
    }

    /// The tempo map this session's [`scope`](Session::scope) plays by.
    fn tempo_for_scope(&self) -> fontelle_model::TempoMap {
        match self.play_mode {
            PlayMode::Song => fontelle_model::effective_tempo_map(&self.project),
            PlayMode::Clip => self.project.tempo_map.clone(),
        }
    }

    /// The bars the clip being edited covers, in song ticks.
    fn clip_span(&self) -> Option<(Tick, Tick)> {
        let clip = self.project.clips.get(self.clip)?;
        Some((clip.start, clip.start + clip.length))
    }

    /// Hands the transport the stretch it should be looping, in **both**
    /// units (§6.3), and whether to loop at all.
    ///
    /// In song mode that is the time selection somebody dragged out on a
    /// ruler; in clip mode it is the clip being edited, whatever the
    /// selection says — that is what clip mode *is*. Both halves are
    /// published together because the RT thread cannot run a `TempoMap`
    /// lookup against a map this thread may be editing.
    fn publish_loop(&self) {
        let Some(transport) = &self.transport else {
            return;
        };
        let range = match self.play_mode {
            PlayMode::Song => self.project.loop_range,
            PlayMode::Clip => self.clip_span(),
        };
        match range {
            Some((from, to)) if to > from => {
                transport.set_loop_range(
                    (from, to),
                    (
                        self.effective_tempo.tick_to_sample(from),
                        self.effective_tempo.tick_to_sample(to),
                    ),
                );
                transport.set_looping(true);
            }
            // Nothing selected is not a loop of nothing: it is no loop.
            _ => transport.set_looping(false),
        }
    }

    /// How long the piece is, in ticks. See [`StudioHost::song_length`].
    fn song_ticks(&self) -> Tick {
        self.project
            .clips
            .values()
            .map(|clip| clip.start + clip.length)
            .max()
            .unwrap_or(0)
            .max(PPQN * i64::from(self.project.beats_per_bar) * NEW_CLIP_BARS)
    }

    /// What an addressed parameter is worth right now, normalised.
    ///
    /// So a fresh automation clip starts by changing nothing: a lane that
    /// jumped the parameter the moment it was created is a lane nobody trusts.
    fn parameter_now(&self, address: &fontelle_types::ParamAddress) -> Option<f64> {
        use fontelle_types::ParamTarget;
        match ParamTarget::parse(address)? {
            // The **box's** tempo, not the automated one: a lane made on the
            // tempo starts flat where the box is, so making it changes
            // nothing you can hear (§12.4).
            ParamTarget::Tempo => Some(fontelle_types::normalised_tempo(
                self.project.tempo_map.tempo_at(0),
            )),
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
            ParamTarget::ChannelGain(id) => {
                let db = self.project.channels.get(id)?.gain_db;
                let span = fontelle_engine::CHANNEL_GAIN_MAX_DB
                    - fontelle_engine::CHANNEL_GAIN_MIN_DB;
                Some(f64::from(
                    ((db - fontelle_engine::CHANNEL_GAIN_MIN_DB) / span).clamp(0.0, 1.0),
                ))
            }
            ParamTarget::ChannelPan(id) => {
                let pan = self.project.channels.get(id)?.pan;
                Some(f64::from((pan + 1.0).clamp(0.0, 2.0) / 2.0))
            }
            // One of the instrument's own knobs. Read through the same table
            // the panel draws from and the audio thread writes through — see
            // `fontelle_core::patch_params`.
            ParamTarget::ChannelPatch { channel, param } => {
                let data = self.project.channels.get(channel)?.patch_data.as_ref()?;
                let patch = fontelle_core::Patch::from_data(data, |file| self.library.resolve(file))
                    .ok()?
                    .patch;
                fontelle_core::patch_params::value(&patch, &param).map(f64::from)
            }
            ParamTarget::Insert { track, slot, param } => {
                let insert = self.project.mixer.tracks.get(track)?.inserts.get(slot)?;
                insert.config.normalised(&param).map(f64::from)
            }
        }
    }

    /// Rebuilds the whole graph and publishes it — what an instrument change,
    /// a new channel or a mute needs.
    ///
    /// Note the order: the node map is rebuilt *first*, because adding a
    /// channel renumbers the nodes, and a timeline compiled against the old map
    /// would address notes to nodes that have moved.
    fn rebuild_graph(&mut self) {
        // Reusing the control surfaces, so a track's fader and meter outlive
        // the graph they were built with — see `realise::fader`.
        match crate::realise::realise_keeping(
            &self.project,
            &self.library,
            self.options,
            &self.track_controls,
            self.metronome.clone(),
            &self.spectrum_taps,
        ) {
            Ok(realised) => {
                self.channel_nodes = realised.channel_nodes;
                self.param_nodes = realised.param_nodes;
                self.audio_nodes = realised.audio_nodes;
                // The old set belonged to the graph that is being replaced.
                self.track_controls = realised.track_controls;
                self.effect_controls = realised.effect_controls;
                self.spectrum_taps = realised.spectrum_taps;
                self.send_controls = realised.send_controls;
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
                // **Every accepted command moves the revision.** The window
                // caches every list it draws and re-reads them only when this
                // moves (`fontelle-ui`'s `refresh_studio`), so a mutation that
                // forgets to bump it is a change you cannot see.
                //
                // It used to be bumped by hand at each call site, and drawing
                // a note was one of the places that did not: harmless while a
                // clip's block on the arrangement carried nothing that changed
                // with its notes, and a preview that never updated the moment
                // the block started showing them. One place to bump it is one
                // place to be wrong, and this is the place every edit already
                // goes through (INVARIANT 9).
                self.revision += 1;
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

    /// One row of the preset list, which is not always the open file's.
    ///
    /// A search reaches across the whole collection (see [`PresetIndex`]), so
    /// a row can be a heading naming a soundfont or a preset inside one that
    /// is not open. Everything that acts on a row — loading it, highlighting
    /// it, naming a channel after it — goes through this, so the list the
    /// panel draws and the list a click is resolved against cannot disagree.
    fn preset_rows(&self) -> Vec<PresetRow> {
        let query = self.query.trim();
        if query.is_empty() {
            // The open soundfont's own presets, in file order — what the
            // browser has always shown.
            return self
                .presets
                .iter()
                .map(|preset| PresetRow::Preset {
                    file: self.open_file.clone().unwrap_or_default(),
                    index: preset.index,
                    name: preset.name.clone(),
                    detail: format!("{}:{}", preset.bank, preset.program),
                })
                .collect();
        }

        // Searching: every soundfont's presets, grouped by the file they are
        // in, with the open one's first.
        let names: Vec<&str> = self
            .preset_index
            .hits
            .iter()
            .map(|hit| hit.name.as_str())
            .collect();
        let mut groups: Vec<(PathBuf, String, Vec<&PresetHit>)> = Vec::new();
        for index in matches_names(&names, query) {
            let hit = &self.preset_index.hits[index];
            match groups.iter_mut().find(|(file, _, _)| *file == hit.file) {
                Some((_, _, rows)) => rows.push(hit),
                None => groups.push((hit.file.clone(), hit.file_name.clone(), vec![hit])),
            }
        }
        // *"if i have a soundfont selected already it should show the results
        // within the one selected at the top first"* — the rest keep the order
        // the index was built in, which is the order the browser lists them.
        if let Some(open) = &self.open_file
            && let Some(at) = groups.iter().position(|(file, _, _)| file == open)
        {
            let mine = groups.remove(at);
            groups.insert(0, mine);
        }

        let mut rows = Vec::new();
        for (file, name, hits) in groups {
            rows.push(PresetRow::Group {
                name,
                detail: match hits.len() {
                    1 => "1 sound".to_string(),
                    n => format!("{n} sounds"),
                },
            });
            for hit in hits {
                rows.push(PresetRow::Preset {
                    file: file.clone(),
                    index: hit.index,
                    name: hit.name.clone(),
                    detail: format!("{}:{}", hit.bank, hit.program),
                });
            }
        }
        rows
    }

    /// Makes sure a search across the collection has something to search.
    ///
    /// Called when the query changes rather than when the bank is scanned: a
    /// person who never types in the box never pays for the read.
    fn want_preset_index(&mut self) {
        if self.query.trim().is_empty() || self.preset_index.ready || self.preset_index.running() {
            return;
        }
        let files: Vec<PathBuf> = self
            .bank
            .entries()
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        self.preset_index.start(files);
    }

    /// Whether the collection is still being read. The status line says so,
    /// and a test waits on it.
    pub fn searching_presets(&self) -> bool {
        self.preset_index.running()
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
        // Which row was clicked, which is not always a preset of the open
        // soundfont: a search lists hits from the whole collection.
        let (file, index, name) = match self.preset_rows().into_iter().nth(preset) {
            Some(PresetRow::Preset {
                file, index, name, ..
            }) => (file, index, name),
            Some(PresetRow::Group { .. }) => {
                return Err("that row is a heading, not a sound".to_string());
            }
            None => return Err("that preset is not in this soundfont".to_string()),
        };
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
        if !name.is_empty() {
            parts.push(Box::new(fontelle_model::RenameChannel::new(
                channel,
                name.clone(),
            )));
        }
        self.history
            .apply(
                Box::new(fontelle_model::Compound::new("Choose instrument", parts)),
                &mut self.project,
            )
            .map_err(|e| e.to_string())?;
        self.history.break_gesture();

        // The browser follows what was loaded. A hit chosen out of a search
        // across the collection is very often in a soundfont that is not open,
        // and leaving the file list pointing somewhere else would make the
        // highlight — and the next click — belong to a different soundfont.
        if self.open_file.as_ref() != Some(&file) {
            self.presets = fontelle_assets::list_presets(&file).unwrap_or_default();
            self.open_file = Some(file.clone());
        }
        self.channel_presets.insert(channel, (file, index));
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        Ok(())
    }

    /// A new channel, with a lane and an empty clip of its own, ready to be
    /// played and drawn in.
    ///
    /// Shared by the two ways of making one — the button, which gives it the
    /// built-in synth, and a Ctrl-clicked preset, which puts a soundfont on it
    /// afterwards. A channel with nowhere to write notes is a channel the roll
    /// cannot open, so the lane and the clip are not optional extras.
    ///
    /// Through the history like everything else (INVARIANT 9), so adding an
    /// instrument by mistake is one Ctrl+Z away.
    fn new_channel(
        &mut self,
        name: String,
        patch_data: Option<fontelle_types::PatchData>,
    ) -> Result<ChannelId, String> {
        let channel = self
            .apply_for::<AddChannel>(Box::new(AddChannel::new(name, patch_data)))?
            .channel()
            .ok_or("the channel was not created")?;
        self.history.break_gesture();

        let lane = self.project.lanes.insert(Lane {
            name: format!("Lane {}", self.project.lanes.len() + 1),
            height: 32.0,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            muted: false,
            locked: false,
            order: next_lane_order(&self.project),
        });
        // As long as the longest clip already there, so a new part lines up
        // with the piece rather than stopping a bar into it.
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
        // The new channel is the selected one, and its clip is what the roll
        // opens: you made it to put something in it.
        self.selected = self.project.channels.len().saturating_sub(1);
        if let Some(id) = self.clip_of_channel(channel) {
            self.clip = id;
        }
        Ok(channel)
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

    /// Takes the instrument off the selected channel, leaving a channel that
    /// plays nothing.
    ///
    /// A real state and a reachable one — it is what every channel used to
    /// start in, and it is what a project saved before the built-in synth
    /// existed opens as. Through the history like every other document
    /// mutation (INVARIANT 9).
    pub fn clear_channel_instrument(&mut self) {
        let Some(channel) = self.selected_channel_id() else {
            return;
        };
        self.run(Box::new(fontelle_model::SetChannelPatch::new(channel, None)));
        self.history.break_gesture();
        self.channel_presets.remove(&channel);
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        self.revision += 1;
    }

    /// Every parameter address the running graph can reach, as the instrument
    /// panel spells them.
    ///
    /// What a test asks to check the one invariant this feature rests on: the
    /// panel's list and the graph's map are the same list. A knob the panel
    /// offers and the graph does not know is a lane that is made, drawn, saved
    /// — and silent, because an unresolved target emits no events at all.
    pub fn automatable_addresses(&self) -> Vec<fontelle_types::ParamAddress> {
        let Some(channel) = self.selected_channel_id() else {
            return Vec::new();
        };
        self.param_nodes
            .keys()
            .filter_map(|address| match fontelle_types::ParamTarget::parse(address) {
                Some(fontelle_types::ParamTarget::ChannelGain(id)) if id == channel => {
                    Some(fontelle_types::ParamAddress::new(crate::instrument::MIXER_GAIN))
                }
                Some(fontelle_types::ParamTarget::ChannelPan(id)) if id == channel => {
                    Some(fontelle_types::ParamAddress::new(crate::instrument::MIXER_PAN))
                }
                Some(fontelle_types::ParamTarget::ChannelPatch { channel: id, param })
                    if id == channel =>
                {
                    Some(fontelle_types::ParamAddress::new(param))
                }
                _ => None,
            })
            .collect()
    }

    /// Which soundfont the browser has open, if any.
    ///
    /// Public so a test can check that choosing a search hit from another
    /// soundfont brought the browser with it.
    pub fn open_file_path(&self) -> Option<PathBuf> {
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
            // The Tools panel's *Add* and *Take off*: every note moved by the
            // same amount from wherever it already was, which is what keeps a
            // phrase's shape while changing its level.
            RollEdit::NudgeProperty {
                ids,
                property,
                delta,
            } => {
                self.run(Box::new(fontelle_model::NudgeNoteProperty::new(
                    clip, ids, property, delta,
                )));
                Vec::new()
            }
            // And the randomizer's: a value each, as one entry in the
            // history so one undo takes the whole roll back.
            RollEdit::SetPropertyEach {
                ids,
                property,
                values,
            } => {
                self.run(Box::new(fontelle_model::SetNotePropertyEach::new(
                    clip, ids, property, values,
                )));
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
        let map = &self.effective_tempo;
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
        let tick = self.effective_tempo.sample_to_tick(position_sample);
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
        self.effective_tempo.tick_to_sample(start + tick.max(0))
    }

    /// A tick of the open clip, as a tick of the song.
    fn song_tick_of_clip_tick(&self, tick: Tick) -> Tick {
        let start = self
            .project
            .clips
            .get(self.clip)
            .map_or(0, |clip| clip.start);
        start + tick
    }

    fn clip_tick_of_song_tick(&self, tick: Tick) -> Tick {
        let start = self
            .project
            .clips
            .get(self.clip)
            .map_or(0, |clip| clip.start);
        tick - start
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
        // Through `apply_for` like every other command that hands its ids
        // back. It used to call `History::apply` itself, which made a **third**
        // way into the history — and the third way was the one that forgot to
        // move the revision, so notes drawn in the roll never reached the
        // block on the arrangement. Two ways in are enough to keep honest.
        let ids = match self.apply_for::<AddNotes>(Box::new(AddNotes::new(clip, notes))) {
            Ok(add) => add.ids().to_vec(),
            Err(e) => {
                eprintln!("Fontelle: {e}");
                self.message = Some(e);
                return Vec::new();
            }
        };
        self.republish();
        ids
    }

    // ------------------------------------------------ importing files ---

    /// Sets the folder `kind` is imported from, remembers it, and reads it.
    ///
    /// The half of `choose_import_dir` that does not involve a dialog, so a
    /// test — or a command-line flag, when there is one — can point the
    /// importer somewhere without a person clicking through a picker.
    pub fn set_import_folder(&mut self, kind: fontelle_types::FolderKind, dir: Option<PathBuf>) {
        self.settings.set_folder(kind, dir);
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        if self.import_kind == kind {
            self.rescan_imports();
        } else {
            self.revision += 1;
        }
    }

    /// Reads the import folder **if the bank is not already the right one**.
    ///
    /// The lazy form, and the one every entry point goes through. It exists
    /// because the eager form was wrong in a way nothing could see: the bank
    /// was rescanned when the *kind changed*, so opening the Import tab on
    /// the kind it already had — which is what "Import MIDI…" does on a fresh
    /// launch — browsed an empty, never-scanned bank and reported "no sf2
    /// files" over a folder of MIDI. Asking "is the bank the one the settings
    /// name?" cannot go stale the way "did something just change?" can.
    ///
    /// Cheap when it is already right: two path comparisons and no disk.
    fn ensure_import_bank(&mut self) {
        let wanted: Vec<PathBuf> = self
            .settings
            .folder(self.import_kind)
            .map(|dir| vec![dir.to_path_buf()])
            .unwrap_or_default();
        if self.import_bank.filter() == BankFilter::Files(self.import_kind)
            && self.import_bank.dirs() == wanted.as_slice()
        {
            return;
        }
        self.rescan_imports();
    }

    /// Points the import bank at the folder for `kind` and reads it.
    ///
    /// The disk read itself. Callers want [`ensure_import_bank`] unless they
    /// know something has changed underneath.
    fn rescan_imports(&mut self) {
        let dirs = self
            .settings
            .folder(self.import_kind)
            .map(|dir| vec![dir.to_path_buf()])
            .unwrap_or_default();
        self.import_bank = FileBank::with_filter(dirs, BankFilter::Files(self.import_kind));
        self.import_bank.rescan();
        self.revision += 1;
    }

    /// The path each row of the Import tab stands for. `None` for a folder or
    /// the `..` row, which are moves rather than files.
    fn import_rows(&self) -> Vec<Option<PathBuf>> {
        if !self.import_query.trim().is_empty() {
            return self
                .import_bank
                .search(&self.import_query)
                .into_iter()
                .map(|entry| Some(entry.path.clone()))
                .collect();
        }
        self.import_bank
            .rows()
            .iter()
            .map(|row| match row {
                BankRow::File(entry) => Some(entry.path.clone()),
                _ => None,
            })
            .collect()
    }

    /// Brings `parts` in as instruments, rows and clips — one history entry.
    ///
    /// The tempo is **not** touched when the project already has clips in it.
    /// A file's tempo is right for the file and wrong for the piece you are
    /// working on, and changing the song's tempo is an edit nobody asked for;
    /// on an empty project it is the only tempo there is, so it is taken.
    fn bring_in(&mut self, what: &str, parts: Vec<ImportPart>, bpm: Option<f64>) -> Result<String, String> {
        if parts.is_empty() {
            return Err(format!("there is nothing in {what} to import"));
        }
        let names: Vec<String> = parts.iter().map(|part| part.name.clone()).collect();
        let empty = self.project.clips.is_empty();
        let command = Box::new(ImportParts::new(what.to_string(), parts));
        let made = self
            .apply_for::<ImportParts>(command)?
            .made()
            .to_vec();
        if made.is_empty() {
            return Err(format!("there is nothing in {what} to import"));
        }

        // The roll opens on what just arrived, which is what somebody who
        // pressed "import" is looking for.
        if let Some(first) = made.first() {
            self.open_clip(first.clip);
        }
        if let (Some(bpm), true) = (bpm, empty) {
            self.set_tempo(bpm);
        }
        self.rebuild_graph();
        self.republish();

        let tempo_note = match (bpm, empty) {
            (Some(bpm), false) => format!(" \u{2014} the file is {bpm:.0} bpm, this song is not"),
            _ => String::new(),
        };
        Ok(match names.len() {
            1 => format!("Imported \u{201c}{}\u{201d}{tempo_note}", names[0]),
            n => format!("Imported {n} parts from {what}{tempo_note}"),
        })
    }

    /// Every part of a surveyed MIDI file, as things to bring in.
    fn midi_parts(path: &Path, which: MidiChannels) -> Result<(Vec<ImportPart>, f64), String> {
        let import = import_midi(path, which).map_err(|e| e.to_string())?;
        let mut parts = Vec::new();
        for (index, channel) in import.channels.iter().enumerate() {
            // The notes are in the project the importer built; they are read
            // out of it rather than re-derived, so what arrives is exactly
            // what `import_midi` is tested to produce.
            let Some(clip) = import
                .project
                .clips
                .values()
                .find(|clip| match &clip.source {
                    ClipSource::Notes(data) => data.channel == channel.channel,
                    _ => false,
                })
            else {
                continue;
            };
            let ClipSource::Notes(data) = &clip.source else {
                continue;
            };
            parts.push(ImportPart {
                name: channel.name.clone(),
                notes: data.notes.values().copied().collect(),
                pan: channel.pan,
                volume_db: channel.volume_db,
                color: IMPORT_COLOURS[index % IMPORT_COLOURS.len()],
            });
        }
        Ok((parts, import.bpm))
    }

    /// Imports a `.mid` file, or asks which part of it to import.
    fn import_midi_file(&mut self, path: &Path, which: Option<MidiChannels>) -> Result<String, String> {
        let name = file_label(path);
        if let Some(which) = which {
            let (parts, bpm) = Self::midi_parts(path, which)?;
            return self.bring_in(&name, parts, Some(bpm));
        }
        let survey = survey_midi(path).map_err(|e| e.to_string())?;
        if survey.parts.is_empty() {
            return Err(format!("there are no notes in {name}"));
        }
        // **One part needs no question.** A prompt with a single answer is a
        // click somebody has to make to get what they already asked for.
        if !survey.is_multi_part() {
            let only = survey.parts[0].channel;
            let (parts, bpm) = Self::midi_parts(path, MidiChannels::Only(only))?;
            return self.bring_in(&name, parts, Some(bpm));
        }
        // More than one: the window asks. What it asks *with* is
        // `pending_import`, which it reads back through `import_prompt`.
        self.pending_import = Some(PendingImport {
            path: path.to_path_buf(),
            survey,
        });
        Ok(String::new())
    }

    /// Brings a sound into the arrangement as a clip on a row of its own
    /// (TDD §15).
    ///
    /// **A row of its own**, because an audio clip is a take or a loop and
    /// dropping one over what is already on a row would replace music with
    /// music. A `.mid` gets a row per part for the same reason.
    ///
    /// Its length is the sound's own **duration**, converted through the
    /// project's tempo map: a clip whose end is not where the sound ends is one
    /// every later trim is measured against wrongly.
    fn import_audio_file(&mut self, path: &Path) -> Result<String, String> {
        let name = file_label(path);
        let imported = self.library.import_audio(path).map_err(|e| e.to_string())?;
        if imported.frames == 0 || imported.sample_rate == 0 {
            return Err(format!("there is no sound in {name}"));
        }

        // Where it lands: the time selection's start if there is one, and
        // otherwise the top of the song. The same rule an imported score
        // follows.
        let start = self.project.loop_range.map(|(from, _)| from.max(0)).unwrap_or(0);
        // Its own duration in ticks, through the tempo map that already owns
        // every sample-to-tick conversion in the project.
        let samples = (imported.frames as f64 * self.options.sample_rate as f64
            / f64::from(imported.sample_rate)) as i64;
        let at = self.project.tempo_map.tick_to_sample(start);
        let length = self.project.tempo_map.sample_to_tick(at + samples) - start;
        let length = length.max(fontelle_model::MIN_CLIP_LENGTH);

        let data = fontelle_types::AudioClipData::whole(
            imported.asset.clone(),
            imported.frames as fontelle_types::Sample,
        );
        let command = Box::new(fontelle_model::AddAudioClip::new(
            name.clone(),
            data,
            start,
            length,
        ));
        self.apply_for::<fontelle_model::AddAudioClip>(command)?;
        // The graph has to be rebuilt: the player nodes hold the audio store,
        // and the one they are holding does not have this file in it.
        self.rebuild_graph();
        self.republish();
        Ok(format!("Imported \u{201c}{name}\u{201d}"))
    }

    /// Imports an FL Studio score into the clip that is open.
    ///
    /// A score is a **phrase** — no instrument, no tempo, no arrangement — so
    /// it lands in the roll rather than as a track of its own. That is what
    /// FL's own *import score* does with one, and it is the difference
    /// between the two formats rather than an inconsistency between them.
    fn import_score_file(&mut self, path: &Path) -> Result<String, String> {
        let score = import_fsc(path).map_err(|e| e.to_string())?;
        let name = file_label(path);
        // Where the phrase goes: the start of the time selection if there is
        // one, and otherwise the top of the clip.
        let at = self
            .project
            .loop_range
            .map(|(from, _)| self.clip_tick_of_song_tick(from).max(0))
            .unwrap_or(0);

        // Every instrument in the score, flattened onto the one clip. FL's own
        // library is single-instrument throughout; a score saved from a
        // pattern of several is rare, and merging is closer to what was asked
        // for than silently dropping all but one.
        let mut notes = score.phrase_on(None);
        if notes.is_empty() {
            return Err(format!("there are no notes in {name}"));
        }
        for note in &mut notes {
            note.start += at;
        }
        let count = notes.len();
        let ids = self.insert(self.clip, notes);
        if ids.is_empty() {
            return Err(format!("{name} could not be added to this clip"));
        }
        self.history.break_gesture();
        Ok(format!(
            "Imported {count} note(s) from \u{201c}{name}\u{201d} (FL {})",
            score.version
        ))
    }

    /// Applies `command` through the history and hands back the entry, so a
    /// caller that needs the id the command minted can downcast for it.
    fn apply_for<T: Command + 'static>(&mut self, command: Box<dyn Command>) -> Result<&T, String> {
        self.history
            .apply(command, &mut self.project)
            .map_err(|e| e.to_string())?;
        self.dirty = true;
        // The second of the two ways into the history — see `run`, which
        // carries the reasoning. Both bump it, so "the document changed" and
        // "the window knows" cannot come apart.
        self.revision += 1;
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
                BankRow::Folder { name, files, .. } => LibraryEntry {
                    name: name.clone(),
                    detail: match files {
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
        self.preset_rows()
            .into_iter()
            .map(|row| match row {
                // Bank and program on a preset, because a General MIDI
                // soundfont has three presets called "Piano" and the numbers
                // are what tell them apart.
                PresetRow::Preset { name, detail, .. } => LibraryEntry::file(name, detail),
                PresetRow::Group { name, detail } => LibraryEntry {
                    name,
                    detail,
                    kind: fontelle_ui::document::LibraryKind::Group,
                },
            })
            .collect()
    }

    fn query(&self) -> &str {
        // Whichever list is showing — see `set_browser_mode`.
        if self.browser_mode == fontelle_ui::canvas::BrowserMode::Import {
            return &self.import_query;
        }
        &self.query
    }

    fn set_browser_mode(&mut self, mode: fontelle_ui::canvas::BrowserMode) {
        if self.browser_mode == mode {
            return;
        }
        self.browser_mode = mode;
        if mode == fontelle_ui::canvas::BrowserMode::Import {
            self.ensure_import_bank();
        }
        self.revision += 1;
    }

    fn set_query(&mut self, query: &str) {
        if self.browser_mode == fontelle_ui::canvas::BrowserMode::Import {
            self.import_query = query.to_string();
            // The list is a different list now — the whole folder tree rather
            // than one folder, or the other way round.
            self.revision += 1;
            return;
        }
        self.query = query.to_string();
        // Typing is what asks for the collection to be read — see
        // `want_preset_index`. Nothing happens on the second keystroke: the
        // index is built once and searched many times.
        self.want_preset_index();
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
        // Nothing is "open" in the Import tab: its rows are files you act on
        // rather than a file you are looking inside. Answering with the
        // soundfont's index would light whichever import row happened to sit
        // at that position — a highlight on a file nobody chose.
        if self.browser_mode == fontelle_ui::canvas::BrowserMode::Import {
            return None;
        }
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
        // Into the list the panel draws — a search that has hidden the chosen
        // preset shows no highlight, which is true.
        self.preset_rows().into_iter().position(|row| {
            matches!(row, PresetRow::Preset { file: f, index: i, .. } if f == *file && i == *index)
        })
    }

    fn add_channel(&mut self) -> Result<(), String> {
        let name = format!("Channel {}", self.project.channels.len() + 1);
        // **The built-in synth, not silence.** A channel with no instrument
        // has no panel, no keys that sound and nothing a knob can change, so
        // the button that made one looked broken until a soundfont had been
        // chosen — and then looked haunted, because the click people made next
        // (a preset, meaning *"swap this channel's sound"*) was the one that
        // finally seemed to do it. See `fontelle_core::Patch::basic_synth`.
        let patch = fontelle_core::Patch::basic_synth()
            .to_data(self.library.provenance())
            .map_err(|e| e.to_string())?;
        let channel = self.new_channel(name, Some(patch))?;
        self.patch_cache = None;
        self.rebuild_graph();
        self.revision += 1;
        let _ = channel;
        Ok(())
    }

    fn add_channel_with(&mut self, preset: usize) -> Result<(), String> {
        let name = match self.preset_rows().into_iter().nth(preset) {
            Some(PresetRow::Preset { name, .. }) if !name.is_empty() => name,
            _ => format!("Channel {}", self.project.channels.len() + 1),
        };
        let channel = self.new_channel(name, None)?;
        self.install_preset(channel, preset)
    }

    fn duplicate_channel(&mut self, index: usize) {
        let Some(id) = self.channel_ids().get(index).copied() else {
            return;
        };
        match self.apply_for::<fontelle_model::DuplicateChannel>(Box::new(
            fontelle_model::DuplicateChannel::new(id),
        )) {
            Ok(command) => {
                let made = command.channel();
                self.history.break_gesture();
                // The copy is what you are now working on — you made it to
                // play it — and the roll follows, the same handshake adding a
                // channel has.
                if let Some(made) = made {
                    self.selected = self
                        .channel_ids()
                        .iter()
                        .position(|c| *c == made)
                        .unwrap_or(self.selected);
                    if let Some(clip) = self.clip_of_channel(made) {
                        self.clip = clip;
                    }
                }
                self.patch_cache = None;
                self.dirty = true;
                self.rebuild_graph();
            }
            Err(e) => self.message = Some(e),
        }
    }

    fn remove_channel(&mut self, index: usize) {
        let ids = self.channel_ids();
        let Some(id) = ids.get(index).copied() else {
            return;
        };
        // The last one stays. A rack with no channels is a studio with nothing
        // to play and no clip for the roll to open — and every way back in
        // makes a channel anyway, so this only ever costs a press of the add
        // button.
        if ids.len() <= 1 {
            self.message = Some("a project needs at least one instrument".to_string());
            return;
        }
        let opened = self.channel_of_clip(self.clip) == Some(id);
        self.run(Box::new(fontelle_model::RemoveChannel::new(id)));
        self.history.break_gesture();
        self.channel_presets.remove(&id);
        self.patch_cache = None;
        self.selected = self
            .selected
            .min(self.project.channels.len().saturating_sub(1));
        // The roll cannot go on showing a clip that is not there.
        if opened && let Some(next) = Self::first_clip(&self.project) {
            self.clip = next;
            self.selected = self.channel_index_of_clip().unwrap_or(self.selected);
        }
        self.dirty = true;
        self.rebuild_graph();
    }

    fn rename_channel(&mut self, index: usize, name: &str) {
        let Some(id) = self.channel_ids().get(index).copied() else {
            return;
        };
        // No `break_gesture`: `RenameChannel::merge_with` folds every keystroke
        // into one entry, so undo takes back the *name*, not the last letter.
        self.run(Box::new(fontelle_model::RenameChannel::new(id, name)));
        self.dirty = true;
        self.revision += 1;
    }

    fn clear_channel_instrument(&mut self, index: usize) {
        let Some(id) = self.channel_ids().get(index).copied() else {
            return;
        };
        let was = self.selected;
        self.selected = index;
        Session::clear_channel_instrument(self);
        self.selected = was.min(self.project.channels.len().saturating_sub(1));
        let _ = id;
    }

    // --- the arrangement's rows ---

    fn add_lane(&mut self) {
        let name = format!("Lane {}", self.project.lanes.len() + 1);
        self.run(Box::new(fontelle_model::AddLane::new(name)));
        self.history.break_gesture();
        self.dirty = true;
        self.revision += 1;
    }

    fn remove_lane(&mut self, index: usize) {
        let Some(id) = self.lane_ids().get(index).copied() else {
            return;
        };
        let opened = self
            .project
            .clips
            .get(self.clip)
            .is_some_and(|clip| clip.lane == id);
        self.run(Box::new(fontelle_model::RemoveLane::new(id)));
        self.history.break_gesture();
        if opened && let Some(next) = Self::first_clip(&self.project) {
            self.clip = next;
            self.selected = self.channel_index_of_clip().unwrap_or(self.selected);
        }
        self.dirty = true;
        // The clips that went with it are gone from the timeline, so the
        // sequencer has to hear about it as well as the window.
        self.rebuild_graph();
    }

    fn rename_lane(&mut self, index: usize, name: &str) {
        let Some(id) = self.lane_ids().get(index).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::RenameLane::new(id, name)));
        self.dirty = true;
        self.revision += 1;
    }

    fn can_remove_lane(&self, index: usize) -> bool {
        self.project.lanes.len() > 1 && index < self.project.lanes.len()
    }

    fn automate_instrument_param(&mut self, address: &fontelle_types::ParamAddress, at: Sample) {
        let Some(id) = self.selected_channel_id() else {
            return;
        };
        // The panel's address is a *panel* address — `mixer/gain`,
        // `patch/filter[0]/cutoff` — and an automation target has to name the
        // channel as well, since two channels' panels offer the same strings.
        // This is the one place the two scrapes of §8.2's scheme meet.
        let target = match address.as_str() {
            crate::instrument::MIXER_GAIN => fontelle_types::ParamTarget::ChannelGain(id),
            crate::instrument::MIXER_PAN => fontelle_types::ParamTarget::ChannelPan(id),
            param if param.starts_with("patch/") => fontelle_types::ParamTarget::ChannelPatch {
                channel: id,
                param: param.to_string(),
            },
            // A control the panel drew and this build cannot address. Nothing
            // rather than a lane pointed at nothing.
            _ => return,
        };
        // What the control is *called*, taken from the panel rather than from
        // the address: "cutoff" is what somebody right-clicked, and
        // `patch/filter[0]/cutoff` is what the file says.
        let label = {
            let channel = self
                .project
                .channels
                .get(id)
                .map_or_else(String::new, |channel| channel.name.clone());
            let caption = self
                .instrument()
                .and_then(|view| {
                    view.groups
                        .iter()
                        .flat_map(|group| group.params.iter())
                        .find(|param| param.address == *address)
                        .map(|param| param.label.clone())
                })
                .unwrap_or_else(|| address.to_string());
            format!("{channel} \u{2014} {caption}")
        };
        let at = self.playhead_song_tick(at);
        self.create_automation(&target.address(), &label, at);
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

    fn settings(&self) -> Vec<LibraryEntry> {
        crate::settings::SETTING_ROWS
            .iter()
            .map(|row| LibraryEntry::file(row.label(), row.value(&self.settings)))
            .collect()
    }

    fn settings_status(&self) -> String {
        // The file, not the folder: "where do I edit this by hand" is the
        // question a settings tab leaves somebody with, and the answer is a
        // path. Elided from the left like the bank's, because the end of a
        // path is the half that says where you are.
        match self.settings_path.clone().or_else(Settings::config_path) {
            Some(path) => crate::desktop::elide_path(&path, 2),
            None => "there is no home directory to keep settings in".to_string(),
        }
    }

    fn nudge_setting(&mut self, index: usize, delta: i32) {
        let Some(row) = crate::settings::SETTING_ROWS.get(index) else {
            return;
        };
        // A folder row is a **button**, not a value to step: clicking it asks
        // for a folder. `SettingRow::nudge` deliberately does nothing to one
        // (a row that was both would change the row above it), so the branch
        // is here, at the one place a settings row is pressed.
        if let Some(kind) = row.folder() {
            self.import_kind = kind;
            self.import_query.clear();
            self.choose_import_dir();
            self.revision += 1;
            return;
        }
        let before = self.settings.midi_input;
        row.nudge(&mut self.settings.midi_input, delta);
        if self.settings.midi_input == before {
            // A heading, or a number already at its end. Neither is worth a
            // write to disk or a redraw.
            return;
        }
        // Onto the keyboard first and into the file second: what somebody is
        // adjusting is how the next note feels, and a disk write that fails
        // must not stop that.
        self.publish_input_settings();
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.revision += 1;
    }

    // ------------------------------------------------ importing files ---

    fn import_kind(&self) -> fontelle_types::FolderKind {
        self.import_kind
    }

    fn set_import_kind(&mut self, kind: fontelle_types::FolderKind) {
        if self.import_kind != kind {
            self.import_kind = kind;
            // A different folder is a different list, and a query typed
            // against one means nothing against the other.
            self.import_query.clear();
        }
        // **Unconditionally**, not only when the kind moved: on a fresh launch
        // the kind has not moved and the bank has never been read.
        self.ensure_import_bank();
    }

    fn has_import_dir(&self, kind: fontelle_types::FolderKind) -> bool {
        self.settings.folder(kind).is_some()
    }

    fn import_files(&self) -> Vec<LibraryEntry> {
        use fontelle_ui::document::LibraryKind;
        if !self.import_query.trim().is_empty() {
            return self
                .import_bank
                .search(&self.import_query)
                .into_iter()
                .map(|entry| LibraryEntry {
                    name: entry.name.clone(),
                    // Where it is, not how big: two files called `Intro` in
                    // different folders is the commonest thing in a
                    // collection, and a flat list of names cannot tell them
                    // apart.
                    detail: match self.import_bank.folder_of(entry) {
                        folder if folder.is_empty() => human_size(entry.size_bytes),
                        folder => folder,
                    },
                    kind: LibraryKind::File,
                })
                .collect();
        }
        let noun = self.import_bank.filter().noun();
        self.import_bank
            .rows()
            .iter()
            .map(|row| match row {
                BankRow::Up { .. } => LibraryEntry {
                    name: "..".to_string(),
                    detail: String::new(),
                    kind: LibraryKind::Up,
                },
                BankRow::Folder { name, files, .. } => LibraryEntry {
                    name: name.clone(),
                    detail: match files {
                        0 => String::new(),
                        n => format!("{n} {noun}"),
                    },
                    kind: LibraryKind::Folder,
                },
                BankRow::File(entry) => LibraryEntry {
                    name: entry.name.clone(),
                    detail: human_size(entry.size_bytes),
                    kind: LibraryKind::File,
                },
            })
            .collect()
    }

    fn import_status(&self) -> String {
        let Some(dir) = self.settings.folder(self.import_kind) else {
            return format!(
                "no {} folder yet \u{2014} set one in Settings",
                self.import_kind.tab_label()
            );
        };
        if let Some((path, why)) = self.import_bank.unreadable().first() {
            return format!("{}: {why}", crate::desktop::elide_path(path, 2));
        }
        let at = self.import_bank.at().unwrap_or(dir);
        match self.import_bank.entries().len() {
            0 => format!(
                "no {} files under {}",
                self.import_bank.filter().noun(),
                crate::desktop::elide_path(at, 2)
            ),
            n => format!("{n} file(s) \u{2014} {}", crate::desktop::elide_path(at, 2)),
        }
    }

    fn open_import(&mut self, index: usize) -> Result<(), String> {
        self.ensure_import_bank();
        // A folder row moves the browser and opens nothing. Only while
        // *browsing*: a search lists files wherever they are, and a hit is
        // always a file.
        if self.import_query.trim().is_empty() && self.import_bank.open_row(index) {
            self.revision += 1;
            return Ok(());
        }
        let path = self
            .import_rows()
            .get(index)
            .cloned()
            .flatten()
            .ok_or("that file is not in the folder any more")?;
        let result = match self.import_kind {
            fontelle_types::FolderKind::Midi => self.import_midi_file(&path, None),
            fontelle_types::FolderKind::Scores => self.import_score_file(&path),
            fontelle_types::FolderKind::Audio => self.import_audio_file(&path),
        };
        match result {
            // An empty message is the multi-part prompt going up: it has not
            // imported anything yet and there is nothing to announce.
            Ok(message) => {
                if !message.is_empty() {
                    self.message = Some(message);
                }
                self.revision += 1;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn choose_import_dir(&mut self) {
        let kind = self.import_kind;
        let start = self
            .settings
            .folder(kind)
            .map(std::path::Path::to_path_buf)
            .or_else(|| self.settings.projects_dir.clone());
        match crate::desktop::choose_folder(kind.picker_title(), start.as_deref()) {
            Ok(Some(dir)) => {
                self.settings.set_folder(kind, Some(dir));
                if let Err(e) = self.save_settings() {
                    self.message = Some(format!("could not write settings: {e}"));
                }
                self.rescan_imports();
                self.message = Some(match self.import_bank.entries().len() {
                    0 => format!("no {} files in there", self.import_bank.filter().noun()),
                    n => format!("found {n} file(s)"),
                });
            }
            // A cancel is not an event.
            Ok(None) => {}
            Err(e) => self.message = Some(e),
        }
    }

    fn reveal_import_dir(&mut self) {
        let dir = self
            .import_bank
            .at()
            .map(std::path::Path::to_path_buf)
            .or_else(|| {
                self.settings
                    .folder(self.import_kind)
                    .map(std::path::Path::to_path_buf)
            });
        let Some(dir) = dir else {
            self.message = Some(format!(
                "no {} folder yet \u{2014} set one in Settings",
                self.import_kind.tab_label()
            ));
            return;
        };
        if let Err(e) = crate::desktop::reveal(&dir) {
            self.message = Some(e);
        }
    }

    fn import_prompt(&self) -> Option<fontelle_ui::document::ImportPrompt> {
        let pending = self.pending_import.as_ref()?;
        let survey = &pending.survey;
        let mut choices = vec![format!(
            "All {} parts, as separate tracks",
            survey.parts.len()
        )];
        // Then one line per part, named and counted, so choosing between
        // "Bass" and "Strings" does not mean knowing which MIDI channel each
        // of them was on.
        for part in &survey.parts {
            choices.push(format!("Only \u{201c}{}\u{201d} \u{2014} {} notes", part.name, part.notes));
        }
        Some(fontelle_ui::document::ImportPrompt {
            title: format!(
                "{} \u{2014} {:.0} bpm",
                file_label(&pending.path),
                survey.bpm
            ),
            choices,
        })
    }

    fn answer_import(&mut self, choice: usize) {
        let Some(pending) = self.pending_import.take() else {
            return;
        };
        let which = match choice.checked_sub(1) {
            // Every part. Percussion included: it is on the list the person
            // just read, so leaving it out would be leaving out something
            // they were shown and chose.
            None => MidiChannels::All,
            Some(index) => match pending.survey.parts.get(index) {
                Some(part) => MidiChannels::Only(part.channel),
                None => return,
            },
        };
        match self.import_midi_file(&pending.path, Some(which)) {
            Ok(message) if !message.is_empty() => self.message = Some(message),
            Ok(_) => {}
            Err(e) => self.message = Some(e),
        }
        self.revision += 1;
    }

    fn cancel_import(&mut self) {
        if self.pending_import.take().is_some() {
            self.revision += 1;
        }
    }

    fn drop_file(&mut self, path: &Path) -> Result<String, String> {
        let name = file_label(path);
        if !path.exists() {
            return Err(format!("{name} is not there"));
        }
        // By extension, which is the only thing a drop carries. Each kind is
        // asked whether it is *its* file rather than the extension being
        // matched here, so adding one is a variant and nothing else.
        if fontelle_types::FolderKind::Midi.accepts(path) {
            return self.import_midi_file(path, None);
        }
        if fontelle_types::FolderKind::Scores.accepts(path) {
            return self.import_score_file(path);
        }
        if fontelle_types::FolderKind::Audio.accepts(path) {
            return self.import_audio_file(path);
        }
        if crate::bank::is_soundfont(path) {
            // Not an import into the song: a soundfont is an *instrument*, so
            // dropping one puts it on the selected channel the way clicking
            // one in the browser does.
            self.presets = fontelle_assets::list_presets(path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            self.open_file = Some(path.to_path_buf());
            self.revision += 1;
            self.set_channel_instrument(0)?;
            return Ok(format!("Loaded \u{201c}{name}\u{201d}"));
        }
        Err(format!(
            "{name} is not something Fontelle can open \u{2014} it reads .wav, .flac, .mp3, \
             .ogg, .mid, .fsc and .sf2 files"
        ))
    }

    fn audio_clip(&self, clip: ClipId) -> Option<fontelle_types::AudioClipData> {
        match &self.project.clips.get(clip)?.source {
            ClipSource::Audio(data) => Some(data.clone()),
            _ => None,
        }
    }

    fn audio_clip_rate(&self, clip: ClipId) -> u32 {
        let Some(ClipSource::Audio(data)) = self.project.clips.get(clip).map(|c| &c.source) else {
            return 0;
        };
        self.library
            .audio_store()
            .get(data.asset.id)
            .map_or(0, |buffer| buffer.sample_rate)
    }

    fn set_audio_clip(&mut self, clip: ClipId, data: fontelle_types::AudioClipData) {
        // The clip's length on the arrangement follows its trim: a clip that
        // draws four bars and plays two is the picture lying about the sound.
        // Not its *speed*, though — a clip played at half speed still occupies
        // the block it was given, which is what makes a loop stretchable.
        // `run` republishes and bumps the revision, so the block on the
        // arrangement redraws with its new fades on the same frame.
        self.run(Box::new(fontelle_model::SetAudioClip::new(clip, data)));
    }

    fn reveal_config_dir(&mut self) {
        let dir = self
            .settings_path
            .as_deref()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(Settings::config_dir);
        let Some(dir) = dir else {
            self.message = Some("there is no home directory to keep settings in".to_string());
            return;
        };
        if let Err(e) = crate::desktop::reveal(&dir) {
            self.message = Some(e);
        }
    }

    fn choose_projects_dir(&mut self) {
        // One projects folder, not a list: "where do my projects live" has one
        // answer, unlike a soundfont bank, which is genuinely several places.
        match crate::desktop::choose_folder("Projects folder", self.projects.dir()) {
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
            let files = self.bank.search(&self.query).len();
            // The sounds *inside* the soundfonts, which is the half of the
            // answer a file-name search cannot give: nothing is called `tuba`
            // and four soundfonts have one in them.
            let sounds = self
                .preset_rows()
                .iter()
                .filter(|row| matches!(row, PresetRow::Preset { .. }))
                .count();
            if self.preset_index.running() {
                return format!(
                    "reading the collection\u{2026} {} of {} soundfonts",
                    self.preset_index.scanned, self.preset_index.total
                );
            }
            let query = self.query.trim();
            return match (files, sounds) {
                (0, 0) => format!("nothing matching \u{201c}{query}\u{201d}"),
                (0, n) => format!("{n} sounds matching \u{201c}{query}\u{201d}"),
                (1, 0) => "1 match in the whole collection".to_string(),
                (f, 0) => format!("{f} matches in the whole collection"),
                (f, n) => format!("{f} soundfonts and {n} sounds matching \u{201c}{query}\u{201d}"),
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
        match crate::desktop::choose_folder("Soundfont folder", start.as_deref()) {
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
        // **The channel's own level and placement**, not its mixer track's.
        // Every channel goes to the master until somebody routes it somewhere
        // else, so a panel reading the track was every panel reading one
        // fader — see `Channel::gain_db`.
        let mut view = crate::instrument::describe(
            &channel.name,
            &patch,
            channel.gain_db,
            channel.pan,
        );
        // The ring §12.2 asks for. Marked here rather than inside `describe`,
        // which knows about patches and not about clips — and marked from one
        // set rather than one question per knob, since answering it walks
        // every clip in the project.
        // Built once for the whole panel: `automated_targets` walks every
        // clip in the project, and a closure that called it per knob would
        // walk them forty-nine times for an EQ.
        let automated = fontelle_model::automated_targets(&self.project);
        view.mark_automated(|address| automated.contains(address));
        Some(view)
    }

    fn clip_clipboard_len(&self) -> usize {
        self.clip_clipboard.len()
    }

    fn key_style(&self) -> fontelle_ui::canvas::KeyStyle {
        let named = self
            .channel_ids()
            .get(self.selected)
            .and_then(|id| self.project.channels.get(*id))
            .is_some_and(|channel| channel.named_keys);
        if named {
            fontelle_ui::canvas::KeyStyle::Names
        } else {
            fontelle_ui::canvas::KeyStyle::Piano
        }
    }

    fn set_key_style(&mut self, style: fontelle_ui::canvas::KeyStyle) {
        let Some(id) = self.channel_ids().get(self.selected).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::SetFlag::new(
            fontelle_model::FlagTarget::ChannelNamedKeys(id),
            style == fontelle_ui::canvas::KeyStyle::Names,
        )));
        self.history.break_gesture();
        // The strip is a different width in the two views, so the grid beside
        // it moves: the window has to lay the panel out again, and it does
        // that when the revision moves.
        self.revision += 1;
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
        // The two that live on the **channel** rather than in its patch. Not
        // on its mixer track: a track is a destination several channels may
        // share, and a panel that wrote to one was two channels' panels
        // turning the same fader. See `Channel::gain_db`.
        match address.as_str() {
            crate::instrument::MIXER_GAIN => {
                let db = crate::instrument::GAIN_MIN_DB
                    + value.clamp(0.0, 1.0)
                        * (crate::instrument::GAIN_MAX_DB - crate::instrument::GAIN_MIN_DB);
                self.run(Box::new(fontelle_model::SetNumber::new(
                    fontelle_model::NumberTarget::ChannelGainDb(channel_id),
                    f64::from(db),
                )));
                self.rebuild_graph();
                self.revision += 1;
                return;
            }
            crate::instrument::MIXER_PAN => {
                self.run(Box::new(fontelle_model::SetNumber::new(
                    fontelle_model::NumberTarget::ChannelPan(channel_id),
                    f64::from(value.clamp(0.0, 1.0) * 2.0 - 1.0),
                )));
                self.rebuild_graph();
                self.revision += 1;
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
        if !crate::instrument::set(&mut patch, address.as_str(), value) {
            return;
        }
        self.store_patch(channel_id, patch);
    }

    // --- the mixer (TDD §13) ---

    fn mixer_strips(&self) -> Vec<MixerStrip> {
        let ids = self.mixer_track_ids();
        // Built once for the whole rack rather than per insert: it walks every
        // clip in the project, and asking it per slot would walk them again
        // for each one.
        let automated = fontelle_model::automated_targets(&self.project);
        ids.iter()
            .copied()
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
                        .enumerate()
                        .map(|(index, slot)| fontelle_ui::canvas::InsertInfo {
                            label: slot.kind().label().to_string(),
                            bypassed: slot.bypassed,
                            mix: slot.config.mix(),
                            mix_automated: automated.contains(
                                &fontelle_types::ParamTarget::Insert {
                                    track: id,
                                    slot: index,
                                    param: fontelle_types::MIX.into(),
                                }
                                .address(),
                            ),
                        })
                        .collect(),
                    sends: track
                        .sends
                        .iter()
                        .map(|send| {
                            // A send at a track that is not in the list can
                            // only be a document that got past the commands,
                            // and the panel says so rather than pointing at
                            // whichever strip happens to be first.
                            let target = ids.iter().position(|id| *id == send.target);
                            fontelle_ui::document::SendInfo {
                                target: target.unwrap_or(usize::MAX),
                                target_name: target
                                    .and_then(|index| ids.get(index))
                                    .and_then(|id| self.project.mixer.tracks.get(*id))
                                    .map(|track| track.name.clone())
                                    .unwrap_or_else(|| "\u{2014}".to_string()),
                                level_db: send.level_db,
                                pre_fader: send.pre_fader,
                            }
                        })
                        .collect(),
                })
            })
            .collect()
    }

    fn add_send(&mut self, strip: usize, target: usize) {
        let ids = self.mixer_track_ids();
        let (Some(id), Some(target)) = (ids.get(strip).copied(), ids.get(target).copied()) else {
            return;
        };
        // A loop is refused by the command and reported by `run`, which is
        // what puts the message in front of the user (§13.2).
        self.run(Box::new(fontelle_model::AddSend::new(id, target)));
        self.history.break_gesture();
        // A send is a node in the schedule, so this is the graph's shape
        // changing rather than a value in it.
        self.rebuild_graph();
    }

    fn remove_send(&mut self, strip: usize, index: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        // Guarded here rather than left to the command: a panel and a document
        // disagree for a frame every time something is deleted, and a stale
        // index arriving from a click is ordinary rather than worth a message.
        if index >= self.sends_of(id) {
            return;
        }
        self.run(Box::new(fontelle_model::RemoveSend::new(id, index)));
        self.history.break_gesture();
        self.rebuild_graph();
    }

    fn set_send_level(&mut self, strip: usize, index: usize, level_db: f32) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        if index >= self.sends_of(id) {
            return;
        }
        // Both, like a fader: the command for undo and for the file, and the
        // atomic for the sound between now and the next rebuild.
        self.run(Box::new(fontelle_model::SetSendLevel::new(
            id, index, level_db,
        )));
        if let Some(live) = self.send_controls.get(&(id, index)) {
            live.set_level_db(level_db);
        }
        self.revision += 1;
    }

    fn toggle_send_pre_fader(&mut self, strip: usize, index: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        let Some(pre) = self
            .project
            .mixer
            .tracks
            .get(id)
            .and_then(|track| track.sends.get(index))
            .map(|send| send.pre_fader)
        else {
            return;
        };
        self.run(Box::new(fontelle_model::SetSendPreFader::new(
            id, index, !pre,
        )));
        self.history.break_gesture();
        // Where the tap is taken is where the node sits in the schedule.
        self.rebuild_graph();
    }

    fn live_keys(&self) -> u128 {
        self.live_keys.as_ref().map_or(0, |keys| keys.snapshot())
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
        // The words come from the panel that asked; the document knows only
        // the address. Recorded first, so the block is captioned whichever
        // branch below is taken.
        self.automation_names
            .insert(address.clone(), label.to_string());

        // **Reused when this parameter already has a clip.** A right-click on
        // a control that already has one means "show me that one", not "give
        // me a second on top of the first" — two clips for one parameter over
        // the same bars is two curves fighting over one value, and the RT
        // side would apply whichever was compiled last. The one covering the
        // playhead wins where there are several.
        let mut fallback = None;
        let mut covering = None;
        for (id, clip) in self.project.clips.iter() {
            let ClipSource::Automation(data) = &clip.source else {
                continue;
            };
            if data.target != *address {
                continue;
            }
            fallback.get_or_insert(id);
            if clip.start <= at && at < clip.start + clip.length {
                covering = Some(id);
                break;
            }
        }
        if let Some(existing) = covering.or(fallback) {
            self.automation_clip = Some(existing);
            self.revision += 1;
            return;
        }

        // **Over the time selection, or the whole song**, on a lane of its
        // own. *"it creates a new automation clip in my arrangement just flat
        // on the value that its currently at basically with the clip
        // extending the current length of the song or time selection."* A
        // one-bar clip at the playhead — which is what this used to make — is
        // a clip you have to stretch before you can draw anything worth
        // hearing in it.
        //
        // The lane is its own for the reason it always was: §12.4 says "the
        // current lane", and that reading put the curve on top of the notes,
        // which is one of them drawn over the other.
        let (start, length) = match self.project.loop_range {
            Some((from, to)) if to > from => (from, to - from),
            _ => (0, self.song_ticks()),
        };
        let length = length.max(1);
        let lane = self.automation_lane(address, label);

        // Two points at the value the control is at now, one at each end, so
        // the clip starts by changing nothing: an automation lane that jumped
        // the parameter the moment it was created would be a lane nobody
        // trusts.
        let value = self.parameter_now(address).unwrap_or(0.5);
        let mut points = fontelle_model::Arena::default();
        for tick in [0, length] {
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
            length,
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
            // It is the block in hand: you made it to draw in it, and the
            // drawing happens where it sits (§12.4).
            self.automation_clip = Some(id);
        }
        self.history.break_gesture();
        self.republish();
        self.revision += 1;
    }

    fn loop_range(&self) -> Option<(Tick, Tick)> {
        self.project.loop_range
    }

    fn set_loop_range(&mut self, range: Option<(Tick, Tick)>) {
        self.run(Box::new(fontelle_model::SetLoopRange::new(range)));
        self.history.break_gesture();
        // `run` republished, which published the loop — but only if the
        // command was accepted, and this costs two atomic stores either way.
        self.publish_loop();
        self.revision += 1;
    }

    fn play_mode(&self) -> PlayMode {
        self.play_mode
    }

    fn set_play_mode(&mut self, mode: PlayMode) {
        if self.play_mode == mode {
            return;
        }
        self.play_mode = mode;
        // Both halves, and `republish` is both: the timeline is recompiled to
        // carry one clip or all of them, and the transport is handed the
        // stretch to loop.
        self.republish();
        self.revision += 1;
    }

    fn focused_clip_span(&self) -> Option<(Tick, Tick)> {
        self.clip_span()
    }

    fn is_automated(&self, address: &fontelle_types::ParamAddress) -> bool {
        fontelle_model::automated_targets(&self.project).contains(address)
    }

    fn insert_view(&self, strip: usize, slot: usize) -> Option<InstrumentView> {
        let config = self.insert_config(strip, slot)?;
        // The EQ draws itself. See the trait's docs.
        if matches!(config, fontelle_types::EffectConfig::Eq(_)) {
            return None;
        }
        let id = self.mixer_track_ids().get(strip).copied()?;
        let name = self
            .project
            .mixer
            .tracks
            .get(id)
            .map_or_else(String::new, |track| track.name.clone());
        // `effect_view` was written for this and had no window to be drawn in:
        // it turns an effect's own `specs()` into a panel, so an effect added
        // later gets one without anybody writing it. Its addresses are the
        // full automation addresses, which is what makes right-clicking a knob
        // in it need no translation at all.
        // And the strips its detector could be pointed at, if it has one: the
        // window offers the key, the document validates it and the compiler
        // orders it. Every strip including this one — a track keying an
        // insert on itself is refused by the command with a message that says
        // what "no key" already means, which is more use than a name missing
        // from a list.
        let strips: Vec<String> = self
            .mixer_track_ids()
            .iter()
            .map(|id| {
                self.project
                    .mixer
                    .tracks
                    .get(*id)
                    .map_or_else(String::new, |track| track.name.clone())
            })
            .collect();
        let mut view =
            fontelle_ui::canvas::effect_view(&name, slot, id, &config, &strips, self.insert_key(strip, slot));
        // Built once for the whole panel: `automated_targets` walks every
        // clip in the project, and a closure that called it per knob would
        // walk them forty-nine times for an EQ.
        let automated = fontelle_model::automated_targets(&self.project);
        view.mark_automated(|address| automated.contains(address));
        Some(view)
    }

    fn set_insert_param(&mut self, strip: usize, slot: usize, param: &str, value: f32) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        // The panel hands back the **full** automation address it was built
        // with; the command names the parameter inside the effect. This is the
        // one place the two halves of §8.2's scheme meet, and it takes either.
        let param = match fontelle_types::ParamTarget::parse(&fontelle_types::ParamAddress::new(
            param,
        )) {
            Some(fontelle_types::ParamTarget::Insert { param, .. }) => param,
            _ => param.to_string(),
        };
        let param = param.as_str();
        // No `break_gesture`: `SetInsertParam::merge_with` folds a whole drag
        // into one entry, and the window ends it on mouse-up like every other
        // knob.
        self.run(Box::new(fontelle_model::SetInsertParam::new(
            id, slot, param, value,
        )));
        // Straight to the running graph, not through a rebuild: an effect knob
        // is something somebody drags, and rebuilding deserialises every
        // channel's patch. Same path `set_eq_band` takes.
        if let Some(config) = self.insert_config(strip, slot)
            && let Some(controls) = self.effect_controls.get_mut(&(id, slot))
        {
            controls.publish(config);
        }
        self.dirty = true;
        self.revision += 1;
    }

    fn set_insert_preset(&mut self, strip: usize, slot: usize, preset: usize) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        // One entry on the history, whatever the preset moved — see
        // `fontelle_model::SetInsertPreset`.
        self.run(Box::new(fontelle_model::SetInsertPreset::new(
            id, slot, preset,
        )));
        // Straight to the running graph, the same path a knob takes.
        if let Some(config) = self.insert_config(strip, slot)
            && let Some(controls) = self.effect_controls.get_mut(&(id, slot))
        {
            controls.publish(config);
        }
        self.dirty = true;
        self.revision += 1;
    }

    fn set_insert_key(&mut self, strip: usize, slot: usize, key: Option<usize>) {
        let ids = self.mixer_track_ids();
        let Some(id) = ids.get(strip).copied() else {
            return;
        };
        // `None` is "listen to yourself", which is what an insert does with no
        // key; a strip index nobody has is the same answer rather than a
        // guess at the nearest one.
        let key = match key {
            Some(index) => match ids.get(index).copied() {
                Some(target) => Some(target),
                None => return,
            },
            None => None,
        };
        // A key is a routing edge, so this **rebuilds** the graph rather than
        // publishing to the live end: what changed is the order the schedule
        // runs in and which node fills which tap, neither of which a control
        // surface can carry. The same thing adding a send does.
        self.run(Box::new(fontelle_model::SetInsertKey::new(id, slot, key)));
        self.rebuild_graph();
        self.dirty = true;
        self.revision += 1;
    }

    fn insert_key(&self, strip: usize, slot: usize) -> Option<usize> {
        let ids = self.mixer_track_ids();
        let id = ids.get(strip).copied()?;
        let key = self
            .project
            .mixer
            .tracks
            .get(id)?
            .inserts
            .get(slot)?
            .effective_key()?;
        ids.iter().position(|other| *other == key)
    }

    fn spectrum(&mut self, strip: usize, slot: usize) -> Vec<f32> {
        use fontelle_ui::canvas::{SPECTRUM_BANDS, SPECTRUM_BOTTOM_DB, spectrum_band_hz};

        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return Vec::new();
        };
        let Some(tap) = self.spectrum_taps.get(&(id, slot)) else {
            return Vec::new();
        };
        // Nothing has gone through it: an offline session, or a graph built a
        // moment ago. An empty spectrum draws nothing, which is honest.
        if tap.frames_written() == 0 {
            return Vec::new();
        }
        tap.read(&mut self.spectrum_scratch);
        let magnitudes = self.analyser.analyse(&self.spectrum_scratch);

        // The transform's bins are **linear** in frequency and the picture is
        // logarithmic, so the bottom of the plot has a handful of bins spread
        // across a third of the width and the top has hundreds crammed into
        // the last inch. Each band takes the **loudest** bin it covers rather
        // than the average: an analyser is read for where the peaks are, and
        // averaging fifty bins in the top octave buries every one of them in
        // the quiet between.
        let bin_hz = fontelle_dsp::bin_width_hz(self.options.sample_rate as f32);
        (0..SPECTRUM_BANDS)
            .map(|band| {
                let (low, high) = spectrum_band_hz(band);
                let first = (low / bin_hz).floor().max(1.0) as usize;
                let last = (high / bin_hz).ceil() as usize;
                // A band narrower than one bin still reads one — the bottom
                // three octaves, where a 23 Hz bin is wider than the band.
                let last = last.max(first + 1).min(magnitudes.len());
                magnitudes
                    .get(first..last)
                    .map(|bins| bins.iter().cloned().fold(SPECTRUM_BOTTOM_DB, f32::max))
                    .unwrap_or(SPECTRUM_BOTTOM_DB)
            })
            .collect()
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
        // And the window, which re-reads its lists only when this moves. Left
        // out, the sound changed and the curve did not — see
        // `tests/eq_editor.rs`, which is the report this line answers.
        self.revision += 1;
    }

    fn set_insert_mix(&mut self, strip: usize, slot: usize, mix: f32) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::SetInsertMix::new(id, slot, mix)));
        // Both ends, exactly as a band does and a fader does: the command for
        // undo and for the file, the live channel for the sound between now
        // and the next rebuild.
        if let Some(config) = self.insert_config(strip, slot)
            && let Some(controls) = self.effect_controls.get_mut(&(id, slot))
        {
            controls.publish(config);
        }
        self.revision += 1;
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
        // **Through `lane_ids`**, which is the order the arrangement stacks
        // them — not the arena's. `clips()` below already indexes rows by
        // position in that list, so a row list built any other way would draw
        // every clip against the wrong row the moment somebody reordered one.
        // That is precisely the two-lists-that-must-agree defect this codebase
        // keeps finding, and it was live for exactly as long as it took a test
        // to move a row.
        self.lane_ids()
            .into_iter()
            .filter_map(|id| self.project.lanes.get(id))
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
            .map(|(id, clip)| {
                let mut audio = fontelle_ui::document::AudioPreview::default();
                let (kind, name, curve, notes) = match &clip.source {
                    // A note block is captioned with the channel it plays, not
                    // with a clip name — a clip has none, and "what instrument
                    // is this" is what somebody scanning an arrangement asks.
                    ClipSource::Notes(data) => {
                        // The clip's own notes, in time order and **not**
                        // expanded across a loop's passes — the canvas tiles
                        // them the way it tiles the seams. See
                        // `fontelle_ui::document::ClipInfo::notes`.
                        let mut notes: Vec<fontelle_ui::document::NotePreview> = data
                            .notes
                            .values()
                            .map(|note| fontelle_ui::document::NotePreview {
                                start: note.start,
                                length: note.length,
                                key: note.key,
                            })
                            .collect();
                        notes.sort_by_key(|note| (note.start, note.key));
                        (
                            ClipKind::Notes,
                            self.channel_of_clip(id)
                                .and_then(|channel| self.project.channels.get(channel))
                                .map(|channel| channel.name.clone())
                                .unwrap_or_else(|| "Clip".to_string()),
                            Vec::new(),
                            notes,
                        )
                    }
                    // And an automation block with the parameter it moves,
                    // which is the same question asked of the other kind.
                    ClipSource::Automation(data) => {
                        let mut points: Vec<CurvePoint> = data
                            .points
                            .iter()
                            .map(|(id, point)| CurvePoint {
                                id,
                                tick: point.tick,
                                value: point.value,
                                curve: point.curve,
                            })
                            .collect();
                        // In time order, so the canvas draws a polyline —
                        // and evaluates a curve — without having to sort a
                        // copy every frame.
                        points.sort_by_key(|point| point.tick);
                        (
                            ClipKind::Automation,
                            self.automation_name(&data.target),
                            points,
                            Vec::new(),
                        )
                    }
                    // A take or a loop, captioned with the file it came
                    // from — the same question the other two answer: what is
                    // this, at a glance, without opening it.
                    ClipSource::Audio(data) => {
                        audio = self.audio_preview(data);
                        (
                            ClipKind::Audio,
                            data.asset
                                .path
                                .file_stem()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Audio".to_string()),
                            Vec::new(),
                            Vec::new(),
                        )
                    }
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
                    notes,
                    audio,
                }
            })
            .collect()
    }


    fn arrange(&mut self, edit: ArrangeEdit) -> Created {
        let lanes = self.lane_ids();
        let mut points: Vec<fontelle_types::PointId> = Vec::new();
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
                    return Created { clips: created, points };
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
                    return Created { clips: created, points };
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
            ArrangeEdit::Split { cuts } => {
                // One command per clip, and **one gesture**: a line drawn
                // across four rows is one thing somebody did, so `break_gesture`
                // comes after the lot rather than between them.
                //
                // Deliberately no `created` here: the halves a cut makes are
                // not a new selection. `clips_inserted` would make them one,
                // and a cut that left you holding four clips you did not
                // choose is a cut you have to click somewhere to escape.
                for (id, at) in cuts {
                    self.run(Box::new(fontelle_model::SplitClip::new(id, at)));
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
                return Created { clips: created, points };
            }
            ArrangeEdit::Paste { at } => {
                if self.clip_clipboard.is_empty() {
                    return Created { clips: created, points };
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
            // --- the points of an automation block, edited where it sits ---
            ArrangeEdit::AddPoint { clip, tick, value } => {
                let point = fontelle_model::AutomationPoint {
                    tick,
                    value,
                    curve: fontelle_model::CurveShape::Linear,
                    tension: 0.0,
                };
                if let Ok(command) = self.apply_for::<fontelle_model::AddAutomationPoint>(
                    Box::new(fontelle_model::AddAutomationPoint::new(clip, point)),
                ) {
                    // The point the click just made is the one the drag that
                    // follows moves — the same handshake drawing a note has.
                    points.extend(command.id());
                }
                self.history.break_gesture();
                self.republish();
            }
            ArrangeEdit::MovePoints {
                clip,
                ids,
                tick_delta,
                value_delta,
            } => {
                // No `break_gesture`: a drag is sixty of these and one entry
                // on the history, and only the window knows the mouse came up.
                self.run(Box::new(fontelle_model::MoveAutomationPoints::new(
                    clip,
                    ids,
                    tick_delta,
                    value_delta,
                )));
            }
            ArrangeEdit::RemovePoints { clip, ids } => {
                self.run(Box::new(fontelle_model::RemoveAutomationPoints::new(
                    clip, ids,
                )));
                self.history.break_gesture();
            }
            ArrangeEdit::SetPointCurve { clip, ids, curve } => {
                self.run(Box::new(fontelle_model::SetPointCurve::new(
                    clip, ids, curve,
                )));
                self.history.break_gesture();
            }
        }
        self.revision += 1;
        Created {
            clips: created,
            points,
        }
    }

    fn open_clip(&mut self, clip: ClipId) {
        let Some(open) = self.project.clips.get(clip) else {
            return;
        };
        // A block opens what is in it, whichever kind it is — the same
        // handshake a note clip has always had, for the other kind of clip.
        if let ClipSource::Automation(_) = &open.source {
            // An automation block is edited where it sits, so opening one is
            // only a matter of saying which block is in hand.
            self.automation_clip = Some(clip);
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
        self.song_ticks()
    }

    fn playhead_song_tick(&self, position_sample: Sample) -> Tick {
        self.effective_tempo.sample_to_tick(position_sample.max(0))
    }

    fn sample_of_song_tick(&self, tick: Tick) -> Sample {
        self.effective_tempo.tick_to_sample(tick.max(0))
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

    fn move_lane(&mut self, lane: usize, delta: isize) {
        self.run(Box::new(fontelle_model::MoveLane::new(lane, delta)));
        self.history.break_gesture();
        self.dirty = true;
        // The arrangement re-reads its rows only when this moves — a reorder
        // that forgot it would be a document that had changed and a window
        // that had not.
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
        // And where the collection's presets arrive while it is being read —
        // see [`PresetIndex`]. The revision moves so the list redraws with
        // what has landed, which is what makes a long scan look like a list
        // filling in rather than like a window that has stopped.
        if self.preset_index.drain() {
            self.revision += 1;
        }
    }
}

/// One preset, somewhere in the collection — a row of the search index.
///
/// The *name* is what is searched and the path is what is opened. Kept flat
/// rather than as a map from file to presets, because the ordering the panel
/// wants (the open soundfont first, then everything else) is decided per
/// search and not once.
#[derive(Debug, Clone)]
pub struct PresetHit {
    pub file: PathBuf,
    /// What the browser calls the soundfont — its file stem.
    pub file_name: String,
    /// Which preset inside that file, as [`fontelle_assets::list_presets`]
    /// numbers them.
    pub index: usize,
    pub name: String,
    pub bank: u16,
    pub program: u16,
}

/// The collection's presets, read on a thread of its own.
///
/// Asked for from using the browser: *"we are not able to search for sounds
/// from within ALL of our soundfonts!"*.
///
/// **Why a thread.** Listing a soundfont's presets means reading and parsing
/// the whole file — `fontelle_assets::list_presets` does — and a collection is
/// hundreds of megabytes across dozens of files. Doing that on the UI thread
/// the first time somebody types a letter would freeze the window for seconds,
/// which is a worse feature than no feature. So the scan runs behind a channel
/// and the results arrive in [`Session::pump`], the same place the graph's
/// leavings are freed; the list fills in as they land.
#[derive(Default)]
struct PresetIndex {
    hits: Vec<PresetHit>,
    /// The receiving end while a scan is running. `None` when there is no scan
    /// — either because none has been asked for, or because the last one
    /// finished.
    incoming: Option<std::sync::mpsc::Receiver<Vec<PresetHit>>>,
    /// Whether what `hits` holds describes the bank as it is now. Cleared when
    /// the collection changes under it.
    ready: bool,
    /// How many files have been read, and how many there are — for the status
    /// line, which otherwise looks like a search that found nothing.
    scanned: usize,
    total: usize,
}

impl PresetIndex {
    /// Starts a scan of `files`, throwing away anything an earlier one found.
    fn start(&mut self, files: Vec<PathBuf>) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.hits.clear();
        self.ready = false;
        self.scanned = 0;
        self.total = files.len();
        self.incoming = Some(rx);
        std::thread::spawn(move || {
            for path in files {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                // A file that will not parse is not an error here: the browser
                // already says so where it lists it, and one bad soundfont
                // must not stop the other sixty being searchable.
                let presets = fontelle_assets::list_presets(&path).unwrap_or_default();
                let hits = presets
                    .into_iter()
                    .map(|preset| PresetHit {
                        file: path.clone(),
                        file_name: name.clone(),
                        index: preset.index,
                        name: preset.name,
                        bank: preset.bank,
                        program: preset.program,
                    })
                    .collect();
                // A send that fails is a window that has closed. Stop.
                if tx.send(hits).is_err() {
                    return;
                }
            }
        });
    }

    /// Takes whatever the scan has finished since the last call. Returns
    /// whether anything arrived, which is what decides a redraw.
    fn drain(&mut self) -> bool {
        let Some(rx) = &self.incoming else {
            return false;
        };
        let mut moved = false;
        loop {
            match rx.try_recv() {
                Ok(hits) => {
                    self.hits.extend(hits);
                    self.scanned += 1;
                    moved = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                // The worker is done and gone.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.incoming = None;
                    self.ready = true;
                    moved = true;
                    break;
                }
            }
        }
        moved
    }

    fn running(&self) -> bool {
        self.incoming.is_some()
    }
}

/// One row of the browser's preset list.
enum PresetRow {
    /// A heading naming the soundfont the rows under it came from. Not
    /// something to click.
    Group { name: String, detail: String },
    /// A preset: which file it is in, and which preset of that file.
    Preset {
        file: PathBuf,
        index: usize,
        name: String,
        detail: String,
    },
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

/// The `order` a row added now should carry: past the bottom of the stack.
///
/// The same rule `AddLane` follows, and it has to be the same rule: a row
/// created straight on the arena rather than through the command — an
/// automation lane, a channel's own lane — must still turn up where somebody
/// adding a row is looking for it, and not wherever a default of zero sorts
/// once the rows have been reordered.
fn next_lane_order(project: &fontelle_model::Project) -> u32 {
    project
        .lanes
        .values()
        .map(|lane| lane.order)
        .max()
        .map_or(0, |highest| highest.saturating_add(1))
}

/// A `.mid` file that holds more than one part, waiting on the question
/// *"import them all as separate tracks, or only one of them?"*.
///
/// The survey is kept rather than the parts themselves: a survey builds
/// nothing, so a question that is never answered has cost a file read and no
/// document at all.
pub struct PendingImport {
    pub path: PathBuf,
    pub survey: fontelle_assets::MidiSurvey,
}

/// Enough distinct row colours that an imported file does not arrive as
/// sixteen identical rows.
const IMPORT_COLOURS: [[u8; 4]; 8] = [
    [0x4f, 0x8f, 0xd0, 0xff],
    [0xd0, 0x7f, 0x4f, 0xff],
    [0x6f, 0xc0, 0x7f, 0xff],
    [0xc0, 0x6f, 0xb0, 0xff],
    [0xd0, 0xc0, 0x5f, 0xff],
    [0x5f, 0xc0, 0xc0, 0xff],
    [0x9f, 0x8f, 0xd0, 0xff],
    [0xa0, 0xa0, 0xa0, 0xff],
];

/// A file's name without its extension — what an import calls itself.
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
