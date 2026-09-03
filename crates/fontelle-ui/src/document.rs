//! The window's view of the open studio, and the only way it may change it.
//!
//! The same shape as [`crate::transport::TransportHost`], for the same reason:
//! `fontelle-app` is the layer allowed to see everything, so the seam lives
//! here as a trait and this crate stays testable with a fake.
//!
//! **INVARIANT 2 and INVARIANT 9 are what these traits are for.** The roll
//! never writes to a `Project`; it produces [`RollEdit`](crate::canvas::RollEdit)
//! values, and the implementation on the other side turns each into a
//! `Command` and puts it through `History`. There is deliberately no method
//! here that hands out a mutable document.
//!
//! Two traits rather than one, and the split is the same split the crate
//! dependency graph has:
//!
//! - [`DocumentHost`] is the *clip* — notes in, edits out. The piano roll uses
//!   only this, which is why the roll's tests need nothing but an `Arena`.
//! - [`StudioHost`] is everything else the window shows: the channel rack, the
//!   soundfont bank, and the audition path that makes a drawn note audible.
//!   Reading a soundfont is a filesystem operation, so none of it can live in
//!   this crate — only the shape of the question.

use fontelle_model::{Arena, Note};
use fontelle_types::{ClipId, NoteId, PPQN, PointId, Sample, Tick};

use crate::canvas::{ArrangeEdit, InstrumentView, RollEdit};

pub trait DocumentHost {
    /// The notes of the clip the roll is showing.
    fn notes(&self) -> &Arena<NoteId, Note>;

    /// Applies one edit as a command, recording it in the history.
    ///
    /// Returns the ids of any notes it **created** — empty for every edit that
    /// creates none. The roll needs them: drawing a note and dragging it to
    /// length is one gesture, and the second half of it cannot start until the
    /// id of the note the first half made is known.
    fn edit(&mut self, edit: RollEdit) -> Vec<NoteId>;

    fn undo(&mut self);
    fn redo(&mut self);

    /// Ends the current gesture, so the next edit starts a new undo entry
    /// rather than coalescing into the last. Called on mouse-up: only the
    /// caller knows the drag is over (see `History::break_gesture`).
    fn end_gesture(&mut self);

    /// How long one tick is, in seconds, around the part of the song being
    /// edited.
    ///
    /// What the audition needs: clicking a half note should sound a half note,
    /// and the roll knows the length in ticks but has no tempo and should not
    /// want one (INVARIANT 5 — a song with a tempo change has no single BPM,
    /// and only the document holds the map).
    ///
    /// Defaulted to 120 BPM so a host that has not got a tempo map — a test
    /// fake, a window with no project open — still sounds a note of roughly
    /// the right length rather than having to answer a question about a clock
    /// it does not own.
    fn seconds_per_tick(&self) -> f64 {
        0.5 / PPQN as f64
    }

    /// The time signature's numerator, for the grid and the read-out.
    fn beats_per_bar(&self) -> u32;

    /// The tempo in force at the **start** of the piece, in beats per minute.
    ///
    /// The start of it, and the transport bar's box says so by being one
    /// number: a song with a tempo change has no single BPM (INVARIANT 5), and
    /// the rest of the curve is the tempo map's, which nothing in this crate
    /// may see. An imported file's ramps survive somebody nudging this.
    fn tempo(&self) -> f64;

    /// Moves it. One `Command` through the history, like every other edit —
    /// which is what makes a dragged tempo undoable and saved.
    ///
    /// Coalescing is the implementation's business (`SetNumber::merge_with`),
    /// so a whole drag is one undo entry; the window calls
    /// [`end_gesture`](DocumentHost::end_gesture) when the mouse comes up, the
    /// same as it does for a note drag.
    fn set_tempo(&mut self, bpm: f64);

    /// Sets the time signature's numerator. See
    /// [`beats_per_bar`](DocumentHost::beats_per_bar).
    fn set_beats_per_bar(&mut self, beats: u32);

    /// Where `position_sample` falls inside this clip, in the clip's own
    /// ticks, or `None` when the playhead is somewhere the clip does not
    /// cover.
    fn playhead_tick(&self, position_sample: Sample) -> Option<Tick>;

    /// The other direction: where a tick *in this clip* is in the song, in
    /// samples. What clicking the roll's ruler to move the playhead needs.
    ///
    /// It has to be asked rather than worked out, because a song with a tempo
    /// change has no single BPM to multiply by (INVARIANT 5) and only the
    /// document holds the map.
    fn sample_of_clip_tick(&self, tick: Tick) -> Sample;

    /// A tick in this clip, as a tick in the **song** — what a time selection
    /// dragged on the roll's ruler is stored as (TDD §6.3: loop points are
    /// ticks, and they are the song's).
    ///
    /// Defaulted to the identity for a host with no clip offset, which is
    /// what every test fake is.
    fn song_tick_of_clip_tick(&self, tick: Tick) -> Tick {
        tick
    }

    /// The other direction, for drawing the song's selection on the roll's
    /// ruler. May land before the clip or after it; the ruler clips it.
    fn clip_tick_of_song_tick(&self, tick: Tick) -> Tick {
        tick
    }

    /// Whether there are changes not yet on disk.
    fn is_dirty(&self) -> bool;

    /// Writes the project out. `Err` carries something worth showing a person.
    fn save(&mut self) -> Result<(), String>;

    /// Where it would be saved, for the title bar.
    fn name(&self) -> &str;
}

/// One channel, as the rack draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInfo {
    pub name: String,
    pub muted: bool,
    pub soloed: bool,
    /// A channel with no soundfont on it yet. Drawn differently, because a
    /// silent channel that looks like every other channel is a mystery.
    pub has_instrument: bool,
    /// Which mixer strip this channel plays through, as an index into
    /// [`StudioHost::route_names`]. `None` is the master.
    ///
    /// A channel no longer owns a track (see `fontelle_model::Channel`), so
    /// "where does this go" is a question the rack has to be able to both ask
    /// and answer — hence a chip on the row rather than a fact you could only
    /// find by reading the file.
    pub route: Option<usize>,
}

