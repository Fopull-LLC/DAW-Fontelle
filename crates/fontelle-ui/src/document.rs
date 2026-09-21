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

/// Where the start menu's update check has got to.
///
/// The window draws one line from this and one button; the check itself —
/// the network, the archive, the swap of the binary — is `fontelle-app`'s
/// (`updates.rs`), because none of it is a picture. The window only ever
/// asks, through [`StudioHost::update_status`], and only ever presses,
/// through [`StudioHost::upgrade`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Nobody has asked yet.
    Unchecked,
    /// The user switched the check off in the settings. Said out loud rather
    /// than drawn as nothing, so a menu with no update line is not mistaken
    /// for one that is up to date.
    Off,
    /// The request is in flight.
    Checking,
    /// This build is the newest release.
    UpToDate,
    /// A newer release exists; the button offers it.
    Available { version: String },
    /// The archive is on its way down and being verified. `done` is how
    /// many bytes have arrived and `total` how many there are, when the
    /// server said — what the start menu's bar is drawn from.
    Downloading {
        version: String,
        done: u64,
        total: Option<u64>,
    },
    /// The new binary is in place. It runs on the next launch.
    Installed { version: String },
    /// Why the check or the install did not happen — no network, a checksum
    /// that did not match, a folder that cannot be written.
    Failed(String),
}

/// One row of the start menu's *Recent* list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProject {
    /// The project's name — the bundle folder's stem.
    pub name: String,
    /// Where it lives, drawn muted under the name.
    pub path: std::path::PathBuf,
    /// Whether the bundle is still there. A project that was moved or
    /// deleted stays in the list, drawn dead, so it can be forgotten from
    /// the menu rather than silently vanishing from it.
    pub exists: bool,
}

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

    /// The tempo **actually in force** at `position_sample`, in beats per
    /// minute — the box value bent by the tempo lane (§12.3).
    ///
    /// > *"the tempo indicator at the top is not reacting to tempo automation
    /// > changes."*
    ///
    /// Two numbers rather than one because they answer different questions and
    /// only one of them is editable: [`tempo`](Self::tempo) is what the
    /// document *says*, which is what a drag on the box writes, and this is
    /// what the song is *doing*, which is what the box should read. On a
    /// project with no tempo lane they are the same number and nothing looks
    /// any different.
    ///
    /// Defaults to the box value, so a host that has no tempo map still shows
    /// something true.
    fn tempo_at(&self, _position_sample: fontelle_types::Sample) -> f64 {
        self.tempo()
    }

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

    /// How long the clip being edited is, in its own ticks.
    ///
    /// The roll shades the grid past it, because a note written beyond a
    /// clip's end does not sound (TDD §11.4) and a clip does not grow to
    /// swallow one. `None` for a host with no clip — every test fake — which
    /// shades nothing.
    fn clip_length(&self) -> Option<Tick> {
        None
    }

    /// Whether there are changes not yet on disk.
    fn is_dirty(&self) -> bool;

    /// Whether the document has a file yet.
    ///
    /// A studio started with no arguments has a project and nowhere to put it.
    /// The window asks this before saving, because the answer to Ctrl+S there
    /// is *"what shall I call it"* rather than an error — see
    /// [`save_as`](Self::save_as).
    ///
    /// `true` by default, so a host that has no concept of a file (a test, a
    /// plugin build) is never asked to name one.
    fn has_file(&self) -> bool {
        true
    }

    /// Writes the project out. `Err` carries something worth showing a person.
    fn save(&mut self) -> Result<(), String>;

    /// Saves what is open into a **new** project of that name.
    ///
    /// > *"if i try to save and theirs no project directory it can just make a
    /// > new one ... giving you the option to name it and stuff."*
    ///
    /// A name, not a path: where it lands is the host's business, and with no
    /// projects folder configured the answer is an `Err` a person can act on
    /// rather than a guess (INVARIANT 10).
    fn save_as(&mut self, name: &str) -> Result<(), String> {
        let _ = name;
        Err("this build cannot save".to_string())
    }

    /// Where it would be saved, for the title bar.
    fn name(&self) -> &str;
}

/// One plugin the machine has, as a menu row (TDD §8.4).
///
/// Names only. Which plugin a row *is* stays behind
/// [`StudioHost::plugin_instruments`]'s ordering, for the reason INVARIANT 2
/// gives: the window points at a row, and the half that owns the document
/// resolves what that means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginListing {
    pub name: String,
    /// Who wrote it, for the row's second line. Empty when it did not say.
    pub vendor: String,
    /// Which plugin, permanently — what a star on its row is kept by (see
    /// [`fontelle_types::Favorite`]). The window still *chooses* by position;
    /// this is so it can say which rows are starred, which a name cannot
    /// (two vendors can ship a "Reverb").
    pub key: fontelle_types::PluginKey,
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
    /// A take or a loop: audio, played from a file (TDD §15).
    Audio,
}

/// What an audio clip's block shows.
///
/// Reported from using the window: *"i should be able to see the waveform of
/// the audio inside the clip."* The same idea the note preview answers, pointed
/// at the other kind of clip.
///
/// **A summary, not the samples.** A four-bar take is four hundred thousand
/// frames and the block is a few hundred pixels wide, so what reaches the
/// canvas is the loudest and quietest sample in each bucket —
/// `fontelle_assets::generate_peaks`, resampled onto the clip's own trimmed
/// range once per change to what the clip *shows* rather than per frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioPreview {
    /// `(min, max)` per bucket, evenly covering the clip's own range **in play
    /// order** — so a reversed clip's picture is reversed too, because what you
    /// see has to be what you hear.
    ///
    /// **Fine, and shared.** A bucket is a few milliseconds of the take
    /// whatever its length — *"it looks like i start talking sooner than i
    /// actually do audibly"* was a fixed five hundred buckets across a long
    /// take, each drawn loud from its own start. That is far more buckets
    /// than a block has columns, so the canvas folds them (`clip_waveform`),
    /// and it is behind an `Arc` so the host hands the same picture out
    /// again on every revision that does not change it — a clip being
    /// dragged is a revision per pointer move.
    pub peaks: std::sync::Arc<[(f32, f32)]>,
    /// The RMS of each bucket of `peaks`, bucket for bucket — the take's
    /// loudness, drawn as a solid core inside the outline the extremes
    /// make. Empty when the host has no loudness for it yet, and the block
    /// then draws the outline alone.
    pub rms: std::sync::Arc<[f32]>,
    /// The fades, as a fraction of the clip's length (TDD §15.2). Drawn into
    /// the waveform rather than beside it, for the same reason: the picture is
    /// the envelope.
    pub fade_in: f32,
    pub fade_out: f32,
    /// How each fade's curve is bent, −1..1 — `fontelle_types::Fade::tension`,
    /// carried so the block draws the bend the node asked for.
    pub fade_in_tension: f32,
    pub fade_out_tension: f32,
    /// How long the clip's range of the file is, in seconds at the file's
    /// own rate — so a fade can be read out as a time while its handle is
    /// dragged (`canvas::fade_caption`). Zero when the rate is not known.
    pub seconds: f32,
    /// How many ticks of the song the file takes **at its own rate**, from
    /// the start of each pass — the length a drop gives the block, and the
    /// length the block keeps drawing the file at after it has been dragged
    /// longer or shorter with the Stretch switch off.
    ///
    /// In ticks, through the tempo map, because that is what the block is
    /// measured in; a tempo change moves it, the way it moves the sound. Zero
    /// is *not known* (a clip whose rate is not on it yet), and the canvas
    /// then fills the block — §15.3's "draw what exists", never a blank.
    pub natural_length: Tick,
    /// Whether the clip follows its block (`ClipStretch::Resample`) rather
    /// than its file. Carried so the canvas can draw the file filling each
    /// pass, and so an edge drag knows whether the switch it is under would
    /// change anything.
    pub stretched: bool,
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
    /// For [`ClipKind::Audio`], the waveform to draw inside it. Empty peaks for
    /// every other kind — and for an audio clip whose file has not been decoded
    /// yet, which §15.3 says to draw as what exists rather than as a slab.
    pub audio: AudioPreview,
    /// The **prefab** this block is a place for, by name — `None` for an
    /// ordinary clip (TDD §10.5).
    ///
    /// A place has to be drawn as one, because the difference between a copy
    /// and a place is invisible until you edit one and four other blocks
    /// change. The name rather than the id, for the reason the rest of
    /// `ClipInfo` carries names: the canvas may not see a `Project`
    /// (INVARIANT 2), and what it draws is a caption.
    pub prefab: Option<String>,
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

