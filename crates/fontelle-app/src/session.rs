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

use fontelle_assets::{MidiChannels, import_fsc, import_midi, survey_midi};
use fontelle_engine::{GraphPublisher, TimelinePublisher};
use fontelle_model::{
    AddChannel, AddClip, AddNotes, Arena, Clip, ClipSource, Command, DuplicateClip, FlagTarget,
    History, ImportPart, ImportParts, Lane, MoveClip, MoveNotes, Note, NoteData, NumberTarget,
    Project, RemoveClip, RemoveNotes, ResizeClip, ResizeNotes, SetFlag, SetNoteProperty, SetNumber,
};
use fontelle_types::{
    ChannelId, ClipId, EventPayload, LaneId, MixerTrackId, NodeId, NoteId, PPQN, Sample, Tick,
    TimedEvent,
};
use fontelle_ui::canvas::{ArrangeEdit, InstrumentView, PresetDevice, RollEdit};
use fontelle_ui::document::{
    ChannelInfo, ClipInfo, ClipKind, Created, CurvePoint, DocumentHost, GhostFilter, GhostNote,
    LaneInfo, LibraryEntry, MixerStrip, PlayMode, RecentProject, StudioHost, UpdateStatus,
};

use crate::bank::{BankFilter, BankRow, FileBank, SoundfontBank, matches_names};
use crate::library::SampleLibrary;
use crate::projects::ProjectLibrary;
use crate::realise::{RealiseOptions, apply_mixer_controls, apply_send_controls, realise};
use crate::settings::Settings;
use crate::updates::Updater;

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

/// How long an input that would not open waits before it is tried again.
///
/// Long enough that a device that is really gone is not probed at frame
/// rate (`pw-dump` on PipeWire, a walk of every ALSA card without it); short
/// enough that one that was busy, suspended, or plugged in a moment after
/// the project opened starts working without anybody touching the menu.
pub const INPUT_RETRY: std::time::Duration = std::time::Duration::from_secs(3);

/// An input that would not open, and when it was last tried. See
/// [`Session::sync_audio_input`].
#[derive(Debug, Clone)]
pub struct InputFailure {
    pub name: String,
    pub tried: std::time::Instant,
}

impl InputFailure {
    /// Whether it is time to try the device again.
    pub fn due(&self, now: std::time::Instant, after: std::time::Duration) -> bool {
        now.duration_since(self.tried) >= after
    }
}

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
    /// What the record button records (TDD §14.7, §15.4) — see
    /// `fontelle_ui::transport::RecordMode`. Window state rather than
    /// document state: which kind of thing you are about to capture is no more
    /// part of a song than which tool is selected is.
    record_mode: fontelle_ui::transport::RecordMode,
    /// The audio-input ring, while a capture stream is open, and what the
    /// device opened at. `None` is a session with no microphone attached,
    /// which is nearly all of them.
    input: Option<fontelle_engine::InputReader>,
    /// The take as it accumulates, drained off the ring by `pump`.
    input_take: fontelle_engine::InputCapture,
    input_rate: u32,
    /// The device the capture stream belongs to. Kept because dropping it
    /// closes the stream, and a stream closed the moment it was opened is a
    /// microphone that records nothing with no error anywhere.
    input_device: Option<fontelle_engine::AudioDevice>,
    /// Which input is open, and for which track — so `sync_audio_input` can
    /// tell "nothing changed" from "a different microphone" without asking the
    /// operating system once a frame.
    input_open: Option<(MixerTrackId, String)>,
    /// The last input that would not open, and when it was tried — so it is
    /// not retried on every frame, and *is* retried after
    /// [`INPUT_RETRY`], because a device that is busy, suspended or not
    /// plugged in yet is not a device that is gone. Cleared the moment the
    /// answer to "which input" changes, and by [`adopt`](Self::adopt).
    input_failed: Option<InputFailure>,
    /// How long a failed input waits before it is tried again. The constant,
    /// except in a test that cannot wait three seconds.
    input_retry: std::time::Duration,
    /// Whether what the ring delivers is being **kept**.
    ///
    /// The stream is open whenever a track names an input, because that is
    /// what monitoring is — but a take only accumulates while record is
    /// armed. Without this a microphone left plugged in would grow a take for
    /// as long as the window stayed open.
    capturing: bool,
    /// The ring that carries the input into the graph, and the node that plays
    /// it (TDD §15.4). `None` in every offline session — a bounce and a test
    /// have no microphone and no speakers.
    monitor: Option<std::sync::Arc<fontelle_engine::InputMonitor>>,
    /// The live end of every mixer track's fader and meter, as the graph that
    /// is currently playing sees them.
    ///
    /// Replaced wholesale by [`Session::rebuild_graph`], because the graph that
    /// owned the old set has been handed back to be freed. Between rebuilds
    /// this is how a fader is *heard* — see `fontelle_engine::TrackControls`
    /// for why writing the document alone is not enough.
    track_controls: HashMap<MixerTrackId, std::sync::Arc<fontelle_engine::TrackControls>>,
    /// One voice meter per channel — how many notes each instrument is
    /// actually playing (`docs/flopsynth-plan.md` §11, phase 6).
    ///
    /// Re-taken on every rebuild, like `track_controls`: a meter belongs to
    /// the node that writes it, and one left over from a graph that no longer
    /// exists would report a count that never moves.
    voice_meters: HashMap<ChannelId, std::sync::Arc<fontelle_engine::VoiceMeter>>,
    /// The live end of every insert in the running graph, keyed as the panel
    /// addresses one — see [`crate::Realised::effect_controls`]. Replaced
    /// wholesale on a rebuild, because a triple buffer's two ends cannot be
    /// re-paired; the document is the source of truth either way.
    effect_controls: HashMap<(MixerTrackId, usize), fontelle_engine::EffectControls>,
    /// One analyser tap per insert. **Kept across a rebuild** — see
    /// [`crate::Realised::spectrum_taps`].
    spectrum_taps: HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::SpectrumTap>>,
    /// The pitch traces the corrector's windows read, kept across a rebuild
    /// for the same reason (`docs/tune-plan.md` §7.3).
    tune_taps: HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::TuneTap>>,
    /// Every plugin somebody else wrote that this session has open (TDD §8.4).
    ///
    /// **Kept across a rebuild**, and that is the whole reason it is a field
    /// rather than something `realise` makes: a plugin may be activated once,
    /// and the graph is rebuilt whenever anything structural changes. See
    /// [`crate::PluginRack`].
    plugins: crate::PluginRack,
    /// Whether the plugins a project names have been hosted since it was
    /// opened. The window's first graph is built before this session exists
    /// and without a rack, so a project that names a plugin has to be
    /// rebuilt once, on the first pump — after every builder has run and the
    /// settings' plugin folders are known. See [`Session::pump`].
    plugins_hosted: bool,
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
    /// The prefab picked out of the prefab list, when the roll is editing one
    /// rather than a clip on the arrangement (TDD §10.5).
    ///
    /// > *"you could also edit it just by selecting the prefab in the prefab
    /// > menu and then selecting the instrument you want to edit in the prefab
    /// > clip and then just editing the piano roll of it."*
    ///
    /// Session state like `automation_clip` and for the same reason: where you
    /// were looking is not something a project sent to somebody else should
    /// arrive with. Read only through [`note_target`](Session::note_target).
    prefab: Option<fontelle_types::PrefabId>,
    /// Which of the left-hand panel's two lists is showing. Session state, as
    /// above.
    rack_tab: fontelle_ui::document::RackTab,
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
    /// The master meter the transport bar reads. Kept across a rebuild for
    /// the same reason — see `with_master_meter`.
    master_meter: Option<std::sync::Arc<fontelle_engine::MasterMeter>>,
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
    /// Every preset on this machine: the factory tree in the binary and the
    /// user's own folder (`docs/flopsynth-plan.md` §P.4).
    ///
    /// On the session rather than beside it because the `*` rule needs it
    /// (§P.6): whether a device is dirty is *recognised* by comparing its
    /// state against the bank's copy of the file it came from, and that
    /// comparison happens wherever a bar is drawn.
    preset_bank: crate::preset_bank::PresetBank,
    /// Which device's presets the browser's Presets tab is showing (§P.8).
    ///
    /// The tab is the Sounds tab's shape with a different list in it: devices
    /// above, that device's presets below. This is the "open file".
    preset_device_open: Option<fontelle_types::DeviceKind>,
    /// Which insert has a window open, as the *window* knows it.
    ///
    /// Told rather than worked out, because which one is open is a fact about
    /// the window and not about the document — and the browser needs it: an
    /// effect preset clicked in the Presets tab lands in the insert you are
    /// looking at, and is refused when you are not looking at one (§P.8).
    open_insert: Option<(usize, usize)>,
    /// Where the settings are read from and written back to. `None` is the
    /// user's own config directory; a path is how a test keeps its hands off
    /// it, which is not a nicety — the first run of this crate's own studio
    /// tests wrote a soundfont folder in `/tmp` into the developer's real
    /// `~/.config/fontelle/settings.json`.
    settings_path: Option<PathBuf>,
    /// The last reversible settings action, kept so a toast's "Undo" can put it
    /// back: which plugin folder was removed, and from where in the list.
    settings_undo: Option<(usize, PathBuf)>,
    /// A note the last settings press left for the window to show as a transient
    /// banner, and whether it can be undone. Taken by `take_settings_toast`.
    settings_toast: Option<(String, bool)>,
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
    /// The start menu's update check (`updates.rs`). Built switched off, and
    /// replaced through [`Session::with_updater`] by the one the launch
    /// decides on, so a session made for a test never touches the network.
    updater: Updater,
    query: String,
    /// Which soundfont is open, and what is inside it.
    ///
    /// The **path**, not a row number. The list under it moves — a search, a
    /// folder change, a rescan — and an index would name whatever happened to
    /// land in that slot afterwards. The row to highlight is worked out from
    /// the path when the panel asks (see `selected_file`).
    open_file: Option<PathBuf>,
    /// Whether the browser is looking inside **Flopsynth's factory bank**
    /// rather than inside a soundfont.
    ///
    /// The bank is a row in the Sounds tab and its presets are the rows under
    /// it, so a hundred and twenty-eight of them arrive through the list that
    /// already has a search, a virtualised draw, a click that puts one on the
    /// selected channel and a Ctrl+click that puts one on a new one. A second
    /// list of presets, eleven rows tall and hanging off the instrument panel,
    /// would be worse at every one of those — which is the argument
    /// `BrowserMode::Import` already makes for itself.
    flopsynth_open: bool,
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
    /// The **preview voice**: a sampler on the master that is not in the
    /// document — see `realise::Realised::preview_node`.
    preview_node: NodeId,
    /// What is loaded into it, if anything. Held because a graph rebuild has
    /// to put it back: the node is minted fresh every time.
    preview_patch: Option<fontelle_core::Patch>,
    /// Whether live notes are going to the preview voice rather than to the
    /// selected channel. Set by the browser, cleared by anything that
    /// auditions the instrument you are actually working on.
    previewing: bool,
    message: Option<String>,

    /// Where an audition goes. `None` when the window was opened without a
    /// live-input channel, and then drawing a note is silent until playback
    /// reaches it.
    audition: Option<Box<dyn fontelle_types::EventSink>>,

    /// What the roll is shown: the open clip's notes on the selected
    /// channel. See `refresh_roll_notes`.
    roll_notes: Arena<NoteId, Note>,
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

/// How loud an oscillator comes on at when a sound is dropped onto it.
///
/// Under the Init patch's own first oscillator (−12 dB) rather than level
/// with it: a sound arriving at full strength on top of what is already
/// playing is a jump in level nobody asked for, and the layer's own knob is
/// right there.
const DROPPED_OSC_DB: f32 = -18.0;

impl Session {
    /// Says something in the window's status line, once.
    ///
    /// The same one-line channel every edit reports through
    /// (`StudioHost::take_message`), for something the *process* has to say
    /// rather than a command — what happened to the last run
    /// ([`crate::crashlog`]) is the case it was added for. A crash report
    /// nobody is told about is a file nobody reads.
    pub fn announce(&mut self, said: impl Into<String>) {
        self.message = Some(said.into());
    }

    /// Hands the session a capture ring **that is being recorded**, and what
    /// the device it came from opened at (TDD §15.4).
    ///
    /// For a caller that owns a stream of its own rather than letting
    /// `sync_audio_input` open one — the offline recorder, and every test that
    /// pushes samples in by hand, because `AudioDevice` is the engine's and a
    /// session has no business holding a sound card.
    ///
    /// It arms as well as attaches, which is the difference between it and the
    /// path a microphone takes: a stream opened for **monitoring** is drained
    /// and thrown away until record is pressed, and a ring handed over by hand
    /// is one whose samples somebody means to keep.
    pub fn set_audio_input(
        &mut self,
        reader: fontelle_engine::InputReader,
        sample_rate: u32,
        channels: u16,
    ) {
        self.attach_audio_input(reader, sample_rate, channels);
        self.capturing = true;
        // Being handed a ring **is** the stream being open, so `input_open`
        // records which track and which device it belongs to. Without that,
        // `sync_audio_input` would see a track asking for an input it believed
        // nothing had opened and go looking for a sound card — which in a test
        // means throwing this ring away and finding no microphone.
        self.input_open = self.armed_track().zip(self.audio_input_wanted());
    }

    /// The same, without arming: what `sync_audio_input` uses when it opens a
    /// device because a track names one.
    fn attach_audio_input(
        &mut self,
        reader: fontelle_engine::InputReader,
        sample_rate: u32,
        channels: u16,
    ) {
        self.input = Some(reader);
        self.input_rate = sample_rate;
        self.input_take = fontelle_engine::InputCapture::new(channels);
    }

    /// Closes the capture. Whatever had been captured is thrown away — a take
    /// nobody kept is a take nobody wanted.
    pub fn clear_audio_input(&mut self) {
        self.input = None;
        self.input_take.clear();
        self.input_device = None;
        self.input_open = None;
        self.capturing = false;
        // The device closes the ring when it goes (`AudioDevice::drop`); a
        // ring handed in by hand (`set_audio_input`) has no device to do it,
        // and a ring left saying "a stream is open" keeps the graph awake for
        // a microphone nobody chose.
        if let Some(monitor) = &self.monitor {
            monitor.close();
        }
    }

    /// Opens, closes or re-points the input stream so that it matches what the
    /// document asks for (TDD §15.4).
    ///
    /// > *"i should be able to hear routed input playing even when song isnt
    /// > playing or im not recording."*
    ///
    /// Which means the device is open because a **track names an input**, not
    /// because record was pressed — the same rule FL follows, and the reason
    /// arming does not have to open anything. Called wherever that answer can
    /// change: choosing an input, and once a frame from [`Session::pump`], so
    /// that opening a project with an armed strip in it starts monitoring
    /// without anybody pressing anything.
    ///
    /// Cheap when nothing has changed, which is nearly always: two comparisons
    /// against `input_open`. A device that refuses to open is remembered in
    /// `input_failed` rather than retried sixty times a second — and tried
    /// again after [`INPUT_RETRY`], because:
    ///
    /// > *"when opening a project that has a track with an input set, you
    /// > have to change the input then change it back for it to actually
    /// > start capturing the sound"*
    ///
    /// which is what a memo with no expiry does to a device that was busy or
    /// suspended for the one moment the project opened. Choosing another
    /// input and choosing back was the only thing that cleared it.
    fn sync_audio_input(&mut self) {
        let wanted = self.armed_track().zip(self.audio_input_wanted());
        // The memo is keyed on the **name that failed**, not on what is open:
        // a failed open leaves nothing open, so comparing against that would
        // clear the memo on the very next frame and retry the same dead device
        // sixty times a second.
        if self.input_failed.as_ref().is_some_and(|failed| {
            Some(failed.name.as_str()) != wanted.as_ref().map(|(_, name)| name.as_str())
        }) {
            self.input_failed = None;
        }
        if wanted == self.input_open {
            return;
        }
        let track_changed =
            wanted.as_ref().map(|(id, _)| *id) != self.input_open.as_ref().map(|(id, _)| *id);
        let Some((track, name)) = wanted else {
            self.clear_audio_input();
            // The monitor node moves with the armed track, so the schedule
            // changes shape.
            self.rebuild_graph();
            return;
        };
        // The same device on a different strip: nothing to reopen, only the
        // node to move.
        if self
            .input_open
            .as_ref()
            .is_some_and(|(_, open)| *open == name)
        {
            self.input_open = Some((track, name));
            if track_changed {
                self.rebuild_graph();
            }
            return;
        }
        if self.input_failed.as_ref().is_some_and(|failed| {
            failed.name == name && !failed.due(std::time::Instant::now(), self.input_retry)
        }) {
            return;
        }
        // The old one first: a machine with one interface cannot open its
        // capture twice, and the failure would look like a broken input menu.
        // Dropping the device closes its stream and the monitor with it —
        // and the PipeWire stream is closed off this thread, see
        // `fontelle_engine::PipeWireInput`.
        self.input = None;
        self.input_device = None;
        self.input_open = None;

        let mut device = fontelle_engine::AudioDevice::default_host();
        if let Some(monitor) = &self.monitor {
            device = device.with_monitor(std::sync::Arc::clone(monitor));
        }
        // A second of audio at 48 kHz stereo, which is orders of magnitude
        // more than the gap between two drains. §15.4's ring, sized so a stall
        // in the window cannot cost a take.
        let (writer, reader) = fontelle_engine::input_capture_channel(96_000 * 2);
        match device.start_input_stream(Some(&name), writer) {
            Ok((rate, channels)) => {
                // Attached, not armed: an open microphone is monitoring until
                // record says otherwise.
                self.attach_audio_input(reader, rate, channels);
                // The device is kept, or the stream is dropped and closed the
                // moment this returns.
                self.input_device = Some(device);
                self.input_open = Some((track, name));
            }
            Err(e) => {
                self.message = Some(format!("could not open \u{201c}{name}\u{201d}: {e}"));
                self.input_failed = Some(InputFailure {
                    name,
                    tried: std::time::Instant::now(),
                });
            }
        }
        self.rebuild_graph();
    }

    /// Turns whatever the input has captured into an audio clip on the
    /// arrangement, and says how many frames it kept (TDD §15.4).
    ///
    /// `at` is the song sample recording started at, so the take lands where it
    /// was played rather than at the top of the song.
    ///
    /// **Written to the project's own `recordings/` folder** (§17.1), under a
    /// name nothing there has: v1 records a new clip per take, and two takes
    /// sharing a filename would be one take and a clip pointing at somebody
    /// else's audio.
    ///
    /// Nothing captured is nothing kept: no file, no clip, and not a failure —
    /// `Ok(0)`. Pressing record and stopping without playing is an ordinary
    /// thing to do. `Err` is the other thing: a take that **was** captured and
    /// could not be kept, with the reason, because the window used to report
    /// every refusal as *"nothing arrived on the input"*.
    pub fn keep_audio_take(
        &mut self,
        at: fontelle_types::Sample,
        end_sample: fontelle_types::Sample,
    ) -> Result<usize, String> {
        // Drain whatever is still in the ring first, or the last blocks of the
        // take — the end of the phrase — are the ones that get lost.
        self.pump();
        let _ = end_sample;
        let frames = self.input_take.frames();
        if frames == 0 {
            return Ok(0);
        }
        let channels = self.input_take.channels();
        let rate = if self.input_rate == 0 {
            self.options.sample_rate
        } else {
            self.input_rate
        };
        let samples = self.input_take.take();

        let path = self.take_path()?;
        let mut writer =
            fontelle_assets::WavWriter::create(&path, rate, channels).map_err(|e| e.to_string())?;
        writer
            .write(&samples)
            .and_then(|()| writer.finish())
            .map_err(|e| e.to_string())?;

        // Straight back in through the ordinary import path, so a take and a
        // dropped file are the same kind of thing from here on — one code path
        // for the waveform, the editor, the playback and the undo.
        self.import_audio_at(&path, at, self.take_track(), None)?;
        Ok(frames)
    }

    /// Where the next take goes: `<bundle>/recordings/Take N.wav`, under a name
    /// nothing in the folder has.
    ///
    /// # A take makes an unsaved project real
    ///
    /// Reported from using the window: *"when i record it was working at
    /// first until i pressed stop to finish the recording and the clip didint
    /// get made."* The studio had been started with no arguments, so there
    /// was no bundle, and the take was refused. INVARIANT 10 says Fontelle
    /// writes nowhere the user has not named — but the **projects folder** is
    /// somewhere they have named, so the project is saved into it under its
    /// own name, exactly as the Projects tab's *New* would have, and the take
    /// goes into that. Losing a first recording because nobody had pressed
    /// Ctrl+S yet is the wrong answer to a first recording.
    ///
    /// `Err` only when there is no projects folder either: then there really
    /// is nowhere, and a take dropped in a temp directory is one nobody can
    /// find and one a bundle export would miss.
    fn take_path(&mut self) -> Result<PathBuf, String> {
        if self.bundle.is_none() {
            let path = self
                .projects
                .new_project_path(&self.project.meta.name)
                .ok_or_else(|| {
                    "the take has nowhere to live \u{2014} choose a projects folder on the \
                     Projects tab, or save the project, and record again"
                        .to_string()
                })?;
            crate::save_project(&self.project, &path).map_err(|e| e.to_string())?;
            if let Some(stem) = path.file_stem() {
                self.project.meta.name = stem.to_string_lossy().into_owned();
            }
            self.bundle = Some(path);
            self.projects.rescan();
            self.dirty = false;
            self.touch();
        }
        let bundle = self.bundle.as_ref().expect("just made");
        let dir = bundle.join("recordings");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for n in 1..10_000 {
            let path = dir.join(format!("Take {n}.wav"));
            if !path.exists() {
                return Ok(path);
            }
        }
        Err("the recordings folder is full".to_string())
    }

    /// The mixer track a take is recorded through, if there is one.
    ///
    /// *"when its recording its going through that track."* **Naming an input
    /// is the arming gesture**, so this is the track that names one — the
    /// selected strip when that is the one, and otherwise the first that does.
    ///
    /// It used to be the selected strip alone, which meant that clicking
    /// another track to move its fader quietly redirected the next take, and
    /// that monitoring would follow the selection around the mixer. Selection
    /// is where you are looking; an input is a decision you made.
    ///
    /// `None` for the master, which is where a take goes when nobody has built
    /// a strip for it.
    fn armed_track(&self) -> Option<MixerTrackId> {
        let ids = self.mixer_track_ids();
        let master = self.project.mixer.master;
        let records = |id: &MixerTrackId| {
            Some(*id) != master
                && self
                    .project
                    .mixer
                    .tracks
                    .get(*id)
                    .is_some_and(|track| track.input.is_some())
        };
        ids.get(self.selected_track)
            .filter(|id| records(id))
            .or_else(|| ids.iter().find(|id| records(id)))
            .copied()
    }

    /// The audio input this project wants open, if any.
    ///
    /// *"i should be able to hear routed input playing even when song isnt
    /// playing or im not recording"* — so the device is opened because a track
    /// **names** one, not because record was pressed. `None` closes it, which
    /// is what a project with no armed track should leave a microphone doing.
    pub fn audio_input_wanted(&self) -> Option<String> {
        let id = self.armed_track()?;
        self.project.mixer.tracks.get(id)?.input.clone()
    }