/// What is inside a clip, as far as drawing it goes.
///
/// The arrangement draws two things very differently: a note clip is a block
/// with a caption, and an automation clip is a **curve** — and a curve drawn
/// as a block is what *"a lane of them looks like a lane of empty clips"*
/// describes.
///
/// Audio clips are §15 (M6) and are not a variant yet: adding one before there
/// is anything to draw would be a case the canvas has to handle and can never
/// be given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipKind {
    #[default]
    Notes,
    /// A parameter's curve over time (TDD §12.1).
    Automation,
}

/// One clip, as the arrangement canvas draws it.
///
/// A flattened view rather than a `&Clip`: the canvas may not see a `Project`
/// (INVARIANT 2), and everything it needs to draw and hit-test a block is here.
///
/// `PartialEq` but not `Eq`, since an automation block carries its curve and a
/// curve is floats. Nothing compares these for identity — the `id` is what
/// identity means here — and the derive is for tests.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipInfo {
    pub id: ClipId,
    /// Which row it is on, counted from the top of the project's lane list.
    pub lane: usize,
    pub start: Tick,
    pub length: Tick,
    /// What is written on the block — the channel it plays, which is the thing
    /// a person is looking for when they scan an arrangement.
    pub name: String,
    pub muted: bool,
    /// Whether this is the clip the piano roll has open, so the arrangement can
    /// say which one you are editing.
    pub open: bool,
    pub color: [u8; 4],
    /// How long the clip's content is before it repeats, in ticks. `None` is a
    /// clip that plays once.
    ///
    /// The canvas needs it to draw the seams between the repeats (see
    /// [`loop_marks`](crate::canvas::loop_marks)) and to know what period a
    /// Shift-drag on the edge should keep rather than overwrite.
    pub loop_length: Option<Tick>,
    /// Which kind of block this is, and so which way it is drawn.
    pub kind: ClipKind,
    /// For [`ClipKind::Automation`], the points of its curve, **in time
    /// order**.
    ///
    /// Flattened onto the block for the reason the rest of `ClipInfo` is: the
    /// canvas may not see a `Project` (INVARIANT 2). Empty for every other
    /// kind, and read on the studio's revision rather than per frame, so a
    /// `Vec` per automation clip costs nothing per frame. The block is
    /// **edited** through these — a point's id is what a drag names — which
    /// is why they are points rather than a sampled line.
    pub curve: Vec<CurvePoint>,
    /// For [`ClipKind::Notes`], the clip's own notes — **the pattern**, in the
    /// clip's ticks, not expanded across a loop's passes.
    ///
    /// *"make it so the midi clips in the arrangement arent just blank
    /// rectangles but instead actually show a preview of the notes drawn out
    /// inside of it like how other daws do."*
    ///
    /// Unexpanded on purpose: a two-hundred-bar clip looping one bar would be
    /// two hundred copies of the same list, held per clip, rebuilt on every
    /// revision. The canvas tiles them the way it already tiles the seams
    /// (`canvas::loop_marks`), which is also what makes the picture and the
    /// seams one picture rather than two that can disagree.
    ///
    /// Flattened onto the block for the reason the rest of `ClipInfo` is: the
    /// canvas may not see a `Project` (INVARIANT 2). Empty for every other
    /// kind.
    pub notes: Vec<NotePreview>,
}

/// One note, as the arrangement draws it inside its clip.
///
/// Position and pitch and nothing else: a preview is a shape, and velocity,
/// pan and the rest are the roll's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotePreview {
    /// In the clip's own ticks.
    pub start: Tick,
    pub length: Tick,
    pub key: u8,
}

/// One point of an automation clip's curve, as the arrangement draws and
/// edits it (TDD §12.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurvePoint {
    pub id: PointId,
    /// In the clip's own ticks.
    pub tick: Tick,
    /// Normalised, 0..1 — §12.1's unit.
    pub value: f64,
    /// The shape of the segment **following** this point.
    pub curve: fontelle_model::CurveShape,
}

/// What pressing play plays.
///
/// FL Studio's song/pattern switch. *"there should also be a way to swap
/// between clip and song mode currently its always on song so you cant ONLY
/// focus one instrument."* Session state rather than the document's: it is
/// how you are listening, not what the song is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayMode {
    /// The arrangement, end to end.
    #[default]
    Song,
    /// Only the clip being edited, round and round.
    Clip,
}

impl PlayMode {
    /// The other one — the chip on the transport bar toggles.
    pub fn next(self) -> Self {
        match self {
            Self::Song => Self::Clip,
            Self::Clip => Self::Song,
        }
    }

    /// What the chip says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Song => "Song",
            Self::Clip => "Clip",
        }
    }
}

/// What an arrangement edit made, handed back so the gesture that made it
/// can carry on — see [`StudioHost::arrange`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Created {
    /// Clips a duplicate, a paste or a draw put down. They become the
    /// selection — see `Timeline::clips_inserted`.
    pub clips: Vec<ClipId>,
    /// Points a press on a curve made. The drag that follows moves them —
    /// see `Timeline::points_inserted`.
    pub points: Vec<PointId>,
}

/// Which other instruments' notes the piano roll shows behind its own.
///
/// Reported as *"onion skins"*: writing a melody against a chord part means
/// looking at both, and the two live on different channels in different clips.
/// The roll shows the other one ghosted, lined up in time.
///
/// A filter rather than a switch, because on a busy project "all of them" is
/// as unreadable as none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GhostFilter {
    #[default]
    Off,
    /// Every channel but the one being edited.
    All,
    /// One channel, by its index in [`StudioHost::channels`].
    Channel(usize),
}