/// A question a file has raised, and the ways of answering it.
///
/// Strings rather than anything structured, for the reason
/// [`crate::canvas::ContextMenu`] lists strings: what each line *means* is the
/// host's to remember, and the window's job is to draw them and say which one
/// was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPrompt {
    /// What is being imported — drawn greyed at the top, so a menu of six
    /// lines is not one you have to remember what you clicked to read.
    pub title: String,
    pub choices: Vec<String>,
}

/// A sound in the air, for [`StudioHost::sound_footprint`]: a file from the
/// desktop, or a row of the Import tab, which the host turns into a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarriedSound<'a> {
    File(&'a std::path::Path),
    ImportRow(usize),
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

/// One route reaching a control, as the window needs to know it
/// (`docs/flopsynth-plan.md` §8.4).
///
/// The **depth's own address** is carried rather than a route index, so a ring
/// drag is an ordinary parameter write down the ordinary live wire: a
/// modulation depth is a knob like any other, and the one that is drawn round
/// the destination is the same control the matrix row shows.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteInfo {
    /// What the source is called — "ENV 2", "LFO 1", a macro's name.
    pub source: String,
    /// Bipolar, -1..=1.
    pub depth: f32,
    pub depth_address: fontelle_types::ParamAddress,
}

/// What one of the Flopsynth window's controls wears: a glow because a
/// source badge could land on it, and a ring at `depth` when a route already
/// reaches it. See [`StudioHost::modulation_marks`].
#[derive(Clone, Debug, PartialEq)]
pub struct ModMark {
    pub address: fontelle_types::ParamAddress,
    /// The newest route's depth, bipolar -1..=1 — `None` for a control
    /// nothing modulates yet. The last of `rings`, kept for the callers
    /// that want only that.
    pub depth: Option<f32>,
    /// Every route to the control, oldest first: one ring each
    /// (`docs/flopsynth-next.md` §3.3), in its source's colour.
    pub rings: Vec<ModRing>,
}

/// One route's ring: its source's family (the colour), its depth (the
/// arc), and which source it is (the live dot's value).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModRing {
    pub family: SourceFamily,
    /// Bipolar -1..=1.
    pub depth: f32,
    /// The source, as an index into [`StudioHost::mod_sources`].
    pub source: usize,
}

/// Which column the Matrix page's table is sorted by when its head is
/// pressed (`docs/flopsynth-next.md` §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteSort {
    Source,
    Destination,
}

/// What kind of thing a modulation source is — the five inks the rings
/// wear (§3.3): envelopes violet, LFOs teal, macros amber, the note's own
/// values green, the performance's rose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFamily {
    Envelope,
    Lfo,
    Macro,
    /// Velocity, key, the note's X and Y, the random draw, the counter.
    Note,
    /// The wheel, the bend, aftertouch.
    Performance,
}

impl SourceFamily {
    /// Whether the family's sources swing both ways: an LFO and the bend
    /// do; an envelope, a macro, a velocity push one way.
    pub fn bipolar(self) -> bool {
        matches!(self, Self::Lfo | Self::Performance)
    }
}

/// Which list the left-hand panel is showing.
///
/// > *"the prefab tab should be where the channel rack is can be tabbed
/// > between instruments and prefabs."*
///
/// Two lists in one panel rather than two panels, because they are answers to
/// the same question — *what have I got to work with* — and because the
/// sidebar has room for one list at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RackTab {
    /// The channel rack: what is in the project and what it plays.
    #[default]
    Instruments,
    /// The prefabs: content you can draw in more than one place.
    Prefabs,
}

impl RackTab {
    pub const ALL: [Self; 2] = [Self::Instruments, Self::Prefabs];

    /// The tab this one is not. What **F** shows: *"make it so the f key
    /// toggles the tab between instrument and prefab."*
    pub fn other(self) -> Self {
        match self {
            Self::Instruments => Self::Prefabs,
            Self::Prefabs => Self::Instruments,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Instruments => "Instruments",
            Self::Prefabs => "Prefabs",
        }
    }
}

/// One prefab, as its list draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefabInfo {
    pub name: String,
    /// How many places on the arrangement follow it.
    ///
    /// What makes "delete this" a decision somebody can make: a prefab used
    /// nowhere is a scratch idea, and one used eleven times is the song.
    pub uses: usize,
    /// Whether this is the one the roll is editing.
    pub open: bool,
}

/// Everything the window shows that is not the clip.
/// What an export is asked to be (§15's bounce, with the questions FL's
/// export dialog asks first).
///
/// > *"when i click export it prompts me with the export options so i can
/// > chose things like time selection, whole song, keep things like reverb
/// > tail or cut short, etc."*
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    pub range: ExportRange,
    pub tail: ExportTail,
}

/// Which stretch of the song an export renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportRange {
    /// From the start to the end of the last thing that sounds.
    WholeSong,
    /// The time selection on the ruler — the loop range. An export asked
    /// for this with no selection is refused with a line saying so.
    Selection,
}

