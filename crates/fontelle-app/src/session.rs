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
    AddChannel, AddClip, AddNotes, Arena, Clip, ClipSource, Command, FlagTarget, History, Lane,
    MoveNotes, Note, NoteData, Project, RemoveNotes, ResizeNotes, SetFlag, SetNoteVelocity,
};
use fontelle_types::{
    ChannelId, ClipId, EventPayload, NodeId, NoteId, PPQN, Sample, Tick, TimedEvent,
};
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{ChannelInfo, DocumentHost, LibraryEntry, StudioHost};

use crate::bank::{SoundfontBank, matches, matches_names};
use crate::library::SampleLibrary;
use crate::realise::{RealiseOptions, realise};
use crate::settings::Settings;

/// Until the time signature is in the document (§10 has nowhere to put one
/// yet), 4/4 — which is what the importer and the demo both assume.
const BEATS_PER_BAR: u32 = 4;

/// How long a clip a freshly added channel gets, in bars.
const NEW_CLIP_BARS: i64 = 8;

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
    query: String,
    /// Which entry of the *filtered* file list is open, and what is inside it.
    open_file: Option<usize>,
    presets: Vec<fontelle_assets::PresetInfo>,
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
            query: String::new(),
            open_file: None,
            presets: Vec::new(),
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

    /// Gives the session the graph channel, so instrument changes reach a
    /// running stream. Without one it still edits — it just cannot be heard
    /// until the next launch, which is what the offline paths want.
    pub fn with_graphs(mut self, graphs: GraphPublisher) -> Self {
        self.graphs = Some(graphs);
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

    /// Adds a folder to the bank and remembers it (INVARIANT 10: it is only
    /// ever the user who says where).
    pub fn add_soundfont_dir(&mut self, dir: &Path) {
        let dir = dir.to_path_buf();
        if !self.settings.soundfont_dirs.contains(&dir) {
            self.settings.soundfont_dirs.push(dir);
        }
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
    fn republish(&mut self) {
        let timeline = fontelle_sequencer::compile(&self.project, &self.channel_nodes);
        self.publisher.publish(timeline);
    }

    /// Rebuilds the whole graph and publishes it — what an instrument change,
    /// a new channel or a mute needs.
    ///
    /// Note the order: the node map is rebuilt *first*, because adding a
    /// channel renumbers the nodes, and a timeline compiled against the old map
    /// would address notes to nodes that have moved.
    fn rebuild_graph(&mut self) {
        match realise(&self.project, &self.library, self.options) {
            Ok(realised) => {
                self.channel_nodes = realised.channel_nodes;
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

    /// The file list as the browser shows it: filtered by the live search.
    fn filtered_files(&self) -> Vec<usize> {
        matches(self.bank.entries(), &self.query)
    }

    fn filtered_presets(&self) -> Vec<usize> {
        let names: Vec<&str> = self.presets.iter().map(|p| p.name.as_str()).collect();
        matches_names(&names, &self.query)
    }

    /// The node an audition should reach: the selected channel's.
    fn audition_target(&self) -> NodeId {
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
        self.history
            .apply(
                Box::new(fontelle_model::SetChannelPatch::new(channel, Some(data))),
                &mut self.project,
            )
            .map_err(|e| e.to_string())?;
        self.history.break_gesture();
        self.dirty = true;
        self.rebuild_graph();
        Ok(())
    }

    fn open_file_path(&self) -> Option<PathBuf> {
        let index = self.open_file?;
        let files = self.filtered_files();
        let entry = self.bank.entries().get(*files.get(index)?)?;
        Some(entry.path.clone())
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
            RollEdit::Add {
                tick,
                key,
                length,
                velocity,
            } => self.insert(clip, vec![blank_note(tick, length, key, velocity)]),
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
            RollEdit::SetVelocity { ids, velocity } => {
                self.run(Box::new(SetNoteVelocity::new(clip, ids, velocity)));
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
            self.republish();
        }
    }

    fn redo(&mut self) {
        if let Some(result) = self.history.redo(&mut self.project) {
            if let Err(e) = result {
                eprintln!("Fontelle: could not redo — {e}");
                return;
            }
            self.dirty = true;
            self.republish();
        }
    }

    fn end_gesture(&mut self) {
        self.history.break_gesture();
    }

    fn beats_per_bar(&self) -> u32 {
        BEATS_PER_BAR
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
            .map(|channel| {
                let track = self.project.mixer.tracks.get(channel.mixer_track);
                ChannelInfo {
                    name: channel.name.clone(),
                    muted: track.is_some_and(|t| t.mute),
                    soloed: track.is_some_and(|t| t.solo),
                    has_instrument: channel.patch_data.is_some(),
                }
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
        // The roll follows the rack: selecting a channel opens its clip, which
        // is the whole reason a rack and a roll are next to each other.
        if let Some(clip) = self.clip_of_channel(channel) {
            self.clip = clip;
        }
        self.revision += 1;
    }

    fn toggle_mute(&mut self, index: usize) {
        let Some(channel) = self.channel_ids().get(index).copied() else {
            return;
        };
        let Some(track) = self.project.channels.get(channel).map(|c| c.mixer_track) else {
            return;
        };
        let now = self.project.mixer.tracks.get(track).is_some_and(|t| t.mute);
        self.run(Box::new(SetFlag::new(FlagTarget::TrackMute(track), !now)));
        // A mute is a property of the *graph*, not of the notes — the fader
        // node carries it — so the graph is what has to be republished.
        self.rebuild_graph();
    }

    fn toggle_solo(&mut self, index: usize) {
        let Some(channel) = self.channel_ids().get(index).copied() else {
            return;
        };
        let Some(track) = self.project.channels.get(channel).map(|c| c.mixer_track) else {
            return;
        };
        let now = self.project.mixer.tracks.get(track).is_some_and(|t| t.solo);
        self.run(Box::new(SetFlag::new(FlagTarget::TrackSolo(track), !now)));
        self.rebuild_graph();
    }

    fn library_files(&self) -> Vec<LibraryEntry> {
        self.filtered_files()
            .into_iter()
            .filter_map(|index| self.bank.entries().get(index))
            .map(|entry| LibraryEntry {
                name: entry.name.clone(),
                detail: human_size(entry.size_bytes),
            })
            .collect()
    }

    fn library_presets(&self) -> Vec<LibraryEntry> {
        self.filtered_presets()
            .into_iter()
            .filter_map(|index| self.presets.get(index))
            .map(|preset| LibraryEntry {
                name: preset.name.clone(),
                // Bank and program, because a General MIDI soundfont has three
                // presets called "Piano" and the numbers are what tell them
                // apart.
                detail: format!("{}:{}", preset.bank, preset.program),
            })
            .collect()
    }

    fn query(&self) -> &str {
        &self.query
    }

    fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        // The open file was named by its position in the *filtered* list, and
        // the filter just changed under it.
        self.open_file = None;
        self.presets.clear();
        self.revision += 1;
    }

    fn open_file(&mut self, index: usize) -> Result<(), String> {
        let files = self.filtered_files();
        let entry = files
            .get(index)
            .and_then(|i| self.bank.entries().get(*i))
            .ok_or("that soundfont is not in the bank any more")?;
        let path = entry.path.clone();
        // Presets only: this reads the file's headers and decodes no audio, so
        // clicking through a collection costs nothing (TDD §17.5's "audition
        // on click without loading into a channel" is the next step past it).
        let presets =
            fontelle_assets::list_presets(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.presets = presets;
        self.open_file = Some(index);
        self.revision += 1;
        Ok(())
    }

    fn selected_file(&self) -> Option<usize> {
        self.open_file
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

    fn library_status(&self) -> String {
        let folders = self.bank.dirs();
        if self.bank.entries().is_empty() {
            return match folders.first() {
                Some(dir) => format!("no .sf2 files yet — put them in {}", dir.display()),
                None => "no soundfont folder configured".to_string(),
            };
        }
        format!(
            "{} soundfonts in {} folder(s)",
            self.bank.entries().len(),
            folders.len()
        )
    }

    fn rescan_library(&mut self) {
        self.bank.rescan();
        self.revision += 1;
    }

    fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    fn audition_on(&mut self, key: u8, velocity: u8) {
        self.send_live(EventPayload::NoteOn {
            key,
            velocity,
            voice_context: AUDITION_VOICE_CONTEXT,
        });
    }

    fn audition_off(&mut self, key: u8) {
        self.send_live(EventPayload::NoteOff {
            key,
            voice_context: AUDITION_VOICE_CONTEXT,
        });
    }

    fn pump(&mut self) {
        if let Some(graphs) = &mut self.graphs {
            // Where a `CompiledGraph` the audio thread stopped using is freed:
            // on this thread, never on that one (INVARIANT 1).
            graphs.pump();
        }
    }
}

fn blank_note(start: Tick, length: Tick, key: u8, velocity: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
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