impl GhostFilter {
    /// The next filter the toolbar chip steps to.
    ///
    /// Off, then everything, then one instrument at a time, then off again.
    /// `channels` is how many the project has *including* the one being
    /// edited — with one channel there is nothing to skin against, and with
    /// two the per-channel steps and "all" are the same picture, but stepping
    /// through them anyway would be a chip that appears to do nothing.
    pub fn next(self, channels: usize) -> Self {
        if channels == 0 {
            return Self::Off;
        }
        match self {
            Self::Off if channels > 1 => Self::All,
            Self::Off => Self::Off,
            Self::All if channels > 1 => Self::Channel(0),
            Self::All => Self::Off,
            // A filter naming a channel that has since been deleted falls back
            // to off rather than showing nothing for ever.
            Self::Channel(index) if index + 1 < channels => Self::Channel(index + 1),
            Self::Channel(_) => Self::Off,
        }
    }

    /// What the chip says.
    pub fn label(self) -> String {
        match self {
            Self::Off => "skin".to_string(),
            Self::All => "all".to_string(),
            Self::Channel(index) => format!("ch{}", index + 1),
        }
    }
}

/// One note from *another* channel, drawn behind the roll's own.
///
/// Already in the open clip's tick space: a ghost that is not lined up with
/// the notes it is meant to be read against is worse than no ghost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GhostNote {
    pub start: Tick,
    pub length: Tick,
    pub key: u8,
    /// The colour of the lane it came from, so two ghosted parts are tellable
    /// apart.
    pub color: [u8; 4],
}

/// One send, as the track-options column draws it (TDD §13.2).
///
/// A send is a copy of a track's signal at a level into another track — which
/// is what a reverb bus is, and the whole difference from an *output*: routing
/// an output moves the signal, a send leaves the dry path alone.
#[derive(Debug, Clone, PartialEq)]
pub struct SendInfo {
    /// Where it goes, as an index into [`StudioHost::route_names`].
    pub target: usize,
    /// And what that track is called, so the row can be drawn without the
    /// panel holding the whole list (INVARIANT 2).
    pub target_name: String,
    pub level_db: f32,
    /// Taken before the fader rather than after it.
    ///
    /// The difference is audible and is why the switch exists: a post-fader
    /// send follows the fader down, which is what a reverb wants, and a
    /// pre-fader one does not, which is what a cue mix wants.
    pub pre_fader: bool,
}

/// One mixer track, as the mixer panel draws it.
///
/// A flattened view rather than a `&MixerTrack`, like [`ChannelInfo`] and
/// [`ClipInfo`] and for the same reason: the canvas may not see a `Project`
/// (INVARIANT 2).
///
/// The meters are **not** here. Everything in this struct changes when
/// somebody clicks something, so it is read on
/// [`StudioHost::revision`](StudioHost::revision) with the rest of the lists; a
/// meter changes every block, and lives in
/// [`StudioHost::mixer_peaks`](StudioHost::mixer_peaks), which is read once a
/// frame while the panel is showing.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerStrip {
    pub name: String,
    /// The fader, in decibels. Unity is 0.0.
    pub gain_db: f32,
    /// The track's **balance**: -1.0 hard left, 0.0 centre, +1.0 hard right.
    ///
    /// Not the same control as a channel's own pan, and the difference is
    /// audible — see `fontelle_model::Channel::pan`. This one sits over a bus
    /// whose contents have already been placed.
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// The master strip is drawn apart from the others and pinned to the
    /// right-hand end: it is where everything arrives, not one of the things
    /// arriving.
    pub is_master: bool,
    pub color: [u8; 4],
    /// The track's insert chain, in the order the sound goes through it.
    ///
    /// Labels and switches, not parameters: a strip shows *what is on the
    /// track*, and what each one is set to is the editor's business. Keeping
    /// it flat is INVARIANT 2 again — a canvas may not see a `Project`, and an
    /// `EffectConfig` per strip per frame would be eight bands copied for
    /// every track on screen to draw a three-letter word.
    pub inserts: Vec<crate::canvas::InsertInfo>,
    /// The copies of this track's signal going to other tracks (§13.2). Drawn
    /// in the track-options column, not on the strip: a send is two numbers
    /// and a destination, and a 76-pixel strip has room for none of them.
    pub sends: Vec<SendInfo>,
}

/// One lane, as the arrangement's header column draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneInfo {
    pub name: String,
    pub muted: bool,
}

/// What a row of the browser *is*, so the panel can draw it and route a click.
///
/// A collection is organised into folders on purpose, and until this existed
/// the browser flattened the whole tree into one list of names — throwing that
/// organisation away. See `fontelle_app::bank::BankRow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryKind {
    /// A soundfont, or a preset inside one. Clicking it opens it.
    #[default]
    File,
    /// A folder. Clicking it goes in.
    Folder,
    /// The row back up. Always first when it is there.
    Up,
    /// A heading over the rows under it — the soundfont a run of search hits
    /// came from. Not something to click: it says where, it does not go there.
    Group,
}

/// One row of the browser: a soundfont, a folder, or a preset inside a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub name: String,
    /// The size of a file, the count inside a folder, or the bank and program
    /// of a preset — the second column, in the muted ink.
    pub detail: String,
    /// What clicking it does. The **host** acts on it — see
    /// [`open_file`](StudioHost::open_file), which is "activate row N"
    /// whatever the row turns out to be — and the panel uses it only to
    /// choose a glyph.
    pub kind: LibraryKind,
}

impl LibraryEntry {
    /// A plain file row, which is what every caller that predates folders
    /// wants.
    pub fn file(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            detail: detail.into(),
            kind: LibraryKind::File,
        }
    }
}

/// What the instrument on a channel can actually play, key by key, and what
/// each key is called.
///
/// Reported from using the window: a drum soundfont draws exactly like a piano
/// one, so the only way to find the four keys that make a sound is to click
/// all 128. Both facts needed to fix that were already in the patch — a
/// `Layer` names the key range it covers, and since the importer began keeping
/// sample names it also knows the layer on key 38 is the snare — and neither
/// had any way to reach a canvas.
///
/// Flattened, like [`ChannelInfo`] and [`ClipInfo`], because the roll may not
/// see a `Patch` (INVARIANT 2). Built in `fontelle-app`, which is the one
/// layer that sees both the patch and the sample library.
///
/// **An empty map means "not known", not "plays nothing"** — see
/// [`KeyMap::unknown`]. The two are very different statements and drawing them
/// the same way would grey out the whole keyboard of a channel whose
/// instrument has simply not been chosen yet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyMap {
    /// Empty, or exactly [`KeyMap::KEYS`] long.
    keys: Vec<KeyInfo>,
}