/// What happens to what is still ringing when the stretch ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportTail {
    /// Render on past the end until the sound has died away — a reverb, a
    /// delay, a long release — and keep it. The file ends where the sound
    /// does.
    Keep,
    /// Stop exactly at the end of the stretch, whatever is still ringing.
    Cut,
}

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

    // --- prefabs (TDD §10.5) ---

    /// Which of the panel's two lists is showing.
    fn rack_tab(&self) -> RackTab {
        RackTab::Instruments
    }
    fn set_rack_tab(&mut self, _tab: RackTab) {}

    /// The prefabs, in the order the list draws them.
    fn prefabs(&self) -> Vec<PrefabInfo> {
        Vec::new()
    }
    /// Makes an empty one and selects it — the plus icon.
    fn add_prefab(&mut self) {}
    fn rename_prefab(&mut self, _index: usize, _name: &str) {}
    /// Deletes it, **baking what it held into every place that followed it**,
    /// so the arrangement goes on sounding the same.
    fn remove_prefab(&mut self, _index: usize) {}

    /// Which prefab the piano roll is editing, if it is editing one rather
    /// than a clip on the arrangement.
    ///
    /// > *"you could also edit it just by selecting the prefab in the prefab
    /// > menu and then selecting the instrument you want to edit in the prefab
    /// > clip and then just editing the piano roll of it."*
    fn selected_prefab(&self) -> Option<usize> {
        None
    }
    fn select_prefab(&mut self, _index: Option<usize>) {}

    /// Puts a **place** for prefab `index` on row `lane` at `start`, and hands
    /// back the clip it made.
    ///
    /// *"clips that you can basically draw into your arrangement."*
    fn draw_prefab(&mut self, _index: usize, _lane: usize, _start: Tick) -> Option<ClipId> {
        None
    }
    /// Takes `clip` off its prefab, keeping what it was playing.
    fn detach_prefab(&mut self, _clip: ClipId) {}
    /// Turns a clip that has been written into a prefab, in place: the clip
    /// becomes the first place for it, and its content becomes the prefab's.
    fn make_prefab_from(&mut self, _clip: ClipId) -> Result<(), String> {
        Err("this studio has no prefabs".to_string())
    }

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
    /// A new channel that **is** one of the three instruments — see
    /// [`fontelle_types::InstrumentKind`].
    ///
    /// *"when you select new instrument it lets you select one of those and
    /// then you actually edit it from there."* Defaults to
    /// [`add_channel`](Self::add_channel) so a host that has not grown the
    /// choice yet still makes one.
    fn add_channel_of(&mut self, _kind: fontelle_types::InstrumentKind) -> Result<(), String> {
        self.add_channel()
    }
    /// Turns channel `index` into an instrument of `kind`, replacing whatever
    /// it was playing with that kind's starter instrument.
    ///
    /// *"cant replace an instrument with a different instrument."* Setting the
    /// kind it already is does nothing at all, since choosing "3OSC" on a 3OSC
    /// you have spent ten minutes editing must not throw the edit away.
    fn set_channel_kind(&mut self, _index: usize, _kind: fontelle_types::InstrumentKind) {}
    /// Which of the three channel `index` is, for the rack and the menus.
    fn channel_kind(&self, _index: usize) -> Option<fontelle_types::InstrumentKind> {
        None
    }
    /// Puts preset `index` of the open file onto a **new** channel.
    fn add_channel_with(&mut self, preset: usize) -> Result<(), String>;
    /// Makes a **sampler** on a new channel out of row `index` of the Import
    /// tab's list.
    ///
    /// *"i cannot drag an audio clip from the audio import tab into the
    /// channel rack to turn it into a sampler, please add this feature."*
    fn add_sampler_from_import(&mut self, _index: usize) -> Result<(), String> {
        Err("this studio cannot make samplers".to_string())
    }
    /// The same file, onto the channel at rack position `channel`, replacing
    /// whatever it was playing.
    ///
    /// *"i want to be able to click and drag them into the sampler or into the
    /// channel rack to make it have a sampler with that clip sampled."* The
    /// rack's own rule: empty space makes a new channel, a row changes that
    /// one.
    fn set_sampler_from_import(&mut self, _channel: usize, _index: usize) -> Result<(), String> {
        Err("this studio cannot make samplers".to_string())
    }
    /// Puts it on the channel the rack has selected instead.
    fn set_channel_instrument(&mut self, preset: usize) -> Result<(), String>;
    /// Puts it on the channel at rack position `channel` — which is not always
    /// the selected one, because a preset dropped on a row means *that* row.
    ///
    /// Defaults to [`set_channel_instrument`](Self::set_channel_instrument), so
    /// a host that has not grown the distinction still assigns something.
    fn set_channel_instrument_on(&mut self, _channel: usize, preset: usize) -> Result<(), String> {
        self.set_channel_instrument(preset)
    }
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

    /// Adds a lane at the bottom of the stack.
    fn add_lane(&mut self) {}
    /// Adds one **at** `index`, pushing that row and everything under it down.
    ///
    /// *"when i right click a lane in the arrangement i want the option to add
    /// a lane above or a lane below the lane i right clicked on right now
    /// theyre all going to the end."* Above and below are this with the index
    /// differing by one; past the end is the end.
    ///
    /// Defaults to [`add_lane`](Self::add_lane) so a host that has not grown
    /// one yet still puts a row somewhere rather than nowhere.
    fn add_lane_at(&mut self, _index: usize) {
        self.add_lane();
    }
    /// Bounces row `index` to audio and puts the take on a new row under it,
    /// named `<name> (rendered)`.
    ///
    /// `span` is the stretch to render; `None` is the whole row. The window
    /// asks which when there is a time selection to ask about — guessing is
    /// the one thing it must not do, since a render of the wrong range costs
    /// minutes.
    fn render_lane(
        &mut self,
        _index: usize,
        _span: Option<(Tick, Tick)>,
    ) -> Result<String, String> {
        Err("this studio cannot render".to_string())
    }
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
        self.new_project_named("Untitled")
    }

    /// The same, under a name somebody typed.
    ///
    /// > *"when i make a new project i need to be prompted to name it"*
    fn new_project_named(&mut self, name: &str) -> Result<(), String> {
        let _ = name;
        Err("this build cannot make projects".to_string())
    }

    /// Opens project `index` of [`projects`](StudioHost::projects), replacing
    /// what is open now.
    fn open_project(&mut self, index: usize) -> Result<(), String> {
        let _ = index;
        Err("this build cannot open projects".to_string())
    }

    // --- the start menu ---

    /// The projects this machine was last in, newest first.
    fn recent_projects(&self) -> Vec<RecentProject> {
        Vec::new()
    }
    /// Opens the bundle at `path` — a *Recent* row, or what *Open…* chose.
    fn open_project_path(&mut self, _path: &std::path::Path) -> Result<(), String> {
        Err("no projects here".to_string())
    }
    /// Takes the `index`th recent project off the list. A row drawn dead
    /// because its bundle has gone is forgotten this way, on purpose.
    fn forget_recent(&mut self, _index: usize) {}
    /// Whether a projects folder has been chosen — what the start menu asks
    /// before it asks for a name, so that *New project* on a fresh machine
    /// picks the folder first rather than failing after the name is typed.
    fn has_projects_dir(&self) -> bool {
        false
    }
    /// Asks the desktop for a bundle folder and opens it. `Ok(false)` is a
    /// cancelled dialog, which is not an error and not a message.
    fn choose_and_open_project(&mut self) -> Result<bool, String> {
        Ok(false)
    }
    /// Where the update check has got to — read once a frame while the menu
    /// is up, so the line and the button follow the thread doing the work.
    fn update_status(&self) -> UpdateStatus {
        UpdateStatus::Unchecked
    }
    /// Starts the check. Asked once, when the menu opens.
    fn check_for_updates(&mut self) {}
    /// Downloads and installs the release the check found.
    fn upgrade(&mut self) {}
    /// The release's own page, for when the install cannot be done here.
    fn open_release_page(&mut self) {}
    /// A web address, in the desktop's browser — the footer's links.
    fn open_url(&mut self, _url: &str) {}

    /// Bounces the whole project to a WAV inside its own `renders/` folder.
    /// `Ok` carries a line worth showing; `Err` does too.
    fn export_wav(&mut self) -> Result<String, String> {
        Err("this build cannot export".to_string())
    }

    /// [`export_wav`](Self::export_wav) with a say in what: which stretch
    /// of the song, and whether what rings past its end is kept. The
    /// window asks these of the person first — see `MenuTarget::Export`.
    fn export_wav_with(&mut self, _options: ExportOptions) -> Result<String, String> {
        Err("this build cannot export".to_string())
    }

    /// Saves the song out as a Standard MIDI File at a place the user picks.
    /// The inverse of importing one. `Ok` and `Err` both carry a status line.
    fn export_midi(&mut self) -> Result<String, String> {
        Err("this build cannot export MIDI".to_string())
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

    /// What kind of control each settings row is, parallel to [`settings`], so
    /// the panel draws a real control — a slider, a switch, a drop-down —
    /// rather than a value you click to step (the report this answers).
    ///
    /// A [`SettingControl`](crate::canvas::SettingControl) per row and not
    /// anything the window has to interpret: the host says "this one is a
    /// slider at 0.4", and which row means what stays entirely the host's, the
    /// same boundary [`settings`] itself keeps.
    fn setting_controls(&self) -> Vec<crate::canvas::SettingControl> {
        Vec::new()
    }

    /// Sets slider row `index` to `fraction` of its groove (0..=1), as a drag
    /// does. The host maps the fraction back to the row's own units.
    fn set_setting_fraction(&mut self, index: usize, fraction: f32) {
        let _ = (index, fraction);
    }

    /// Sets drop-down row `index` to its `option`th entry, as choosing from its
    /// menu does.
    fn choose_setting(&mut self, index: usize, option: usize) {
        let _ = (index, option);
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

    /// The question to ask before pressing setting `index`, for a row whose
    /// action cannot be cheaply undone — uninstalling an extension, say, which
    /// takes a download to get back. `None` for everything else: a reversible
    /// action does not stop to ask, it acts and offers an undo instead (see
    /// [`take_settings_toast`](Self::take_settings_toast)).
    fn settings_confirm(&self, _index: usize) -> Option<String> {
        None
    }

    /// A one-off note the last settings press left for the user, and whether it
    /// can be undone. The window shows it as a transient banner — "Removed X —
    /// Undo" — rather than as a silent change. Taken, so it is shown once.
    fn take_settings_toast(&mut self) -> Option<(String, bool)> {
        None
    }

    /// Reverses the last undoable settings action (the one a toast offered to
    /// undo), returning what to say about it. `None` if there is nothing to
    /// undo.
    fn undo_settings(&mut self) -> Option<String> {
        None
    }

    /// Shows the folder the settings file lives in.
    fn reveal_config_dir(&mut self) {}

    // ------------------------------------------------------ the keymap ---

    /// The shortcuts that differ from the defaults, as the settings file
    /// keeps them: `(action id, chords)` pairs from
    /// [`Keymap::overrides`](crate::canvas::Keymap::overrides). The window
    /// owns the map; the host only remembers it.
    fn keymap_overrides(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Remembers `overrides` for the next launch. Written at once — a
    /// shortcut changed and lost on quitting is worse than one that could
    /// not be changed.
    fn set_keymap_overrides(&mut self, overrides: Vec<(String, String)>) {
        let _ = overrides;
    }

    // ------------------------------------------------- files to import ---

    /// Tells the host which of the browser's lists is on screen.
    ///
    /// The search box filters *whichever list is showing*, so the host has to
    /// know which one that is: a query typed against MIDI files means nothing
    /// against soundfonts, and a single shared box would carry one into the
    /// other every time the tab changed.
    fn set_browser_mode(&mut self, _mode: crate::canvas::BrowserMode) {}

    /// Which kind of file the Import tab is showing.
    fn import_kind(&self) -> fontelle_types::FolderKind {
        fontelle_types::FolderKind::Midi
    }

    fn set_import_kind(&mut self, _kind: fontelle_types::FolderKind) {}

    /// Whether a folder has been chosen for `kind` at all.
    ///
    /// The whole of *"if I don't have a folder selected yet, take me to the
    /// settings menu"*: the window asks this before it opens the tab, and
    /// sends you to set one when the answer is no. Fontelle reads nothing the
    /// user has not named (INVARIANT 10), so there is no folder to fall back
    /// on and offering an empty list would be a browser that looks broken.
    fn has_import_dir(&self, _kind: fontelle_types::FolderKind) -> bool {
        false
    }

    /// The rows of the Import tab: folders, then files, in the folder the
    /// browser is standing in — or the search's hits across the whole
    /// collection when there is a query.
    fn import_files(&self) -> Vec<LibraryEntry> {
        Vec::new()
    }

    /// One line saying where the folder is, or what just went wrong.
    fn import_status(&self) -> String {
        String::new()
    }

    /// Activates row `index`: walks into a folder, or imports a file.
    fn open_import(&mut self, _index: usize) -> Result<(), String> {
        Err("importing files is not available".to_string())
    }

    /// Brings row `index` of the Import tab in as a **clip**, starting at song
    /// sample `at` — what a row let go over the arrangement means.
    ///
    /// > *"im trying to drag into the channel rack or playlist to turn into an
    /// > instrument or clip."*
    ///
    /// [`open_import`](Self::open_import) is the same file with no position:
    /// it lands at the top of the song (or the time selection), which is what
    /// a *click* on the row means. A drag names a bar, and a drop that ignored
    /// it would put the clip somewhere nobody was looking — which is exactly
    /// what a mark drawn at the pointer must not promise.
    ///
    /// Defaults to the click, so a host that has not grown the distinction
    /// still imports what it was handed.
    fn drop_import_at(
        &mut self,
        index: usize,
        _at: fontelle_types::Sample,
        _lane: Option<usize>,
    ) -> Result<(), String> {
        self.open_import(index)
    }

    /// Picks the folder for the kind the tab is showing.
    fn choose_import_dir(&mut self) {}

    /// Shows it in the desktop's file manager.
    fn reveal_import_dir(&mut self) {}

    /// The question a file has raised, if one is waiting.
    ///
    /// A `.mid` holding several parts is the case this exists for: *"if I
    /// import a midi with multiple instruments inside it, prompt me if I want
    /// to import them all as separate named tracks or only import a single
    /// instrument"*. The **host** decides what the choices are, because it is
    /// the half that read the file; the window draws them as a menu.
    fn import_prompt(&self) -> Option<ImportPrompt> {
        None
    }

    /// Answers it, by the index of the chosen line.
    fn answer_import(&mut self, _choice: usize) {}

    /// Drops the question without importing anything.
    fn cancel_import(&mut self) {}

    /// Imports the file at `path`, whatever kind it turns out to be —
    /// what a file **dropped on the window** goes through.
    ///
    /// `Ok` carries what to say about it; `Err` says why not.
    fn drop_file(&mut self, _path: &std::path::Path) -> Result<String, String> {
        Err("this build cannot open dropped files".to_string())
    }

    /// The same, landing at `at` on the song rather than at the top of it.
    ///
    /// > *"please make sure its possible to import files into the arrangement
    /// > as clips."*
    ///
    /// For the kinds of file that *have* a position — an audio clip is a
    /// stretch of song and a drop names where it goes. The kinds that do not
    /// (a soundfont is an instrument, a score is a phrase for the open clip)
    /// ignore it, which is why this is one method and not a second import
    /// path: what a file is decides what a position means to it.
    ///
    /// Defaults to [`drop_file`](Self::drop_file), so a host that has not
    /// implemented it still opens what it is handed.
    fn drop_file_at(
        &mut self,
        path: &std::path::Path,
        _at: fontelle_types::Sample,
    ) -> Result<String, String> {
        self.drop_file(path)
    }

    /// The same, landing on **row `row`** of the arrangement: the one the
    /// pointer was over, which is made when it is past the last (a drop into
    /// the empty space under the stack). `None` is a drop that named no row
    /// — over the browser, say — and lands where
    /// [`set_arrival_row`](Self::set_arrival_row) points.
    ///
    /// > *"i wish instead if i was dragging it in, it showed me a preview
    /// > where im dragging it and let me drag it exactly where i wanted on
    /// > any lane instead of making a new one automatically for me and
    /// > putting it there on the bottom."*
    ///
    /// Only a sound has a row; every other kind of file ignores it, for the
    /// reason [`drop_file_at`](Self::drop_file_at) gives about the position.
    fn drop_file_on(
        &mut self,
        path: &std::path::Path,
        at: fontelle_types::Sample,
        _row: Option<usize>,
    ) -> Result<String, String> {
        self.drop_file_at(path, at)
    }

    /// A sound from the desktop let go on channel `channel` of the rack: that
    /// channel becomes a sampler playing it — what a row out of the Import
    /// tab does there ([`set_sampler_from_import`](Self::set_sampler_from_import)).
    fn drop_file_on_channel(
        &mut self,
        _channel: usize,
        _path: &std::path::Path,
    ) -> Result<(), String> {
        Err("this build cannot open dropped files".to_string())
    }

    /// A sound from the desktop let go on the rack itself: a sampler channel
    /// of its own.
    fn drop_file_as_channel(&mut self, _path: &std::path::Path) -> Result<(), String> {
        Err("this build cannot open dropped files".to_string())
    }

    /// How long `sound` would be on the arrangement, in ticks, starting at
    /// song tick `from` — the width of the block the window draws under a
    /// dragged sound, before it is dropped.
    ///
    /// > *"the preview for dragging in things into the arrangement was
    /// > showing it in the correct vertical lane, but was not positioning the
    /// > clip horizontally correctly in the preview ... it showed the
    /// > preview just taking up the entire lane."*
    ///
    /// The same arithmetic the import does, through the same tempo map, so
    /// the block drawn is the block that lands. `None` for a file that is not
    /// a sound, and the window falls back to lighting the row.
    fn sound_footprint(&mut self, _sound: CarriedSound<'_>, _from: Tick) -> Option<Tick> {
        None
    }

    /// Where a sound, a take or a file's parts go when they arrive with **no
    /// row of their own**: a new row at this index in the stack, pushing what
    /// is there down.
    ///
    /// > *"if i wasnt dragging however and imported some other way it should
    /// > go on a new lane added in between the lane in the middlemost of your
    /// > arrangement screen that way its cleanly visible for you."*
    ///
    /// The window keeps it current — it is [`crate::canvas::arrival_row`], the
    /// middle of the rows on screen — and the host reads it when a recording
    /// stops, when a row of the Import tab is double-clicked, when a `.mid` is
    /// answered. Past the stack is the foot.
    fn set_arrival_row(&mut self, _row: usize) {}

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
    /// Loads preset `index` into the **preview voice** and aims the live path
    /// at it, so the next [`audition_on`](Self::audition_on) is that
    /// instrument rather than the selected channel's.
    ///
    /// *"if i click a soundfont in the soundfont menu it plays that instrument
    /// at a c tone ... so i can easily click on instruments and hear how they
    /// sound."* Nothing is written to the document: hearing an instrument is
    /// not choosing one, and the old answer to "what does this sound like" was
    /// to put it on a channel, which is what was reported as weird.
    fn preview_preset(&mut self, _preset: usize) -> Result<(), String> {
        Err("this studio cannot preview".to_string())
    }

    /// Loads the Import tab's file at `index` into the preview voice and hands
    /// back how long it is in seconds, so a click on it *plays the file* the
    /// way a click on a soundfont plays the instrument — a listen, not an
    /// import. `Ok(seconds)` is how long to hold the note for the whole file.
    fn preview_import(&mut self, _index: usize) -> Result<f64, String> {
        Err("this studio cannot preview".to_string())
    }
    /// Aims the live path back at the selected channel.
    fn end_preview(&mut self) {}
    fn audition_off(&mut self, key: u8);

    // --- the instrument editor (TDD §7.2) ---
    /// The selected channel's instrument, as a panel full of controls.
    ///
    /// `None` when the channel has no patch on it, because a knob that writes
    /// to nothing is worse than no knob. Rebuilt when
    /// [`revision`](StudioHost::revision) moves, like every other list here.
    fn instrument(&self) -> Option<InstrumentView>;

    /// The selected channel's instrument as **Flopsynth's own window**, when
    /// that is what it is (`docs/flopsynth-plan.md` §8).
    ///
    /// `None` for every other instrument, and then
    /// [`instrument`](StudioHost::instrument)'s knob grid is drawn instead —
    /// which is what makes this a window a host grows into rather than one
    /// every host has to have. A synthesiser's window is a *picture of a
    /// signal path*, and the grid draws a list; both are right for what they
    /// are for.
    ///
    /// Rebuilt when [`revision`](StudioHost::revision) moves, like every other
    /// list here.
    fn flopsynth(
        &self,
        _page: crate::canvas::FlopsynthPage,
    ) -> Option<crate::canvas::FlopsynthView> {
        None
    }

    /// [`flopsynth`](Self::flopsynth), with a source being inspected
    /// (`docs/flopsynth-next.md` §3.4): the source's card comes in the view
    /// marked for the inspector's drawer. `None` inspects nothing.
    fn flopsynth_inspecting(
        &self,
        page: crate::canvas::FlopsynthPage,
        inspector: Option<usize>,
    ) -> Option<crate::canvas::FlopsynthView> {
        self.flopsynth_showing(
            page,
            crate::canvas::FlopsynthShowing {
                inspector,
                fx_slot: None,
                wave_tool: Default::default(),
            },
        )
    }

    /// [`flopsynth`](Self::flopsynth), with everything the window chooses
    /// to show: the inspected source, and which effect slot's card the
    /// Effects page has (§3.6; `None` is the first). The host builds that
    /// slot's card, and only that, into the view beside the rack.
    fn flopsynth_showing(
        &self,
        page: crate::canvas::FlopsynthPage,
        _showing: crate::canvas::FlopsynthShowing,
    ) -> Option<crate::canvas::FlopsynthView> {
        self.flopsynth(page)
    }

    /// Loads a sound file onto one of the instrument's oscillators, as the
    /// waveform it reads — *"drag audio files into it to use those waveforms
    /// in the synthesis"*.
    ///
    /// `layer` is the oscillator's own index, which the window reads off the
    /// card the file was dropped on
    /// ([`FlopsynthCard::oscillator`](crate::canvas::FlopsynthCard::oscillator)).
    /// Says what arrived, or why nothing did.
    fn load_wavetable(&mut self, _layer: usize, _path: &std::path::Path) -> Result<String, String> {
        Err("this instrument does not take sounds".to_string())
    }

    /// Loads a sound onto one of the instrument's oscillators as the
    /// **recording** it plays, kept whole and pitched across the keyboard
    /// — *"we could actually sample a real piano sound"*. A folder is a
    /// multi-sample: one zone per file. Says what arrived, or why nothing
    /// did.
    fn load_sample(&mut self, _layer: usize, _path: &std::path::Path) -> Result<String, String> {
        Err("this instrument does not take sounds".to_string())
    }

    /// A sound dropped on an oscillator, whichever it turns out to be: a
    /// recording, or a wavetable when the file is shaped like one. What the
    /// window calls for a drop, so the rule lives in one place.
    fn load_sound(&mut self, _layer: usize, _path: &std::path::Path) -> Result<String, String> {
        Err("this instrument does not take sounds".to_string())
    }

    /// What the instrument on the selected channel is putting out right
    /// now, for the sky through Flopsynth's canopy (`sky.rs`): the
    /// analyser's bands and a stretch of waveform, off a tap on the
    /// instrument's node. Read once a frame while the window is open, like
    /// [`spectrum`](StudioHost::spectrum), and for the same reason. `None`
    /// when nothing is running — an offline session, a graph built a moment
    /// ago — and the sky then hears silence.
    fn instrument_sound(&mut self) -> Option<crate::sky::SkySound> {
        None
    }

    /// The sounds an oscillator card's own menu offers: every audio file in
    /// the Import tab's audio folder, by name, whichever tab the browser is
    /// on. Empty with no folder set. A pick is
    /// [`load_audio_sound_into_oscillator`](Self::load_audio_sound_into_oscillator)
    /// by the same index.
    fn audio_sounds(&self) -> Vec<String> {
        Vec::new()
    }

    /// [`load_sound`](Self::load_sound) for entry `index` of
    /// [`audio_sounds`](Self::audio_sounds).
    fn load_audio_sound_into_oscillator(
        &mut self,
        _layer: usize,
        _index: usize,
    ) -> Result<String, String> {
        Err("this instrument does not take sounds".to_string())
    }

    /// How many voices the selected channel's instrument is sounding right
    /// now — the number the window's tab strip shows. Off the audio thread's
    /// own count, so it moves between revisions; the window polls it once a
    /// frame while the instrument's window is open.
    fn instrument_voices(&self) -> usize {
        0
    }

    /// [`load_sound`](Self::load_sound) for row `index` of the Import tab,
    /// which is what the browser's drag carries.
    fn load_import_into_oscillator(
        &mut self,
        _layer: usize,
        _index: usize,
    ) -> Result<String, String> {
        Err("this instrument does not take sounds".to_string())
    }

    // --- the preset system (`docs/flopsynth-plan.md` §P) ---
    /// What the preset bar shows for one device.
    ///
    /// Eight methods, and the same eight for a channel playing Flopsynth, an
    /// insert holding a reverb and a slot hosting somebody's CLAP — which is
    /// the whole argument of §P. A device contributes **what kind it is** and
    /// **what its state is**; everything else (the file, the name, the star,
    /// the undo) is one implementation.
    ///
    /// The default is a bar with nothing on it, so a host that has no bank
    /// draws no preset controls rather than empty ones.
    /// Which insert has a window open, as the window knows it.
    ///
    /// Told rather than asked, because it is a fact about the *window*: the
    /// browser needs it so that an effect preset clicked in the Presets tab
    /// lands in the insert you are looking at, and is refused when you are not
    /// looking at one (§P.8).
    fn note_open_insert(&mut self, _insert: Option<(usize, usize)>) {}

    /// Opens the user's preset folder in the file manager.
    fn reveal_preset_dir(&mut self) {}

    /// Asks for a different one.
    fn choose_preset_dir(&mut self) {}

    /// What the browser's Presets tab says along its bottom: where the user's
    /// own bank is, and how many presets each origin holds (§P.8).
    fn preset_status(&self) -> String {
        String::new()
    }

    fn preset_bar(&self, _device: crate::canvas::PresetDevice) -> crate::canvas::PresetBarView {
        crate::canvas::PresetBarView::default()
    }

    /// Every preset this device could be loaded with, in the bank's order —
    /// factory then user, category then name.
    fn preset_choices(
        &self,
        _device: crate::canvas::PresetDevice,
    ) -> Vec<crate::canvas::PresetChoice> {
        Vec::new()
    }

    /// The categories this device has presets in. What "Save as…" offers, and
    /// what the Presets page's left column lists.
    fn preset_categories(&self, _device: crate::canvas::PresetDevice) -> Vec<String> {
        Vec::new()
    }

    /// Loads the preset at `index` of [`preset_choices`](Self::preset_choices).
    /// One undo entry.
    fn apply_preset(&mut self, _device: crate::canvas::PresetDevice, _index: usize) {}

    /// Puts the preset at `index` into the **preview voice** and aims the
    /// live path at it, so the next [`audition_on`](Self::audition_on) is
    /// that preset rather than the channel's — the Presets page's audition
    /// (`docs/flopsynth-next.md` §5.2). Nothing is written to the document:
    /// a listen is not a load, which is what lets a single click be a
    /// listen and keeps a loaded preset's edits from a stray one.
    fn audition_preset(
        &mut self,
        _device: crate::canvas::PresetDevice,
        _index: usize,
    ) -> Result<(), String> {
        Err("this studio cannot preview".to_string())
    }

    /// Writes every preset the user made for `device` as one pack file,
    /// asking where (§5). What it did, or why not.
    fn export_pack(&mut self, _device: crate::canvas::PresetDevice) -> Result<String, String> {
        Err("this studio cannot export packs".to_string())
    }

    /// Reads a pack into the user's bank, asking which; a name already
    /// taken is skipped rather than written over.
    fn import_pack(&mut self) -> Result<String, String> {
        Err("this studio cannot import packs".to_string())
    }

    // --- the synth window's header actions (`docs/flopsynth-next.md` §3.2, §5) ---
    /// Which slot of the selected channel's A/B pair is playing: 0 for A,
    /// 1 for B. Every channel starts on A.
    fn ab_slot(&self) -> usize {
        0
    }

    /// Switches the selected channel to the other slot of its A/B pair —
    /// a copy of what is playing, the first time. One undo entry.
    fn ab_switch(&mut self) {}

    /// Copies what is playing over the other slot, without switching.
    fn ab_copy(&mut self) {}

    /// Puts the Init patch on the selected channel: a fresh Flopsynth that
    /// came from no preset. One undo entry.
    fn init_patch(&mut self) {}

    /// Moves every continuous control of the selected instrument by up to
    /// `amount` of its travel, either way, at random — and **nothing
    /// else**: no source, chooser or switch changes, so what comes out is
    /// a variation in the patch's family. §5's Randomise is a fifth,
    /// Mutate a twentieth. One undo entry.
    fn randomise_patch(&mut self, _amount: f32) {}

    /// The preset at `index`'s thumbnail (§5.2), once its preview is
    /// rendered — what the inspector draws under a selected row's name.
    fn preset_thumbnail(
        &self,
        _device: crate::canvas::PresetDevice,
        _index: usize,
    ) -> Option<crate::canvas::PresetThumbnail> {
        None
    }

    /// The presets that sound most like the one at `index`, by name,
    /// nearest first — what the inspector lists under a selected row.
    /// Empty until the previews are in, and for a row that is not there.
    fn preset_sounds_like(
        &self,
        _device: crate::canvas::PresetDevice,
        _index: usize,
    ) -> Vec<String> {
        Vec::new()
    }

    /// The previous or next preset in this device's bank, wrapping.
    fn step_preset(&mut self, _device: crate::canvas::PresetDevice, _delta: i32) {}

    /// Writes this device's state over the file it came from.
    fn save_preset(&mut self, _device: crate::canvas::PresetDevice) {}

    /// Writes this device's state as a preset of the user's own.
    fn save_preset_as(
        &mut self,
        _device: crate::canvas::PresetDevice,
        _name: &str,
        _category: &str,
    ) {
    }

    /// Stars this device's preset, or takes the star off.
    fn toggle_preset_favorite(&mut self, _device: crate::canvas::PresetDevice) {}

    /// Stars one preset of this device's bank, by its place in
    /// [`preset_choices`](Self::preset_choices), or takes the star off. The
    /// Presets page's star on a row, which need not be the loaded preset.
    fn toggle_preset_star(&mut self, _device: crate::canvas::PresetDevice, _index: usize) {}

    // --- the modulation matrix (`docs/flopsynth-plan.md` §8.4) ---
    /// Every modulation source this instrument has, in the order the badge row
    /// shows them.
    ///
    /// Names rather than a type of their own, and an **index** is how one is
    /// named back: `fontelle-core`'s `ModSource` may not cross into this crate
    /// (INVARIANT 4), and a list plus a position is the shape every other menu
    /// here uses for the same reason.
    fn mod_sources(&self) -> Vec<String> {
        Vec::new()
    }

    /// Which of [`mod_sources`](Self::mod_sources) are the macros — what
    /// *Assign to macro…* lists (`docs/flopsynth-next.md` §3.3).
    fn macro_sources(&self) -> Vec<usize> {
        Vec::new()
    }

    /// Where every source is **now**, one per [`mod_sources`](Self::mod_sources)
    /// — an LFO's value at its phase, a macro's value; what draws the live
    /// dot on a ring's band (§3.3). Asked once a frame while the window is
    /// open, so it has to be cheap. Zero for a source with no live reading
    /// yet (the envelopes, until Phase 3 sends their levels).
    fn mod_source_values(&self) -> Vec<f32> {
        Vec::new()
    }

    /// The routes reaching the control at `address`, oldest first.
    ///
    /// Empty for a control nothing modulates, which is also what draws no
    /// ring round it.
    fn routes_to(&self, _address: &fontelle_types::ParamAddress) -> Vec<RouteInfo> {
        Vec::new()
    }

    /// Whether anything **could** be routed to this control — what lights a
    /// knob up while a source badge is being dragged.
    ///
    /// Not the same question as [`routes_to`](Self::routes_to): the output
    /// trim has no routes *and* can have none, and a drag has to be able to
    /// tell those apart before it lights one up and then does nothing.
    fn is_mod_destination(&self, _address: &fontelle_types::ParamAddress) -> bool {
        false
    }

    /// Every control a route could reach, with the depth of the one that
    /// does — the marks the window's knobs wear, **for the whole window in
    /// one call**.
    ///
    /// The window used to ask [`routes_to`](Self::routes_to) and
    /// [`is_mod_destination`](Self::is_mod_destination) for every control
    /// each time the revision moved, and each of those answers was a clone
    /// of the patch and a walk of its destinations: nine milliseconds a
    /// revision on the bank's Grand Piano. With snap off a drag is a
    /// revision per pointer motion, which is where *"stuttering when
    /// dragging audio clips"* came from. One question a revision is a
    /// fraction of a frame; see `tests/mod_marks.rs`.
    fn modulation_marks(&self) -> Vec<ModMark> {
        Vec::new()
    }

    /// Adds a route from source `source` to the control at `address`.
    ///
    /// At a depth §8.4 states: +0.5, so the route is audible the moment it is
    /// made. A route that arrived at zero would look like a gesture that did
    /// nothing.
    fn add_route(&mut self, _source: usize, _address: &fontelle_types::ParamAddress) {}

    /// Removes the route at `index` of [`routes_to`](Self::routes_to).
    fn remove_route(&mut self, _address: &fontelle_types::ParamAddress, _index: usize) {}

    /// Writes LFO `lfo`'s drawn shape (`docs/flopsynth-next.md` §3.4) —
    /// a point dragged, a segment bent, a point added or taken out, a
    /// factory shape chosen. Coalesced like a knob drag until
    /// [`end_gesture`](DocumentHost::end_gesture).
    fn set_lfo_shape(&mut self, _lfo: usize, _shape: fontelle_types::LfoShape) {}

    // ------------------------------------------- the wavetable editor (§4.3)

    /// Applies one edit to the table layer `layer` reads — the patch's own,
    /// or nothing if the layer reads the bank's. Coalesced like a knob
    /// drag: one undo for one gesture, broken by `end_gesture`.
    fn edit_wavetable(
        &mut self,
        _layer: usize,
        _edit: fontelle_types::WavetableEdit,
    ) -> Result<(), String> {
        Err("this instrument has no table to edit".to_string())
    }

    /// Makes the bank table layer `layer` reads the patch's own copy, named
    /// after it, so it can be edited: the bank is recipes, and an edit
    /// starts by taking a copy.
    fn adopt_wavetable(&mut self, _layer: usize) -> Result<(), String> {
        Err("this instrument has no table to adopt".to_string())
    }

    /// The frame under `layer`'s position knob replaced by `text` evaluated
    /// over the phase (`fontelle_core::formula`). Says what is wrong with a
    /// formula that is not one.
    fn apply_wavetable_formula(&mut self, _layer: usize, _text: &str) -> Result<(), String> {
        Err("this instrument has no table to edit".to_string())
    }

    /// The last formula applied to `layer`'s table, for the prompt to seed
    /// with; empty when there was none.
    fn wavetable_formula(&self, _layer: usize) -> String {
        String::new()
    }

    /// Writes `layer`'s table as a WAV Serum reads, asking where through
    /// the desktop's picker. Says where it went, or why it did not.
    fn export_wavetable(&mut self, _layer: usize) -> Result<String, String> {
        Err("this instrument has no table to export".to_string())
    }

    /// LFO `lfo`'s wave, sampled as a shape to draw on — what the shapes
    /// menu's last row starts over from.
    fn lfo_wave_shape(&self, _lfo: usize) -> Option<fontelle_types::LfoShape> {
        None
    }

    // --- the Matrix page's table (`docs/flopsynth-next.md` §3.4) ---
    //
    // Every one of these names a row by its **position in the whole
    // matrix** — `FlopsynthView::routes`' index — and a choice by its
    // position in the list the view offers for that cell: `sources` for a
    // source or a via, `destinations` for a destination, `curves` for a
    // curve. Each is one undo. A row or a choice that is not there does
    // nothing.

    /// Sets row `row`'s source to `sources[source]`.
    fn set_route_source(&mut self, _row: usize, _source: usize) {}
    /// Sets row `row`'s destination to `destinations[destination]`.
    fn set_route_destination(&mut self, _row: usize, _destination: usize) {}
    /// Sets, or clears, the source scaling row `row`'s depth.
    fn set_route_via(&mut self, _row: usize, _via: Option<usize>) {}
    /// Sets row `row`'s curve to `curves[curve]`.
    fn set_route_curve(&mut self, _row: usize, _curve: usize) {}
    /// Whether row `row` reads `1 - source`.
    fn set_route_invert(&mut self, _row: usize, _invert: bool) {}
    /// Whether row `row` is kept and not heard.
    fn set_route_bypass(&mut self, _row: usize, _bypass: bool) {}
    /// Takes row `row` out of the matrix.
    fn remove_route_row(&mut self, _row: usize) {}
    /// The `+`: a new row at the end, audible at once.
    fn add_route_row(&mut self) {}
    /// Row `row` dragged by its grip and let go on row `to`: it goes before
    /// the row that was there, or last for `to` past the end. Dropped where
    /// it was, nothing happens.
    fn move_route(&mut self, _row: usize, _to: usize) {}
    /// Sorts the rows, stably, by the order the view lists that column's
    /// choices in.
    fn sort_routes(&mut self, _by: RouteSort) {}

    // --- the instrument's own effects (`docs/flopsynth-plan.md` §8.5) ---
    /// The kinds the `+ effect` list offers, in its order: the ones that cost
    /// no latency, because an instrument's latency is the one case the graph
    /// does not compensate (§2.2).
    fn patch_effect_kinds(&self) -> Vec<fontelle_types::EffectKind> {
        Vec::new()
    }

    /// Flopsynth's window scale (`docs/flopsynth-next.md` §3.2), one of
    /// `canvas::SCALES`. The view carries it (`FlopsynthView::scale`) and
    /// the window opens at `layout::flopsynth_window_size` of it.
    fn flopsynth_scale(&self) -> f32 {
        1.0
    }
    /// Chooses the scale — kept as a setting. A scale the window does not
    /// offer changes nothing.
    fn set_flopsynth_scale(&mut self, _scale: f32) {}

    /// The loaded preset's value for a control, normalised — what Alt-click
    /// and *Reset to preset* put back (`docs/flopsynth-next.md` §3.3).
    /// `None` on a channel that came from no preset.
    fn instrument_param_preset_value(
        &self,
        _address: &fontelle_types::ParamAddress,
    ) -> Option<f32> {
        None
    }
    /// The Init patch's value for a control — *Reset to default*.
    fn instrument_param_default_value(
        &self,
        _address: &fontelle_types::ParamAddress,
    ) -> Option<f32> {
        None
    }
    /// The normalised value a typed entry means for a control, read against
    /// the control's own read-out ("2.4k", "-12", "1/8", "37%", or an
    /// option's name); `None` when it reads as nothing.
    fn instrument_param_from_text(
        &self,
        _address: &fontelle_types::ParamAddress,
        _text: &str,
    ) -> Option<f32> {
        None
    }

    /// Puts an effect of `kind` on the end of the selected instrument's own
    /// chain. Refused, quietly, when the chain is full.
    fn add_patch_effect(&mut self, _kind: fontelle_types::EffectKind) {}

    /// Takes slot `index` off the selected instrument's own chain.
    fn remove_patch_effect(&mut self, _index: usize) {}
    /// Moves slot `from` of the selected instrument's own chain to `to`, the
    /// slots between sliding — a card dragged by its header onto another
    /// (§8.5). One undo.
    fn move_patch_effect(&mut self, _from: usize, _to: usize) {}

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

    /// Every plugin on this machine that can go on an instrument channel
    /// (TDD §8.4).
    ///
    /// The window picks by **position**: INVARIANT 2 says the window never
    /// mutates the document, and a window that resolved plugin identities
    /// would be a window holding half of one. Each listing does carry its
    /// [`fontelle_types::PluginKey`], but only so the window can say which
    /// rows are starred. Empty until something has scanned — see
    /// [`rescan_plugins`](Self::rescan_plugins).
    fn plugin_instruments(&self) -> Vec<PluginListing> {
        Vec::new()
    }

    /// The same, for plugins that can go in an insert slot.
    fn plugin_effects(&self) -> Vec<PluginListing> {
        Vec::new()
    }

    /// Everything that has been starred — see [`fontelle_types::Favorite`].
    ///
    /// Read whenever a menu that lists effects, instruments or plugins is
    /// built: the favourites go at the top and are drawn lit wherever they
    /// appear. Empty on a fresh install, and for a host that keeps none.
    fn favorites(&self) -> Vec<fontelle_types::Favorite> {
        Vec::new()
    }

    /// A press on a row's star: stars the thing if it is not, and un-stars it
    /// if it is. The host keeps the list (it is a fact about the person, so
    /// it goes with the settings rather than the project) and says which way
    /// it went through [`take_message`](Self::take_message).
    fn toggle_favorite(&mut self, _favorite: fontelle_types::Favorite) {}

    /// Makes a new channel playing the instrument at `which` in
    /// [`plugin_instruments`](Self::plugin_instruments).
    fn add_plugin_channel(&mut self, _which: usize) {}

    /// Puts that plugin on an existing channel, by rack position.
    fn set_channel_plugin(&mut self, _channel: usize, _which: usize) {}

    /// Puts the effect at `which` in [`plugin_effects`](Self::plugin_effects)
    /// on the end of `strip`'s insert chain.
    fn add_plugin_insert(&mut self, _strip: usize, _which: usize) {}

    /// Opens the plugin's **own** editor for the channel at `index`, if it has
    /// one. Whether it did.
    ///
    /// > *"shouldnt these also be showing the custom plugins own display in
    /// > their windows not a auto made one from the parameters."*
    ///
    /// `false` means the fallback: Fontelle's own panel of the plugin's
    /// parameters, which is what a plugin with no editor — and every LV2 one
    /// in this build — gets. The window asks this before opening its own
    /// instrument window, so the two are never both up for one plugin.
    fn open_plugin_editor_for_channel(&mut self, index: usize) -> bool {
        let _ = index;
        false
    }

    /// The same for one insert of one mixer strip.
    fn open_plugin_editor_for_insert(&mut self, strip: usize, slot: usize) -> bool {
        let (_, _) = (strip, slot);
        false
    }

    /// Gives every open plugin editor its frame, and says whether any is still
    /// open.
    ///
    /// **Called once per pass of the event loop**, and the answer is what
    /// keeps the loop awake: a CLAP editor repaints on a timer the host fires,
    /// so a window that went back to sleep would be a plugin editor that
    /// froze. See `fontelle_host::gui`.
    fn tick_plugin_editors(&mut self) -> bool {
        false
    }

    /// Walks the plugin folders again.
    ///
    /// A thing somebody asks for rather than something a menu does when it
    /// opens: a scan `dlopen`s every bundle it finds, which is seconds on a
    /// machine with a real collection installed.
    fn rescan_plugins(&mut self) {}

    /// Walks them if nothing has yet.
    ///
    /// What the browser calls before it lists anything, so the first time it
    /// is opened it has something to show — and every time after that it costs
    /// nothing.
    fn scan_plugins_once(&mut self) {}

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

    /// Whether `strip`'s output is connected at all — see
    /// `fontelle_model::MixerTrack::output_on`.
    ///
    /// `true` for every strip until somebody switches one off, which is why
    /// the default is what it is: a host that has not implemented this is a
    /// host whose tracks are all routed.
    fn track_output_on(&self, _strip: usize) -> bool {
        true
    }

    /// Switches that output on or off.
    ///
    /// *"if i chose to not route it to master, i wont be hearing my own input
    /// but it will still be recording the audio clip."* Not a mute: the
    /// track's sends still carry, and the strip still records.
    fn set_track_output_on(&mut self, _strip: usize, _on: bool) {}

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
    /// Points one insert at a **channel's notes** — the melody to force or the
    /// scale to allow (`docs/tune-plan.md` §5.3). `None` is "no MIDI", which
    /// is what a corrector does with nothing named.
    ///
    /// Refused, and left alone, for an effect that has no use for notes.
    fn set_insert_notes(&mut self, _strip: usize, _slot: usize, _notes: Option<usize>) {}

    /// Which channel one insert listens to, if any — what a window draws a
    /// tick beside, indexed the same way [`channels`](StudioHost::channels)
    /// lists them.
    fn insert_notes(&self, _strip: usize, _slot: usize) -> Option<usize> {
        None
    }

    /// The corrector's own window, when this insert is one
    /// (`docs/tune-plan.md` §7.6).
    ///
    /// `None` for every other insert, exactly as
    /// [`eq_config`](StudioHost::eq_config) is `None` for everything but an
    /// EQ: the three effect windows are told apart by which of them offers a
    /// view, and an insert that is none of the three gets the grid of knobs.
    fn tune_view(&self, _strip: usize, _slot: usize) -> Option<crate::canvas::TuneView> {
        None
    }

    /// The pitch trace one corrector insert has written, oldest hop first
    /// (`docs/tune-plan.md` §7.3).
    ///
    /// Read **once a frame while the corrector's window is open**, like the
    /// spectrum below it and for the same reason. Empty when there is nothing
    /// to show — no such insert, no running graph, or an offline session — and
    /// an empty trace draws nothing rather than a line along the floor.
    fn tune_trace(&self, _strip: usize, _slot: usize) -> Vec<fontelle_types::TuneFrame> {
        Vec::new()
    }

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

    /// The notes of the take being recorded, so far, in the open clip's own
    /// ticks — with whatever is still held drawn out to `now`, the
    /// transport's position.
    ///
    /// > *"recording notes also doesnt show you the notes as youre recording
    /// > them which would be nice and for it like audio to show you it
    /// > making the clip as youre recording it."*
    ///
    /// Read, not taken: the take is kept when the transport stops
    /// ([`keep_take`](Self::keep_take)), and showing it must not spend it.
    /// Empty for a host with no capture, and for one that is not recording.
    fn recording_notes(&self, _now: Sample) -> Vec<NotePreview> {
        Vec::new()
    }

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

    // --- audio clips (TDD §15.1) ---
    /// One audio clip's properties, for the editor to show and change.
    ///
    /// `None` for a clip that is not audio, or one that has gone — the editor
    /// closes itself rather than editing a clip nobody can see.
    fn audio_clip(&self, _clip: ClipId) -> Option<fontelle_types::AudioClipData> {
        None
    }

    /// The rate the file behind `clip` was recorded at, so a fade can be shown
    /// in milliseconds rather than in frames.
    ///
    /// Zero when it is not known, which the editor reads as "say nothing about
    /// time" rather than dividing by it.
    fn audio_clip_rate(&self, _clip: ClipId) -> u32 {
        0
    }

    /// Sets every property of one audio clip at once.
    ///
    /// All of them together rather than one call per knob, for the reason
    /// `SetAudioClip` is one command: a clip *is* a list of numbers, the editor
    /// hands back a whole list, and a call per field would be twenty that each
    /// have to agree about what "unchanged" means.
    fn set_audio_clip(&mut self, _clip: ClipId, _data: fontelle_types::AudioClipData) {}
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

    // --- recording (TDD §14.7, §15.4) ---
    /// What the record button records — see
    /// [`RecordMode`](crate::transport::RecordMode).
    ///
    /// Remembered rather than asked every time: somebody recording eight vocal
    /// takes should answer once.
    fn record_mode(&self) -> crate::transport::RecordMode {
        crate::transport::RecordMode::default()
    }

    fn set_record_mode(&mut self, _mode: crate::transport::RecordMode) {}

    /// Every audio input the machine has, by name (TDD §15.4).
    ///
    /// Empty on a machine with no microphone, which is a state and not a
    /// failure.
    fn audio_inputs(&self) -> Vec<String> {
        Vec::new()
    }

    /// Which input a mixer strip records from, if any.
    fn track_input(&self, _strip: usize) -> Option<String> {
        None
    }

    /// Sets it. `None` is a track that records nothing.
    fn set_track_input(&mut self, _strip: usize, _input: Option<String>) {}

    /// Opens the capture stream on whatever the armed mixer strip's input
    /// names, and says which one (TDD §15.4).
    ///
    /// `Err` carries a sentence for the status line: no strip armed, no input
    /// chosen, or a microphone that is not there. Opened on **arming** rather
    /// than on play, so a missing device is reported while you are still
    /// setting up rather than in the middle of a take.
    fn open_audio_input(&mut self) -> Result<String, String> {
        Err(String::new())
    }

    /// Closes it, throwing away anything captured.
    fn close_audio_input(&mut self) {}

    /// Throws away what the input has captured so far, keeping the stream open.
    ///
    /// What the end of a count-in calls: the bar you were counted in over is
    /// on the ring too, and a take that began with it would begin with a
    /// woodblock.
    fn discard_audio_take(&mut self) {}

    /// How long one beat of this project is, in samples — what a count-in is
    /// measured in.
    fn samples_per_beat(&self) -> fontelle_types::Sample {
        0
    }

    /// Turns whatever the input captured into an audio clip at song sample
    /// `at`, and says how many frames it kept — or **why it could not**.
    ///
    /// `Ok(0)` is nothing arrived, which is an ordinary thing to happen to a
    /// record button. `Err` is a take that existed and was lost: nowhere to
    /// write it, a file that would not write, a decode that failed. The two
    /// used to be one number, and the window read every refusal as silence.
    fn keep_audio_take(
        &mut self,
        _at: fontelle_types::Sample,
        _end_sample: fontelle_types::Sample,
    ) -> Result<usize, String> {
        Ok(0)
    }

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
