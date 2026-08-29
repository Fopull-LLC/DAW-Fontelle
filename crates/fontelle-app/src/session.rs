//! The open document, and everything that has to happen when it changes.
//!
//! `fontelle-app` is the one layer allowed to see the model, the engine and the
//! UI at once, so this is where the three meet: the window asks for an edit,
//! the edit becomes a `Command`, the command goes through `History`, and the
//! result is recompiled and published to the audio thread.
//!
//! Two invariants live or die here:
//!
//! - **INVARIANT 9** — every mutation is a command. There is no `&mut Project`
//!   reachable from the UI; [`Session::edit`] is the whole surface.
//! - **INVARIANT 2** — the roll is a view. It sends [`RollEdit`] values and
//!   reads notes back; it never writes.

use std::collections::HashMap;
use std::path::PathBuf;

use fontelle_engine::TimelinePublisher;
use fontelle_model::{
    AddNotes, Arena, ClipSource, Command, History, MoveNotes, Note, Project, RemoveNotes,
    ResizeNotes,
};
use fontelle_types::{ChannelId, ClipId, NodeId, NoteId, Sample, Tick};
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::DocumentHost;

/// Until the time signature is in the document (§10 has nowhere to put one
/// yet), 4/4 — which is what the importer and the demo both assume.
const BEATS_PER_BAR: u32 = 4;

pub struct Session {
    project: Project,
    history: History,
    channel_nodes: HashMap<ChannelId, NodeId>,
    publisher: TimelinePublisher,
    /// The clip the piano roll is showing. One open clip is the gate's scope;
    /// item 9's timeline is what picks a different one.
    clip: ClipId,
    bundle: Option<PathBuf>,
    dirty: bool,
    /// Handed back when the roll asks for notes and the clip has gone.
    empty: Arena<NoteId, Note>,
}

impl Session {
    pub fn new(
        project: Project,
        channel_nodes: HashMap<ChannelId, NodeId>,
        publisher: TimelinePublisher,
        clip: ClipId,
        bundle: Option<PathBuf>,
    ) -> Self {
        Self {
            project,
            history: History::new(),
            channel_nodes,
            publisher,
            clip,
            bundle,
            dirty: false,
            empty: Arena::default(),
        }
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

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Recompiles and hands the result to the audio thread.
    ///
    /// A full recompile, not the segmented one §11.3 describes: at the sizes
    /// this opens today it is well under a frame, and an incremental path that
    /// is wrong is worse than a whole one that is slow. `DirtyBars` is where
    /// that goes when it is needed.
    fn republish(&mut self) {
        let timeline = fontelle_sequencer::compile(&self.project, &self.channel_nodes);
        self.publisher.publish(timeline);
    }

    fn run(&mut self, command: Box<dyn Command>) {
        match self.history.apply(command, &mut self.project) {
            Ok(()) => {
                self.dirty = true;
                self.republish();
            }
            // A refused edit is not a crash and not a history entry — a note
            // dragged past key 127 simply does not move.
            Err(e) => eprintln!("Fontelle: {e}"),
        }
    }

    fn notes_of_clip(&self) -> Option<&Arena<NoteId, Note>> {
        match &self.project.clips.get(self.clip)?.source {
            ClipSource::Notes(data) => Some(&data.notes),
            _ => None,
        }
    }
}

impl DocumentHost for Session {
    fn notes(&self) -> &Arena<NoteId, Note> {
        self.notes_of_clip().unwrap_or(&self.empty)
    }

    fn edit(&mut self, edit: RollEdit) {
        let clip = self.clip;
        let command: Box<dyn Command> = match edit {
            RollEdit::Add {
                tick,
                key,
                length,
                velocity,
            } => Box::new(AddNotes::new(
                clip,
                vec![Note {
                    start: tick,
                    length,
                    key,
                    velocity,
                    pan: 0,
                    fine_pitch: 0,
                    release: 0,
                    mod_x: 0,
                    mod_y: 0,
                }],
            )),
            RollEdit::Remove(ids) => Box::new(RemoveNotes::new(clip, ids)),
            RollEdit::Move {
                ids,
                tick_delta,
                key_delta,
            } => Box::new(MoveNotes::new(clip, ids, tick_delta, key_delta)),
            RollEdit::Resize { ids, tick_delta } => {
                Box::new(ResizeNotes::new(clip, ids, tick_delta))
            }
        };
        self.run(command);
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

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn save(&mut self) -> Result<(), String> {
        let bundle = self.bundle.clone().ok_or_else(|| {
            "this project has no file yet — open it with --save <path>".to_string()
        })?;
        crate::save_project(&self.project, &bundle).map_err(|e| e.to_string())?;
        self.dirty = false;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.project.meta.name
    }
}