/// One key's entry in a [`KeyMap`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyInfo {
    /// Whether any layer of the instrument covers this key. A note on a key
    /// nothing covers starts a voice with every layer inactive and is silent —
    /// see `Voice::trigger_note` — which is exactly what the roll greys out.
    pub playable: bool,
    /// What the sample on this key is called, when the zone carrying it is
    /// narrow enough that the name describes *this key* rather than a
    /// register. `None` for a pitched multisample, which is what keeps a
    /// normal session looking normal.
    pub name: Option<String>,
}

impl KeyMap {
    /// MIDI's whole keyboard.
    pub const KEYS: usize = 128;

    /// Nothing is known: no instrument on the channel, or one this build
    /// could not read. Everything reads playable and nothing is named.
    pub fn unknown() -> Self {
        Self::default()
    }

    /// A map over all 128 keys. Shorter input is padded with unplayable keys
    /// and longer input is truncated, so callers cannot produce a map the
    /// canvas has to bounds-check.
    pub fn new(mut keys: Vec<KeyInfo>) -> Self {
        keys.resize(Self::KEYS, KeyInfo::default());
        Self { keys }
    }

    /// Whether this map says anything at all. `false` is
    /// [`KeyMap::unknown`].
    pub fn is_known(&self) -> bool {
        !self.keys.is_empty()
    }

    /// Whether `key` sounds. **True when nothing is known**, because the
    /// absence of an instrument is not evidence that a key is silent.
    pub fn plays(&self, key: u8) -> bool {
        self.keys
            .get(usize::from(key))
            .is_none_or(|info| info.playable)
    }

    pub fn name(&self, key: u8) -> Option<&str> {
        self.keys.get(usize::from(key))?.name.as_deref()
    }

    /// Whether any key carries a name — what tells the roll to make room for
    /// them beside its keyboard. A melodic instrument names nothing, so its
    /// roll is the roll it always was.
    pub fn is_named(&self) -> bool {
        self.keys.iter().any(|info| info.name.is_some())
    }
}

/// Everything the window shows that is not the clip.
pub trait StudioHost: DocumentHost {
    /// Bumped whenever anything a panel draws has changed.
    ///
    /// The window caches the lists below and re-reads them when this moves,
    /// rather than rebuilding a `Vec<String>` every frame for a list that
    /// changes when somebody clicks something.
    fn revision(&self) -> u64;

    // --- the channel rack ---
    fn channels(&self) -> Vec<ChannelInfo>;
    /// Which channel the piano roll is showing.
    fn selected_channel(&self) -> usize;
    fn select_channel(&mut self, index: usize);
    fn toggle_mute(&mut self, index: usize);
    fn toggle_solo(&mut self, index: usize);

    // --- routing (TDD §13.1) ---
    /// What every mixer strip is called, in the order
    /// [`mixer_strips`](StudioHost::mixer_strips) gives — so the rack's route
    /// chip and the mixer's strips are indexed the same way, and the master is
    /// last.
    fn route_names(&self) -> Vec<String>;

    /// Points channel `channel` at strip `route`. `None` — and the master's
    /// own strip — both mean the master.
    fn set_channel_route(&mut self, channel: usize, route: Option<usize>);

    /// Makes a mixer track. **Nothing else makes one**: a strip is a
    /// destination somebody builds, which is exactly the distinction the old
    /// one-track-per-channel model could not express.
    fn add_mixer_track(&mut self);
    fn remove_mixer_track(&mut self, strip: usize);
    fn rename_mixer_track(&mut self, strip: usize, name: &str);

    // --- the soundfont bank (TDD §17.5) ---
    /// The bank, already filtered and ordered by the live search.
    fn library_files(&self) -> Vec<LibraryEntry>;
    /// The presets inside the open file, filtered by the same search.
    fn library_presets(&self) -> Vec<LibraryEntry>;
    fn query(&self) -> &str;
    fn set_query(&mut self, query: &str);
    /// Activates row `index` of [`library_files`](StudioHost::library_files).
    ///
    /// **Whatever the row is**: a soundfont's presets are read, a folder is
    /// opened, and the row back up goes back up. The decision is the host's
    /// because only it knows what row `index` turned out to be — the panel has
    /// a [`LibraryKind`] for drawing and no business acting on it.
    fn open_file(&mut self, index: usize) -> Result<(), String>;
    fn selected_file(&self) -> Option<usize>;
    /// Which preset of the open file the **selected channel** is playing, as an
    /// index into [`library_presets`](StudioHost::library_presets).
    ///
    /// `None` when the channel is playing nothing, or something from a file
    /// other than the one that is open, or something the live search has
    /// filtered out of the list. Without this the browser could not say which
    /// instrument was chosen, and flicking through presets changed the sound
    /// while the panel said nothing had happened at all.
    fn selected_preset(&self) -> Option<usize>;
    /// Makes a **blank** channel: the host's built-in instrument, a lane and an
    /// empty clip, selected and ready to play.
    ///
    /// The rack's add button calls this, and it takes no argument on purpose.
    /// It used to need a preset chosen in the browser first, so pressing it
    /// with nothing chosen did nothing at all — reported as *"clicking new
    /// instrument doesnt do anything until i select an instrument in the
    /// soundfonts tab... then it actually happening later when you werent
    /// intending, when you were trying to swap instruments on a channel"*. A
    /// button whose effect arrives on somebody else's click is worse than a
    /// button that does nothing, and both are fixed by having something to
    /// make.
    fn add_channel(&mut self) -> Result<(), String> {
        Err("this studio cannot add channels".to_string())
    }
    /// Puts preset `index` of the open file onto a **new** channel.
    fn add_channel_with(&mut self, preset: usize) -> Result<(), String>;
    /// Puts it on the channel the rack has selected instead.
    fn set_channel_instrument(&mut self, preset: usize) -> Result<(), String>;
    /// Copies channel `index` — its instrument, its settings and its clips —
    /// onto a new channel and a row of its own.
    ///
    /// *"stuff like being able to right click and duplicate too, for
    /// instruments in the channel rack for example."*
    fn duplicate_channel(&mut self, _index: usize) {}
    /// Deletes channel `index` and the clips that play it.
    fn remove_channel(&mut self, _index: usize) {}
    /// Renames it. Called per keystroke while a name is being typed; the
    /// implementation coalesces them into one undo entry, the same way a
    /// dragged knob does.
    fn rename_channel(&mut self, _index: usize, _name: &str) {}
    /// Takes the instrument off it, leaving a channel that plays nothing.
    fn clear_channel_instrument(&mut self, _index: usize) {}