    /// Where a take has to be put so that it can be heard.
    ///
    /// > *"for ease of use make it so that if the input track has no output
    /// > send it automatically will just route it to master for the clip you
    /// > record putting it on that mixer track instead of the one you recorded
    /// > on that way your recording will actually be audible after playing it
    /// > even if you werent using monitoring."*
    ///
    /// The armed track normally, and the **master** when that track's signal
    /// arrives nowhere — a clip on a strip nobody can hear is a recording that
    /// plays back silent, which reads as a take that failed.
    fn take_track(&self) -> Option<MixerTrackId> {
        let id = self.armed_track()?;
        self.project.mixer.reaches_master(id).then_some(id)
    }

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
    ///
    /// `start` is where the clip sits on the song, because the length its
    /// file takes **in ticks** depends on the tempo there: that length is
    /// what lets the block draw the file ending where it ends when the clip
    /// is not stretched (`AudioPreview::natural_length`).
    fn audio_preview(
        &self,
        start: Tick,
        data: &fontelle_types::AudioClipData,
    ) -> fontelle_ui::document::AudioPreview {
        let mut preview = fontelle_ui::document::AudioPreview {
            peaks: Vec::new(),
            fade_in: 0.0,
            fade_out: 0.0,
            fade_in_tension: data.fade_in.tension,
            fade_out_tension: data.fade_out.tension,
            natural_length: 0,
            stretched: data.stretch == fontelle_types::ClipStretch::Resample,
        };
        let frames = data.source_frames();
        if frames > 0 && data.sample_rate > 0 {
            // The same arithmetic a drop uses to size the block
            // (`import_audio_at`), through the same map — so a clip that has
            // never been dragged draws its file exactly filling it.
            //
            // Divided by `time_rate()`, which is how fast the clip moves
            // through the file — its speed, and under `Resample` its pitch
            // too: the player consumes the file that much faster, so a clip
            // at double speed is over in half the ticks and its waveform has
            // to stop there. Pitch with stretch off is deliberately *not* in
            // it: it moves what is heard and nothing about time.
            let read = f64::from(data.sample_rate) * data.time_rate();
            let samples = (frames as f64 * f64::from(self.options.sample_rate) / read).round()
                as fontelle_types::Sample;
            let from = self.effective_tempo.tick_to_sample(start);
            preview.natural_length =
                (self.effective_tempo.sample_to_tick(from + samples) - start).max(0);
        }
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
        // How many of the **clip's own** frames the picture covers, which is
        // not the same number as the file's frames and is the whole of
        // *"when stretch is off ... it still is visually stretching the clip
        // in the arrangement"*.
        //
        // `source_position` takes a clip frame and multiplies it by the rate.
        // So handing it a count of *file* frames scales the file by the rate
        // a second time — an octave up drew half the file across the whole
        // strip and smeared its last bucket over the rest, which is a picture
        // of a clip being stretched rather than repitched.
        //
        // The two modes want two different spans, for the reason
        // `AudioClipData::read_ratio` gives:
        //
        // - **Off**: the block already shrank (`natural_length` divides by the
        //   time rate), so the picture covers `frames / time_rate` clip
        //   frames and `source_position` turns that back into the whole file.
        //   The file, drawn in the room its speed gives it.
        // - **Resample**: the block is the constant and the player's own
        //   ratio folds the pass length in, so one pass is `frames` clip
        //   frames whatever the pitch. An octave up really is through the
        //   file in half the block, and the picture says so.
        let covered = match data.stretch {
            fontelle_types::ClipStretch::Off => {
                frames as f64 / data.time_rate().max(f64::MIN_POSITIVE)
            }
            fontelle_types::ClipStretch::Resample => frames as f64,
        };
        let per_bucket = covered / PREVIEW_BUCKETS as f64;
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
                level[a..=b].iter().fold((0.0f32, 0.0f32), |acc, (lo, hi)| {
                    (acc.0.min(*lo), acc.1.max(*hi))
                })
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
        let plugin_dirs = settings.plugin_dirs.clone();
        let mut session = Self {
            project,
            history: History::new(),
            library,
            channel_nodes,
            param_nodes: HashMap::new(),
            audio_nodes: HashMap::new(),
            record_mode: fontelle_ui::transport::RecordMode::default(),
            input: None,
            input_take: fontelle_engine::InputCapture::new(1),
            input_rate: 0,
            input_device: None,
            input_open: None,
            input_failed: None,
            capturing: false,
            monitor: None,
            input_retry: INPUT_RETRY,
            track_controls: HashMap::new(),
            voice_meters: HashMap::new(),
            effect_controls: HashMap::new(),
            spectrum_taps: HashMap::new(),
            tune_taps: HashMap::new(),
            plugins: crate::PluginRack::new(),
            plugins_hosted: false,
            analyser: fontelle_dsp::SpectrumAnalyser::new(),
            spectrum_scratch: Vec::new(),
            automation_clip: None,
            prefab: None,
            rack_tab: fontelle_ui::document::RackTab::default(),
            send_controls: HashMap::new(),
            automation_names: HashMap::new(),
            selected_track: 0,
            live_target: None,
            live_input: None,
            live_keys: None,
            metronome: None,
            master_meter: None,
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
            preset_bank: crate::preset_bank::PresetBank::new(settings.user_preset_dir()),
            preset_device_open: None,
            open_insert: None,
            settings,
            settings_path: None,
            settings_undo: None,
            settings_toast: None,
            import_bank: FileBank::default(),
            import_kind: fontelle_types::FolderKind::Midi,
            import_query: String::new(),
            pending_import: None,
            browser_mode: fontelle_ui::canvas::BrowserMode::Sounds,
            bank: SoundfontBank::default(),
            projects: ProjectLibrary::default(),
            updater: Updater::disabled(),
            query: String::new(),
            open_file: None,
            flopsynth_open: false,
            presets: Vec::new(),
            preset_index: PresetIndex::default(),
            clip_clipboard: Vec::new(),
            channel_presets: HashMap::new(),
            patch_cache: None,
            preview_node: NodeId::default(),
            preview_patch: None,
            previewing: false,
            message: error.map(|e| e.to_string()),
            audition: None,
            roll_notes: Arena::default(),
        };
        session.selected = session.channel_index_of_clip().unwrap_or(0);
        session.effective_tempo = session.tempo_for_scope();
        session.refresh_roll_notes();
        // The folders, but not a scan: walking them `dlopen`s every bundle on
        // the machine, and a *test* that never touches a plugin must not pay
        // for that. The studio scans a moment later, while it is still
        // opening — see `Session::scan_plugins`, which is the caller that
        // knows this is a window rather than a rig.
        session.plugins.set_folders(plugin_dirs);
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
    /// Points the plugin browser at `folders` instead of the standard ones,
    /// and walks them now.
    ///
    /// For a test, which must not depend on what is installed on the machine
    /// running it, and for the setting that lets somebody keep their plugins
    /// somewhere the format does not nominate.
    /// Looks for plugins in **these folders and nowhere else**.
    ///
    /// The standard folders are turned off with them, which is the whole
    /// point: a test that says "the browser offers one instrument and one
    /// effect" is otherwise counting whatever this machine has installed.
    /// See `PluginRack::search_standard_folders`, which learnt this the day
    /// three hundred and seventy real plugins arrived on the developer's own.
    pub fn with_plugin_folders(mut self, folders: Vec<PathBuf>) -> Self {
        self.plugins.search_standard_folders(false);
        self.plugins.set_folders(folders);
        self.plugins.rescan();
        self
    }

    /// Walks the plugin folders now, and says what was in them:
    /// `(found, would not load)`.
    ///
    /// **The studio calls this while it is opening.** A scan `dlopen`s every
    /// bundle on the machine, which on a well-stocked one is seconds; done
    /// lazily that cost lands on the first person to open the *+ fx* menu,
    /// where a menu that does not come down for three seconds reads as a menu
    /// that is broken. Startup is where a wait is expected, so the wait goes
    /// there — reported from using it: *"can you make it load plugins when the
    /// program starts instead of loading them when you go to add a plugin"*.
    ///
    /// The lazy path is still there and is now a no-op in the studio: opening
    /// a project that names a plugin, or a menu that wants the list, still
    /// asks (`PluginRack::scan_once`), and a test that never scans still
    /// never pays for one.
    pub fn scan_plugins(&mut self) -> (usize, usize) {
        self.plugins.rescan();
        (
            self.plugins.scan().plugins.len(),
            self.plugins.scan().failures.len(),
        )
    }

    pub fn with_settings_path(mut self, path: PathBuf) -> Self {
        let (settings, error) = Settings::load_from(&path);
        self.preset_bank.set_user_dir(settings.user_preset_dir());
        self.settings = settings;
        if let Some(e) = error {
            self.message = Some(e.to_string());
        }
        self.settings_path = Some(path);
        self
    }

    /// Where this session's settings are read from and written to, when it
    /// was given a path of its own rather than the real config file.
    pub fn settings_path(&self) -> Option<&Path> {
        self.settings_path.as_deref()
    }

    /// Gives the session the update check the launch decided on — the real
    /// one, or the disabled one when the settings say not to ask.
    pub fn with_updater(mut self, updater: Updater) -> Self {
        self.updater = updater;
        self
    }

    /// Whether the settings say the start menu may ask GitHub for a newer
    /// release.
    pub fn checks_for_updates(&self) -> bool {
        self.settings.check_for_updates
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

    /// Keeps the voice meters the first graph was built with.
    ///
    /// The same reason `with_spectrum_taps` exists: without it a session only
    /// learns about them on its first *rebuild*, so a project opened on a
    /// synth would say "0 voices" while it played until something else
    /// changed the graph.
    pub fn with_voice_meters(
        mut self,
        meters: HashMap<ChannelId, std::sync::Arc<fontelle_engine::VoiceMeter>>,
    ) -> Self {
        self.voice_meters = meters;
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

    /// The pitch traces the realised graph writes, so a corrector's window can
    /// draw them — [`with_spectrum_taps`](Self::with_spectrum_taps)'s sibling
    /// and the same reason (`docs/tune-plan.md` §7.3).
    pub fn with_tune_taps(
        mut self,
        taps: HashMap<(MixerTrackId, usize), std::sync::Arc<fontelle_engine::TuneTap>>,
    ) -> Self {
        self.tune_taps = taps;
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
        self.touch();
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
        self.touch();
    }

    /// The projects this machine was last in, newest first — the start
    /// menu's list. Each is marked with whether its bundle is still there,
    /// because a project that was moved or deleted is drawn dead rather than
    /// dropped: the person who moved it is the one to say so.
    pub fn recent_projects(&self) -> Vec<RecentProject> {
        self.settings
            .recent_projects
            .iter()
            .map(|path| RecentProject {
                name: path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string()),
                exists: path.join("project.json").is_file(),
                path: path.clone(),
            })
            .collect()
    }

    /// Opens the bundle at `path`, leaving what is open behind.
    ///
    /// The start menu's *Recent* rows and its *Open…* both land here; the
    /// Projects tab's rows go through [`StudioHost::open_project`] by index.
    /// One bundle that is not there any more is an error naming it, not an
    /// empty studio.
    pub fn open_project_path(&mut self, path: &Path) -> Result<(), String> {
        if !path.join("project.json").is_file() {
            // The name and the end of the folder, not the whole path: the
            // sentence is drawn on the start menu, and a path from `/tmp`
            // down does not fit two lines there.
            let name = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let folder = path
                .parent()
                .map(|parent| crate::desktop::elide_path(parent, 2))
                .unwrap_or_default();
            return Err(format!(
                "{name} is not there any more \u{2014} it was in {folder}"
            ));
        }
        let opened = crate::open_project(path).map_err(|e| e.to_string())?;
        self.adopt(opened, path.to_path_buf());
        Ok(())
    }

    /// Takes the `index`th recent project off the list.
    pub fn forget_recent(&mut self, index: usize) {
        let Some(path) = self.settings.recent_projects.get(index).cloned() else {
            return;
        };
        self.settings.forget_project(&path);
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.touch();
    }

    /// Puts `path` at the top of the recent list and writes the settings.
    ///
    /// Called from the three places a bundle path enters the session —
    /// [`adopt`](Self::adopt) and [`save_as`](Self::save_as) — so nothing
    /// that opens or names a project can forget to.
    fn remember_project(&mut self, path: &Path) {
        self.settings.remember_project(path);
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
    }

    /// Points the session at a projects folder and remembers it. For tests,
    /// and for a `--projects <dir>` flag when there is one.
    pub fn set_projects_dir(&mut self, dir: Option<PathBuf>) {
        self.settings.projects_dir = dir.clone();
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.projects.set_dir(dir);
        self.touch();
    }

    /// Where the open project lives, if it has been saved anywhere.
    pub fn bundle_path(&self) -> Option<&Path> {
        self.bundle.as_deref()
    }

    /// Whether the document has a file yet.
    ///
    /// What the window asks before saving: a studio started with no arguments
    /// has a project and nowhere to put it, and the answer to Ctrl+S there is
    /// *"what shall I call it"* rather than an error.
    pub fn has_file(&self) -> bool {
        self.bundle.is_some()
    }

    /// Saves what is open into a **new** project of that name, in the projects
    /// folder, and stays in it.
    ///
    /// > *"when im not in a project yet, i currently cant save that blank no
    /// > project into a new project ... if i try to save and theirs no project
    /// > directory it can just make a new one."*
    ///
    /// The same move [`take_path`](Self::take_path) already made for a first
    /// recording, with the name asked for rather than assumed. A name is not a
    /// path: it is made safe and made unique inside the folder the user chose,
    /// which is **INVARIANT 10** — with no projects folder there is nowhere
    /// they have named, and the answer is a sentence rather than a guess at
    /// `~/Documents`.
    pub fn save_as(&mut self, name: &str) -> Result<(), String> {
        let path = self.projects.new_project_path(name).ok_or_else(|| {
            "there is no projects folder yet \u{2014} choose one on the Projects tab".to_string()
        })?;
        // The **file name** is the project's name from here on, so what the
        // Projects tab lists and what the title bar says are the same word —
        // and it is the unique one, not the one that was typed, when the
        // folder already held it.
        if let Some(stem) = path.file_stem() {
            self.project.meta.name = stem.to_string_lossy().into_owned();
        }
        self.capture_plugin_states();
        crate::save_project(&self.project, &path).map_err(|e| e.to_string())?;
        self.remember_project(&path);
        self.bundle = Some(path);
        self.projects.rescan();
        self.dirty = false;
        self.touch();
        Ok(())
    }

    /// Makes a project of that name in the projects folder and opens it,
    /// leaving what is open behind. See [`DocumentHost::new_project`].
    pub fn new_project_named(&mut self, name: &str) -> Result<(), String> {
        let path = self
            .projects
            .new_project_path(name)
            .ok_or_else(|| "no projects folder yet \u{2014} \"Change...\" picks one".to_string())?;
        // Written to disk before it is opened, so the thing in the list and
        // the thing on screen are the same thing from the first moment. A new
        // project that only exists in memory until somebody remembers to save
        // is the commonest way to lose one.
        let mut project = crate::blank_project(NEW_PROJECT_BARS, 120.0, self.options.sample_rate);
        if let Some(stem) = path.file_stem() {
            project.meta.name = stem.to_string_lossy().into_owned();
        }
        crate::save_project(&project, &path).map_err(|e| e.to_string())?;
        self.projects.rescan();
        let opened = crate::open_project(&path).map_err(|e| e.to_string())?;
        self.adopt(opened, path);
        Ok(())
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
        self.remember_project(&path);
        self.bundle = Some(path);
        self.dirty = false;
        self.patch_cache = None;
        self.channel_presets.clear();
        self.clip_clipboard.clear();
        self.track_controls.clear();
        self.selected = self.channel_index_of_clip().unwrap_or(0);
        self.automation_clip = None;
        self.prefab = None;
        // A different document is a different tempo curve and a different
        // loop; both are read off the project that has just arrived.
        self.effective_tempo = self.tempo_for_scope();
        self.publish_loop();
        // The memo belongs to the session, the question to the project: a
        // microphone that would not open for the last project is asked for
        // again by this one, on the first frame, whatever the last answer was.
        self.input_failed = None;
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
        self.touch();
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

    /// Gives the session the master meter the transport bar reads.
    ///
    /// **Without this the bar's meter goes dead on the first rebuild** —
    /// the same fault `with_metronome` closes, on the other `Arc` the first
    /// graph hands out. `rebuild_graph` passes whatever it holds to
    /// `realise`, and `realise` mints a fresh `MasterMeter` when handed
    /// `None`; the bar's `EngineHost` goes on holding the original. Reported
    /// as *"it seems to show sometimes but not always"*: it showed until
    /// anything rebuilt the graph. See `fontelle-app/tests/master_meter.rs`.
    pub fn with_master_meter(
        mut self,
        meter: std::sync::Arc<fontelle_engine::MasterMeter>,
    ) -> Self {
        self.master_meter = Some(meter);
        self
    }

    /// The meter the running graph publishes into, so a caller can check it
    /// is the one the bar was given.
    pub fn master_meter(&self) -> std::sync::Arc<fontelle_engine::MasterMeter> {
        self.master_meter
            .clone()
            .unwrap_or_else(|| std::sync::Arc::new(fontelle_engine::MasterMeter::default()))
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
    pub fn with_input_settings(mut self, cell: std::sync::Arc<fontelle_midi::LiveMapping>) -> Self {
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

    /// A changed MIDI-input setting onto the keyboard and into the file.
    ///
    /// Onto the keyboard first and into the file second: what somebody is
    /// adjusting is how the next note feels, and a disk write that fails must
    /// not stop that. The three ways a MIDI row changes — a nudge, a slider
    /// drag, a drop-down pick — all end here so they cannot drift.
    fn write_input_settings(&mut self) {
        self.publish_input_settings();
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.touch();
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

    /// Gives the session the ring a live input is heard through (TDD §15.4).
    ///
    /// The same `Arc` the audio device was given, so the input callback that
    /// fills it and the node in the graph that plays it are two ends of one
    /// thing. Without one the studio still records; it simply cannot let you
    /// hear yourself, which is what every offline path wants.
    pub fn with_monitor(mut self, monitor: std::sync::Arc<fontelle_engine::InputMonitor>) -> Self {
        self.monitor = Some(monitor);
        self
    }

    /// How long an input that would not open waits before it is tried
    /// again. For tests; the window keeps [`INPUT_RETRY`].
    pub fn retry_inputs_every(&mut self, every: std::time::Duration) {
        self.input_retry = every;
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
    /// Bounces one row of the arrangement to audio and puts the result on a
    /// row of its own underneath it.
    ///
    /// > *"add the ability to render a track into an audio clip by right
    /// > clicking and in the options there should be a new render option ...
    /// > it will either do my time selection (if i have one) ... or if there
    /// > was no time selection just render the whole track. this should add a
    /// > new lane below it called whatever the original track name is plus
    /// > "(rendered)" at the end."*
    ///
    /// `span` is the stretch to bounce; `None` is the whole row. The window
    /// asks which when there is a time selection to ask about, because
    /// guessing is the one thing it must not do — a render of the wrong range
    /// is minutes of somebody's time.
    ///
    /// **Through `CompileScope::Lane`**, so what comes out is that row and not
    /// the row plus whatever was playing beside it.
    pub fn render_lane(
        &mut self,
        index: usize,
        span: Option<(Tick, Tick)>,
    ) -> Result<String, String> {
        let bundle = self
            .bundle
            .clone()
            .ok_or_else(|| "save this project first — a render goes inside it".to_string())?;
        let lane = self
            .lane_ids()
            .get(index)
            .copied()
            .ok_or("that row is not there")?;
        let name = self
            .project
            .lanes
            .get(lane)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "Lane".to_string());

        // What to bounce: the selection when there is one, and otherwise
        // everything on the row from the start of the song to the end of its
        // last clip.
        // **The tail is for a whole-row bounce only.** Asking for a stretch
        // means that stretch: a range render that ran two bars past its own
        // end would not line up with the selection it came from. A whole-row
        // render has no such edge to line up with, so it keeps the release so
        // the last note is not cut off mid-ring.
        let keep_tail = !matches!(span, Some((from, to)) if to > from);
        let (from_tick, to_tick) = match span {
            Some((from, to)) if to > from => (from, to),
            _ => {
                let end = self
                    .project
                    .clips
                    .values()
                    .filter(|clip| clip.lane == lane && !clip.muted)
                    .map(|clip| clip.start + clip.length)
                    .max()
                    .unwrap_or(0);
                (0, end)
            }
        };
        if to_tick <= from_tick {
            return Err("there is nothing on that row to render".to_string());
        }

        // The rows there were, so the one the import makes can be picked out
        // of them afterwards.
        let before = self.lane_ids();

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
            fontelle_sequencer::CompileScope::Lane(lane),
        );
        // Rendered from the song's zero and then cut to the range asked for:
        // a note that starts before the selection is still ringing inside it,
        // and starting the render at the selection would drop its front.
        let from = self.project.tempo_map.tick_to_sample(from_tick).max(0);
        let to = self.project.tempo_map.tick_to_sample(to_tick).max(from);
        let tail = if keep_tail {
            self.project.tempo_map.tick_to_sample(RELEASE_TAIL).max(0)
        } else {
            0
        };
        let audio = crate::render_offline(&timeline, &mut realised.graph, to + tail);
        // Interleaved stereo, so a frame is two samples.
        let cut: Vec<f32> = audio
            .iter()
            .copied()
            .skip((from as usize).saturating_mul(2))
            .take(((to + tail - from) as usize).saturating_mul(2))
            .collect();

        let renders = bundle.join("renders");
        std::fs::create_dir_all(&renders).map_err(|e| format!("{}: {e}", renders.display()))?;
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
        let stem = crate::unique_name(&name, &taken.iter().map(String::as_str).collect::<Vec<_>>());
        let path = renders.join(format!("{stem}.wav"));
        crate::write_wav16(&path, &cut, 2, self.options.sample_rate)
            .map_err(|e| format!("{}: {e}", path.display()))?;

        // The import makes a row of its own (`AddAudioClip`), so what is left
        // is to **name** it for the row it came from and to **put it directly
        // under** that row — a bounce at the bottom of a long arrangement is a
        // bounce you have to go looking for.
        let at = self.project.tempo_map.tick_to_sample(from_tick);
        self.import_audio_at(&path, at, None, None)?;
        let made = self
            .lane_ids()
            .into_iter()
            .find(|id| !before.contains(id))
            .ok_or("the row for the render was not made")?;
        if let Some(lane) = self.project.lanes.get_mut(made) {
            lane.name = format!("{name} (rendered)");
        }
        // Ordered under the row it is a render of, by renumbering rather than
        // by touching ids: a clip names a lane id, and shuffling the arena
        // would move somebody's music (`AddLane::at`'s rule).
        let mut order: Vec<(fontelle_types::LaneId, u32)> = self
            .project
            .lanes
            .iter()
            .map(|(id, lane)| (id, lane.order))
            .collect();
        order.sort_by_key(|(_, order)| *order);
        order.retain(|(id, _)| *id != made);
        let seat = order
            .iter()
            .position(|(id, _)| *id == lane)
            .map_or(order.len(), |at| at + 1);
        order.insert(seat, (made, 0));
        for (position, (id, _)) in order.iter().enumerate() {
            if let Some(lane) = self.project.lanes.get_mut(*id) {
                lane.order = position as u32;
            }
        }
        self.history.break_gesture();
        self.dirty = true;
        self.republish();
        self.touch();
        Ok(format!("Rendered \u{2014} {}", path.display()))
    }

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

        self.touch();
        Ok(match clipped {
            // Surfaced rather than swallowed, which is what §11 of the plan
            // asks for: a bounce that clipped is one you want to know about
            // before you send it anywhere.
            0 => format!("exported {}", path.display()),
            1 => format!("exported {} \u{2014} 1 clipped sample", path.display()),
            n => format!("exported {} \u{2014} {n} clipped samples", path.display()),
        })
    }

    /// Saves the song out as a Standard MIDI File the user chooses the place
    /// for.
    ///
    /// Unlike a WAV bounce, which lives *inside* the project bundle because a
    /// render belongs to the project, a `.mid` is an interchange file: you
    /// export it to open somewhere else, so it asks where to put it. The
    /// picker opens on the projects folder with the song's name already filled
    /// in. `Ok(None)`'s status is the cancel; a machine with no picker at all
    /// falls back to writing beside the bundle so the feature is not lost on a
    /// headless box.
    ///
    /// It is the inverse of [`import_midi`](fontelle_assets::import_midi): the
    /// notes, the loops unrolled, and the tempo, on the project's own tick
    /// grid. See `fontelle_assets::export_project_to_midi` for what a `.mid`
    /// can and cannot carry.
    pub fn export_midi(&mut self) -> Result<String, String> {
        let default_name = {
            let name = self.project.meta.name.trim();
            let stem = if name.is_empty() { "song" } else { name };
            format!("{stem}.mid")
        };
        // Open the picker where the user's projects live, or beside the bundle.
        let start = self.settings.projects_dir.clone().or_else(|| {
            self.bundle
                .as_ref()
                .and_then(|b| b.parent().map(Path::to_path_buf))
        });

        let path = match crate::desktop::choose_save_file(
            "Export project as MIDI",
            &default_name,
            start.as_deref(),
        ) {
            Ok(Some(mut path)) => {
                // A picker that gave a name with no extension gets `.mid`, so a
                // file called "song" is still a MIDI file to everything else.
                if path.extension().is_none() {
                    path.set_extension("mid");
                }
                path
            }
            Ok(None) => return Ok("MIDI export cancelled".to_string()),
            // No picker on this machine: write beside the bundle rather than
            // losing the feature, and say where it went.
            Err(_) => {
                let dir = self
                    .bundle
                    .as_ref()
                    .and_then(|b| b.parent())
                    .map(Path::to_path_buf)
                    .or_else(|| self.settings.projects_dir.clone())
                    .ok_or_else(|| {
                        "no folder picker on this machine, and no project folder to \
                         write into — save this project first"
                            .to_string()
                    })?;
                dir.join(&default_name)
            }
        };

        fontelle_assets::export_midi(&self.project, &path).map_err(|e| e.to_string())?;
        Ok(format!("exported {}", path.display()))
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
        // A backup that recovers a project without its plugins' settings is a
        // backup of half the session. After the dirty check, so this still
        // writes nothing when nothing has changed.
        self.capture_plugin_states();
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
        self.touch();
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
        self.touch();
    }

    /// What the document says one insert is set to, whatever kind it holds.
    ///
    /// [`eq_config`](StudioHost::eq_config)'s general case: the curve editor
    /// wants the EQ specifically, and publishing to the live end wants
    /// whatever is there.
    fn insert_config(&self, strip: usize, slot: usize) -> Option<fontelle_types::EffectConfig> {
        let id = self.mixer_track_ids().get(strip).copied()?;
        // `None` for an insert holding a plugin: there is no `EffectConfig`
        // behind one, and every caller here asks this first.
        self.project
            .mixer
            .tracks
            .get(id)?
            .inserts
            .get(slot)?
            .config()
            .copied()
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
                let span =
                    fontelle_engine::CHANNEL_GAIN_MAX_DB - fontelle_engine::CHANNEL_GAIN_MIN_DB;
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
                let patch =
                    fontelle_core::Patch::from_data(data, |file| self.library.resolve(file))
                        .ok()?
                        .patch;
                fontelle_core::patch_params::value(&patch, &param).map(f64::from)
            }
            ParamTarget::Insert { track, slot, param } => {
                let insert = self.project.mixer.tracks.get(track)?.inserts.get(slot)?;
                // A hosted plugin's parameters are stored plain, in the
                // plugin's own units, and the lane wants where they sit on
                // its 0..1 travel — which only the plugin's range can say.
                if let Some(state) = &insert.plugin {
                    let id: u32 = param.parse().ok()?;
                    let value = state.param(id)?;
                    let hosted = self
                        .plugins
                        .plugin(crate::PluginSlot::Insert { track, slot })?
                        .param(id)?;
                    return Some(hosted.normalise(value));
                }
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
    /// Asks for a folder to look for plugins in, and looks in it.
    ///
    /// **Adds**, rather than replacing: the standard CLAP locations are
    /// searched whether or not anything is configured, and somebody with a
    /// collection on two disks has two folders. Its own picker rather than the
    /// import browser's, because a plugin folder is not somewhere files are
    /// imported from — see `FolderKind`, which is about the browser's tabs.
    fn choose_plugin_dir(&mut self) {
        let start = self
            .settings
            .plugin_dirs
            .last()
            .cloned()
            .or_else(|| self.settings.projects_dir.clone());
        match crate::desktop::choose_folder("Plugin folder", start.as_deref()) {
            Ok(Some(dir)) => {
                if !self.settings.plugin_dirs.contains(&dir) {
                    self.settings.plugin_dirs.push(dir);
                }
                if let Err(e) = self.save_settings() {
                    self.message = Some(format!("could not write settings: {e}"));
                }
                self.plugins.set_folders(self.settings.plugin_dirs.clone());
                <Self as StudioHost>::rescan_plugins(self);
            }
            // A cancel is not an event.
            Ok(None) => {}
            Err(e) => self.message = Some(e),
        }
    }

    /// Takes the `which`th folder off the list and scans again.
    fn remove_plugin_dir(&mut self, which: usize) {
        if which >= self.settings.plugin_dirs.len() {
            return;
        }
        let gone = self.settings.plugin_dirs.remove(which);
        let shown = crate::desktop::elide_path(&gone, 2);
        // Reversible: keep what was removed so the toast's "Undo" can put it
        // back, and offer that rather than a confirmation prompt up front.
        self.settings_undo = Some((which, gone));
        self.settings_toast = Some((format!("No longer searching {shown}"), true));
        self.message = Some(format!("No longer searching {shown}"));
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.plugins.set_folders(self.settings.plugin_dirs.clone());
        <Self as StudioHost>::rescan_plugins(self);
    }

    /// Adds the folders FL Studio searches, and says how many were new.
    fn import_fl_folders(&mut self) {
        let found = crate::daw_folders::fl_studio_folders();
        if found.is_empty() {
            self.message = Some(
                "No FL Studio settings found on this machine (its extra search folders \
                 live in the registry, or in a Wine prefix's user.reg)"
                    .to_string(),
            );
            return;
        }
        let mut added = 0;
        for dir in found {
            if !self.settings.plugin_dirs.contains(&dir) {
                self.settings.plugin_dirs.push(dir);
                added += 1;
            }
        }
        self.message = Some(match added {
            0 => "FL Studio's folders were already listed".to_string(),
            1 => "Added the folder FL Studio searches".to_string(),
            n => format!("Added {n} folders FL Studio searches"),
        });
        if added == 0 {
            return;
        }
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.plugins.set_folders(self.settings.plugin_dirs.clone());
        <Self as StudioHost>::rescan_plugins(self);
    }

    /// Installs or removes the `which`th catalogue extension, depending on
    /// its state. Both are refused while a plugin is open through it — the
    /// rack asserts this, so the message says to close the project first
    /// rather than the rack panicking (`docs/vst-plan.md` §4.2).
    fn press_extension(&mut self, which: usize) {
        let Some(extension) = crate::extensions::CATALOGUE.get(which) else {
            return;
        };
        let installed = crate::extensions::is_installed(extension);
        let state = crate::extensions::ExtensionState::of(extension, installed, None);
        match crate::extensions::action_for(&state) {
            crate::extensions::ExtensionAction::None => {
                self.message = Some(format!("{} needs a newer Fontelle", extension.name));
            }
            crate::extensions::ExtensionAction::Remove => {
                if self.plugins.has_open_plugins() {
                    self.message =
                        Some("Close the project before changing an extension".to_string());
                    return;
                }
                match crate::extensions::remove(extension) {
                    Ok(()) => {
                        // Not undoable by a click: getting it back is a
                        // download, which is why this one asks first (see
                        // `settings_confirm`) rather than offering an undo.
                        self.settings_toast = Some((format!("Removed {}", extension.name), false));
                        self.message = Some(format!("Removed {}", extension.name));
                        self.plugins.reload_bridges();
                        <Self as StudioHost>::rescan_plugins(self);
                    }
                    Err(why) => self.message = Some(why),
                }
            }
            crate::extensions::ExtensionAction::Install => {
                if self.plugins.has_open_plugins() {
                    self.message =
                        Some("Close the project before changing an extension".to_string());
                    return;
                }
                self.message = Some(format!("Installing {}\u{2026}", extension.name));
                let target = crate::updates::target_triple();
                let fetch: crate::updates::Fetcher = Box::new(crate::updates::fetch_with_progress);
                match crate::extensions::install(extension, &target, &fetch, &mut |_, _| {}) {
                    Ok(()) => {
                        self.message = Some(format!(
                            "Installed {} \u{2014} its plugins are found on the next scan",
                            extension.name
                        ));
                        self.plugins.reload_bridges();
                        <Self as StudioHost>::rescan_plugins(self);
                    }
                    Err(why) => {
                        self.message = Some(format!("Could not install {}: {why}", extension.name));
                    }
                }
            }
        }
    }

    /// The `PluginState` for the instrument at `which` in the browser's list.
    fn instrument_state(&self, which: usize) -> Option<fontelle_types::PluginState> {
        self.plugins
            .scan()
            .instruments()
            .nth(which)
            .map(|found| self.plugins.state_for(found))
    }

    /// Reads every open plugin's state back into the document, before it is
    /// written to a file.
    ///
    /// **Not a command**, and that is deliberate. Nothing a person did caused
    /// it: a plugin's own blob is opaque, changes whenever the plugin feels
    /// like it, and putting it in the undo history would fill the history with
    /// entries nobody can see the effect of and cannot undo the meaning of.
    /// The parameters are already there — every knob went through
    /// [`SetPluginParam`](fontelle_model::SetPluginParam) — so what this adds
    /// is the half only the plugin knows.
    fn capture_plugin_states(&mut self) {
        let slots: Vec<crate::PluginSlot> = crate::plugin_slots(&self.project);
        for slot in slots {
            let Some(state) = self.plugins.snapshot(slot) else {
                continue;
            };
            match slot {
                crate::PluginSlot::Channel(channel) => {
                    if let Some(channel) = self.project.channels.get_mut(channel) {
                        channel.plugin = Some(state);
                    }
                }
                crate::PluginSlot::Insert { track, slot } => {
                    if let Some(insert) = self
                        .project
                        .mixer
                        .tracks
                        .get_mut(track)
                        .and_then(|track| track.inserts.get_mut(slot))
                    {
                        insert.plugin = Some(state);
                    }
                }
            }
        }
    }

    /// Opens one plugin's own editor, and says whether it went up.
    ///
    /// `false` is not a failure: it means this plugin has nothing of its own
    /// to show and Fontelle's panel is the answer. A refusal that *is* a
    /// failure — no X server to make a window on — is reported and still
    /// answers `false`, so the panel opens and the person is told why they got
    /// it.
    fn open_plugin_editor(&mut self, slot: crate::PluginSlot) -> bool {
        match self.plugins.open_editor(slot) {
            Ok(opened) => opened,
            Err(e) => {
                self.message = Some(e);
                self.touch();
                false
            }
        }
    }

    fn rebuild_graph(&mut self) {
        // Reusing the control surfaces, so a track's fader and meter outlive
        // the graph they were built with — see `realise::fader`.
        // The live input goes onto the armed track, so the strip you chose an
        // input on is the strip whose fader, inserts and routing decide what
        // you hear of yourself (TDD §15.4).
        let monitor = self.monitor.clone().map(|monitor| crate::MonitorPlan {
            monitor,
            track: self.armed_track(),
        });
        // The plugins first: opening what the document has started asking for
        // and closing what it has not, before the graph that will hold them is
        // built. See `PluginRack` for why they cannot belong to the graph.
        let plugins = self.plugins.realise(
            &self.project,
            f64::from(self.options.sample_rate),
            self.options.block_size as u32,
        );
        if let Some(message) = self.plugins.take_message() {
            self.message = Some(message);
        }
        match crate::realise::realise_hosting(
            &self.project,
            &self.library,
            self.options,
            &self.track_controls,
            self.metronome.clone(),
            &crate::realise::KeptTaps {
                spectrum: self.spectrum_taps.clone(),
                tune: self.tune_taps.clone(),
                master: self.master_meter.clone(),
            },
            monitor.as_ref(),
            // Put back whatever the browser was letting you hear: the node is
            // minted fresh on every rebuild, so an instrument left in it would
            // otherwise go quiet the next time anything changed.
            self.preview_patch.as_ref(),
            &plugins,
        ) {
            Ok(realised) => {
                self.channel_nodes = realised.channel_nodes;
                self.param_nodes = realised.param_nodes;
                self.audio_nodes = realised.audio_nodes;
                // New graph, new node — and the preview has to follow it or
                // the next click would send a note to a node that is gone.
                self.preview_node = realised.preview_node;
                // The old set belonged to the graph that is being replaced.
                self.track_controls = realised.track_controls;
                self.voice_meters = realised.voice_meters;
                self.effect_controls = realised.effect_controls;
                self.spectrum_taps = realised.spectrum_taps;
                self.tune_taps = realised.tune_taps;
                self.send_controls = realised.send_controls;
                self.metronome = Some(realised.metronome);
                self.publish_metronome();
                self.master_meter = Some(realised.master);
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
        self.touch();
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
                self.touch();
            }
            // A refused edit is not a crash and not a history entry — a note
            // dragged past key 127 simply does not move.
            Err(e) => {
                eprintln!("Fontelle: {e}");
                self.message = Some(e.to_string());
            }
        }
    }

    /// The open clip's home channel, if it is a note clip.
    fn home_channel(&self, home: fontelle_model::NoteHome) -> Option<ChannelId> {
        use fontelle_model::NoteHome;
        let source = match home {
            NoteHome::Clip(clip) => self.project.clip_source(clip)?,
            NoteHome::Prefab(prefab) => {
                std::borrow::Cow::Borrowed(&self.project.prefabs.get(prefab)?.source)
            }
        };
        match source.as_ref() {
            ClipSource::Notes(data) => Some(data.channel),
            _ => None,
        }
    }

    /// Rebuilds what the roll sees: the open clip's notes **on the selected
    /// channel**, under their own ids.
    ///
    /// A copy rather than a filter over the document, because
    /// `DocumentHost::notes` hands the roll a whole arena by reference and
    /// the roll addresses everything by `NoteId` — the ids are the
    /// document's, so an edit the roll asks for lands on the right note.
    /// Rebuilt by [`touch`](Self::touch), which every revision goes through,
    /// so the view can never be a revision behind the document.
    fn refresh_roll_notes(&mut self) {
        let selected = self.selected_channel_id();
        let mut view = Arena::default();
        // **Through the note target**, not through `self.clip`: the roll may
        // be looking at a prefab picked from the list, or at a place on the
        // arrangement whose notes are that prefab's. See `note_target`.
        let held = self.note_target_source();
        if let Some(ClipSource::Notes(data)) = held.as_deref()
            && let Some(channel) = selected
        {
            for (id, note) in data.notes_on(channel) {
                view.insert_at(id, *note);
            }
        }
        self.roll_notes = view;
    }

    /// **Where a note edit goes, and where the roll reads from.**
    ///
    /// > *"editing a prefab clip basically works like just editing a normal
    /// > clip except you dont have to only be selecting it in the arrangement
    /// > a clip thats referencing the prefab you could also edit it just by
    /// > selecting the prefab in the prefab menu."*
    ///
    /// Two ways in, one answer, in one function — because "which of the two
    /// did you mean" is a question the roll, the keyboard and the arrangement
    /// would each get to answer differently otherwise:
    ///
    /// 1. A prefab picked from the list wins, because picking one is saying
    ///    "edit this" out loud.
    /// 2. Otherwise the open clip decides, through
    ///    [`Project::note_home`](fontelle_model::Project::note_home) — which
    ///    is the prefab when the clip follows one, and the clip itself when it
    ///    does not.
    fn note_target(&self) -> fontelle_model::NoteHome {
        use fontelle_model::NoteHome;
        if let Some(prefab) = self.prefab
            && self.project.prefabs.contains_key(prefab)
        {
            return NoteHome::Prefab(prefab);
        }
        self.project
            .note_home(self.clip)
            .unwrap_or(NoteHome::Clip(self.clip))
    }

    /// The content [`note_target`](Self::note_target) names.
    fn note_target_source(&self) -> Option<std::borrow::Cow<'_, ClipSource>> {
        use fontelle_model::NoteHome;
        match self.note_target() {
            NoteHome::Clip(clip) => self.project.clip_source(clip),
            NoteHome::Prefab(prefab) => self
                .project
                .prefabs
                .get(prefab)
                .map(|prefab| std::borrow::Cow::Borrowed(&prefab.source)),
        }
    }

    /// The prefab ids, in the order the list draws them.
    fn prefab_ids(&self) -> Vec<fontelle_types::PrefabId> {
        self.project.prefab_ids()
    }

    /// A name no prefab in this project has yet.
    ///
    /// Numbered from the count rather than from the list's length alone, so
    /// making one, deleting it and making another does not give two "Prefab 1"
    /// — a list of things that share a name is a list you cannot work from.
    fn next_prefab_name(&self) -> String {
        let taken: Vec<&str> = self
            .project
            .prefabs
            .values()
            .map(|prefab| prefab.name.as_str())
            .collect();
        (1..)
            .map(|n| format!("Prefab {n}"))
            .find(|name| !taken.iter().any(|taken| *taken == name))
            .unwrap_or_else(|| "Prefab".to_string())
    }

    /// The channel a new prefab's content plays: the selected one, or the
    /// first there is.
    ///
    /// A prefab holds notes and notes name a channel, so it needs one to be
    /// made at all. The rack's selection, because that is what every other
    /// interaction in this program means by "the instrument".
    fn prefab_home_channel(&self) -> Option<ChannelId> {
        self.selected_channel_id()
            .or_else(|| self.project.channels.keys().next())
    }

    /// Moves the revision — what the window watches — and refreshes what
    /// depends on it. The one way to do so; a bare `revision += 1` that
    /// forgot the roll's view would show a note that was undone away.
    fn touch(&mut self) {
        self.revision += 1;
        self.refresh_roll_notes();
    }

    /// A clip that plays `channel`, if any — the open one when it does, so
    /// selecting an instrument does not move the roll off the clip in hand.
    fn clip_of_channel(&self, channel: ChannelId) -> Option<ClipId> {
        let plays = |clip: &Clip| match &clip.source {
            ClipSource::Notes(data) => data.channels().contains(&channel),
            _ => false,
        };
        if self.project.clips.get(self.clip).is_some_and(plays) {
            return Some(self.clip);
        }
        self.project
            .clips
            .iter()
            .find(|(_, clip)| plays(clip))
            .map(|(id, _)| id)
    }

    /// The paths the browser's first list is showing, row by row.
    ///
    /// `None` for a row that is not a soundfont — the way up, and a folder.
    /// One function so `open_file`, `selected_file` and the panel can never
    /// disagree about what row `n` is.
    /// Whether the browser is showing Flopsynth's own row above the bank.
    ///
    /// Only at the top of the tree and only while not searching: a search
    /// lists soundfont files wherever they are, and a built-in bank is not one
    /// of them.
    fn shows_flopsynth_row(&self) -> bool {
        self.query.trim().is_empty()
            && self
                .bank
                .rows()
                .iter()
                .all(|row| !matches!(row, BankRow::Up { .. }))
    }

    /// How far the synthetic row shifts every index into the files list.
    ///
    /// One number, asked by everything that indexes it, rather than three
    /// places each remembering to add one — which is the arithmetic that goes
    /// wrong silently and opens the wrong soundfont.
    fn flopsynth_row_offset(&self) -> usize {
        usize::from(self.shows_flopsynth_row())
    }

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
        if self.browser_mode == fontelle_ui::canvas::BrowserMode::Presets {
            return self.preset_tab_rows();
        }
        let query = self.query.trim();
        if self.flopsynth_open {
            // Grouped by category, with the category as a heading — which is
            // how the brief asked for them: *"organized by type"*. The
            // `file` field is empty and the `index` is the row's place in
            // `FACTORY`, which is what `install_preset` reads back.
            let mut rows = Vec::new();
            for category in fontelle_core::flopsynth::presets::FlopsynthCategory::ALL {
                let mut under = Vec::new();
                for (index, preset) in fontelle_core::flopsynth::presets::FACTORY
                    .iter()
                    .enumerate()
                {
                    if preset.category != category {
                        continue;
                    }
                    if !query.is_empty()
                        && !preset.name.to_lowercase().contains(&query.to_lowercase())
                    {
                        continue;
                    }
                    under.push(PresetRow::Preset {
                        file: PathBuf::new(),
                        index,
                        name: preset.name.to_string(),
                        detail: String::new(),
                    });
                }
                if under.is_empty() {
                    // A heading over nothing is a heading that says the
                    // search found nothing here, which is what the empty list
                    // already says.
                    continue;
                }
                rows.push(PresetRow::Group {
                    name: category.label().to_string(),
                    detail: format!("{}", under.len()),
                });
                rows.extend(under);
            }
            return rows;
        }
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
        // The preview voice while the browser is letting you hear something,
        // and the selected channel the rest of the time. One switch rather
        // than a second live path, so "every note-on is matched by one
        // note-off" stays a property of `Auditions` — the note goes wherever
        // this says, and both halves of it say the same thing because nothing
        // changes it mid-note.
        if self.previewing {
            return self.preview_node;
        }
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
    /// One of Flopsynth's factory presets, onto `channel`.
    ///
    /// The same shape [`install_preset`](Self::install_preset) has and for the
    /// same reasons: one history entry for the patch, the kind and the name,
    /// because choosing an instrument is one thing a person did and changing
    /// it by mistake has to be one Ctrl+Z away.
    fn install_flopsynth_preset(
        &mut self,
        channel: ChannelId,
        preset: usize,
    ) -> Result<(), String> {
        let (name, index) = match self.preset_rows().into_iter().nth(preset) {
            Some(PresetRow::Preset { name, index, .. }) => (name, index),
            Some(PresetRow::Group { .. }) => {
                return Err("that row is a heading, not a sound".to_string());
            }
            None => return Err("that preset is not in the bank".to_string()),
        };
        let row = fontelle_core::flopsynth::presets::FACTORY
            .get(index)
            .ok_or("that preset is not in the bank")?;
        let data = (row.build)()
            .to_data(self.library.provenance())
            .map_err(|e| e.to_string())?;
        let parts: Vec<Box<dyn Command>> = vec![
            Box::new(fontelle_model::SetChannelPatch::new(channel, Some(data))),
            // A channel with a Flopsynth preset on it **is** a Flopsynth,
            // whatever it was before: the assignment is the choice, and a rack
            // still saying "SoundFont" over a synth preset is the loudest
            // "nothing happened" it can give.
            Box::new(fontelle_model::SetChannelKind::new(
                channel,
                fontelle_types::InstrumentKind::Flopsynth,
            )),
            Box::new(fontelle_model::RenameChannel::new(channel, name.clone())),
        ];
        self.run(Box::new(fontelle_model::Compound::new(
            "Choose preset",
            parts,
        )));
        self.history.break_gesture();
        // A Flopsynth names no soundfont, so nothing goes in the map that
        // remembers which file a channel's preset came from — and
        // `selected_preset` correctly shows no soundfont highlight.
        self.channel_presets.remove(&channel);
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        self.touch();
        Ok(())
    }

    fn install_preset(&mut self, channel: ChannelId, preset: usize) -> Result<(), String> {
        if self.browser_mode == fontelle_ui::canvas::BrowserMode::Presets {
            // The bank's own tab: which device it is for is the preset's to
            // say, not the channel's.
            let _ = channel;
            return self.install_bank_preset(preset);
        }
        if self.flopsynth_open {
            return self.install_flopsynth_preset(channel, preset);
        }
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
        let mut parts: Vec<Box<dyn Command>> = vec![
            Box::new(fontelle_model::SetChannelPatch::new(channel, Some(data))),
            // **A channel with a soundfont on it is a soundfont player**,
            // whatever it was before. The assignment is the choice, so the
            // rack has to agree with what is actually loaded rather than
            // still saying "Sampler" over a preset.
            Box::new(fontelle_model::SetChannelKind::new(
                channel,
                fontelle_types::InstrumentKind::SoundFont,
            )),
        ];
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
        self.new_channel_inner(name, patch_data, None)
    }

    /// [`new_channel`](Self::new_channel), saying which of the three
    /// instruments it is.
    fn new_channel_of(
        &mut self,
        name: String,
        patch_data: Option<fontelle_types::PatchData>,
        kind: fontelle_types::InstrumentKind,
    ) -> Result<ChannelId, String> {
        self.new_channel_inner(name, patch_data, Some(kind))
    }

    fn new_channel_inner(
        &mut self,
        name: String,
        patch_data: Option<fontelle_types::PatchData>,
        kind: Option<fontelle_types::InstrumentKind>,
    ) -> Result<ChannelId, String> {
        let mut add = AddChannel::new(name, patch_data);
        if let Some(kind) = kind {
            add = add.of_kind(kind);
        }
        let channel = self
            .apply_for::<AddChannel>(Box::new(add))?
            .channel()
            .ok_or("the channel was not created")?;
        self.history.break_gesture();

        self.dirty = true;
        // The new channel is the selected one: you made it to play it. It
        // used to come with a lane and a clip of its own, because a clip
        // could only play one instrument; a clip holds several now
        // (`Note::channel`) and is drawn where it is wanted, so the roll
        // stays on the clip that is open and writes this instrument into
        // it — which is what *"base our interactions on what your currently
        // selected instrument in the channel rack is"* means.
        self.selected = self.project.channels.len().saturating_sub(1);
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
    /// The patch on the selected channel, resolved against the library.
    ///
    /// `pub` so a test can read what the panel is drawing without a window —
    /// there is no other route to a channel's instrument from outside.
    pub fn selected_patch(&self) -> Option<fontelle_core::Patch> {
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

    /// How many entries `undo` could walk back through — the history's own
    /// depth, exposed so a test can say "that press was not a change".
    pub fn undo_depth(&self) -> usize {
        self.history.depth()
    }

    /// Which of the three instruments channel `index` is (TDD §7,
    /// [`fontelle_types::InstrumentKind`]).
    ///
    /// The channel's own answer when it has one, and otherwise **read off the
    /// patch that is loaded** — which is what a project written before the
    /// field existed needs, and is exactly the derivation the field replaced:
    /// a layer's `Source` says whether it is a soundfont zone, a sample or an
    /// oscillator. A channel with nothing on it and nothing said reads as a
    /// soundfont player, since that is what this program is for.
    pub fn channel_kind(&self, index: usize) -> Option<fontelle_types::InstrumentKind> {
        use fontelle_types::InstrumentKind;
        let id = self.channel_ids().get(index).copied()?;
        let channel = self.project.channels.get(id)?;
        if let Some(kind) = channel.instrument {
            return Some(kind);
        }
        let Some(data) = channel.patch_data.as_ref() else {
            return Some(InstrumentKind::default());
        };
        let Ok(loaded) = fontelle_core::Patch::from_data(data, |file| self.library.resolve(file))
        else {
            return Some(InstrumentKind::default());
        };
        Some(Self::kind_of(&loaded.patch))
    }

    /// What a patch's layers say it is.
    ///
    /// A soundfont zone anywhere makes it a soundfont player and a sample
    /// anywhere makes it a sampler, because those are the two that name a
    /// file; oscillators are what is left. An empty patch cannot be told
    /// apart, which is the whole reason `Channel::instrument` exists.
    fn kind_of(patch: &fontelle_core::Patch) -> fontelle_types::InstrumentKind {
        use fontelle_core::Source;
        use fontelle_types::InstrumentKind;
        if patch
            .layers
            .iter()
            .any(|layer| matches!(layer.source, Source::Sf2Zone { .. }))
        {
            return InstrumentKind::SoundFont;
        }
        if patch
            .layers
            .iter()
            .any(|layer| matches!(layer.source, Source::Sample { .. }))
        {
            return InstrumentKind::Sampler;
        }
        // Before the oscillator fallback and after the two that name files,
        // for the same reason they are in this order: a drum hit is the most
        // specific thing a layer can be.
        if patch
            .layers
            .iter()
            .any(|layer| matches!(layer.source, Source::Drum(_)))
        {
            return InstrumentKind::DrumMachine;
        }
        // Before the oscillator fallback and after the three above, for the
        // same reason they are in that order: a synth oscillator is a more
        // specific thing for a layer to be than "an oscillator".
        if fontelle_core::flopsynth::is_flopsynth(patch) {
            return InstrumentKind::Flopsynth;
        }
        if patch.layers.is_empty() {
            return InstrumentKind::default();
        }
        InstrumentKind::Osc3
    }

    /// A **sampler** on a new channel, playing `path` across the keyboard.
    ///
    /// *"i cannot drag an audio clip from the audio import tab into the
    /// channel rack to turn it into a sampler, please add this feature."*
    ///
    /// One layer over every key, rooted at middle C, so the file plays at its
    /// own pitch there and transposes either side of it — which is what a
    /// sampler *is*, and is the only reading of a file that has no key
    /// mapping of its own to offer.
    pub fn add_sampler_from(&mut self, path: &Path) -> Result<String, String> {
        let (name, data) = self.sampler_patch_for(path)?;
        self.new_channel_of(
            name.clone(),
            Some(data),
            fontelle_types::InstrumentKind::Sampler,
        )?;
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        self.touch();
        Ok(name)
    }

    /// The same file, onto a channel that is **already there**.
    ///
    /// > *"i want to be able to click and drag them into the sampler or into
    /// > the channel rack to make it have a sampler with that clip sampled."*
    ///
    /// Two targets and two meanings, and the difference is the one every drop
    /// on this rack already makes: empty space makes a new channel, a row
    /// changes that one. Without it, building a kit means dragging eight files
    /// in and then deleting the eight channels they landed beside.
    ///
    /// One history entry for the three things it does — the patch, the kind
    /// and the name — for the reason [`install_preset`](Self::install_preset)
    /// gives: choosing an instrument is one thing a person did, and changing
    /// it by mistake has to be one Ctrl+Z away.
    pub fn set_channel_sampler_from(
        &mut self,
        index: usize,
        path: &Path,
    ) -> Result<String, String> {
        let channel = self
            .channel_ids()
            .get(index)
            .copied()
            .ok_or("there is no channel there")?;
        // **Decoded before anything is applied.** A file that turns out to be
        // silent or unreadable must leave the channel playing what it played:
        // this one replaces an instrument rather than adding one, so a failure
        // half way through is an instrument somebody was using, gone.
        let (name, data) = self.sampler_patch_for(path)?;
        self.history
            .apply(
                Box::new(fontelle_model::Compound::new(
                    "Load sample",
                    vec![
                        Box::new(fontelle_model::SetChannelPatch::new(channel, Some(data))),
                        Box::new(fontelle_model::SetChannelKind::new(
                            channel,
                            fontelle_types::InstrumentKind::Sampler,
                        )),
                        Box::new(fontelle_model::RenameChannel::new(channel, name.clone())),
                    ],
                )),
                &mut self.project,
            )
            .map_err(|e| e.to_string())?;
        self.history.break_gesture();
        // A channel that was a soundfont player is not one any more, so the
        // preset the browser remembered for it would name a sound it no longer
        // plays.
        self.channel_presets.remove(&channel);
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        self.touch();
        Ok(name)
    }

    /// What a file becomes when it is played as a sampler: the name to put on
    /// the channel, and the patch to put in it.
    ///
    /// Shared by the two ways one arrives — a new channel and an existing one —
    /// so "a sampler made from a file" means exactly one thing whichever way
    /// the file came in.
    fn sampler_patch_for(
        &mut self,
        path: &Path,
    ) -> Result<(String, fontelle_types::PatchData), String> {
        let (name, patch, _seconds) = self.sampler_patch(path)?;
        let data = patch
            .to_data(self.library.provenance())
            .map_err(|e| e.to_string())?;
        Ok((name, data))
    }

    /// A one-shot sampler [`Patch`](fontelle_core::Patch) that plays the whole
    /// of `path`, its display name, and how long the file is in seconds.
    ///
    /// The one builder for "a file, as an instrument", shared by the two
    /// callers that turn a file into one — a channel's sampler
    /// ([`sampler_patch_for`](Self::sampler_patch_for)) and the Import tab's
    /// **preview**, which loads it into the preview voice to hear it without
    /// putting it on a channel.
    fn sampler_patch(
        &mut self,
        path: &Path,
    ) -> Result<(String, fontelle_core::Patch, f64), String> {
        let imported = self
            .library
            .import_sample(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        // The **stem**, not the file name: a rack of "Kick.wav", "Snare.wav"
        // is a rack you read around rather than read. An audio *clip* keeps
        // its extension (`file_label`) because there it says what the file is;
        // an instrument's name says what it plays.
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| file_label(path));
        let seconds = if imported.sample_rate > 0 {
            imported.frames as f64 / imported.sample_rate as f64
        } else {
            0.0
        };
        let patch = fontelle_core::Patch {
            layers: vec![fontelle_core::Layer {
                source: fontelle_core::Source::Sample { file: imported.id },
                // Every key: a file has no recorded range to run out of.
                key_range: (0, 127),
                vel_range: (0, 127),
                // Middle C, so the file plays at its own speed there.
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: fontelle_core::PlaybackConfig {
                    // **The whole file.** `end_offset` is an absolute frame
                    // position and its default is zero, which is right for an
                    // oscillator — it has no end — and means "play nothing at
                    // all" for a sample. A sampler built on the default made a
                    // channel that said Sampler in the rack and rendered
                    // silence: *"i clicked and dragged my hardstyle kick in and
                    // it did make a sampler but nothing i did was audible at
                    // all."* SF2 import has always set this (`sf2_import`'s
                    // `buffer_len + end_delta`); this is the same fact for a
                    // file that brings no generators with it.
                    end_offset: imported.frames as f64,
                    ..Default::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            ..fontelle_core::Patch::basic_synth()
        };
        Ok((name, patch, seconds))
    }

    /// The instrument a channel of `kind` arrives with.
    ///
    /// Only the synth arrives able to make a sound; the other two are waiting
    /// for a file, and one that arrived playing a saw would be lying about
    /// what it is (`InstrumentKind::plays_on_arrival`).
    fn starter_patch(
        &self,
        kind: fontelle_types::InstrumentKind,
    ) -> Option<fontelle_types::PatchData> {
        use fontelle_types::InstrumentKind;
        match kind {
            InstrumentKind::Osc3 => fontelle_core::Patch::basic_synth()
                .to_data(self.library.provenance())
                .ok(),
            // A kit references no file, so it arrives complete — the one
            // instrument here that works on a fresh install with nothing
            // configured. `Studio` because it is the neutral one: a menu that
            // handed you an 808 would be making a statement.
            InstrumentKind::DrumMachine => {
                fontelle_core::drum_kit(fontelle_core::DrumKitStyle::default())
                    .to_data(self.library.provenance())
                    .ok()
            }
            // A plugin is not a patch and never becomes one: what a channel
            // playing one holds is a `PluginState` beside its patch, and the
            // browser is what fills it in.
            // The **third** instrument that arrives able to play, and the
            // second that never wants a file: every wavetable it reads is
            // generated from a recipe at first use, so a fresh install with
            // nothing configured plays it.
            InstrumentKind::Flopsynth => fontelle_core::flopsynth::flopsynth_init()
                .to_data(self.library.provenance())
                .ok(),
            // A plugin is not a patch and never becomes one: what a channel
            // playing one holds is a `PluginState` beside its patch, and the
            // browser is what fills it in.
            InstrumentKind::SoundFont | InstrumentKind::Sampler | InstrumentKind::Plugin => None,
        }
    }

    /// Writes `patch` back onto `channel` through the history **without
    /// rebuilding the graph** (`docs/flopsynth-plan.md` §2.3).
    ///
    /// The half of a parameter edit that is about the *document*: the history
    /// entry, the cache and the dirty flag. What makes it audible is the other
    /// half — a `ParamValue` on the live wire, aimed at the channel's own node,
    /// which the running sampler applies between one block and the next.
    ///
    /// **Why this exists.** [`store_patch`](Self::store_patch) rebuilds the
    /// graph, which builds a new `Sampler` and a fresh voice pool. That is
    /// tolerable for a soundfont's release knob and unacceptable for a
    /// wavetable position swept by hand under a held chord — every mouse move
    /// would cut every sounding note. See
    /// [`Self::set_instrument_param`], which is the one place that decides
    /// which of the two an edit is.
    fn store_patch_quiet(&mut self, channel: ChannelId, patch: fontelle_core::Patch) {
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
    }

    /// One patch parameter, onto the live wire, aimed at `channel`'s node.
    ///
    /// The address is spelled the way the document spells it — the same
    /// `channel:<id>/patch/...` an automation lane carries — because the node
    /// reads it with one `find("/patch/")` and a subslice, and two spellings
    /// of the same control would be two things to keep in step.
    fn send_patch_param(
        &mut self,
        channel: ChannelId,
        address: &fontelle_types::ParamAddress,
        value: f32,
    ) {
        let Some(target) = self.channel_nodes.get(&channel).copied() else {
            return;
        };
        let full = fontelle_types::ParamTarget::ChannelPatch {
            channel,
            param: address.as_str().to_string(),
        }
        .address();
        if let Some(sink) = &mut self.audition {
            sink.send(fontelle_types::TimedEvent {
                // Sample zero: the live drain stamps events with the position
                // the audio thread is actually at, so a timestamp from here
                // would only be a guess about a clock this thread cannot read.
                sample: 0,
                target,
                payload: EventPayload::ParamValue {
                    target: full,
                    value: f64::from(value),
                },
            });
        }
    }

    /// Writes `patch` back onto `channel` through the history, coalescing with
    /// its own predecessor so a knob drag is one undo entry.
    /// Loads a sound file onto one of Flopsynth's oscillators, as the table
    /// that oscillator reads.
    ///
    /// > *"i want to like with omnisphere or serum ... be able to drag audio
    /// > files into it to use those waveforms in the synthesis."*
    ///
    /// `layer` is the oscillator's index in the patch — 0..4 for A, B, C, the
    /// sub and the noise, which is the order
    /// [`fontelle_core::flopsynth::layer_role`] fixes.
    ///
    /// The samples are cut into frames of one cycle each and **stored in the
    /// patch** (see `fontelle_core::UserWavetable`), so the instrument stays
    /// self-contained: a preset made this way opens on a machine that has
    /// never seen the file, and nothing here can ever need relinking.
    ///
    /// Three things happen besides the load, and each is a report waiting to
    /// happen if it does not:
    ///
    /// - the oscillator is **switched on**, because every one but the first
    ///   is at the silence floor in the Init patch and a drop that loaded
    ///   silently reads as a drop that did nothing;
    /// - a table the same oscillator was already reading is **replaced**
    ///   rather than added to, since a patch carries its samples and a
    ///   discarded table is dead weight in every copy of the preset from
    ///   then on;
    /// - it goes through `store_patch`, so it is one entry on the history
    ///   and Ctrl+Z takes it back like anything else.
    pub fn load_wavetable(&mut self, layer: usize, path: &Path) -> Result<String, String> {
        let Some(channel) = self.selected_channel_id() else {
            return Err("no channel is selected".to_string());
        };
        let Some(mut patch) = self.selected_patch() else {
            return Err("this channel has no instrument to drop a sound into".to_string());
        };
        if !matches!(
            patch.layers.get(layer).map(|l| &l.source),
            Some(fontelle_core::Source::Synth(_))
        ) {
            return Err("that is not one of this instrument's oscillators".to_string());
        }
        // **Through `import_audio`**, never straight to the decoder: every
        // `.wav` in the packs Fontelle is pointed at is really an Ogg in a
        // RIFF wrapper, and a reader that skips this refuses the whole
        // library. See `fontelle_assets::read_audio`.
        let decoded = fontelle_assets::import_audio(path).map_err(|e| e.to_string())?;
        if decoded.frames == 0 {
            return Err(format!("there is no sound in {}", path.display()));
        }
        // Mono by averaging, the same fold `SampleLibrary::import_sample`
        // uses: a table is one cycle and has no sides.
        let channels = decoded.channels.max(1) as usize;
        let samples: Vec<f32> = decoded
            .samples
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        // As many whole cycles as the file holds, at the table's own frame
        // length — Serum's convention, and what makes a file exported from a
        // wavetable editor come back frame for frame.
        let frames =
            (samples.len() / fontelle_dsp::WAVETABLE_LEN).clamp(1, fontelle_dsp::MAX_USER_FRAMES);
        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Dropped".to_string());
        let table = fontelle_core::UserWavetable {
            name: name.clone(),
            frames,
            samples,
        };

        // Where this oscillator's table goes: over the one it was already
        // reading, or on the end.
        let existing = match &patch.layers[layer].source {
            fontelle_core::Source::Synth(osc) => match osc.source {
                fontelle_dsp::SynthSource::User(at) => Some(at as usize),
                _ => None,
            },
            _ => None,
        };
        let at = match existing.filter(|at| *at < patch.wavetables.len()) {
            Some(at) => {
                patch.wavetables[at] = table;
                at
            }
            None => {
                patch.wavetables.push(table);
                patch.wavetables.len() - 1
            }
        };
        let Ok(index) = u8::try_from(at) else {
            return Err("this instrument is already carrying as many sounds as it can".to_string());
        };
        if let fontelle_core::Source::Synth(osc) = &mut patch.layers[layer].source {
            osc.source = fontelle_dsp::SynthSource::User(index);
        }
        // On, if it was not: see this function's own note.
        if patch.layers[layer].gain_db <= fontelle_core::SILENT_DB {
            patch.layers[layer].gain_db = DROPPED_OSC_DB;
        }
        let role = fontelle_core::flopsynth::layer_role(layer).label();
        self.store_patch(channel, patch);
        self.history.break_gesture();
        self.touch();
        Ok(format!("{name} loaded into {role}"))
    }

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
        // A channel playing a plugin has no patch to clear — the plugin *is*
        // the instrument — so clearing one and not the other would be a menu
        // row that does nothing. Both, in one entry, because taking an
        // instrument off a channel is one thing somebody did.
        let plays_a_plugin = self
            .project
            .channels
            .get(channel)
            .is_some_and(|c| c.plugin.is_some());
        if plays_a_plugin {
            let parts: Vec<Box<dyn fontelle_model::Command>> = vec![
                Box::new(fontelle_model::SetChannelPatch::new(channel, None)),
                Box::new(fontelle_model::SetChannelPlugin::new(channel, None)),
            ];
            self.run(Box::new(fontelle_model::Compound::new(
                "Clear instrument",
                parts,
            )));
            self.history.break_gesture();
            self.channel_presets.remove(&channel);
            self.patch_cache = None;
            self.dirty = true;
            self.rebuild_graph();
            self.touch();
            return;
        }
        self.run(Box::new(fontelle_model::SetChannelPatch::new(
            channel, None,
        )));
        self.history.break_gesture();
        self.channel_presets.remove(&channel);
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        self.touch();
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
            .filter_map(
                |address| match fontelle_types::ParamTarget::parse(address) {
                    Some(fontelle_types::ParamTarget::ChannelGain(id)) if id == channel => Some(
                        fontelle_types::ParamAddress::new(crate::instrument::MIXER_GAIN),
                    ),
                    Some(fontelle_types::ParamTarget::ChannelPan(id)) if id == channel => Some(
                        fontelle_types::ParamAddress::new(crate::instrument::MIXER_PAN),
                    ),
                    Some(fontelle_types::ParamTarget::ChannelPatch { channel: id, param })
                        if id == channel =>
                    {
                        Some(fontelle_types::ParamAddress::new(param))
                    }
                    _ => None,
                },
            )
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
    /// The open clip's notes **on the selected channel** — see
    /// `refresh_roll_notes`. Selecting another instrument in the rack shows
    /// that instrument's notes in the same clip, which is the whole of
    /// *"clips can have multiple instruments"*.
    fn notes(&self) -> &Arena<NoteId, Note> {
        &self.roll_notes
    }

    fn edit(&mut self, edit: RollEdit) -> Vec<NoteId> {
        // Not `self.clip`: an edit lands wherever the roll is looking, which
        // is a prefab when one is picked or when the open clip follows one.
        // Every command below takes `impl Into<NoteHome>`, so this is the only
        // line that had to change when prefabs arrived — and it is the only
        // line that could have got it wrong.
        let clip = self.note_target();
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
            RollEdit::SetLengths { ids, lengths } => {
                // The legato tool. One command, so one press of Ctrl+Z takes
                // the whole phrase back — see `fontelle_model::SetNoteLengths`.
                self.run(Box::new(fontelle_model::SetNoteLengths::new(
                    clip, ids, lengths,
                )));
                self.history.break_gesture();
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

    /// What the song is doing at `position_sample`, which is the box value
    /// bent by the tempo lane.
    ///
    /// Read through `effective_tempo` — **never** `project.tempo_map` — which
    /// is the rule everything that turns a tick into a sample already follows:
    /// the latter is the tempo *box*, the former is that map with the lane
    /// applied, and the two disagree by exactly the automation the report says
    /// the indicator was ignoring.
    fn tempo_at(&self, position_sample: fontelle_types::Sample) -> f64 {
        let tick = self.playhead_song_tick(position_sample);
        self.effective_tempo.tempo_at(tick)
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.run(Box::new(SetNumber::new(NumberTarget::Tempo, bpm)));
        // The click counts in samples, so a new tempo is a new beat length.
        self.publish_metronome();
        // The tempo is the one document value the *timeline* depends on for
        // more than its contents: every tick becomes a different sample.
        // `run` has already republished it. The graph is untouched — nothing
        // in it knows about beats.
        self.touch();
    }

    fn set_beats_per_bar(&mut self, beats: u32) {
        self.run(Box::new(SetNumber::new(
            NumberTarget::BeatsPerBar,
            f64::from(beats),
        )));
        // And a new signature is a new downbeat.
        self.publish_metronome();
        self.touch();
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

    /// How long the open clip is, so the roll can shade what is past it —
    /// a note written out there does not sound (TDD §11.4).
    fn clip_length(&self) -> Option<Tick> {
        Some(self.project.clips.get(self.clip)?.length)
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

    fn has_file(&self) -> bool {
        Session::has_file(self)
    }

    fn save_as(&mut self, name: &str) -> Result<(), String> {
        Session::save_as(self, name)
    }

    fn save(&mut self) -> Result<(), String> {
        self.capture_plugin_states();
        // No file yet is not an error any more — the window asks for a name
        // first (see `has_file`), and this is what a save reached any other
        // way does. `save_as` is where the folder and the name are decided.
        let Some(bundle) = self.bundle.clone() else {
            return Session::save_as(self, &self.project.meta.name.clone());
        };
        crate::save_project(&self.project, &bundle).map_err(|e| e.to_string())?;
        self.dirty = false;
        self.touch();
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
    fn insert(&mut self, clip: fontelle_model::NoteHome, mut notes: Vec<Note>) -> Vec<NoteId> {
        if notes.is_empty() {
            return Vec::new();
        }
        // **On the selected channel**, whatever the note said and whatever
        // the clip's own channel is: the rack's selection is the instrument
        // every interaction means. A note drawn, pasted or recorded while
        // the bass is selected is a bass note, in whichever clip is open.
        // `None` when that is the clip's own channel, so a clip that never
        // mixes instruments is saved exactly as it always was.
        let home = self.home_channel(clip);
        let selected = self.selected_channel_id();
        for note in &mut notes {
            note.channel = match (selected, home) {
                (Some(selected), Some(home)) if selected == home => None,
                (Some(selected), _) => Some(selected),
                (None, _) => None,
            };
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
            self.touch();
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
        self.touch();
    }

    /// The **audio** file row `index` of the Import tab stands for.
    ///
    /// The one lookup both ways of making a sampler share, so the guard is
    /// stated once: a `.mid` is a score and a `.fsc` is a phrase, neither is a
    /// sound a sampler can play, and turning one into a silent instrument
    /// would be worse than saying so.
    fn import_audio_row(&mut self, index: usize) -> Result<PathBuf, String> {
        self.ensure_import_bank();
        if self.import_kind != fontelle_types::FolderKind::Audio {
            return Err("only an audio file can become a sampler".to_string());
        }
        self.import_rows()
            .get(index)
            .cloned()
            .flatten()
            .ok_or_else(|| "that file is not in the folder any more".to_string())
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
    fn bring_in(
        &mut self,
        what: &str,
        parts: Vec<ImportPart>,
        bpm: Option<f64>,
    ) -> Result<String, String> {
        if parts.is_empty() {
            return Err(format!("there is nothing in {what} to import"));
        }
        let names: Vec<String> = parts.iter().map(|part| part.name.clone()).collect();
        let empty = self.project.clips.is_empty();
        let command = Box::new(ImportParts::new(what.to_string(), parts));
        let made = self.apply_for::<ImportParts>(command)?.made().to_vec();
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
    fn import_midi_file(
        &mut self,
        path: &Path,
        which: Option<MidiChannels>,
    ) -> Result<String, String> {
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
        // Where it lands: the time selection's start if there is one, and
        // otherwise the top of the song. The same rule an imported score
        // follows.
        let start = self
            .project
            .loop_range
            .map(|(from, _)| from.max(0))
            .unwrap_or(0);
        let at = self.project.tempo_map.tick_to_sample(start);
        let name = self.import_audio_at(path, at, None, None)?;
        Ok(format!("Imported \u{201c}{name}\u{201d}"))
    }

    /// Brings a sound in at song sample `at`, routed to `track`, and hands back
    /// what it is called.
    ///
    /// The one path a sound arrives by, whether it was dropped on the window,
    /// picked out of the Import tab, or just recorded — so the waveform, the
    /// editor, the playback and the undo are one code path rather than two that
    /// can disagree.
    fn import_audio_at(
        &mut self,
        path: &Path,
        at: fontelle_types::Sample,
        track: Option<MixerTrackId>,
        onto: Option<fontelle_types::LaneId>,
    ) -> Result<String, String> {
        let name = file_label(path);
        let imported = self.library.import_audio(path).map_err(|e| e.to_string())?;
        if imported.frames == 0 || imported.sample_rate == 0 {
            return Err(format!("there is no sound in {name}"));
        }

        let start = self.project.tempo_map.sample_to_tick(at.max(0)).max(0);
        // Its own duration in ticks, through the tempo map that already owns
        // every sample-to-tick conversion in the project.
        let samples = (imported.frames as f64 * self.options.sample_rate as f64
            / f64::from(imported.sample_rate)) as i64;
        let from = self.project.tempo_map.tick_to_sample(start);
        let length = self.project.tempo_map.sample_to_tick(from + samples) - start;
        let length = length.max(fontelle_model::MIN_CLIP_LENGTH);

        let mut data = fontelle_types::AudioClipData::whole(
            imported.asset.clone(),
            imported.frames as fontelle_types::Sample,
            imported.sample_rate,
        );
        data.mixer_track = track;
        let mut clip = fontelle_model::AddAudioClip::new(name.clone(), data, start, length);
        // Onto the row the pointer was over, when the drop named one; otherwise
        // a row of its own past the bottom (`AddAudioClip`'s default).
        if let Some(lane) = onto {
            clip = clip.on_lane(lane);
        }
        let command = Box::new(clip);
        self.apply_for::<fontelle_model::AddAudioClip>(command)?;
        // The graph has to be rebuilt: the player nodes hold the audio store,
        // and the one they are holding does not have this file in it.
        self.rebuild_graph();
        self.republish();
        Ok(name)
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
        let ids = self.insert(self.note_target(), notes);
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
        self.touch();
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
                // A channel playing a plugin has an instrument even though it
                // has no patch: the plugin *is* the instrument (TDD §8.4).
                has_instrument: channel.patch_data.is_some() || channel.plugin.is_some(),
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
        // The roll shows this channel's notes **in the clip that is open**
        // — that is what a shared clip is for. Only when nothing is open,
        // or what is open is not a note clip, does it go looking for one
        // this channel plays.
        if !matches!(
            self.project.clips.get(self.clip).map(|c| &c.source),
            Some(ClipSource::Notes(_))
        ) && let Some(clip) = self
            .clip_of_channel(channel)
            .or_else(|| Self::first_clip(&self.project))
        {
            self.clip = clip;
        }
        self.touch();
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
        self.touch();
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
        self.touch();
    }

    // ----------------------------------------------------- prefabs (§10.5) ---

    fn rack_tab(&self) -> fontelle_ui::document::RackTab {
        self.rack_tab
    }

    fn set_rack_tab(&mut self, tab: fontelle_ui::document::RackTab) {
        if self.rack_tab == tab {
            return;
        }
        self.rack_tab = tab;
        // Leaving the prefab list puts the roll back on the arrangement. A
        // prefab that stayed open behind the instruments tab would be a roll
        // editing something the panel is no longer showing, and the first
        // note drawn would land somewhere nobody could see.
        if tab != fontelle_ui::document::RackTab::Prefabs {
            self.prefab = None;
        }
        self.touch();
    }

    fn prefabs(&self) -> Vec<fontelle_ui::document::PrefabInfo> {
        let open = self.prefab;
        self.prefab_ids()
            .into_iter()
            .filter_map(|id| {
                let prefab = self.project.prefabs.get(id)?;
                Some(fontelle_ui::document::PrefabInfo {
                    name: prefab.name.clone(),
                    uses: self.project.prefab_instances(id).len(),
                    open: Some(id) == open,
                })
            })
            .collect()
    }

    fn add_prefab(&mut self) {
        let Some(channel) = self.prefab_home_channel() else {
            self.message =
                Some("a prefab holds notes, and there is no instrument to play them".to_string());
            return;
        };
        let name = self.next_prefab_name();
        let source = ClipSource::Notes(fontelle_model::NoteData {
            channel,
            notes: Arena::default(),
        });
        let made = match self.apply_for::<fontelle_model::AddPrefab>(Box::new(
            fontelle_model::AddPrefab::new(name, source),
        )) {
            Ok(add) => add.prefab(),
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        self.history.break_gesture();
        // Selected, because you pressed the plus to write something in it.
        self.prefab = made;
        self.rack_tab = fontelle_ui::document::RackTab::Prefabs;
        self.dirty = true;
        self.touch();
    }

    fn rename_prefab(&mut self, index: usize, name: &str) {
        let Some(id) = self.prefab_ids().get(index).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::RenamePrefab::new(id, name)));
        self.dirty = true;
        self.touch();
    }

    fn remove_prefab(&mut self, index: usize) {
        let Some(id) = self.prefab_ids().get(index).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::RemovePrefab::new(id)));
        self.history.break_gesture();
        if self.prefab == Some(id) {
            self.prefab = None;
        }
        self.dirty = true;
        self.republish();
    }

    fn selected_prefab(&self) -> Option<usize> {
        let open = self.prefab?;
        self.prefab_ids().iter().position(|id| *id == open)
    }

    fn select_prefab(&mut self, index: Option<usize>) {
        self.prefab = index.and_then(|index| self.prefab_ids().get(index).copied());
        if self.prefab.is_some() {
            self.rack_tab = fontelle_ui::document::RackTab::Prefabs;
        }
        self.touch();
    }

    fn draw_prefab(&mut self, index: usize, lane: usize, start: Tick) -> Option<ClipId> {
        let id = self.prefab_ids().get(index).copied()?;
        let lane = self.lane_ids().get(lane).copied()?;
        // A bar, the same as the draw tool's own default: a place is a window
        // on the prefab's content and the person who drew it decides how wide.
        let length = fontelle_types::PPQN * 4;
        let made = match self.apply_for::<fontelle_model::AddPrefabInstance>(Box::new(
            fontelle_model::AddPrefabInstance::new(id, lane, start.max(0), length),
        )) {
            Ok(place) => place.clip(),
            Err(e) => {
                self.message = Some(e);
                return None;
            }
        };
        self.history.break_gesture();
        self.dirty = true;
        self.republish();
        made
    }

    fn detach_prefab(&mut self, clip: ClipId) {
        self.run(Box::new(fontelle_model::DetachPrefab::new(clip)));
        self.history.break_gesture();
        self.dirty = true;
        self.republish();
    }

    fn make_prefab_from(&mut self, clip: ClipId) -> Result<(), String> {
        // Checked here as well as in the command, so the studio can *say* why
        // rather than only refusing: the message goes to the status line.
        if !self.project.clips.contains_key(clip) {
            return Err("that block is not in this project".to_string());
        }
        let name = self.next_prefab_name();
        let made = match self.apply_for::<fontelle_model::MakePrefabFromClip>(Box::new(
            fontelle_model::MakePrefabFromClip::new(clip, name),
        )) {
            Ok(make) => make.prefab(),
            Err(e) => {
                self.message = Some(e.clone());
                return Err(e);
            }
        };
        self.history.break_gesture();
        // The clip keeps its id, so the roll is still on the block it was on —
        // which is the whole reason the command works in place. Opening it
        // again would be a no-op; what does need saying is that the *prefab*
        // is now a thing the list shows.
        let _ = made;
        self.dirty = true;
        self.republish();
        Ok(())
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
        self.touch();
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

    fn track_output_on(&self, strip: usize) -> bool {
        self.mixer_track_ids()
            .get(strip)
            .and_then(|id| self.project.mixer.tracks.get(*id))
            .is_none_or(|track| track.output_on)
    }

    fn set_track_output_on(&mut self, strip: usize, on: bool) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::SetFlag::new(
            fontelle_model::FlagTarget::TrackOutputOn(id),
            on,
        )));
        self.history.break_gesture();
        // The bus sum is the edge, so switching it changes the *shape* of the
        // schedule rather than a value in it — see `realise`.
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
        self.touch();
    }

    /// What the browser's first list shows: the folder you are standing in,
    /// or — the moment anything is typed — the whole collection.
    ///
    /// **The search box decides which.** Browsing is a question about
    /// structure and searching is a question about names, and a search that
    /// only looked in the folder you happened to be standing in would not be
    /// §17.5's "instant fuzzy search over a large collection".
    fn library_files(&self) -> Vec<LibraryEntry> {
        if self.browser_mode == fontelle_ui::canvas::BrowserMode::Presets {
            return self.preset_tab_devices();
        }
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

        // Flopsynth first, above the bank: the built-in instrument does not
        // live in the soundfont folder and there is nothing to configure
        // before it works, so it is the one row that is always there.
        let flopsynth = self.shows_flopsynth_row().then(|| LibraryEntry {
            name: "Flopsynth".to_string(),
            detail: format!(
                "{} presets",
                fontelle_core::flopsynth::presets::FACTORY.len()
            ),
            kind: fontelle_ui::document::LibraryKind::Folder,
        });
        flopsynth
            .into_iter()
            .chain(self.bank.rows().iter().map(|row| match row {
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
            }))
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
        self.touch();
    }

    fn set_query(&mut self, query: &str) {
        if self.browser_mode == fontelle_ui::canvas::BrowserMode::Import {
            self.import_query = query.to_string();
            // The list is a different list now — the whole folder tree rather
            // than one folder, or the other way round.
            self.touch();
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
        self.touch();
    }

    fn open_file(&mut self, index: usize) -> Result<(), String> {
        if self.browser_mode == fontelle_ui::canvas::BrowserMode::Presets {
            return self.open_preset_device(index);
        }
        // Flopsynth's own row, when it is showing: opening it puts its
        // hundred and twenty-eight presets in the list below, the same way
        // opening a soundfont puts that file's presets there.
        let offset = self.flopsynth_row_offset();
        if offset > 0 && index == 0 {
            self.flopsynth_open = true;
            self.open_file = None;
            self.presets.clear();
            self.touch();
            return Ok(());
        }
        let index = index - offset;
        // Anything else in the list is a soundfont or a folder, so the bank is
        // no longer what is open.
        self.flopsynth_open = false;
        // A folder row moves the browser and opens nothing. Only while
        // *browsing*: a search lists files wherever they are, and a hit is
        // always a file.
        if self.query.trim().is_empty() && self.bank.open_row(index) {
            // The rows under the pointer are different ones now, and the file
            // that was open is not in this folder.
            self.open_file = None;
            self.presets.clear();
            self.touch();
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
        self.touch();
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
        if self.flopsynth_open {
            // The one row that is not a file: it is highlighted by being the
            // one that is open, which is what `open_file` recorded.
            return self.shows_flopsynth_row().then_some(0);
        }
        let open = self.open_file.as_ref()?;
        // Worked out from the path each time, so a folder change or a search
        // moves the highlight to wherever the file now is — or removes it,
        // which is true when the file is not in the list at all.
        let offset = self.flopsynth_row_offset();
        self.file_rows()
            .into_iter()
            .position(|row| row.as_ref() == Some(open))
            .map(|at| at + offset)
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
        self.touch();
        let _ = channel;
        Ok(())
    }

    fn plugin_instruments(&self) -> Vec<fontelle_ui::PluginListing> {
        listings(self.plugins.scan().instruments())
    }

    fn plugin_effects(&self) -> Vec<fontelle_ui::PluginListing> {
        listings(self.plugins.scan().effects())
    }

    fn scan_plugins_once(&mut self) {
        self.plugins.scan_once();
    }

    fn favorites(&self) -> Vec<fontelle_types::Favorite> {
        self.settings.favorites.clone()
    }

    /// A star pressed. Written to the settings file at once, like a folder
    /// choice: there is no "save" for the settings, and a star that only
    /// lasted the session would be a star that forgot.
    ///
    /// **Not** `touch()`ed and not a command: the project did not change.
    fn toggle_favorite(&mut self, favorite: fontelle_types::Favorite) {
        let name = match &favorite {
            fontelle_types::Favorite::Effect(kind) => kind.label().to_string(),
            fontelle_types::Favorite::Instrument(kind) => kind.label().to_string(),
            fontelle_types::Favorite::Plugin(key) => self
                .plugins
                .scan()
                .plugins
                .iter()
                .find(|plugin| plugin.key == *key)
                .map_or_else(|| key.to_string(), |plugin| plugin.name.clone()),
            // The preset's own name, which is what the row said: a star on
            // "Choir Ahh" reads back as "Choir Ahh" and not as "Flopsynth".
            fontelle_types::Favorite::Preset { name, .. } => name.clone(),
        };
        let starred = self.settings.toggle_favorite(favorite);
        self.message = Some(if starred {
            format!("Favorite: {name}")
        } else {
            format!("No longer a favorite: {name}")
        });
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
    }

    fn open_plugin_editor_for_channel(&mut self, index: usize) -> bool {
        let Some(id) = self.channel_ids().get(index).copied() else {
            return false;
        };
        self.open_plugin_editor(crate::PluginSlot::Channel(id))
    }

    fn open_plugin_editor_for_insert(&mut self, strip: usize, slot: usize) -> bool {
        let Some(track) = self.mixer_track_ids().get(strip).copied() else {
            return false;
        };
        self.open_plugin_editor(crate::PluginSlot::Insert { track, slot })
    }

    fn tick_plugin_editors(&mut self) -> bool {
        let open = self.plugins.tick_editors();
        // An open editor keeps the audio thread running the graph, because
        // an LV2 editor reaches its plugin only through `run` — see
        // `fontelle_engine::IdleGate::set_attended`.
        if let Some(transport) = &self.transport {
            transport.set_attended(open);
        }
        open
    }

    fn rescan_plugins(&mut self) {
        let (found, failed) = self.scan_plugins();
        self.message = Some(match (found, failed) {
            (0, 0) => "no plugins found".to_string(),
            (n, 0) => format!("{n} plugins"),
            (n, f) => format!("{n} plugins, {f} would not load"),
        });
        self.touch();
    }

    fn add_plugin_channel(&mut self, which: usize) {
        let Some(state) = self.instrument_state(which) else {
            return;
        };
        let name = state.name.clone();
        let channel = match self.new_channel_of(name, None, fontelle_types::InstrumentKind::Plugin)
        {
            Ok(channel) => channel,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        self.run(Box::new(fontelle_model::SetChannelPlugin::new(
            channel,
            Some(state),
        )));
        self.history.break_gesture();
        self.patch_cache = None;
        self.rebuild_graph();
        self.touch();
    }

    fn set_channel_plugin(&mut self, channel: usize, which: usize) {
        let Some(id) = self.channel_ids().get(channel).copied() else {
            return;
        };
        let Some(state) = self.instrument_state(which) else {
            return;
        };
        // **The rack has to say what is on it.** Choosing a plugin used to
        // name a channel only when it *made* one, so a Surge XT channel given
        // Calf Organ instead still read "Surge XT" — in the rack and in the
        // title of its own window. Found by driving the window.
        //
        // Only when the name is still the one the last plugin was given:
        // "recognised, not remembered", the rule the preset chips follow. A
        // channel somebody called "Lead" stays "Lead".
        let named_by_its_plugin = self
            .project
            .channels
            .get(id)
            .is_some_and(|c| c.plugin.as_ref().is_some_and(|was| was.name == c.name));
        let rename = named_by_its_plugin.then(|| state.name.clone());
        let mut parts: Vec<Box<dyn Command>> = vec![Box::new(
            fontelle_model::SetChannelPlugin::new(id, Some(state)),
        )];
        if let Some(name) = rename {
            parts.push(Box::new(fontelle_model::RenameChannel::new(id, name)));
        }
        // One entry: "put this plugin on this channel" is one thing somebody
        // did and one press of Ctrl+Z.
        self.run(Box::new(fontelle_model::Compound::new(
            "Choose plugin",
            parts,
        )));
        self.history.break_gesture();
        self.channel_presets.remove(&id);
        self.patch_cache = None;
        self.rebuild_graph();
        self.touch();
    }

    fn add_plugin_insert(&mut self, strip: usize, which: usize) {
        let Some(track) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        let Some(found) = self.plugins.scan().effects().nth(which) else {
            return;
        };
        let state = self.plugins.state_for(found);
        self.run(Box::new(fontelle_model::AddPluginInsert::new(track, state)));
        self.history.break_gesture();
        self.rebuild_graph();
        self.touch();
    }

    fn add_channel_of(&mut self, kind: fontelle_types::InstrumentKind) -> Result<(), String> {
        // Named for what it is rather than "Channel N": a rack of "Sampler",
        // "3OSC", "SoundFont" tells you what you are looking at, and a rack of
        // "Channel 4" does not.
        let name = format!(
            "{} {}",
            kind.default_name(),
            self.project.channels.len() + 1
        );
        let patch = self.starter_patch(kind);
        let channel = self.new_channel_of(name, patch, kind)?;
        self.patch_cache = None;
        self.rebuild_graph();
        self.touch();
        let _ = channel;
        Ok(())
    }

    fn channel_kind(&self, index: usize) -> Option<fontelle_types::InstrumentKind> {
        Session::channel_kind(self, index)
    }

    fn set_channel_kind(&mut self, index: usize, kind: fontelle_types::InstrumentKind) {
        let Some(id) = self.channel_ids().get(index).copied() else {
            return;
        };
        // **Already that kind is not a change.** Choosing "3OSC" on a 3OSC you
        // have spent ten minutes editing must not hand you a fresh one, and a
        // menu press that quietly discards work is the worst thing this could
        // do. Nothing is applied, so it is not a history entry either.
        if Session::channel_kind(self, index) == Some(kind) {
            return;
        }
        // The label and the instrument move together, in one entry: "make this
        // a sampler" is one thing somebody did and one press of Ctrl+Z.
        let patch = self.starter_patch(kind);
        let parts: Vec<Box<dyn fontelle_model::Command>> = vec![
            Box::new(fontelle_model::SetChannelKind::new(id, kind)),
            Box::new(fontelle_model::SetChannelPatch::new(id, patch)),
        ];
        if let Err(e) = self.history.apply(
            Box::new(fontelle_model::Compound::new("Choose instrument", parts)),
            &mut self.project,
        ) {
            self.message = Some(e.to_string());
            return;
        }
        self.history.break_gesture();
        // The channel is not playing that preset any more, whatever it was.
        self.channel_presets.remove(&id);
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        self.touch();
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
        self.run(Box::new(fontelle_model::RemoveChannel::new(id)));
        self.history.break_gesture();
        self.channel_presets.remove(&id);
        self.patch_cache = None;
        self.selected = self
            .selected
            .min(self.project.channels.len().saturating_sub(1));
        // The roll cannot go on showing a clip that is not there.
        if !self.project.clips.contains_key(self.clip)
            && let Some(next) = Self::first_clip(&self.project)
        {
            self.clip = next;
        }
        self.refresh_roll_notes();
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
        self.touch();
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
        self.touch();
    }

    fn render_lane(&mut self, index: usize, span: Option<(Tick, Tick)>) -> Result<String, String> {
        Session::render_lane(self, index, span)
    }

    fn add_lane_at(&mut self, index: usize) {
        // Named for the stack's size and not for its position: two rows called
        // "Lane 4" is worse than a row called "Lane 11" sitting third.
        let name = format!("Lane {}", self.project.lanes.len() + 1);
        self.run(Box::new(fontelle_model::AddLane::at(name, index)));
        self.history.break_gesture();
        self.dirty = true;
        self.touch();
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
        self.touch();
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

    fn export_midi(&mut self) -> Result<String, String> {
        Session::export_midi(self)
    }

    fn autosave(&mut self) -> bool {
        Session::autosave(self)
    }

    fn new_project(&mut self) -> Result<(), String> {
        Session::new_project_named(self, "Untitled")
    }

    fn new_project_named(&mut self, name: &str) -> Result<(), String> {
        Session::new_project_named(self, name)
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

    // --- the start menu ---

    fn recent_projects(&self) -> Vec<RecentProject> {
        Session::recent_projects(self)
    }

    fn open_project_path(&mut self, path: &Path) -> Result<(), String> {
        Session::open_project_path(self, path)
    }

    fn forget_recent(&mut self, index: usize) {
        Session::forget_recent(self, index)
    }

    fn has_projects_dir(&self) -> bool {
        self.projects.dir().is_some()
    }

    fn choose_and_open_project(&mut self) -> Result<bool, String> {
        let start = self
            .bundle
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let start = start.or_else(|| self.projects.dir().map(Path::to_path_buf));
        match crate::desktop::choose_folder("Open a project", start.as_deref()) {
            Ok(Some(path)) => Session::open_project_path(self, &path).map(|()| true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn update_status(&self) -> UpdateStatus {
        self.updater.status()
    }

    fn check_for_updates(&mut self) {
        self.updater.check();
    }

    fn upgrade(&mut self) {
        match std::env::current_exe() {
            Ok(exe) => self.updater.upgrade(exe),
            Err(e) => self.message = Some(format!("could not find this binary: {e}")),
        }
    }

    fn open_release_page(&mut self) {
        let page = self
            .updater
            .release_page()
            .unwrap_or_else(|| crate::updates::RELEASES_PAGE.to_string());
        self.open_url(&page);
    }

    fn open_url(&mut self, url: &str) {
        if let Err(e) = crate::desktop::open_url(url) {
            self.message = Some(e);
        }
    }

    fn settings(&self) -> Vec<LibraryEntry> {
        crate::settings::setting_rows(&self.settings)
            .iter()
            .map(|row| LibraryEntry::file(row.label(&self.settings), row.value(&self.settings)))
            .collect()
    }

    fn setting_controls(&self) -> Vec<fontelle_ui::canvas::SettingControl> {
        use crate::settings::SettingControlKind as K;
        use fontelle_ui::canvas::SettingControl;
        let midi = &self.settings.midi_input;
        crate::settings::setting_rows(&self.settings)
            .iter()
            .map(|row| match row.control_kind() {
                K::Heading => SettingControl::Heading,
                K::Button => SettingControl::Button,
                K::Slider => SettingControl::Slider {
                    fraction: row.fraction(midi).unwrap_or(0.0),
                },
                K::Choice => {
                    let (options, chosen) = row.choices(midi).unwrap_or_default();
                    SettingControl::Choice { options, chosen }
                }
                // The one switch reads its state off the whole settings, not
                // the MIDI half — see `nudge_setting`, which flips it here too.
                K::Switch => SettingControl::Switch {
                    on: self.settings.check_for_updates,
                },
            })
            .collect()
    }

    fn set_setting_fraction(&mut self, index: usize, fraction: f32) {
        let rows = crate::settings::setting_rows(&self.settings);
        let Some(row) = rows.get(index).copied() else {
            return;
        };
        let before = self.settings.midi_input;
        row.set_fraction(&mut self.settings.midi_input, fraction);
        if self.settings.midi_input == before {
            return;
        }
        self.write_input_settings();
    }

    fn choose_setting(&mut self, index: usize, option: usize) {
        let rows = crate::settings::setting_rows(&self.settings);
        let Some(row) = rows.get(index).copied() else {
            return;
        };
        let before = self.settings.midi_input;
        row.choose(&mut self.settings.midi_input, option);
        if self.settings.midi_input == before {
            return;
        }
        self.write_input_settings();
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
        let rows = crate::settings::setting_rows(&self.settings);
        let Some(row) = rows.get(index) else {
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
            self.touch();
            return;
        }
        // The plugin buttons, for the same reason and by the same rule: a row
        // that opens a picker is not a value to step.
        if row.is_plugin_row() {
            match row {
                crate::settings::SettingRow::PluginFolder => self.choose_plugin_dir(),
                crate::settings::SettingRow::PluginDir(which) => self.remove_plugin_dir(*which),
                crate::settings::SettingRow::ImportFlFolders => self.import_fl_folders(),
                _ => <Self as StudioHost>::rescan_plugins(self),
            }
            self.touch();
            return;
        }
        // An extension row is a button too: install it, or remove it.
        if let crate::settings::SettingRow::Extension(index) = *row {
            self.press_extension(index);
            self.touch();
            return;
        }
        // A switch, by the same rule: not a value to step, so it is flipped
        // here rather than in `nudge`, which only sees the MIDI half.
        if *row == crate::settings::SettingRow::CheckForUpdates {
            self.settings.check_for_updates = !self.settings.check_for_updates;
            if let Err(e) = self.save_settings() {
                self.message = Some(format!("could not write settings: {e}"));
            }
            self.touch();
            return;
        }
        let before = self.settings.midi_input;
        row.nudge(&mut self.settings.midi_input, delta);
        if self.settings.midi_input == before {
            // A heading, or a number already at its end. Neither is worth a
            // write to disk or a redraw.
            return;
        }
        self.write_input_settings();
    }

    fn settings_confirm(&self, index: usize) -> Option<String> {
        let rows = crate::settings::setting_rows(&self.settings);
        // Only the irreversible destructive press asks first: uninstalling an
        // installed extension, which a click cannot bring back (it is a
        // download). Everything else acts and offers an undo instead.
        if let crate::settings::SettingRow::Extension(which) = rows.get(index).copied()? {
            let extension = crate::extensions::CATALOGUE.get(which)?;
            if crate::extensions::is_installed(extension) {
                return Some(format!("Remove the {} extension?", extension.name));
            }
        }
        None
    }

    fn take_settings_toast(&mut self) -> Option<(String, bool)> {
        self.settings_toast.take()
    }

    fn undo_settings(&mut self) -> Option<String> {
        let (index, path) = self.settings_undo.take()?;
        let shown = crate::desktop::elide_path(&path, 2);
        let at = index.min(self.settings.plugin_dirs.len());
        self.settings.plugin_dirs.insert(at, path);
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
        self.plugins.set_folders(self.settings.plugin_dirs.clone());
        <Self as StudioHost>::rescan_plugins(self);
        self.touch();
        Some(format!("Searching {shown} again"))
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

    fn add_sampler_from_import(&mut self, index: usize) -> Result<(), String> {
        let path = self.import_audio_row(index)?;
        let name = Session::add_sampler_from(self, &path)?;
        self.message = Some(format!("{name} \u{2014} sampler"));
        Ok(())
    }

    fn set_sampler_from_import(&mut self, channel: usize, index: usize) -> Result<(), String> {
        let path = self.import_audio_row(index)?;
        let name = Session::set_channel_sampler_from(self, channel, &path)?;
        self.message = Some(format!("{name} \u{2014} sampler"));
        Ok(())
    }

    fn set_channel_instrument_on(&mut self, channel: usize, preset: usize) -> Result<(), String> {
        let channel = self
            .channel_ids()
            .get(channel)
            .copied()
            .ok_or("there is no channel there")?;
        self.install_preset(channel, preset)
    }

    fn open_import(&mut self, index: usize) -> Result<(), String> {
        self.ensure_import_bank();
        // A folder row moves the browser and opens nothing. Only while
        // *browsing*: a search lists files wherever they are, and a hit is
        // always a file.
        if self.import_query.trim().is_empty() && self.import_bank.open_row(index) {
            self.touch();
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
                self.touch();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn drop_import_at(
        &mut self,
        index: usize,
        at: fontelle_types::Sample,
        lane: Option<usize>,
    ) -> Result<(), String> {
        // `import_audio_row` is the one place that turns a row into a path,
        // and it is the place that refuses a row that is not a sound —
        // dragging is only armed for those (`canvas::browser_row_carries`),
        // and this is the second half of that claim rather than a repeat of
        // it.
        let path = self.import_audio_row(index)?;
        // The drop's row index into a LaneId. A row that vanished between the
        // drop and here falls back to a new row rather than refusing.
        let onto = lane.and_then(|index| self.project.lane_ids().get(index).copied());
        let name = self.import_audio_at(&path, at.max(0), None, onto)?;
        self.message = Some(format!("Imported \u{201c}{name}\u{201d}"));
        self.touch();
        Ok(())
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
            choices.push(format!(
                "Only \u{201c}{}\u{201d} \u{2014} {} notes",
                part.name, part.notes
            ));
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
        self.touch();
    }

    fn cancel_import(&mut self) {
        if self.pending_import.take().is_some() {
            self.touch();
        }
    }

    fn drop_file(&mut self, path: &Path) -> Result<String, String> {
        self.drop_file_at(path, 0)
    }

    fn drop_file_at(&mut self, path: &Path, at: fontelle_types::Sample) -> Result<String, String> {
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
            // **Where it was dropped.** A clip is a stretch of song, so the
            // bar you let go over is the bar it starts on; every other kind
            // here has no position to be given.
            return self
                .import_audio_at(path, at.max(0), None, None)
                .map(|name| format!("Imported \u{201c}{name}\u{201d}"));
        }
        if crate::bank::is_soundfont(path) {
            // Not an import into the song: a soundfont is an *instrument*, so
            // dropping one puts it on the selected channel the way clicking
            // one in the browser does.
            self.presets = fontelle_assets::list_presets(path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            self.open_file = Some(path.to_path_buf());
            self.touch();
            self.set_channel_instrument(0)?;
            return Ok(format!("Loaded \u{201c}{name}\u{201d}"));
        }
        Err(format!(
            "{name} is not something Fontelle can open \u{2014} it reads .wav, .flac, .mp3, \
             .ogg, .mid, .fsc and .sf2 files"
        ))
    }

    fn record_mode(&self) -> fontelle_ui::transport::RecordMode {
        self.record_mode
    }

    fn set_record_mode(&mut self, mode: fontelle_ui::transport::RecordMode) {
        self.record_mode = mode;
        self.touch();
    }

    fn audio_inputs(&self) -> Vec<String> {
        // Asked of the host every time rather than cached: a microphone
        // plugged in while the window is open should appear in the menu, and
        // enumerating a handful of devices costs nothing beside opening one.
        let mut names = fontelle_engine::AudioDevice::default_host().input_names();
        // The list is filtered by **asking each device whether it will open**,
        // which is the right question and has one blind spot now that a stream
        // stays open for monitoring: a backend that hands out capture
        // exclusively refuses the probe for the device this program is already
        // holding, and the one input that is definitely working would be the
        // one missing from its own menu.
        if let Some((_, open)) = &self.input_open
            && !names.iter().any(|name| name == open)
        {
            names.insert(0, open.clone());
        }
        names
    }

    fn track_input(&self, strip: usize) -> Option<String> {
        let id = self.mixer_track_ids().get(strip).copied()?;
        self.project.mixer.tracks.get(id)?.input.clone()
    }

    fn set_track_input(&mut self, strip: usize, input: Option<String>) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        self.run(Box::new(fontelle_model::SetTrackInput::new(id, input)));
        self.history.break_gesture();
        // Choosing an input is what opens the microphone and what puts the
        // monitor node on that strip — not pressing record. See
        // `sync_audio_input`.
        self.sync_audio_input();
    }

    /// Starts **keeping** what the input delivers.
    ///
    /// The stream itself is opened by `sync_audio_input` the moment a track
    /// names an input, because that is what monitoring is; this is the half
    /// that says a take is being recorded, and it is why arming does not have
    /// to reopen a device that is already running.
    fn open_audio_input(&mut self) -> Result<String, String> {
        self.sync_audio_input();
        let Some(name) = self.audio_input_wanted() else {
            return Err(
                "no track has an input \u{2014} click a strip's input button and choose one".into(),
            );
        };
        if self.input.is_none() {
            // Read rather than taken: the memo is what stops a dead device
            // being retried on every frame, and clearing it here would make
            // pressing record the thing that started that.
            return Err(match &self.input_failed {
                Some(_) => format!("could not open \u{201c}{name}\u{201d}"),
                None => format!("\u{201c}{name}\u{201d} is not open"),
            });
        }
        if !self.capturing {
            // Whatever arrived before record was pressed belongs to nobody:
            // the stream has been running for as long as the input has been
            // chosen, so the ring is holding however long ago that was.
            // Drained while `capturing` is still false, which is what makes
            // this a discard rather than a very long take.
            self.pump();
            self.input_take.clear();
            self.capturing = true;
        }
        Ok(name)
    }

    /// Stops keeping it. The stream stays open if a track still names an
    /// input, because you go on hearing yourself after a take ends.
    fn close_audio_input(&mut self) {
        self.capturing = false;
        self.input_take.clear();
    }

    fn discard_audio_take(&mut self) {
        // The ring first: what is in it is the count-in bar, and leaving it
        // there would put a woodblock at the front of the take.
        self.pump();
        self.input_take.clear();
    }

    fn samples_per_beat(&self) -> fontelle_types::Sample {
        i64::from(crate::beat_samples(&self.project))
    }

    fn keep_audio_take(
        &mut self,
        at: fontelle_types::Sample,
        end_sample: fontelle_types::Sample,
    ) -> Result<usize, String> {
        Session::keep_audio_take(self, at, end_sample)
    }

    fn audio_clip(&self, clip: ClipId) -> Option<fontelle_types::AudioClipData> {
        match &self.project.clips.get(clip)?.source {
            ClipSource::Audio(data) => Some(data.clone()),
            _ => None,
        }
    }

    fn audio_clip_rate(&self, clip: ClipId) -> u32 {
        // Off the clip, which carries it — see `AudioClipData::sample_rate`.
        // It used to be looked up in the audio store, which meant a clip whose
        // file had not been decoded yet read as "no rate" and showed its fades
        // in nothing.
        match self.project.clips.get(clip).map(|c| &c.source) {
            Some(ClipSource::Audio(data)) => data.sample_rate,
            _ => 0,
        }
    }

    fn set_audio_clip(&mut self, clip: ClipId, data: fontelle_types::AudioClipData) {
        // The clip's length on the arrangement follows its trim: a clip that
        // draws four bars and plays two is the picture lying about the sound.
        // Not its *speed*, though — a clip played at half speed still occupies
        // the block it was given, which is what makes a loop stretchable.
        // `run` republishes and bumps the revision, so the block on the
        // arrangement redraws with its new fades on the same frame.
        //
        // The editor's stretch chooser goes through here, and it is the same
        // switch the arrangement's toolbar is: a change of mode keeps the
        // sound where it was (`fontelle_model::with_stretch`), or the two
        // ways of turning stretch off would leave two different sounds.
        let data = match self.project.clips.get(clip) {
            Some(held) => match &held.source {
                ClipSource::Audio(was) if was.stretch != data.stretch => {
                    let stretch = data.stretch;
                    let mut frozen = data;
                    frozen.stretch = was.stretch;
                    fontelle_model::with_stretch(&frozen, held, &self.project.tempo_map, stretch)
                }
                _ => data,
            },
            None => data,
        };
        self.run(Box::new(fontelle_model::SetAudioClip::new(clip, data)));
    }

    fn keymap_overrides(&self) -> Vec<(String, String)> {
        self.settings
            .keybinds
            .iter()
            .map(|(id, chords)| (id.clone(), chords.clone()))
            .collect()
    }

    fn set_keymap_overrides(&mut self, overrides: Vec<(String, String)>) {
        self.settings.keybinds = overrides.into_iter().collect();
        if let Err(e) = self.save_settings() {
            self.message = Some(format!("could not write settings: {e}"));
        }
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
        self.touch();
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

    fn note_open_insert(&mut self, insert: Option<(usize, usize)>) {
        self.open_insert = insert;
    }

    fn preset_status(&self) -> String {
        self.preset_tab_status()
    }

    fn reveal_preset_dir(&mut self) {
        match self.preset_bank.user_dir() {
            Some(dir) => {
                // The folder may not be there yet — nobody has saved a preset
                // — and a file manager opened on nothing says nothing. Made
                // first, which is also INVARIANT 10's line: this is Fontelle's
                // own data directory.
                if let Err(e) = std::fs::create_dir_all(dir) {
                    self.message = Some(format!("could not make the preset folder: {e}"));
                    return;
                }
                let dir = dir.to_path_buf();
                if let Err(e) = crate::desktop::reveal(&dir) {
                    self.message = Some(e);
                }
                // Whatever was dropped in while the file manager was open is
                // picked up next time the panel is looked at.
                self.preset_bank.rescan();
                self.touch();
            }
            None => self.message = Some("there is no preset folder set".to_string()),
        }
    }

    fn choose_preset_dir(&mut self) {
        let start = self.preset_bank.user_dir().map(|dir| dir.to_path_buf());
        match crate::desktop::choose_folder("Preset folder", start.as_deref()) {
            Ok(Some(dir)) => {
                self.settings.preset_dir = Some(dir);
                self.preset_bank
                    .set_user_dir(self.settings.user_preset_dir());
                if let Err(e) = self.save_settings() {
                    self.message = Some(format!("could not write settings: {e}"));
                }
                self.touch();
            }
            Ok(None) => {}
            Err(e) => self.message = Some(e),
        }
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
        self.touch();
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
        self.touch();
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

    fn preview_preset(&mut self, preset: usize) -> Result<(), String> {
        // Which row was clicked, the same way `install_preset` reads it: a
        // search lists hits from the whole collection, so the row is not
        // always a preset of the open file.
        let (file, index) = match self.preset_rows().into_iter().nth(preset) {
            Some(PresetRow::Preset { file, index, .. }) => (file, index),
            Some(PresetRow::Group { .. }) => {
                return Err("that row is a heading, not a sound".to_string());
            }
            None => return Err("that preset is not in this soundfont".to_string()),
        };
        let patch = self
            .library
            .import_sf2(&file, index)
            .map_err(|e| format!("{}: {e}", file.display()))?;
        // **Nothing is written to the document.** Hearing an instrument is not
        // choosing one, which is the whole of the report: the old answer to
        // "what does this sound like" was to put it on a channel.
        self.preview_patch = Some(patch);
        self.previewing = true;
        self.rebuild_graph();
        Ok(())
    }

    fn preview_import(&mut self, index: usize) -> Result<f64, String> {
        // The same row-to-path the drag and the sampler-import use, refusing a
        // folder the same way — dragging and previewing are both only offered
        // for files (`canvas::browser_row_carries`).
        let path = self.import_audio_row(index)?;
        // A file, loaded into the preview voice as a one-shot sampler — the same
        // voice a soundfont preview uses (`preview_preset`), so hearing a file
        // and hearing an instrument are one path. **Nothing is written to the
        // document**: a listen is not an import, which is the whole of the
        // report — *"instead of being like soundfonts where they preview on
        // select ... if i click one it should play that audio."*
        let (_name, patch, seconds) = self.sampler_patch(&path)?;
        self.preview_patch = Some(patch);
        self.previewing = true;
        self.rebuild_graph();
        // How long to hold the note, so the UI can play the whole file and let
        // it stop on its own rather than ringing a silent voice afterwards.
        Ok(seconds)
    }

    fn end_preview(&mut self) {
        // The instrument is left loaded — only the aim changes. Rebuilding the
        // graph to empty it would cost a rebuild on every click away from the
        // browser, and a silent node costs nothing.
        self.previewing = false;
    }

    fn audition_off(&mut self, key: u8) {
        self.send_live(EventPayload::NoteOff {
            key,
            voice_context: AUDITION_VOICE_CONTEXT,
        });
    }

    // -------------------------------------------------- the instrument editor ---

    // --- the modulation matrix (`docs/flopsynth-plan.md` §8.4) ---

    fn mod_sources(&self) -> Vec<String> {
        Session::mod_sources(self)
    }

    fn routes_to(
        &self,
        address: &fontelle_types::ParamAddress,
    ) -> Vec<fontelle_ui::document::RouteInfo> {
        Session::routes_to(self, address)
    }

    fn is_mod_destination(&self, address: &fontelle_types::ParamAddress) -> bool {
        Session::is_mod_destination(self, address)
    }

    fn add_route(&mut self, source: usize, address: &fontelle_types::ParamAddress) {
        Session::add_route(self, source, address);
    }

    fn remove_route(&mut self, address: &fontelle_types::ParamAddress, index: usize) {
        Session::remove_route(self, address, index);
    }

    // --- the preset system (`docs/flopsynth-plan.md` §P) ---
    //
    // Thin overrides onto the inherent methods below, which are where the work
    // is: the window reaches them through the trait, and the app's own tests
    // reach them directly.

    fn preset_bar(
        &self,
        device: fontelle_ui::canvas::PresetDevice,
    ) -> fontelle_ui::canvas::PresetBarView {
        Session::preset_bar(self, device)
    }

    fn preset_choices(
        &self,
        device: fontelle_ui::canvas::PresetDevice,
    ) -> Vec<fontelle_ui::canvas::PresetChoice> {
        Session::preset_choices(self, device)
    }

    fn preset_categories(&self, device: fontelle_ui::canvas::PresetDevice) -> Vec<String> {
        Session::preset_categories(self, device)
    }

    fn apply_preset(&mut self, device: fontelle_ui::canvas::PresetDevice, index: usize) {
        Session::apply_preset(self, device, index);
    }

    fn step_preset(&mut self, device: fontelle_ui::canvas::PresetDevice, delta: i32) {
        Session::step_preset(self, device, delta);
    }

    fn save_preset(&mut self, device: fontelle_ui::canvas::PresetDevice) {
        Session::save_preset(self, device);
    }

    fn save_preset_as(
        &mut self,
        device: fontelle_ui::canvas::PresetDevice,
        name: &str,
        category: &str,
    ) {
        Session::save_preset_as(self, device, name, category);
    }

    fn toggle_preset_favorite(&mut self, device: fontelle_ui::canvas::PresetDevice) {
        Session::toggle_preset_favorite(self, device);
    }

    fn toggle_preset_star(&mut self, device: fontelle_ui::canvas::PresetDevice, index: usize) {
        Session::toggle_preset_star(self, device, index);
    }

    fn patch_effect_kinds(&self) -> Vec<fontelle_types::EffectKind> {
        fontelle_core::flopsynth::PATCH_FX_KINDS.to_vec()
    }

    fn add_patch_effect(&mut self, kind: fontelle_types::EffectKind) {
        Session::add_patch_effect(self, kind);
    }

    fn remove_patch_effect(&mut self, index: usize) {
        Session::remove_patch_effect(self, index);
    }

    fn instrument(&self) -> Option<InstrumentView> {
        let channel_id = self.selected_channel_id()?;
        let channel = self.project.channels.get(channel_id)?;
        // A channel playing a plugin draws the plugin's own parameters. The
        // panel is the same panel — see `instrument::describe_plugin`.
        if channel.instrument == Some(fontelle_types::InstrumentKind::Plugin) {
            let slot = crate::PluginSlot::Channel(channel_id);
            let title = channel
                .plugin
                .as_ref()
                .map_or_else(|| channel.name.clone(), |state| state.name.clone());
            let mut view = crate::instrument::describe_plugin(
                &title,
                self.plugins.params(slot),
                |id| self.plugins.value(slot, id),
                |id| fontelle_types::ParamAddress::new(crate::plugins::param_address(id)),
                |id, _| self.plugins.display(slot, id).map(str::to_string),
            );
            // The channel's own level and placement, above the plugin's
            // knobs: they belong to the channel whatever is playing on it.
            view.groups.insert(
                0,
                crate::instrument::channel_group(channel.gain_db, channel.pan),
            );
            let automated = fontelle_model::automated_targets(&self.project);
            view.mark_automated(|address| automated.contains(address));
            return Some(view);
        }
        let patch = self.selected_patch()?;
        // **The channel's own level and placement**, not its mixer track's.
        // Every channel goes to the master until somebody routes it somewhere
        // else, so a panel reading the track was every panel reading one
        // fader — see `Channel::gain_db`.
        let mut view =
            crate::instrument::describe(&channel.name, &patch, channel.gain_db, channel.pan);
        // The ring §12.2 asks for. Marked here rather than inside `describe`,
        // which knows about patches and not about clips — and marked from one
        // set rather than one question per knob, since answering it walks
        // every clip in the project.
        // Built once for the whole panel: `automated_targets` walks every
        // clip in the project, and a closure that called it per knob would
        // walk them forty-nine times for an EQ.
        let automated = fontelle_model::automated_targets(&self.project);
        view.mark_automated(|address| automated.contains(address));
        // The kits used to be a row of chips here, and are files now: the
        // preset bar in the window's header chooses them, the same bar over
        // every other device (`docs/flopsynth-plan.md` §P.9). The recipes
        // stayed put — `DrumKitStyle` is what `cargo xtask
        // export-factory-presets` runs — they simply no longer sit behind a
        // panel of their own.
        Some(view)
    }

    fn load_wavetable(&mut self, layer: usize, path: &Path) -> Result<String, String> {
        Session::load_wavetable(self, layer, path)
    }

    fn flopsynth(
        &self,
        page: fontelle_ui::canvas::FlopsynthPage,
    ) -> Option<fontelle_ui::canvas::FlopsynthView> {
        let channel_id = self.selected_channel_id()?;
        let channel = self.project.channels.get(channel_id)?;
        // A channel playing a plugin draws the plugin's own parameters, even
        // if its patch happens to still be a Flopsynth's.
        if channel.instrument == Some(fontelle_types::InstrumentKind::Plugin) {
            return None;
        }
        let patch = self.selected_patch()?;
        if !fontelle_core::flopsynth::is_flopsynth(&patch) {
            return None;
        }
        // The bank goes with the Presets page and no other — see `describe`.
        let bank = match page {
            fontelle_ui::canvas::FlopsynthPage::Presets => {
                self.preset_choices(fontelle_ui::canvas::PresetDevice::Instrument)
            }
            _ => Vec::new(),
        };
        let mut view = crate::flopsynth::describe(
            &channel.name,
            &patch,
            channel.gain_db,
            channel.pan,
            page,
            crate::flopsynth::Heard {
                voices: Session::voice_count(self),
                lfo_phases: Session::lfo_phases(self).to_vec(),
            },
            bank,
        );
        // The ring §12.2 asks for. Built once for the whole window rather than
        // one question per knob, since answering it walks every clip.
        let automated = fontelle_model::automated_targets(&self.project);
        for card in &mut view.cards {
            for param in &mut card.group.params {
                param.automated = automated.contains(&param.address);
            }
        }
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
        self.touch();
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
                self.touch();
                return;
            }
            crate::instrument::MIXER_PAN => {
                self.run(Box::new(fontelle_model::SetNumber::new(
                    fontelle_model::NumberTarget::ChannelPan(channel_id),
                    f64::from(value.clamp(0.0, 1.0) * 2.0 - 1.0),
                )));
                self.rebuild_graph();
                self.touch();
                return;
            }
            _ => {}
        }

        // A knob on a hosted plugin's panel. Written to the document as the
        // plugin's own plain value and to the running plugin at once, which is
        // the same two-halves shape a fader has: the command is for the file
        // and the undo, the live write is so a drag can be heard.
        if let Some(id) = crate::plugins::param_id(address.as_str())
            && self
                .project
                .channels
                .get(channel_id)
                .is_some_and(|c| c.instrument == Some(fontelle_types::InstrumentKind::Plugin))
        {
            let which = crate::PluginSlot::Channel(channel_id);
            let Some(plain) = self
                .plugins
                .plugin(which)
                .and_then(|plugin| plugin.param(id))
                .map(|param| param.plain(f64::from(value)))
            else {
                return;
            };
            self.plugins.set_param(which, id, plain);
            self.run(Box::new(fontelle_model::SetPluginParam::channel(
                channel_id, id, plain,
            )));
            self.touch();
            return;
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
        // **The line §2.3 draws.** Almost every patch parameter can be applied
        // to a running sampler between one block and the next, so the document
        // is written quietly and the value goes on the live wire — which is
        // what lets a cutoff be swept under a held chord without the note
        // being cut. The exception is a layer's *table*: choosing one means
        // resolving it, and resolving one locks the wavetable bank and may
        // build half a megabyte, so it goes through `prepare` like every other
        // structural change. `Sampler::is_live_param` is the one place that
        // knows which is which.
        //
        // This is applied to **every** built-in instrument's panel, not only
        // Flopsynth's, because it is the patch that changed and not the synth.
        if fontelle_core::Sampler::is_live_param(address.as_str()) {
            self.store_patch_quiet(channel_id, patch);
            self.send_patch_param(channel_id, address, value);
            self.touch();
        } else {
            self.store_patch(channel_id, patch);
        }
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
                            label: slot.label().to_string(),
                            bypassed: slot.bypassed,
                            // A hosted plugin has no dry/wet of ours: the
                            // blend belongs to `EffectNode`, and a plugin is
                            // not one. Fully wet, which is what it is.
                            mix: slot.config().map_or(1.0, |config| config.mix()),
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
        self.touch();
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
        // And the same for a slot holding a plugin, which has no
        // `EffectControls` because it has no `EffectConfig`.
        self.plugins
            .set_bypassed(crate::PluginSlot::Insert { track: id, slot }, !bypassed);
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
            self.touch();
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
        self.touch();
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
        self.touch();
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
        self.touch();
    }

    fn focused_clip_span(&self) -> Option<(Tick, Tick)> {
        self.clip_span()
    }

    fn is_automated(&self, address: &fontelle_types::ParamAddress) -> bool {
        fontelle_model::automated_targets(&self.project).contains(address)
    }

    fn insert_view(&self, strip: usize, slot: usize) -> Option<InstrumentView> {
        // An insert holding a plugin draws the plugin's own parameters, on the
        // same panel and through the same addresses.
        let track = self.mixer_track_ids().get(strip).copied();
        if let Some(track) = track
            && let Some(insert) = self
                .project
                .mixer
                .tracks
                .get(track)
                .and_then(|t| t.inserts.get(slot))
            && let Some(state) = &insert.plugin
        {
            let which = crate::PluginSlot::Insert { track, slot };
            let mut view = crate::instrument::describe_plugin(
                &state.name,
                self.plugins.params(which),
                |id| self.plugins.value(which, id),
                |id| {
                    fontelle_types::ParamTarget::Insert {
                        track,
                        slot,
                        param: id.to_string(),
                    }
                    .address()
                },
                |id, _| self.plugins.display(which, id).map(str::to_string),
            );
            // The key chips, when the plugin has a sidechain port to feed —
            // the host's knowledge, asked of the rack rather than the
            // document. The same row a compressor's panel has, built by the
            // same rule: "no key" first, then every strip.
            if self
                .plugins
                .plugin(which)
                .is_some_and(fontelle_host::HostedPlugin::takes_key)
            {
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
                view.keys = std::iter::once(fontelle_ui::canvas::NO_KEY.to_string())
                    .chain(strips)
                    .collect();
                view.key = Some(self.insert_key(strip, slot).map_or(0, |strip| strip + 1));
            }
            let automated = fontelle_model::automated_targets(&self.project);
            view.mark_automated(|address| automated.contains(address));
            return Some(view);
        }
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
        let mut view = fontelle_ui::canvas::effect_view(
            &name,
            slot,
            id,
            &config,
            &strips,
            self.insert_key(strip, slot),
        );
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
        // A knob on a hosted plugin's panel — see `set_instrument_param`,
        // which does the same thing one level up.
        if self
            .project
            .mixer
            .tracks
            .get(id)
            .and_then(|track| track.inserts.get(slot))
            .is_some_and(|insert| insert.is_plugin())
        {
            let Some(param_id) = crate::plugins::param_id(param) else {
                return;
            };
            let which = crate::PluginSlot::Insert { track: id, slot };
            let Some(plain) = self
                .plugins
                .plugin(which)
                .and_then(|plugin| plugin.param(param_id))
                .map(|spec| spec.plain(f64::from(value)))
            else {
                return;
            };
            self.plugins.set_param(which, param_id, plain);
            self.run(Box::new(fontelle_model::SetPluginParam::insert(
                id, slot, param_id, plain,
            )));
            self.touch();
            return;
        }
        // The panel hands back the **full** automation address it was built
        // with; the command names the parameter inside the effect. This is the
        // one place the two halves of §8.2's scheme meet, and it takes either.
        let param =
            match fontelle_types::ParamTarget::parse(&fontelle_types::ParamAddress::new(param)) {
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
        self.touch();
    }

    // `set_instrument_preset` and `set_insert_preset` were here.
    //
    // They were the panel's chip rows: the drum machine's twenty-two kits and
    // the three effects that shipped constructor presets. Both are files now
    // (`docs/flopsynth-plan.md` §P.9) and both go through
    // [`Session::apply_preset`], which is the same code for every device —
    // including the ones that never had a chip row, which is the point.

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
        self.touch();
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

    fn set_insert_notes(&mut self, strip: usize, slot: usize, notes: Option<usize>) {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return;
        };
        let channels = self.channel_ids();
        // `None` is "no MIDI", which is what a corrector does with no source;
        // a channel index nobody has is the same answer rather than a guess at
        // the nearest one.
        let notes = match notes {
            Some(index) => match channels.get(index).copied() {
                Some(channel) => Some(channel),
                None => return,
            },
            None => None,
        };
        // A **rebuild**, like the key one line up and for the same reason:
        // which node this insert listens to is wiring, and wiring is not a
        // thing a control surface can carry. Nothing about it is an edge in
        // the audio graph, though — see `EffectSlot::notes`.
        self.run(Box::new(fontelle_model::SetInsertNotes::new(
            id, slot, notes,
        )));
        self.rebuild_graph();
        self.dirty = true;
        self.touch();
    }

    fn insert_notes(&self, strip: usize, slot: usize) -> Option<usize> {
        let id = self.mixer_track_ids().get(strip).copied()?;
        let notes = self
            .project
            .mixer
            .tracks
            .get(id)?
            .inserts
            .get(slot)?
            .effective_notes()?;
        self.channel_ids().iter().position(|other| *other == notes)
    }

    /// Everything the corrector's console draws (`docs/tune-plan.md` §7.6).
    ///
    /// The layer allowed to see both halves: the document for the config and
    /// the rack, the tap for what the node is doing. `None` when the slot
    /// holds something that is not a corrector, which is how the window
    /// decides between the console, the EQ and the generic panel.
    fn tune_view(&self, strip: usize, slot: usize) -> Option<fontelle_ui::canvas::TuneView> {
        let id = self.mixer_track_ids().get(strip).copied()?;
        let track = self.project.mixer.tracks.get(id)?;
        let fontelle_types::EffectConfig::Tune(config) = track.inserts.get(slot)?.config else {
            return None; // this slot holds something else
        };
        let name = track.name.clone();
        let trace = self.tune_trace(strip, slot);
        // The channels in the rack's own order, so row *n* + 1 of the
        // drop-down is `channel_ids()[n]` — the order `set_insert_notes`
        // reads an index back in.
        let channels: Vec<String> = self
            .channel_ids()
            .into_iter()
            .filter_map(|channel| self.project.channels.get(channel))
            .map(|channel| channel.name.clone())
            .collect();
        Some(crate::tune::describe(
            &name,
            id,
            slot,
            &config,
            trace.clone(),
            held_classes(&trace),
            &channels,
            self.insert_notes(strip, slot),
            self.options.sample_rate as f32,
        ))
    }

    fn tune_trace(&self, strip: usize, slot: usize) -> Vec<fontelle_types::TuneFrame> {
        let Some(id) = self.mixer_track_ids().get(strip).copied() else {
            return Vec::new();
        };
        let Some(tap) = self.tune_taps.get(&(id, slot)) else {
            return Vec::new();
        };
        // Everything the ring holds; the window trims it to the seconds it
        // draws, which depend on the mode's hop rather than on this number.
        tap.read(fontelle_engine::TUNE_TRACE_FRAMES)
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
        self.touch();
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
        self.touch();
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

    fn recording_notes(&self, now: Sample) -> Vec<fontelle_ui::document::NotePreview> {
        // Only what has been drained into the take so far — `pump` empties
        // the capture ring every pass, so this is at most a frame behind the
        // keys. Through `notes_from_capture`, the same reading `keep_take`
        // makes, so a note is drawn exactly where it will land: with `now`
        // as the take's end, whatever is still held is closed at the
        // playhead, which is what makes a held key grow.
        if self.capture.is_none() || self.take.is_empty() {
            return Vec::new();
        }
        let start = self
            .project
            .clips
            .get(self.clip)
            .map_or(0, |clip| clip.start);
        let ClipSource::Notes(data) =
            fontelle_model::notes_from_capture(&self.take, &self.project.tempo_map, start, now)
        else {
            return Vec::new();
        };
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
        notes
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

        // Never the channel being edited, whatever the filter says: its own
        // notes are already drawn solid, and a ghost under every one of them
        // is a smear.
        let editing = self.selected_channel_id();
        let mut ghosts = Vec::new();
        for (_, clip) in self.project.clips.iter() {
            let ClipSource::Notes(data) = &clip.source else {
                continue;
            };
            let color = clip.color.unwrap_or_else(|| {
                self.project
                    .lanes
                    .get(clip.lane)
                    .map_or([0x4f, 0x8f, 0xd0, 0xff], |lane| lane.color)
            });
            let offset = clip.start - open.start;
            // **Per note**, and the open clip included: a clip holds several
            // instruments now, and the drums you are writing the bass over
            // are most often in the very clip you are writing into.
            for note in data.notes.values() {
                let channel = note.channel_or(data.channel);
                if Some(channel) == editing {
                    continue;
                }
                if wanted.is_some_and(|wanted| channel != wanted) {
                    continue;
                }
                ghosts.push(GhostNote {
                    start: note.start + offset,
                    length: note.length,
                    key: note.key,
                    color,
                });
            }
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
                // **What this block holds**, which is not `clip.source` for a
                // place that follows a prefab — see `Project::clip_source`.
                // The arrangement draws the notes inside a block, so a place
                // that read its own source would be drawn empty.
                let held = self.project.clip_source(id);
                let source: &ClipSource = held.as_deref().unwrap_or(&clip.source);
                // And what it is a place *for*, by name.
                let prefab = clip.prefab_link.as_ref().and_then(|link| {
                    self.project
                        .prefabs
                        .get(link.prefab)
                        .map(|prefab| prefab.name.clone())
                });
                let (kind, name, curve, notes) = match source {
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
                        // Captioned with its home channel — and, when its
                        // notes play others too, how many: "Drums +1" is a
                        // clip with a bass line in it, which is worth
                        // knowing from across the room.
                        let mut name = self
                            .project
                            .channels
                            .get(data.channel)
                            .map(|channel| channel.name.clone())
                            .unwrap_or_else(|| "Clip".to_string());
                        let others = data
                            .channels()
                            .iter()
                            .filter(|c| {
                                **c != data.channel && self.project.channels.contains_key(**c)
                            })
                            .count();
                        if others > 0 {
                            name = format!("{name} +{others}");
                        }
                        // A **place** is captioned with the prefab's name
                        // rather than the instrument's. The difference
                        // between a copy and a place is invisible until you
                        // edit one and four other blocks change, so the block
                        // has to say which it is from across the room — and
                        // "Grand Piano" on a block that is really "Chorus
                        // riff" is the wrong answer to the question a caption
                        // exists to answer.
                        if let Some(prefab) = &prefab {
                            name = prefab.clone();
                        }
                        (ClipKind::Notes, name, Vec::new(), notes)
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
                        audio = self.audio_preview(clip.start, data);
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
                    prefab,
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
                }
                self.touch();
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
                    return Created {
                        clips: created,
                        points,
                    };
                };
                // What the clip plays: **the rack's selection**. It used to
                // be whatever else was on the lane, and that was reported as
                // *"guessing what instrument i want based on the lane which
                // is super weird"* — a lane has no instrument (TDD §10.3),
                // and the instrument you mean is the one you selected.
                let Some(channel) = self
                    .selected_channel_id()
                    .or_else(|| self.project.channels.keys().next())
                else {
                    self.message =
                        Some("add an instrument first — a clip has to play something".to_string());
                    return Created {
                        clips: created,
                        points,
                    };
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
                    // do: you made it to put notes in it. The rack stays
                    // where it is — it is what decided the instrument.
                    self.clip = id;
                }
                self.history.break_gesture();
                self.republish();
                self.touch();
            }
            ArrangeEdit::Stamp {
                source,
                lane,
                start,
            } => {
                // *"a single click should instead place a exact copy of
                // whatever your last selection is."* The whole clip — notes,
                // curve or sound, loop and settings — on the row and at the
                // bar that was pressed. Through `AddClip` with a clone, so a
                // stamped copy is exactly what a pasted one is.
                let lanes = self.lane_ids();
                let Some(lane_id) = lanes.get(lane).or_else(|| lanes.last()).copied() else {
                    self.message = Some("this project has no lanes to draw on".to_string());
                    return Created {
                        clips: created,
                        points,
                    };
                };
                let Some(original) = self.project.clips.get(source) else {
                    self.message = Some("the clip to copy is not there any more".to_string());
                    return Created {
                        clips: created,
                        points,
                    };
                };
                let clip = Clip {
                    lane: lane_id,
                    start,
                    ..original.clone()
                };
                let is_notes = matches!(clip.source, ClipSource::Notes(_));
                if let Ok(command) = self.apply_for::<AddClip>(Box::new(AddClip::new(clip)))
                    && let Some(id) = command.id()
                {
                    created.push(id);
                    // Opened, as a drawn clip is: you put it down to work in
                    // it. A copied automation block is edited where it sits.
                    if is_notes {
                        self.clip = id;
                    }
                }
                self.history.break_gesture();
                self.republish();
                self.touch();
            }
            // A fade handle, dragged (TDD §15.2). The canvas speaks in
            // fractions of the block; the clip's fades are in frames of its
            // own audio, so the fraction is taken of the trimmed range —
            // which is the same conversion `audio_preview` makes the other
            // way, so the handle lands where the block draws the fade.
            // Through `SetAudioClip`, whose `merge_with` folds a drag into
            // one undo entry; no `break_gesture`, for the same reason a point
            // drag has none — the window says when the mouse came up.
            ArrangeEdit::SetFade {
                clip,
                end,
                fraction,
            } => {
                let Some(ClipSource::Audio(data)) = self.project.clips.get(clip).map(|c| &c.source)
                else {
                    self.message = Some("only an audio clip has a fade".to_string());
                    return Created {
                        clips: created,
                        points,
                    };
                };
                let mut data = data.clone();
                let frames = (f64::from(fraction.clamp(0.0, 1.0)) * data.source_frames() as f64)
                    .round() as fontelle_types::Sample;
                match end {
                    fontelle_ui::canvas::FadeEnd::In => data.fade_in.frames = frames,
                    fontelle_ui::canvas::FadeEnd::Out => data.fade_out.frames = frames,
                }
                self.run(Box::new(fontelle_model::SetAudioClip::new(clip, data)));
            }
            ArrangeEdit::SetFadeTension { clip, end, tension } => {
                let Some(ClipSource::Audio(data)) = self.project.clips.get(clip).map(|c| &c.source)
                else {
                    self.message = Some("only an audio clip has a fade".to_string());
                    return Created {
                        clips: created,
                        points,
                    };
                };
                let mut data = data.clone();
                let tension = tension.clamp(-1.0, 1.0);
                match end {
                    fontelle_ui::canvas::FadeEnd::In => data.fade_in.tension = tension,
                    fontelle_ui::canvas::FadeEnd::Out => data.fade_out.tension = tension,
                }
                self.run(Box::new(fontelle_model::SetAudioClip::new(clip, data)));
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
                self.touch();
            }
            ArrangeEdit::SetStretch { ids, stretch } => {
                // No `break_gesture`, for the reason `SetLoop` gives: this is
                // the first step of an edge drag. A note clip in the list is
                // skipped rather than refused — the canvas does not name one,
                // and a stale id is not a reason to stop the audio clip
                // beside it hearing.
                for id in ids {
                    let Some(clip) = self.project.clips.get(id) else {
                        continue;
                    };
                    let ClipSource::Audio(data) = &clip.source else {
                        continue;
                    };
                    if data.stretch == stretch {
                        continue;
                    }
                    // Through `with_stretch`, which is what keeps the sound
                    // where it was when the switch goes off — see
                    // `fontelle_model::with_stretch`.
                    let data =
                        fontelle_model::with_stretch(data, clip, &self.project.tempo_map, stretch);
                    self.run(Box::new(fontelle_model::SetAudioClip::new(id, data)));
                }
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
                return Created {
                    clips: created,
                    points,
                };
            }
            ArrangeEdit::Paste { at } => {
                if self.clip_clipboard.is_empty() {
                    return Created {
                        clips: created,
                        points,
                    };
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
                if let Ok(command) = self.apply_for::<fontelle_model::AddAutomationPoint>(Box::new(
                    fontelle_model::AddAutomationPoint::new(clip, point),
                )) {
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
        self.touch();
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
            self.touch();
            return;
        }
        self.clip = clip;
        // The rack follows a clip that holds **one** instrument, and only
        // that one. It used to follow every clip, and that was a report: a
        // clip holding several instruments could only ever be edited as the
        // one it was captioned with. Then it followed none, and that was
        // the next one — *"you could click your clips to already have the
        // instrument selected to start editing instead of having to select
        // the clip and the instrument in it"*. Both are right: a clip that
        // plays one instrument is unambiguous about which one you mean, and
        // a clip that plays several is not — opening the drum clip with the
        // bass selected still means *write bass in here*. `channels()` is
        // the same reading the caption's `+1` comes from, so the two agree
        // about which clips hold several.
        let one = self
            .project
            .clip_source(clip)
            .and_then(|source| match source.as_ref() {
                ClipSource::Notes(data) => match data.channels().as_slice() {
                    [only] => Some(*only),
                    _ => None,
                },
                _ => None,
            });
        if let Some(channel) = one
            && let Some(index) = self.channel_ids().iter().position(|id| *id == channel)
            && index != self.selected
        {
            self.select_channel(index);
        }
        self.touch();
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
        self.touch();
    }

    fn move_lane(&mut self, lane: usize, delta: isize) {
        self.run(Box::new(fontelle_model::MoveLane::new(lane, delta)));
        self.history.break_gesture();
        self.dirty = true;
        // The arrangement re-reads its rows only when this moves — a reorder
        // that forgot it would be a document that had changed and a window
        // that had not.
        self.touch();
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
        self.insert(self.note_target(), notes);
        self.touch();
        count
    }

    fn discard_take(&mut self) {
        if let Some(capture) = &mut self.capture {
            capture.drain_into(&mut self.take);
        }
        self.take.clear();
    }

    fn pump(&mut self) {
        // **A project that names plugins is hosted on the first frame.** The
        // graph this session was handed was built with no rack (`main.rs`
        // realises before the session exists), so an LSP sampler in a
        // reopened project sat silent, with an empty panel, until the first
        // edit rebuilt the graph. Once, here: every builder has run and the
        // settings' plugin folders are known by the first pump.
        if !self.plugins_hosted {
            self.plugins_hosted = true;
            if !crate::plugin_slots(&self.project).is_empty() {
                self.rebuild_graph();
            }
        }
        // Emptied every pass, so the ring never fills while a long take is
        // being played — the same rule the CLI's own recording loop follows.
        if let Some(capture) = &mut self.capture {
            capture.drain_into(&mut self.take);
        }
        // And the audio input's, for the same reason: a ring nobody empties
        // fills, and a full ring is a take with a hole in it. **Always
        // drained, kept only while recording** — the stream is open for as
        // long as a track names an input, so a microphone left plugged in
        // would otherwise grow a take for as long as the window stayed open.
        if let Some(input) = &mut self.input {
            let mut block = Vec::new();
            input.drain_into(&mut block);
            if self.capturing && !block.is_empty() {
                self.input_take.push(&block);
            }
        }
        // Where opening a project with an armed strip in it starts monitoring
        // without anybody pressing anything, and where clearing an input
        // closes the device. Cheap when nothing has changed.
        self.sync_audio_input();
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
            self.touch();
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

/// The browser's rows, from a scan.
///
/// A free function rather than a method because both lists want it and neither
/// wants the other's: what can go on a channel and what can go in an insert
/// are two questions the scan already answers (`PluginInfo::is_instrument`).
fn listings<'a>(
    found: impl Iterator<Item = &'a fontelle_host::PluginInfo>,
) -> Vec<fontelle_ui::PluginListing> {
    found
        .map(|plugin| fontelle_ui::PluginListing {
            name: plugin.name.clone(),
            vendor: plugin.vendor.clone(),
            key: plugin.key.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------- presets

/// The preset system's half of the session (`docs/flopsynth-plan.md` §P).
///
/// Everything here is device-agnostic on purpose. A device contributes two
/// things and nothing else — **what kind it is** and **what its state is** —
/// and the eight methods below are the same eight for a channel playing
/// Flopsynth, an insert holding a reverb and a slot hosting somebody's CLAP.
/// That is the whole argument of §P: a preset system for every device is a
/// type and a folder walk rather than a feature per plugin.
impl Session {
    /// Which device the bar is for, as the bank names one.
    fn preset_device(&self, device: PresetDevice) -> Option<fontelle_types::DeviceKind> {
        use fontelle_types::{DeviceKind, InstrumentKind};
        match device {
            PresetDevice::Instrument => {
                let id = self.selected_channel_id()?;
                let channel = self.project.channels.get(id)?;
                let kind = channel
                    .instrument
                    .or_else(|| self.channel_kind(self.selected))?;
                // A channel playing somebody else's plugin is a device named
                // by *which* plugin, not by the fact that it is one: a Diva
                // preset is no use to Surge.
                match (kind, &channel.plugin) {
                    (InstrumentKind::Plugin, Some(state)) => {
                        Some(DeviceKind::Plugin(state.key.clone()))
                    }
                    (InstrumentKind::Plugin, None) => None,
                    (kind, _) => Some(DeviceKind::Instrument(kind)),
                }
            }
            PresetDevice::Insert { strip, slot } => {
                let track = self.mixer_track_ids().get(strip).copied()?;
                let insert = self.project.mixer.tracks.get(track)?.inserts.get(slot)?;
                Some(match &insert.plugin {
                    Some(state) => DeviceKind::Plugin(state.key.clone()),
                    None => DeviceKind::Effect(insert.config.kind()),
                })
            }
            // One kind for every track, not one per track: a chain saved off a
            // vocal is exactly the thing you want on a different vocal.
            PresetDevice::Track { strip } => {
                self.mixer_track_ids().get(strip).copied()?;
                Some(DeviceKind::Track)
            }
        }
    }

    /// What this device's state is, in the shape a preset file holds.
    fn preset_payload(&self, device: PresetDevice) -> Option<fontelle_types::PresetPayload> {
        use fontelle_types::PresetPayload;
        match device {
            PresetDevice::Instrument => {
                let id = self.selected_channel_id()?;
                let channel = self.project.channels.get(id)?;
                if channel.instrument == Some(fontelle_types::InstrumentKind::Plugin) {
                    return channel.plugin.clone().map(PresetPayload::Plugin);
                }
                channel.patch_data.clone().map(PresetPayload::Patch)
            }
            PresetDevice::Insert { strip, slot } => {
                let track = self.mixer_track_ids().get(strip).copied()?;
                let insert = self.project.mixer.tracks.get(track)?.inserts.get(slot)?;
                Some(match &insert.plugin {
                    Some(state) => PresetPayload::Plugin(state.clone()),
                    None => PresetPayload::Effect(insert.config),
                })
            }
            PresetDevice::Track { strip } => Some(PresetPayload::Track(self.track_chain(strip)?)),
        }
    }

    /// A track's chain, in the shape a preset holds it.
    ///
    /// **Hosted plugins are left out**, with the built-in inserts either side
    /// of them kept: a preset naming a plugin this machine has not got could
    /// only fail at load, and failing quietly part-way down a chain is the
    /// worst of the ways to fail. `save_track_chain` says how many it left.
    fn track_chain(&self, strip: usize) -> Option<fontelle_types::TrackChain> {
        let id = self.mixer_track_ids().get(strip).copied()?;
        let track = self.project.mixer.tracks.get(id)?;
        Some(fontelle_types::TrackChain {
            gain_db: track.gain_db,
            pan: track.pan,
            phase_invert: track.phase_invert,
            inserts: track
                .inserts
                .iter()
                .filter(|slot| slot.plugin.is_none())
                .map(|slot| fontelle_types::TrackInsert {
                    config: slot.config,
                    bypassed: slot.bypassed,
                    preset: slot.preset.clone(),
                })
                .collect(),
        })
    }

    /// How many inserts a saved chain would leave behind, so the message can
    /// say so rather than the preset quietly being short.
    fn plugins_in_chain(&self, strip: usize) -> usize {
        self.mixer_track_ids()
            .get(strip)
            .copied()
            .and_then(|id| self.project.mixer.tracks.get(id))
            .map_or(0, |track| {
                track
                    .inserts
                    .iter()
                    .filter(|slot| slot.plugin.is_some())
                    .count()
            })
    }

    /// The preset this device says it was loaded from, if it says one.
    fn preset_ref(&self, device: PresetDevice) -> Option<fontelle_types::PresetRef> {
        match device {
            PresetDevice::Instrument => {
                let id = self.selected_channel_id()?;
                self.project.channels.get(id)?.preset.clone()
            }
            PresetDevice::Insert { strip, slot } => {
                let track = self.mixer_track_ids().get(strip).copied()?;
                self.project
                    .mixer
                    .tracks
                    .get(track)?
                    .inserts
                    .get(slot)?
                    .preset
                    .clone()
            }
            // A track does not remember which chain it came from. There is no
            // bar to say so in, and a `*` nobody can see is a fact nobody can
            // use — see `PresetDevice::Track`.
            PresetDevice::Track { .. } => None,
        }
    }

    /// Where a command writes this device.
    fn preset_target(&self, device: PresetDevice) -> Option<fontelle_model::PresetTarget> {
        match device {
            PresetDevice::Instrument => self
                .selected_channel_id()
                .map(fontelle_model::PresetTarget::Channel),
            PresetDevice::Insert { strip, slot } => self
                .mixer_track_ids()
                .get(strip)
                .copied()
                .map(|track| fontelle_model::PresetTarget::Insert { track, index: slot }),
            // A chain is not written by `ApplyPreset`: it replaces a rack
            // rather than one device's state, and `ApplyTrackChain` is the
            // command for that. `apply_preset_entry` branches before it gets
            // here.
            PresetDevice::Track { .. } => None,
        }
    }

    /// What this device is when it has just been made — what a `*` is measured
    /// against on a device that came from no preset at all (§P.6).
    ///
    /// `None` for the devices that have no such thing: a soundfont player with
    /// a soundfont loaded, a sampler with a sample in it and a hosted plugin
    /// are all in states nobody can call "untouched", and a bar that guessed
    /// would be a `*` that never goes out.
    fn preset_init(&self, device: PresetDevice) -> Option<fontelle_types::PresetPayload> {
        use fontelle_types::{DeviceKind, PresetPayload};
        match self.preset_device(device)? {
            DeviceKind::Instrument(kind) => self.starter_patch(kind).map(PresetPayload::Patch),
            DeviceKind::Effect(kind) => Some(PresetPayload::Effect(
                fontelle_types::EffectConfig::new(kind),
            )),
            DeviceKind::Plugin(_) => None,
            // A track with nothing on it, at unity — which is exactly what a
            // fresh track is, so a bare track reads as "no preset" rather
            // than as one that has drifted.
            DeviceKind::Track => Some(PresetPayload::Track(fontelle_types::TrackChain::new())),
        }
    }

    /// What the bar shows for this device.
    pub fn preset_bar(&self, device: PresetDevice) -> fontelle_ui::canvas::PresetBarView {
        use fontelle_ui::canvas::PresetBarView;
        let Some(kind) = self.preset_device(device) else {
            return PresetBarView::default();
        };
        let payload = self.preset_payload(device);
        let reference = self.preset_ref(device);
        // §P.6, in one expression. A device with a ref is dirty when it no
        // longer matches the file; a device with none is dirty when it is not
        // what it would be fresh. A ref whose file has gone is dirty too —
        // there is nothing left to be clean against.
        let dirty = match (&reference, &payload) {
            (Some(reference), Some(payload)) => self
                .preset_bank
                .find(&kind, reference)
                .and_then(|entry| self.preset_bank.load(entry).ok())
                .is_none_or(|preset| preset.payload != *payload),
            (None, Some(payload)) => self
                .preset_init(device)
                .is_some_and(|init| init != *payload),
            _ => false,
        };
        let favourite = reference.as_ref().is_some_and(|reference| {
            self.settings
                .is_favorite(&fontelle_types::Favorite::Preset {
                    device: kind.clone(),
                    name: reference.name.clone(),
                    origin: reference.origin,
                })
        });
        PresetBarView {
            can_save: reference
                .as_ref()
                .is_some_and(|r| r.origin == fontelle_types::PresetOrigin::User),
            name: reference.as_ref().map(|r| r.name.clone()),
            category: reference
                .as_ref()
                .map(|r| r.category.clone())
                .unwrap_or_default(),
            origin: reference.as_ref().map(|r| r.origin),
            dirty,
            favourite,
        }
    }

    /// Every preset this device could be loaded with, in the bank's order.
    pub fn preset_choices(&self, device: PresetDevice) -> Vec<fontelle_ui::canvas::PresetChoice> {
        let Some(kind) = self.preset_device(device) else {
            return Vec::new();
        };
        self.preset_bank
            .for_device(&kind)
            .into_iter()
            .map(|entry| fontelle_ui::canvas::PresetChoice {
                favourite: self
                    .settings
                    .is_favorite(&fontelle_types::Favorite::Preset {
                        device: kind.clone(),
                        name: entry.name.clone(),
                        origin: entry.origin,
                    }),
                name: entry.name.clone(),
                category: entry.category.clone(),
                origin: entry.origin,
            })
            .collect()
    }

    /// The categories this device has presets in — the Presets page's left
    /// column, and the list "Save as…" offers.
    pub fn preset_categories(&self, device: PresetDevice) -> Vec<String> {
        match self.preset_device(device) {
            Some(kind) => self.preset_bank.categories(&kind),
            None => Vec::new(),
        }
    }

    /// Loads the preset at `index` of [`preset_choices`](Self::preset_choices).
    ///
    /// **One undo entry**, which is what `ApplyPreset` is for, and for a
    /// channel it carries the rename too: a rack row still saying "SoundFont"
    /// over a synth preset is the loudest "nothing happened" it can give.
    pub fn apply_preset(&mut self, device: PresetDevice, index: usize) {
        if let Err(why) = self.try_apply_preset(device, index) {
            self.message = Some(why);
        }
    }

    fn try_apply_preset(&mut self, device: PresetDevice, index: usize) -> Result<(), String> {
        let kind = self
            .preset_device(device)
            .ok_or("there is no device here to load a preset onto")?;
        let entry = self
            .preset_bank
            .for_device(&kind)
            .into_iter()
            .nth(index)
            .ok_or("that preset is not in the bank")?
            .clone();
        self.apply_preset_entry(device, &entry)
    }

    /// Loads one entry of the bank onto `device`.
    ///
    /// **By entry rather than by position**, which is the half the browser
    /// needs: its rows are the whole bank, and a position in that list is not
    /// a position in the target device's own — a Flopsynth preset clicked
    /// while an Osc3 channel is selected would otherwise index the *Osc3*
    /// list and load whatever happened to be there. That defect is what this
    /// signature exists to make impossible.
    fn apply_preset_entry(
        &mut self,
        device: PresetDevice,
        entry: &crate::preset_bank::PresetEntry,
    ) -> Result<(), String> {
        let preset = self.preset_bank.load(entry)?;
        // **A chain is not a device's state**: it replaces the whole rack, so
        // it takes `ApplyTrackChain` rather than `ApplyPreset`, and it takes
        // it before `preset_target` is asked — a track has no `PresetTarget`
        // for the same reason.
        if let PresetDevice::Track { strip } = device {
            let fontelle_types::PresetPayload::Track(chain) = preset.payload else {
                return Err(format!("{} is not a track chain", preset.name));
            };
            let id = self
                .mixer_track_ids()
                .get(strip)
                .copied()
                .ok_or("that track is not there")?;
            self.run(Box::new(fontelle_model::ApplyTrackChain::new(id, chain)));
            self.history.break_gesture();
            self.dirty = true;
            self.rebuild_graph();
            self.touch();
            return Ok(());
        }
        let target = self
            .preset_target(device)
            .ok_or("that device is not there")?;
        let name = preset.name.clone();
        let mut parts: Vec<Box<dyn Command>> = vec![Box::new(
            fontelle_model::ApplyPreset::from_bank(target, preset, entry.origin),
        )];
        if let fontelle_model::PresetTarget::Channel(channel) = target {
            parts.push(Box::new(fontelle_model::RenameChannel::new(
                channel,
                name.clone(),
            )));
        }
        self.run(Box::new(fontelle_model::Compound::new(
            "Load preset",
            parts,
        )));
        self.history.break_gesture();
        if let fontelle_model::PresetTarget::Channel(channel) = target {
            // A preset names no soundfont, so nothing goes in the map that
            // remembers which file a channel's sound came from.
            self.channel_presets.remove(&channel);
        }
        self.patch_cache = None;
        self.dirty = true;
        self.rebuild_graph();
        self.touch();
        Ok(())
    }

    /// The previous or next preset in this device's bank, wrapping.
    ///
    /// Wrapping because the bank is a **ring** with no ends: stopping at the
    /// last would mean knowing to go back through, which is `SettingRow`'s
    /// rule for a choice as against a number.
    pub fn step_preset(&mut self, device: PresetDevice, delta: i32) {
        let choices = self.preset_choices(device);
        if choices.is_empty() || delta == 0 {
            return;
        }
        let here = self.preset_ref(device).and_then(|reference| {
            choices.iter().position(|choice| {
                choice.name == reference.name
                    && choice.category == reference.category
                    && choice.origin == reference.origin
            })
        });
        let count = choices.len() as i32;
        let next = match here {
            Some(here) => (here as i32 + delta).rem_euclid(count),
            // From nowhere, a step forwards lands on the first and a step back
            // on the last — the same ring, entered at its seam.
            None if delta > 0 => 0,
            None => count - 1,
        };
        self.apply_preset(device, next as usize);
    }

    /// Writes this device's state over the file it was loaded from.
    ///
    /// Refused on a factory preset **here** as well as by the drawn-disabled
    /// button, because read-only is a rule and a disabled button is a
    /// courtesy.
    pub fn save_preset(&mut self, device: PresetDevice) {
        let reference = match self.preset_ref(device) {
            Some(reference) if reference.origin == fontelle_types::PresetOrigin::User => reference,
            Some(_) => {
                self.message =
                    Some("factory presets are read-only \u{2014} use Save as\u{2026}".to_string());
                return;
            }
            None => {
                self.message = Some(
                    "this has no preset to save over \u{2014} use Save as\u{2026}".to_string(),
                );
                return;
            }
        };
        self.save_preset_as(device, &reference.name, &reference.category);
    }

    /// Writes this device's state as a preset of the user's own.
    ///
    /// The document command runs **after** the file is on disk, so an undo of
    /// the save puts the old name back and leaves the file where it is: undo
    /// is not a delete (§P.5).
    pub fn save_preset_as(&mut self, device: PresetDevice, name: &str, category: &str) {
        if let Err(why) = self.try_save_preset_as(device, name, category) {
            self.message = Some(why);
        }
    }

    fn try_save_preset_as(
        &mut self,
        device: PresetDevice,
        name: &str,
        category: &str,
    ) -> Result<(), String> {
        let kind = self
            .preset_device(device)
            .ok_or("there is nothing here to save")?;
        let payload = self
            .preset_payload(device)
            .ok_or("this device has no state to save")?;
        let preset = fontelle_types::Preset::new(kind, name, category, payload);
        // `true`: this is the one call that may replace a file, and both
        // buttons reach it — "Save" with the name it already has, "Save as…"
        // with a name the prompt has already asked about.
        let reference = self.preset_bank.save(&preset, true)?;
        // A track keeps no reference to the chain it was saved as: there is no
        // bar to show one in (`PresetDevice::Track`). What it does get is a
        // word about anything the save could not carry, because a chain that
        // came back two inserts short with nothing said would be blamed on the
        // load rather than on the save.
        if let PresetDevice::Track { strip } = device {
            let left = self.plugins_in_chain(strip);
            self.message = Some(match left {
                0 => format!("Saved track preset \u{201c}{name}\u{201d}"),
                1 => format!(
                    "Saved track preset \u{201c}{name}\u{201d} \u{2014} one hosted plugin left out"
                ),
                n => format!(
                    "Saved track preset \u{201c}{name}\u{201d} \u{2014} {n} hosted plugins left out"
                ),
            });
            self.dirty = true;
            self.touch();
            return Ok(());
        }
        let target = self
            .preset_target(device)
            .ok_or("that device is not there")?;
        self.run(Box::new(fontelle_model::SetPresetRef::new(
            target,
            Some(reference),
        )));
        self.history.break_gesture();
        self.dirty = true;
        self.touch();
        Ok(())
    }

    /// Stars one preset of this device's bank, by its place in
    /// [`Session::preset_choices`], or takes the star off — the Presets page's
    /// star on a row, which need not be the loaded preset.
    pub fn toggle_preset_star(&mut self, device: PresetDevice, index: usize) {
        let Some(kind) = self.preset_device(device) else {
            return;
        };
        let Some((name, origin)) = self
            .preset_bank
            .for_device(&kind)
            .get(index)
            .map(|entry| (entry.name.clone(), entry.origin))
        else {
            return;
        };
        self.toggle_favorite(fontelle_types::Favorite::Preset {
            device: kind,
            name,
            origin,
        });
    }

    /// Stars this device's preset, or takes the star off.
    pub fn toggle_preset_favorite(&mut self, device: PresetDevice) {
        let (Some(kind), Some(reference)) = (self.preset_device(device), self.preset_ref(device))
        else {
            return;
        };
        self.toggle_favorite(fontelle_types::Favorite::Preset {
            device: kind,
            name: reference.name,
            origin: reference.origin,
        });
    }

    /// Deletes one of the user's presets, by position in
    /// [`preset_choices`](Self::preset_choices).
    pub fn delete_preset(&mut self, device: PresetDevice, index: usize) {
        let Some(kind) = self.preset_device(device) else {
            return;
        };
        let entry = self
            .preset_bank
            .for_device(&kind)
            .into_iter()
            .nth(index)
            .cloned();
        let Some(entry) = entry else { return };
        if let Err(why) = self.preset_bank.delete(&entry) {
            self.message = Some(why);
        }
        self.touch();
    }
}

// ------------------------------------------------------- the mod matrix ---

/// The modulation matrix, as the window asks about it
/// (`docs/flopsynth-plan.md` §8.4).
///
/// Every one of these is about the **selected channel's** patch, because the
/// matrix is per-voice and a voice belongs to a channel. An insert has no
/// matrix and never will: an effect is not per-voice (§3.6), which is why the
/// Effects page's knobs do not light up during a drag-to-assign.
impl Session {
    /// This patch, if it is one with a matrix.
    fn matrix_patch(&self) -> Option<fontelle_core::Patch> {
        self.selected_patch()
    }

    /// Puts an effect of `kind` on the end of the selected instrument's own
    /// chain (`docs/flopsynth-plan.md` §8.5). A kind the chain may not hold,
    /// or a chain that is full, is refused and nothing is written — the
    /// window only offers what would be taken, so neither is a case a person
    /// reaches from it.
    pub fn add_patch_effect(&mut self, kind: fontelle_types::EffectKind) {
        let Some(channel) = self.selected_channel_id() else {
            return;
        };
        let Some(mut patch) = self.matrix_patch() else {
            return;
        };
        if !fontelle_core::flopsynth::PATCH_FX_KINDS.contains(&kind)
            || patch.fx.len() >= fontelle_core::MAX_PATCH_FX
        {
            return;
        }
        patch.fx.push(fontelle_core::PatchFx {
            config: fontelle_types::EffectConfig::new(kind),
            enabled: true,
        });
        // Structural, like a route: the node's chain is rebuilt in `prepare`
        // rather than nudged down the live wire.
        self.store_patch(channel, patch);
    }

    /// Takes slot `index` off the selected instrument's own chain.
    pub fn remove_patch_effect(&mut self, index: usize) {
        let Some(channel) = self.selected_channel_id() else {
            return;
        };
        let Some(mut patch) = self.matrix_patch() else {
            return;
        };
        if index >= patch.fx.len() {
            return;
        }
        patch.fx.remove(index);
        self.store_patch(channel, patch);
    }

    pub fn mod_sources(&self) -> Vec<String> {
        match self.matrix_patch() {
            Some(patch) => fontelle_core::flopsynth::sources(&patch)
                .into_iter()
                .map(|(_, label)| label)
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn routes_to(
        &self,
        address: &fontelle_types::ParamAddress,
    ) -> Vec<fontelle_ui::document::RouteInfo> {
        let Some(patch) = self.matrix_patch() else {
            return Vec::new();
        };
        let Some(dest) = fontelle_core::flopsynth::dest_for_address(&patch, address.as_str())
        else {
            return Vec::new();
        };
        let names: Vec<(fontelle_core::ModSource, String)> =
            fontelle_core::flopsynth::sources(&patch);
        patch
            .mod_matrix
            .routes
            .iter()
            .enumerate()
            .filter(|(_, route)| route.destination == dest)
            .map(|(index, route)| fontelle_ui::document::RouteInfo {
                source: names
                    .iter()
                    .find(|(source, _)| *source == route.source)
                    .map(|(_, label)| label.clone())
                    // A source the badge row does not list — one a patch from
                    // another build carries — is named rather than hidden: a
                    // route you cannot see is a route you cannot remove.
                    .unwrap_or_else(|| format!("{:?}", route.source)),
                depth: route.depth,
                depth_address: fontelle_types::ParamAddress::new(format!(
                    "patch/mod[{index}]/depth"
                )),
            })
            .collect()
    }

    pub fn is_mod_destination(&self, address: &fontelle_types::ParamAddress) -> bool {
        self.matrix_patch().is_some_and(|patch| {
            fontelle_core::flopsynth::dest_for_address(&patch, address.as_str()).is_some()
        })
    }

    pub fn add_route(&mut self, source: usize, address: &fontelle_types::ParamAddress) {
        let Some(channel) = self.selected_channel_id() else {
            return;
        };
        let Some(mut patch) = self.matrix_patch() else {
            return;
        };
        let Some(dest) = fontelle_core::flopsynth::dest_for_address(&patch, address.as_str())
        else {
            return;
        };
        let Some((source, _)) = fontelle_core::flopsynth::sources(&patch)
            .into_iter()
            .nth(source)
        else {
            return;
        };
        // A route that is already there is **moved to full depth** rather than
        // doubled: dropping the same source on the same knob twice is somebody
        // saying "more of that", not asking for two routes that add up.
        if let Some(existing) = patch
            .mod_matrix
            .routes
            .iter_mut()
            .find(|route| route.source == source && route.destination == dest)
        {
            existing.depth = NEW_ROUTE_DEPTH;
        } else {
            patch.mod_matrix.routes.push(fontelle_core::ModRoute {
                source,
                destination: dest,
                depth: NEW_ROUTE_DEPTH,
                curve: fontelle_core::Curve::Linear,
                via: None,
                invert: false,
            });
        }
        // Structural: the number of routes changed, so the running voice's
        // matrix has to be rebuilt rather than nudged down the live wire —
        // which is the same rule a wavetable swap follows (§2.3).
        self.store_patch(channel, patch);
    }

    pub fn remove_route(&mut self, address: &fontelle_types::ParamAddress, index: usize) {
        let Some(channel) = self.selected_channel_id() else {
            return;
        };
        let Some(mut patch) = self.matrix_patch() else {
            return;
        };
        let Some(dest) = fontelle_core::flopsynth::dest_for_address(&patch, address.as_str())
        else {
            return;
        };
        // `index` counts the routes *to this destination*, which is what the
        // window listed; the matrix is indexed by all of them.
        let at = patch
            .mod_matrix
            .routes
            .iter()
            .enumerate()
            .filter(|(_, route)| route.destination == dest)
            .map(|(at, _)| at)
            .nth(index);
        let Some(at) = at else { return };
        patch.mod_matrix.routes.remove(at);
        self.store_patch(channel, patch);
    }
}

/// What a route made by dragging a badge onto a knob starts at.
///
/// §8.4: half depth, so it is audible the moment it is made. A route that
/// arrived at zero would look like a gesture that did nothing.
const NEW_ROUTE_DEPTH: f32 = 0.5;

// ----------------------------------------------------- the Presets tab ---

/// The browser's fifth tab (`docs/flopsynth-plan.md` §P.8).
///
/// **The Sounds tab's shape with a different list in it**: devices above,
/// that device's presets below, grouped by category. Reusing the shape rather
/// than inventing a folder walk of its own is why a fifth tab is a variant and
/// a few functions rather than a panel — the search, the virtualised list, the
/// stars and the click-to-apply are all already here.
impl Session {
    /// Every preset the tab can show, in the order its rows appear.
    ///
    /// One list, used by the row builder *and* by the click — so a row and
    /// what it does cannot come to disagree, which is the defect a menu of
    /// strings indexed into a second list has every time.
    fn preset_tab_entries(&self) -> Vec<&crate::preset_bank::PresetEntry> {
        let query = self.query.trim();
        if !query.is_empty() {
            // Searching runs across **every** device: you go looking for
            // "hall" without first deciding it is a reverb.
            return self.preset_bank.search(query);
        }
        match &self.preset_device_open {
            Some(device) => self.preset_bank.for_device(device),
            None => Vec::new(),
        }
    }

    /// The left-hand list: one row per device that has presets.
    fn preset_tab_devices(&self) -> Vec<fontelle_ui::document::LibraryEntry> {
        self.preset_bank
            .devices()
            .into_iter()
            .map(|device| {
                let count = self.preset_bank.for_device(&device).len();
                fontelle_ui::document::LibraryEntry {
                    name: device.label(),
                    detail: match count {
                        1 => "1 preset".to_string(),
                        n => format!("{n} presets"),
                    },
                    kind: fontelle_ui::document::LibraryKind::Folder,
                }
            })
            .collect()
    }

    /// The right-hand list: the open device's presets, under category
    /// headings — or, while searching, every device's, under device headings.
    fn preset_tab_rows(&self) -> Vec<PresetRow> {
        let entries = self.preset_tab_entries();
        let searching = !self.query.trim().is_empty();
        let mut rows = Vec::new();
        let mut heading: Option<String> = None;
        for (index, entry) in entries.iter().enumerate() {
            let group = match searching {
                true => entry.device.label(),
                false => entry.category.clone(),
            };
            if heading.as_deref() != Some(group.as_str()) {
                rows.push(PresetRow::Group {
                    name: group.clone(),
                    detail: String::new(),
                });
                heading = Some(group);
            }
            rows.push(PresetRow::Preset {
                file: entry.path.clone(),
                index,
                name: entry.name.clone(),
                // Which bank it came from, so a preset of your own is
                // tellable from a factory one of the same name (§P.3).
                detail: match entry.origin {
                    fontelle_types::PresetOrigin::User => "mine".to_string(),
                    fontelle_types::PresetOrigin::Factory => String::new(),
                },
            });
        }
        rows
    }

    /// Opens a device row.
    fn open_preset_device(&mut self, index: usize) -> Result<(), String> {
        let device = self
            .preset_bank
            .devices()
            .into_iter()
            .nth(index)
            .ok_or("that device is not in the bank")?;
        self.preset_device_open = Some(device);
        self.touch();
        Ok(())
    }

    /// Applies the preset at row `index` to whatever it is for.
    ///
    /// An **instrument** preset goes onto the selected channel, switching its
    /// kind if it has to (§P.5); an **effect** preset goes into the insert
    /// whose window is open, and is refused when none is — a preset that
    /// landed on whichever insert happened to be first would be a click that
    /// changed a sound nobody was looking at.
    fn install_bank_preset(&mut self, row: usize) -> Result<(), String> {
        // The **row**, which is what was clicked: the list has headings in it,
        // so a row number is not an entry number. The row carries the entry's
        // own index, exactly as a soundfont's preset row carries its place in
        // the file — which is what keeps the two from drifting apart.
        let index = match self.preset_tab_rows().into_iter().nth(row) {
            Some(PresetRow::Preset { index, .. }) => index,
            Some(PresetRow::Group { .. }) => {
                return Err("that row is a heading, not a preset".to_string());
            }
            None => return Err("that preset is not in the bank".to_string()),
        };
        let entry = self
            .preset_tab_entries()
            .into_iter()
            .nth(index)
            .ok_or("that preset is not in the bank")?
            .clone();
        let device = match &entry.device {
            fontelle_types::DeviceKind::Effect(_) => {
                let (strip, slot) = self
                    .open_insert
                    .ok_or("open an effect\u{2019}s window first")?;
                fontelle_ui::canvas::PresetDevice::Insert { strip, slot }
            }
            _ => fontelle_ui::canvas::PresetDevice::Instrument,
        };
        // Through the same command every other route uses, by *entry*: this
        // list is the whole bank and the channel may be playing something
        // else entirely. A preset for an effect this slot is not is refused by
        // the command itself, which is where that rule lives.
        self.apply_preset_entry(device, &entry)
    }

    /// What the tab says along its bottom.
    fn preset_tab_status(&self) -> String {
        let (mut factory, mut user) = (0, 0);
        for entry in self.preset_bank.entries() {
            match entry.origin {
                fontelle_types::PresetOrigin::Factory => factory += 1,
                fontelle_types::PresetOrigin::User => user += 1,
            }
        }
        let query = self.query.trim();
        if !query.is_empty() {
            return match self.preset_tab_entries().len() {
                0 => format!("nothing matching \u{201c}{query}\u{201d}"),
                1 => format!("1 preset matching \u{201c}{query}\u{201d}"),
                n => format!("{n} presets matching \u{201c}{query}\u{201d}"),
            };
        }
        match self.preset_bank.user_dir() {
            Some(dir) => format!(
                "{factory} built in, {user} of yours in {}",
                crate::desktop::elide_path(dir, 2)
            ),
            None => format!("{factory} built in \u{2014} no folder for your own"),
        }
    }
}

impl Session {
    /// How many voices the **selected channel's** instrument is playing
    /// (`docs/flopsynth-plan.md` §11, phase 6).
    ///
    /// Zero for a channel with no meter — one whose graph has not been built
    /// yet, and every channel in an offline render, where nobody is watching.
    pub fn voice_count(&self) -> usize {
        self.selected_channel_id()
            .and_then(|id| self.voice_meters.get(&id))
            .map_or(0, |meter| meter.voices())
    }

    /// Where the selected channel's newest voice has its LFOs, 0..1 each.
    ///
    /// All zero when nothing is sounding, which is a dot parked at the start —
    /// honest about a synthesiser that is not playing.
    pub fn lfo_phases(&self) -> [f32; fontelle_core::MAX_LFOS] {
        self.selected_channel_id()
            .and_then(|id| self.voice_meters.get(&id))
            .map_or([0.0; fontelle_core::MAX_LFOS], |meter| meter.lfo_phases())
    }
}

/// Which pitch classes MIDI is forcing right now, bit 0 = C.
///
/// Read off the newest frame the tap holds rather than from the node's own
/// `HeldKeys`, which lives on the audio thread behind the graph and has no
/// wire back. [`fontelle_types::TuneFrame`] is deliberately four aligned
/// words — that is what lets the node write one with four stores that cannot
/// tear — so a held mask does not go in it.
///
/// What this shows is therefore the note being **forced**, not every key
/// somebody is leaning on: a chord held on the source channel lights the one
/// note the corrector chose from it. That is the note the trace is drawn
/// against, so it is the one the keyboard should agree with; a keyboard
/// lighting three keys while the trace bent towards one of them would be a
/// picture disagreeing with itself.
fn held_classes(trace: &[fontelle_types::TuneFrame]) -> u16 {
    let Some(frame) = trace.last() else {
        return 0;
    };
    if frame.flags & fontelle_types::TUNE_FROM_MIDI == 0 {
        return 0;
    }
    let semitone = (frame.target_cents / 100.0).round();
    if !semitone.is_finite() || !(0.0..=127.0).contains(&semitone) {
        return 0;
    }
    1 << (semitone as u16 % 12)
}