    // --- the arrangement's rows (TDD §10.3) ---

    /// Adds a lane.
    fn add_lane(&mut self) {}
    /// Deletes lane `index` **and the clips on it** — a clip on no lane is one
    /// nothing can draw and nothing can reach.
    ///
    /// *"i made one i dont want but i cant right click and delete it."*
    fn remove_lane(&mut self, _index: usize) {}
    /// Renames it, per keystroke, like [`rename_channel`](Self::rename_channel).
    fn rename_lane(&mut self, _index: usize, _name: &str) {}
    /// Whether the arrangement would still have a row without lane `index` —
    /// what greys out "Delete lane" rather than letting a press fail.
    fn can_remove_lane(&self, _index: usize) -> bool {
        false
    }

    /// Makes an automation lane for one control on the **selected channel's**
    /// instrument panel, by the address the panel gave it.
    ///
    /// Every one of them: the channel's own level and placement, and the
    /// knobs inside the patch — a cutoff, an envelope stage, an oscillator's
    /// level. *"i want to be able to right click on a knob and select create
    /// automation clip with value and then it appears in my timeline."*
    ///
    /// A parameter has **one** lane, so a second right-click opens the one
    /// that is there rather than making another.
    fn automate_instrument_param(&mut self, _address: &fontelle_types::ParamAddress, _at: Sample) {}
    // --- the projects folder (TDD §17.1, §17.3) ---
    /// Every project in the configured projects folder, name and age.
    ///
    /// Empty when there is no folder configured, which on a first run there is
    /// not: **INVARIANT 10** says Fontelle writes nothing outside places the
    /// user named, so there is no default and no guess at `~/Documents`.
    fn projects(&self) -> Vec<LibraryEntry> {
        Vec::new()
    }

    /// One line about where the projects are — what to say when the list is
    /// empty, which on a first run it is.
    fn project_status(&self) -> String {
        String::new()
    }

    /// Makes a project in that folder and opens it. `Err` carries something
    /// worth showing a person — chiefly "there is no folder yet".
    fn new_project(&mut self) -> Result<(), String> {
        Err("this build cannot make projects".to_string())
    }

    /// Opens project `index` of [`projects`](StudioHost::projects), replacing
    /// what is open now.
    fn open_project(&mut self, index: usize) -> Result<(), String> {
        let _ = index;
        Err("this build cannot open projects".to_string())
    }

    /// Bounces the whole project to a WAV inside its own `renders/` folder.
    /// `Ok` carries a line worth showing; `Err` does too.
    fn export_wav(&mut self) -> Result<String, String> {
        Err("this build cannot export".to_string())
    }

    /// Writes a backup into the project's `backups/` folder, and says whether
    /// anything was written.
    ///
    /// **Nothing is unless there are unsaved changes**, which is what lets the
    /// window call this on a timer without keeping itself awake (§16.3).
    fn autosave(&mut self) -> bool {
        false
    }

    /// Asks the user for a folder and makes it the projects folder,
    /// remembering it. **Blocking**, like [`choose_library_dir`].
    ///
    /// [`choose_library_dir`]: StudioHost::choose_library_dir
    fn choose_projects_dir(&mut self) {}

    // --- what Fontelle is set to (TDD §14.3, §18) ---
    /// Every setting the window can change, as a name and the value it is at.
    ///
    /// A [`LibraryEntry`] and not a type of its own, deliberately: a setting
    /// row *is* a name and a value, which is what a browser row already is —
    /// so the settings tab reuses the list, its virtualisation, its rows and
    /// its hit-testing rather than growing a second list widget beside them.
    ///
    /// The host decides what is in here and what each row means. Nothing in
    /// this crate knows that row 0 is a velocity curve, which is what keeps
    /// `fontelle-ui` free of any dependency on the MIDI layer and what makes
    /// the list something more settings can simply be added to.
    fn settings(&self) -> Vec<LibraryEntry> {
        Vec::new()
    }

    /// One line about the settings — where the file is, or what just went
    /// wrong writing it.
    fn settings_status(&self) -> String {
        String::new()
    }

    /// Steps setting `index` to its next value, or its previous one when
    /// `delta` is negative. The size of a step is the host's business: a
    /// choice has a next one and a number has an amount.
    fn nudge_setting(&mut self, index: usize, delta: i32) {
        let _ = (index, delta);
    }

    /// Shows the folder the settings file lives in.
    fn reveal_config_dir(&mut self) {}

    /// Shows the projects folder in the desktop's file manager.
    fn reveal_projects_dir(&mut self) {}

    /// How many soundfonts the whole collection holds.
    ///
    /// The **collection**, not the folder being browsed: it is the panel's
    /// heading, and a heading that counted the rows in front of you would say
    /// "3" inside a folder of three and read as the collection having shrunk.
    fn library_count(&self) -> usize {
        0
    }

    /// One line about where the bank is and what is in it — the thing to say
    /// when it is empty, which on a first run it is.
    fn library_status(&self) -> String;
    fn rescan_library(&mut self);

    /// Shows the bank folder in the desktop's file manager, creating it if it
    /// is not there yet.
    ///
    /// The whole reason to press this is that the folder is empty and you want
    /// to put something in it, so a file manager opening on a folder that does
    /// not exist is not an answer.
    fn reveal_library_dir(&mut self);

    /// Asks the user for a folder and makes it the bank, remembering it.
    ///
    /// `add` keeps the folders already configured and appends this one;
    /// otherwise it replaces them. **Blocking** — the picker is the desktop's
    /// own, and the window is frozen while it is up, which is what every other
    /// application does too.
    fn choose_library_dir(&mut self, add: bool);

    /// The last thing that went wrong, for the status line. Taking it clears
    /// it, so a message shows once rather than forever.
    fn take_message(&mut self) -> Option<String>;

    // --- auditioning (TDD §14.1's live path, driven by the mouse) ---
    /// Sounds `key` now, on the selected channel, outside the timeline. What
    /// makes a note you draw audible the moment you draw it, whether or not
    /// the transport is rolling.
    ///
    /// `pan` is §16.5's per-note pan, in the range `Note::pan` is stored in —
    /// so a note written hard left is auditioned hard left rather than being
    /// centred until the transport reaches it.
    fn audition_on(&mut self, key: u8, velocity: u8, pan: i8);
    fn audition_off(&mut self, key: u8);

    // --- the instrument editor (TDD §7.2) ---
    /// The selected channel's instrument, as a panel full of controls.
    ///
    /// `None` when the channel has no patch on it, because a knob that writes
    /// to nothing is worse than no knob. Rebuilt when
    /// [`revision`](StudioHost::revision) moves, like every other list here.
    fn instrument(&self) -> Option<InstrumentView>;

    /// Moves one control. `value` is normalised, 0..=1, exactly as the panel
    /// draws it; what it means is the implementation's business.
    ///
    /// An address the implementation does not recognise is **ignored**, not an
    /// error: addresses are stable across versions (INVARIANT 7), so an old
    /// project naming a parameter this build has dropped must open rather than
    /// refuse.
    fn set_instrument_param(&mut self, address: &fontelle_types::ParamAddress, value: f32);

    /// Whether the roll draws its key strip as a keyboard or as a list of
    /// names, for the selected channel (TDD §16.4).
    ///
    /// Per channel and saved with the song: a drum kit's keys are a list of
    /// sounds and a piano's are a keyboard, and a project with both wants
    /// both.
    fn key_style(&self) -> crate::canvas::KeyStyle {
        crate::canvas::KeyStyle::default()
    }

    /// Switches it. Undoable like every other edit to the document.
    fn set_key_style(&mut self, _style: crate::canvas::KeyStyle) {}

    /// Which keys the selected channel's instrument can play, and what each
    /// one is called. See [`KeyMap`] — [`KeyMap::unknown`] when the channel
    /// has no instrument, which greys nothing.
    ///
    /// Rebuilt when [`revision`](StudioHost::revision) moves, like every other
    /// list here.
    fn key_map(&self) -> KeyMap {
        KeyMap::unknown()
    }

    /// Which keys a MIDI keyboard is holding down right now, one bit each
    /// (TDD §14.1) — lit on the roll's own keyboard so a phrase played on a
    /// controller can be seen and then written in.
    ///
    /// Read once a frame, like [`mixer_peaks`](StudioHost::mixer_peaks) and
    /// for the same reason: a key goes down between revisions, and bumping the
    /// revision for one would rebuild every list in the window. Zero when
    /// nothing is plugged in, which is what every offline path returns.
    fn live_keys(&self) -> u128 {
        0
    }

    // --- the mixer (TDD §13) ---
    /// Every mixer track, in the order the panel lays them out: the ordinary
    /// tracks first, the master last. The index of a strip in this list is
    /// what every other mixer method below takes.
    fn mixer_strips(&self) -> Vec<MixerStrip>;

    /// The peak each strip has hit since this was last called, per channel.
    /// Same order and length as [`mixer_strips`](StudioHost::mixer_strips).
    ///
    /// Read once a frame while the mixer is showing, rather than on the
    /// revision like every other list here — a meter moves every block, and
    /// bumping a revision for it would rebuild every panel sixty times a
    /// second. Reading takes the peak, the same as the master meter's.
    fn mixer_peaks(&mut self) -> Vec<[f32; 2]>;

    /// Moves a fader. Applied as a command **and** heard immediately: see
    /// `fontelle_engine::TrackControls` for why it has to be both.
    /// Puts an effect on the end of `strip`'s insert chain (TDD §13.4).
    ///
    /// By strip index, the way every other mixer method addresses a track:
    /// the panel knows which column was clicked and nothing else about it
    /// (INVARIANT 2).
    fn add_insert(&mut self, _strip: usize, _kind: fontelle_types::EffectKind) {}

    /// Takes one off.
    fn remove_insert(&mut self, _strip: usize, _slot: usize) {}

    /// Moves one insert's wet/dry mix, 0 (the signal that went in) to 1 (the
    /// effect).
    ///
    /// Every effect has one, which is why it is here rather than in the EQ's
    /// own methods — parallel compression is a compressor mixed under the dry
    /// track, and a bell blended back is how a heavy cut is made gentle.
    fn set_insert_mix(&mut self, _strip: usize, _slot: usize, _mix: f32) {}

    /// Switches one out of the chain, or back in. Not a delete: the settings
    /// stay, because the reason to reach for a bypass is to hear the
    /// difference and then put it back.
    fn toggle_insert_bypass(&mut self, _strip: usize, _slot: usize) {}

    /// Which strip the track-options column is about, by its index in
    /// [`mixer_strips`](StudioHost::mixer_strips).
    ///
    /// The mixer's own selection, not the rack's: several channels may share
    /// one track (§13.1), so "the selected channel" does not name a strip.
    fn selected_mixer_track(&self) -> usize {
        0
    }

    /// Points it at another strip. An index past the end is ignored rather
    /// than clamped — a panel and a document disagree for a frame every time a
    /// track is deleted, and a selection that followed the panel off the end
    /// would be an index nothing else could use.
    fn select_mixer_track(&mut self, _strip: usize) {}

    /// Where `strip`'s output goes, as an index into
    /// [`route_names`](StudioHost::route_names) — `None` is the master, the
    /// same spelling [`ChannelInfo::route`] uses and for the same reason.
    fn track_output(&self, _strip: usize) -> Option<usize> {
        None
    }

    /// Routes it somewhere else (TDD §13.2).
    ///
    /// A routing that would close a feedback loop is **refused**, and the
    /// refusal is reported through [`take_message`](StudioHost::take_message):
    /// §13.2 requires the graph be validated acyclic on every mutation, and a
    /// menu row that silently does nothing is worse than one not offered.
    fn set_track_output(&mut self, _strip: usize, _target: Option<usize>) {}

    /// Sends a copy of `strip`'s signal to another track (TDD §13.2).
    ///
    /// `target` is an index into [`route_names`](StudioHost::route_names). A
    /// send that would close a feedback loop is refused and reported through
    /// [`take_message`](StudioHost::take_message), the same as a routing —
    /// §13.2 counts both edge kinds, because a cycle through a send is the
    /// same loop as one through an output and only harder to see.
    ///
    /// It starts **silent**: a send that arrived wide open would change the
    /// mix the moment it was made, which is the opposite of what making one is
    /// for.
    fn add_send(&mut self, _strip: usize, _target: usize) {}

    fn remove_send(&mut self, _strip: usize, _index: usize) {}

    /// How much of the track goes down the send. Applied as a command **and**
    /// heard immediately, the same as a fader and for the same reason.
    fn set_send_level(&mut self, _strip: usize, _index: usize, _level_db: f32) {}

    /// Takes it before the fader instead of after, or back.
    fn toggle_send_pre_fader(&mut self, _strip: usize, _index: usize) {}

    /// Moves one insert to another place in its chain.
    ///
    /// Order is most of what an insert chain *is* — a compressor before an EQ
    /// and after it are two different sounds — so this is an edit, with an
    /// undo entry, like every other. Indices outside the chain do nothing.
    fn move_insert(&mut self, _strip: usize, _from: usize, _to: usize) {}

    /// The document's own id for a mixer strip, so a panel can build the
    /// address of one of its controls (INVARIANT 7).
    ///
    /// It has to be asked rather than derived: a strip index is the panel's
    /// idea of a track and the address needs the document's.
    fn mixer_track_id(&self, _strip: usize) -> Option<fontelle_types::MixerTrackId> {
        None
    }

    /// Makes an automation clip for `address` and selects it on the
    /// arrangement (TDD §12.4).
    ///
    /// **Flat, at the value the control is at now, over the time selection
    /// — or the whole song when there is none.** *"it creates a new
    /// automation clip in my arrangement just flat on the value that its
    /// currently at basically with the clip extending the current length of
    /// the song or time selection."* A control that already has a clip gets
    /// that clip selected rather than a second one.
    ///
    /// `label` is what the lane says — the panel knows "Master — EQ band 1
    /// gain" and the document knows only the address, so the words come from
    /// the side that has them. `at` is where the playhead is, which the window
    /// knows and the document does not; it is what decides which of several
    /// clips on the same control is meant.
    ///
    /// One gesture from **any** control that has an address, which is what
    /// §8.2's single addressing scheme buys: the mixer's fader, an EQ band,
    /// every knob on a compressor, the tempo box and every parameter of
    /// whatever effect is added next are all automatable by the same code
    /// path, with none of them wired up individually.
    fn create_automation(
        &mut self,
        _address: &fontelle_types::ParamAddress,
        _label: &str,
        _at: Tick,
    ) {
    }

    // --- the time selection and the play mode (TDD §6.3) ---

    /// The loop region, in song ticks — the stretch a right-drag on a ruler
    /// selected. `None` when nothing is selected.
    fn loop_range(&self) -> Option<(Tick, Tick)> {
        None
    }

    /// Sets it, or clears it. One `Command` through the history, and saved
    /// with the song (`Project::loop_range`); the implementation hands the
    /// same range to the transport in samples, so a selection is something
    /// the song loops over the moment it is made.
    fn set_loop_range(&mut self, _range: Option<(Tick, Tick)>) {}

    fn play_mode(&self) -> PlayMode {
        PlayMode::Song
    }

    /// Switches between the song and the clip being edited. In clip mode the
    /// transport loops the clip's own bars and the timeline carries that clip
    /// alone.
    fn set_play_mode(&mut self, _mode: PlayMode) {}

    /// The bars the clip being edited covers, in song ticks — what clip mode
    /// loops over, and where the window puts the marker when it switches.
    fn focused_clip_span(&self) -> Option<(Tick, Tick)> {
        None
    }

    /// Whether `address` already has an automation clip, so a control under
    /// automation can be drawn as such (§12.2's "distinct ring colour").
    fn is_automated(&self, _address: &fontelle_types::ParamAddress) -> bool {
        false
    }

    /// The parameters of one insert, when it is an EQ.
    ///
    /// The document's own `EqConfig`, not a view of it, for the reason
    /// `fontelle_types::effect` gives — the alternative is two definitions of
    /// the same eight bands with somewhere for them to drift.
    /// The panel for the insert in `slot` of `strip`, when it is one the
    /// generic grid of knobs can draw.
    ///
    /// `None` for the **EQ**, which has a curve of its own — two panels for one
    /// effect would be two places to change it — and for a slot nothing is in.
    ///
    /// Built from the effect's own [`specs`](fontelle_types::EffectConfig::specs)
    /// rather than hand-drawn per effect, so an effect added later gets a
    /// window without anybody writing one. The same [`InstrumentView`] the
    /// instrument panel uses, because a grid of knobs is a grid of knobs.
    fn insert_view(&self, _strip: usize, _slot: usize) -> Option<InstrumentView> {
        None
    }

    /// Moves one of them, normalised, through the history.
    fn set_insert_param(&mut self, _strip: usize, _slot: usize, _param: &str, _value: f32) {}

    /// Writes the knobs the `preset`th named starting point stands for, on one
    /// insert — see
    /// [`EffectConfig::presets`](fontelle_types::EffectConfig::presets).
    ///
    /// Its own call rather than a run of
    /// [`set_insert_param`](Self::set_insert_param)s, because it is **one**
    /// thing a person did and has to be one thing to undo.
    fn set_insert_preset(&mut self, _strip: usize, _slot: usize, _preset: usize) {}

    /// Points one insert's detector at another mixer strip — the external
    /// sidechain (`docs/effects-catalogue.md` §2.1). `None` puts it back to
    /// listening to the signal passing through it.
    ///
    /// Refused, and left alone, for an effect with no detector or a key that
    /// would make the routing graph feed itself.
    fn set_insert_key(&mut self, _strip: usize, _slot: usize, _key: Option<usize>) {}

    /// Which strip one insert is keyed from, if any — what a window draws a
    /// tick beside.
    fn insert_key(&self, _strip: usize, _slot: usize) -> Option<usize> {
        None
    }

    /// The spectrum arriving at the insert in `slot` of `strip`, in decibels,
    /// one value per band of [`SPECTRUM_BANDS`](crate::canvas::SPECTRUM_BANDS)
    /// spaced logarithmically across the EQ's own frequency axis.
    ///
    /// *"currently theres no eq monitor graph drawn to view the frequency
    /// spectrum and make edits based off it and see in realtime."*
    ///
    /// Read **once a frame while the EQ's window is open**, like the mixer's
    /// meters and for the same reason: it moves every block, and putting it on
    /// the studio's revision would rebuild every panel in the window sixty
    /// times a second. Empty when there is nothing to show — no such insert, no
    /// running graph, or an offline session — and an empty spectrum draws
    /// nothing rather than a flat line at the floor.
    ///
    /// `&mut self` because reading it advances the analyser, exactly as
    /// [`mixer_peaks`](StudioHost::mixer_peaks) does.
    fn spectrum(&mut self, _strip: usize, _slot: usize) -> Vec<f32> {
        Vec::new()
    }

    fn eq_config(&self, _strip: usize, _slot: usize) -> Option<fontelle_types::EqConfig> {
        None
    }

    /// Writes one band. One `Command` through the history like every other
    /// edit, coalescing while a handle is being dragged — the window calls
    /// [`end_gesture`](DocumentHost::end_gesture) when the mouse comes up.
    fn set_eq_band(
        &mut self,
        _strip: usize,
        _slot: usize,
        _band: usize,
        _value: fontelle_types::EqBand,
    ) {
    }

    fn set_track_gain_db(&mut self, strip: usize, gain_db: f32);

    /// Moves a track's balance control, likewise.
    fn set_track_pan(&mut self, strip: usize, pan: f32);

    /// The same switch the channel rack's mute is, addressed by strip rather
    /// than by channel — several channels may share one track (TDD §13.1), so
    /// these are not one to one.
    fn toggle_track_mute(&mut self, strip: usize);
    fn toggle_track_solo(&mut self, strip: usize);

    /// The notes of other channels, in the open clip's tick space (see
    /// [`GhostNote`]). Empty for [`GhostFilter::Off`].
    fn ghost_notes(&self, filter: GhostFilter) -> Vec<GhostNote>;

    // --- the arrangement ---
    /// The lanes, in the order the arrangement stacks them.
    fn lanes(&self) -> Vec<LaneInfo>;
    /// Every clip in the project, flattened for the canvas.
    fn clips(&self) -> Vec<ClipInfo>;
    /// Applies one arrangement edit as a command, recording it in the history.
    ///
    /// Returns what it **created** — nothing for every edit that creates
    /// nothing. The arrangement needs the ids for the same reason the roll
    /// needs `DocumentHost::edit`'s: a duplicate's offset is measured from the
    /// selection, so the copy has to become the selection or pressing the key
    /// twice puts two clips in one place; and a point made by a press is the
    /// point the drag that follows has to move. See `Timeline::clips_inserted`
    /// and `Timeline::points_inserted`.
    fn arrange(&mut self, edit: ArrangeEdit) -> Created;

    /// How many clips `Ctrl+C`/`Ctrl+X` are holding — what the arrangement's
    /// Paste button asks so it can say whether it would do anything.
    ///
    /// The clips live here rather than in the canvas because a canvas may not
    /// see one (INVARIANT 2); see [`ArrangeEdit::Copy`].
    fn clip_clipboard_len(&self) -> usize {
        0
    }
    /// Opens `clip` in the piano roll, selecting the channel that owns it.
    fn open_clip(&mut self, clip: ClipId);
    /// How long the piece is, in ticks — what the arrangement's ruler spans.
    fn song_length(&self) -> Tick;
    /// Where `position_sample` is in the *song*, as against
    /// [`DocumentHost::playhead_tick`], which is inside the open clip.
    fn playhead_song_tick(&self, position_sample: Sample) -> Tick;
    /// The other direction, for the arrangement's ruler.
    fn sample_of_song_tick(&self, tick: Tick) -> Sample;
    fn toggle_lane_mute(&mut self, lane: usize);

    /// Moves one row up (`-1`) or down (`1`) the stack.
    ///
    /// A move off either end does nothing rather than failing — the menu greys
    /// those entries, and a command that errored would make the gesture
    /// something the window has to handle rather than something it can just
    /// ask for.
    fn move_lane(&mut self, _lane: usize, _delta: isize) {}

    // --- recording (TDD §14.7, item 9 of the plan) ---
    /// Turns whatever has been played since recording started into notes on
    /// the open clip, and returns how many.
    ///
    /// `end_sample` is where the transport stopped, so a note still held when
    /// the button was pressed is closed there rather than left open for ever.
    ///
    /// **Zero is an ordinary answer**, not a failure: stopping a recording
    /// nobody played into is the commonest thing that happens to a record
    /// button, and it must not leave an empty clip behind.
    fn keep_take(&mut self, end_sample: Sample) -> usize {
        let _ = end_sample;
        0
    }

    /// Throws away whatever has been captured. What arming does, so a take
    /// does not begin with the last one still in the buffer.
    fn discard_take(&mut self) {}

    /// Called once per pass of the event loop, for whatever the host has to do
    /// off the audio thread — chiefly freeing the graph the RT side handed
    /// back (see `fontelle_engine::GraphPublisher::reclaim`).
    fn pump(&mut self);
}
