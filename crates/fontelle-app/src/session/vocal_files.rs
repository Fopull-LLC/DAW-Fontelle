//! Vocal song files and renders of vocal tracks (`docs/vocal-plan.md` §3,
//! *Writing* and *Hearing*): a UST, USTX, VSQX, MIDI with lyrics or
//! MusicXML brought in as vocal tracks (or into the one it was dropped on),
//! a vocal track written back out as MIDI with lyrics or USTX, a vocal
//! track **frozen** to audio and back, and a selection of a vocal clip
//! rendered to a clip on the row below.
//!
//! **Freezing is the singing, rendered**: the track's final takes through
//! a `VocalNode` of its own with the channel's level and pan — what the
//! track hands its mixer strip — written into `renders/` and played as an
//! audio clip **routed to that same strip**, so the strip's inserts, fader
//! and sends go on applying as before and the song sounds the same. The
//! part is kept whole and unsung while frozen (`Project::is_frozen`).

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, TransportSnapshot, TransportState, VocalNode,
    VocalPlayer,
};
use fontelle_model::{FreezeVocal, ImportVocal, UnfreezeVocal, VocalLanding};
use fontelle_ui::document::VocalFileFormat;
use fontelle_vocal_files as files;

use super::*;

/// How far before the first note of a rendered selection the render
/// starts: a consonant is sung ahead of its note.
const SELECTION_LEAD_SECONDS: f64 = 0.15;
/// And before a frozen track's first phrase, and after its last.
const FREEZE_LEAD_SECONDS: f64 = 0.5;
const FREEZE_TAIL_SECONDS: f64 = 1.0;
const RENDER_BLOCK: usize = 1024;

/// What happens once a vocal render's file is written.
pub(super) enum VocalAfter {
    /// Freeze `channel`: the render at song sample `at`, on a new row at
    /// stack index `row`, named after `name`.
    Freeze {
        channel: ChannelId,
        at: fontelle_types::Sample,
        row: usize,
        name: String,
    },
    /// A selection's render: onto a new row under row `index`, at `at`,
    /// routed to `track`.
    Row {
        index: usize,
        at: fontelle_types::Sample,
        name: String,
        track: Option<MixerTrackId>,
        /// The vocal track it is a render of.
        channel: ChannelId,
    },
}

/// A vocal render ready to run off the session's thread.
pub(super) struct VocalBounce {
    prep: crate::vocal::VocalRenderPrep,
    player: Arc<VocalPlayer>,
    gain_db: f32,
    pan: f32,
    /// Song samples.
    from: i64,
    to: i64,
    sample_rate: u32,
    path: PathBuf,
}

impl VocalBounce {
    /// Waits for every final take (counting them into `singing` as an
    /// export does), plays them through the track's node and writes the
    /// file. Returns the clipped samples.
    fn run(self, progress: &mut dyn FnMut(i64), singing: &AtomicU64) -> Result<usize, String> {
        self.prep.wait_and_publish(&mut |done, total| {
            singing.store(((done as u64) << 32) | total as u64, Ordering::Relaxed);
        })?;
        // Nothing sung is nothing to freeze: say why rather than write
        // silence and call it frozen.
        let (failed, total, why) = self.prep.unsung();
        if total > 0 && failed == total {
            return Err(format!(
                "nothing on that track could be sung \u{2014} {}",
                why.unwrap_or_default()
            ));
        }
        let mut node = VocalNode::new(Arc::clone(&self.player)).on_channel(self.gain_db, self.pan);
        node.prepare(&PrepareContext {
            sample_rate: self.sample_rate as f32,
            max_block_size: RENDER_BLOCK as u32,
        });
        let frames = (self.to - self.from).max(1) as usize;
        let mut out = Vec::with_capacity(frames * 2);
        let mut left = vec![0.0f32; RENDER_BLOCK];
        let mut right = vec![0.0f32; RENDER_BLOCK];
        let mut at = self.from;
        while at < self.to {
            let n = RENDER_BLOCK.min((self.to - at) as usize);
            left[..n].fill(0.0);
            right[..n].fill(0.0);
            {
                let mut outputs: [&mut [f32]; 2] = [&mut left[..n], &mut right[..n]];
                let mut ctx = ProcessContext {
                    inputs: &[],
                    outputs: &mut outputs,
                    all_events: &[],
                    cut_events: &[],
                    live_events: &[],
                    audio: &[],
                    node: fontelle_types::NodeId::default(),
                    transport: TransportSnapshot {
                        state: TransportState::Rendering,
                        position_sample: at,
                        ..Default::default()
                    },
                    sample_range: at..at + n as i64,
                };
                node.process(&mut ctx);
            }
            for i in 0..n {
                out.push(left[i]);
                out.push(right[i]);
            }
            at += n as i64;
            progress(at - self.from);
        }
        crate::write_wav16(&self.path, &out, 2, self.sample_rate)
            .map_err(|e| format!("{}: {e}", self.path.display()))
    }
}

/// Whether nothing in `project` makes a sound yet: no notes, no audio.
fn song_is_empty(project: &Project) -> bool {
    project.clips.values().all(|clip| match &clip.source {
        ClipSource::Notes(data) => data.notes.is_empty(),
        _ => false,
    })
}

/// Whether vocal track `id` has notes in `data` (or `data` is its own).
fn sings_in(data: &fontelle_model::NoteData, id: ChannelId) -> bool {
    data.channel == id
        || data
            .notes
            .values()
            .any(|n| n.channel_or(data.channel) == id)
}

impl Session {
    fn is_vocal_channel_id(&self, id: ChannelId) -> bool {
        self.project
            .channels
            .get(id)
            .is_some_and(|c| c.instrument == Some(fontelle_types::InstrumentKind::Vocal))
    }

    /// The vocal track that sings in `data`: `prefer` when it does, else
    /// the clip's own channel when that is vocal, else the first vocal
    /// track with notes in it. A clip holds several instruments' notes, so
    /// a vocal line is often written into a clip another instrument began.
    fn vocal_in(
        &self,
        data: &fontelle_model::NoteData,
        prefer: Option<ChannelId>,
    ) -> Option<ChannelId> {
        if let Some(id) = prefer.filter(|id| self.is_vocal_channel_id(*id) && sings_in(data, *id)) {
            return Some(id);
        }
        if self.is_vocal_channel_id(data.channel) {
            return Some(data.channel);
        }
        data.notes
            .values()
            .map(|n| n.channel_or(data.channel))
            .find(|id| self.is_vocal_channel_id(*id))
    }

    /// The vocal track whose clips are on row `lane`, if any.
    fn vocal_on_row(&self, lane: fontelle_types::LaneId) -> Option<ChannelId> {
        let prefer = self.selected_channel_id();
        self.project
            .clips
            .values()
            .find_map(|clip| match &clip.source {
                ClipSource::Notes(data) if clip.lane == lane => self.vocal_in(data, prefer),
                _ => None,
            })
    }

    /// Brings a vocal song file in (`fontelle_vocal_files`): dropped on row
    /// `row` of a vocal track, or asked for on vocal track `channel` (a rack
    /// index), into that track; otherwise a vocal track of its own per
    /// sung part. `at` is the song sample it was let go at: the file's bar
    /// 1 lands on the bar that is in, and the file is placed by beats.
    ///
    /// The tempo: into an empty song the file's tempo map and bar come too;
    /// into a song with anything in it the song's are kept. One undo step
    /// either way, and the toast says which.
    pub fn import_vocal_file(
        &mut self,
        path: &Path,
        at: fontelle_types::Sample,
        row: Option<usize>,
        channel: Option<usize>,
    ) -> Result<String, String> {
        let song = files::read_file(path)?;
        let what = file_label(path);
        let empty = song_is_empty(&self.project);
        let rows = self.lane_ids();
        // Where it goes.
        let target = match (channel, row) {
            (Some(index), _) => {
                let id = *self
                    .channel_ids()
                    .get(index)
                    .ok_or("that track is not there")?;
                if !self.is_vocal_channel_id(id) {
                    return Err("that track is not a vocal track".to_string());
                }
                let lane = self.project.clips.values().find_map(|c| match &c.source {
                    ClipSource::Notes(d) if sings_in(d, id) => Some(c.lane),
                    _ => None,
                });
                lane.map(|lane| (id, lane))
            }
            (None, Some(r)) => rows
                .get(r)
                .and_then(|lane| self.vocal_on_row(*lane).map(|c| (c, *lane))),
            (None, None) => None,
        };
        // Onto the bar line at or before where it was put, in the meter the
        // song will have: the file's own signature when it brings it.
        let file_signature = fontelle_types::Signature::new(song.beats_per_bar, song.beat_unit);
        // The file's own bars, which its parts are cut to.
        let file_meter = fontelle_types::Meter::with_song(&file_signature, &[]);
        let meter = if empty {
            file_meter.clone()
        } else {
            self.project.meter()
        };
        let tick = self.project.tempo_map.sample_to_tick(at.max(0)).max(0);
        let offset = meter.tick_of_bar(meter.bar_at(tick).0);
        let mut left_out = song.unmapped.clone();
        let parts: Vec<&files::SongPart> = match target {
            Some(_) => {
                for extra in song.parts.iter().skip(1) {
                    left_out.push(format!(
                        "the part \u{201c}{}\u{201d} (one part goes into a track; drop the \
                         file on an empty row for every part)",
                        extra.name
                    ));
                }
                song.parts.iter().take(1).collect()
            }
            None => song.parts.iter().collect(),
        };
        let clips: Vec<_> = parts
            .iter()
            .map(|p| files::clip_of(p, offset, &file_meter))
            .collect();
        let notes: usize = clips.iter().map(|c| c.notes.len()).sum();
        // Sung by the bank the file was written for, when it is here.
        let named = parts.iter().find_map(|p| p.singer.clone());
        let found = named.as_deref().and_then(|n| self.vocal.voice_named(n));
        let new_voice = found
            .as_ref()
            .map(|(v, _)| v.clone())
            .or_else(|| self.vocal.default_voice());
        let landing = match target {
            Some((channel, lane)) => VocalLanding::Onto { channel, lane },
            None => VocalLanding::NewTracks {
                at: Some(match row {
                    Some(r) => (r + 1).min(rows.len()),
                    None => self.arrival_index(),
                }),
                voice: new_voice.clone(),
            },
        };
        let mut command = ImportVocal::new(&what, clips, landing);
        if empty {
            command = command.with_tempo(song.tempo.clone(), file_signature.clone());
        }
        let made = self
            .apply_for::<ImportVocal>(Box::new(command))?
            .made()
            .to_vec();
        if let Some(first) = made.first() {
            self.open_clip(first.clip);
            if let Some(index) = self.channel_ids().iter().position(|c| *c == first.channel) {
                self.selected = index;
            }
        }
        self.let_go();
        self.rebuild_graph();
        self.republish();
        self.touch();

        let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
        let who = match (target, names.as_slice()) {
            (Some((channel, _)), _) => format!(
                "into \u{201c}{}\u{201d}",
                self.project
                    .channels
                    .get(channel)
                    .map_or("the track", |c| c.name.as_str())
            ),
            (None, [one]) => format!("\u{201c}{one}\u{201d}"),
            (None, many) => format!("{} vocal tracks", many.len()),
        };
        let file_bpm = song.tempo.first().map_or(120.0, |t| t.bpm);
        let tempo = if empty {
            format!(
                " \u{2014} with its tempo, {file_bpm:.0} bpm in {}",
                file_signature.label()
            )
        } else {
            let here = self.project.tempo_map.tempo_at(0);
            if (here - file_bpm).abs() < 0.01 && song.tempo.len() == 1 {
                String::new()
            } else {
                format!(
                    " \u{2014} kept this song's tempo ({here:.0} bpm); the file's \
                     {file_bpm:.0} bpm notes are placed by beats"
                )
            }
        };
        let mut said = format!("Imported {who} from {what}, {notes} notes{tempo}");
        // Who sings it, when the file named somebody and a new track was
        // made for it.
        if target.is_none()
            && let Some(named) = &named
        {
            match (&found, &new_voice) {
                (Some((_, name)), _) => said.push_str(&format!(", sung by {name}")),
                (None, Some(voice)) => said.push_str(&format!(
                    ". \u{201c}{named}\u{201d} is not installed here, so {} sings it",
                    self.vocal.voice_label(voice)
                )),
                (None, None) => {
                    said.push_str(&format!(". \u{201c}{named}\u{201d} is not installed here"))
                }
            }
        }
        if !left_out.is_empty() {
            let shown: Vec<&str> = left_out.iter().take(3).map(String::as_str).collect();
            said.push_str(&format!(
                ". Not brought in: {}{}",
                shown.join("; "),
                if left_out.len() > 3 { "; \u{2026}" } else { "" }
            ));
        }
        Ok(said)
    }

    /// Writes vocal track `index` (a rack index) to `path` as `format`.
    pub fn export_vocal_to(
        &mut self,
        index: usize,
        format: VocalFileFormat,
        path: &Path,
    ) -> Result<String, String> {
        let id = *self
            .channel_ids()
            .get(index)
            .ok_or("that track is not there")?;
        if !self.is_vocal_channel_id(id) {
            return Err("that track is not a vocal track".to_string());
        }
        let song = files::song_of_channel(&self.project, id);
        if song.parts.is_empty() {
            return Err("there are no notes on that track to export".to_string());
        }
        let bytes = match format {
            VocalFileFormat::Midi => files::midi::write(&song),
            VocalFileFormat::Ustx => files::ustx::write(&song).into_bytes(),
        };
        std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(format!(
            "Exported \u{201c}{}\u{201d} as {} \u{2014} {}",
            song.name,
            format.label(),
            path.display()
        ))
    }

    /// [`export_vocal_to`](Self::export_vocal_to) at a place the desktop's
    /// picker asks for (beside the project when there is no picker).
    pub(super) fn export_vocal_chosen(
        &mut self,
        index: usize,
        format: VocalFileFormat,
    ) -> Result<String, String> {
        let name = self
            .channel_ids()
            .get(index)
            .and_then(|id| self.project.channels.get(*id))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "vocal".into());
        let file = format!("{name}.{}", format.extension());
        let start = self.settings.projects_dir.clone().or_else(|| {
            self.bundle
                .as_ref()
                .and_then(|b| b.parent().map(Path::to_path_buf))
        });
        let path = match crate::desktop::choose_save_file(
            &format!("Export vocal track as {}", format.label()),
            &file,
            start.as_deref(),
        ) {
            Ok(Some(mut path)) => {
                if path.extension().is_none() {
                    path.set_extension(format.extension());
                }
                path
            }
            Ok(None) => return Ok("Export cancelled".to_string()),
            Err(_) => start.unwrap_or_else(std::env::temp_dir).join(&file),
        };
        self.export_vocal_to(index, format, &path)
    }

    /// Asks for a vocal song file and imports it — into vocal track
    /// `channel` when given.
    pub(super) fn choose_vocal_file(
        &mut self,
        channel: Option<usize>,
    ) -> Result<Option<String>, String> {
        let start = self.settings.projects_dir.clone();
        let Some(path) = crate::desktop::choose_open_file(
            "Import a vocal song (UST, USTX, VSQX, MIDI, MusicXML)",
            start.as_deref(),
            "*.ust *.ustx *.vsqx *.mid *.midi *.musicxml *.xml *.mxl",
        )?
        else {
            return Ok(None);
        };
        let at = self
            .project
            .loop_range
            .map(|(from, _)| self.project.tempo_map.tick_to_sample(from.max(0)))
            .unwrap_or(0);
        self.import_vocal_file(&path, at, None, channel).map(Some)
    }

    /// Whether rack row `index` is a frozen vocal track.
    pub fn vocal_is_frozen(&self, index: usize) -> bool {
        self.channel_ids()
            .get(index)
            .is_some_and(|id| self.project.is_frozen(*id))
    }

    /// Everything of a freeze that needs the session.
    pub(super) fn prepare_freeze(
        &mut self,
        index: usize,
    ) -> Result<(VocalBounce, AfterBounce), String> {
        let bundle = self
            .bundle
            .clone()
            .ok_or_else(|| "save this project first — a freeze goes inside it".to_string())?;
        let id = *self
            .channel_ids()
            .get(index)
            .ok_or("that track is not there")?;
        if !self.is_vocal_channel_id(id) {
            return Err("only a vocal track freezes".to_string());
        }
        if self.project.is_frozen(id) {
            return Err("that track is already frozen".to_string());
        }
        let channel = self.project.channels[id].clone();
        let (player, prep, from, to) =
            self.vocal.render_prep_for(id, &|_| true).ok_or_else(|| {
                self.vocal
                    .problem_of(id)
                    .unwrap_or_else(|| "there is nothing on that track to freeze".to_string())
            })?;
        let rate = self.options.sample_rate;
        let from = ((from - FREEZE_LEAD_SECONDS).max(0.0) * f64::from(rate)).round() as i64;
        let to = ((to + FREEZE_TAIL_SECONDS) * f64::from(rate)).round() as i64;
        let renders = bundle.join("renders");
        std::fs::create_dir_all(&renders).map_err(|e| format!("{}: {e}", renders.display()))?;
        let name = channel.name.clone();
        let path = free_render_path(&renders, &format!("{name} (frozen)"));
        // Under the last row the track's clips are on.
        let rows = self.lane_ids();
        let row = rows
            .iter()
            .rposition(|lane| {
                self.project.clips.values().any(|c| {
                    c.lane == *lane && matches!(&c.source, ClipSource::Notes(d) if sings_in(d, id))
                })
            })
            .map_or(rows.len(), |r| r + 1);
        Ok((
            VocalBounce {
                prep,
                player,
                gain_db: channel.gain_db,
                pan: channel.pan,
                from,
                to,
                sample_rate: rate,
                path,
            },
            AfterBounce::Vocal(VocalAfter::Freeze {
                channel: id,
                at: from,
                row,
                name,
            }),
        ))
    }

    /// Everything of a selection's render that needs the session: the open
    /// clip's selected notes (all of them when none is), the phrases they
    /// are sung in.
    pub(super) fn prepare_render_selection(
        &mut self,
    ) -> Result<(VocalBounce, AfterBounce), String> {
        let bundle = self
            .bundle
            .clone()
            .ok_or_else(|| "save this project first — a render goes inside it".to_string())?;
        let open = self.clip;
        let clip = self
            .project
            .clips
            .get(open)
            .cloned()
            .ok_or("open a vocal clip in the piano roll first")?;
        let Some(home) = self.project.note_home(open) else {
            return Err("open a vocal clip in the piano roll first".to_string());
        };
        let data = match home {
            fontelle_model::NoteHome::Prefab(p) => {
                self.project.prefabs.get(p).map(|p| p.source.clone())
            }
            fontelle_model::NoteHome::Clip(_) => Some(clip.source.clone()),
        };
        let Some(ClipSource::Notes(data)) = data else {
            return Err("open a vocal clip in the piano roll first".to_string());
        };
        let Some(id) = self.vocal_in(&data, self.selected_channel_id()) else {
            return Err(
                "nothing in the clip in the piano roll is sung by a vocal track".to_string(),
            );
        };
        let selected: Vec<NoteId> = self
            .vocal
            .selection
            .iter()
            .copied()
            .filter(|n| {
                data.notes
                    .get(*n)
                    .is_some_and(|note| note.channel_or(data.channel) == id)
            })
            .collect();
        let wanted = |r: &fontelle_vocal::NoteRef| {
            r.clip == open && (selected.is_empty() || selected.contains(&r.note))
        };
        let (player, prep, _, _) = self.vocal.render_prep_for(id, &wanted).ok_or_else(|| {
            self.vocal
                .problem_of(id)
                .unwrap_or_else(|| "there is nothing sung in that selection".to_string())
        })?;
        // The stretch: the selected notes' (every pass of a looped clip).
        let tempo = fontelle_model::effective_tempo_map(&self.project);
        let mut span: Option<(Tick, Tick)> = None;
        for (_, n) in data.notes.iter().filter(|(nid, n)| {
            n.channel_or(data.channel) == id && (selected.is_empty() || selected.contains(nid))
        }) {
            let (s, e) = (clip.start + n.start, clip.start + n.start + n.length);
            span = Some(span.map_or((s, e), |(a, b)| (a.min(s), b.max(e))));
        }
        let (from_tick, to_tick) = span.ok_or("there are no notes to render")?;
        let rate = f64::from(self.options.sample_rate);
        let from = ((tempo.tick_to_seconds(from_tick) - SELECTION_LEAD_SECONDS).max(0.0) * rate)
            .round() as i64;
        let to =
            ((tempo.tick_to_seconds(to_tick) + FREEZE_TAIL_SECONDS / 2.0) * rate).round() as i64;
        let channel = self.project.channels[id].clone();
        let name = self
            .clips()
            .into_iter()
            .find(|info| info.id == open)
            .map_or_else(|| channel.name.clone(), |info| info.name);
        let renders = bundle.join("renders");
        std::fs::create_dir_all(&renders).map_err(|e| format!("{}: {e}", renders.display()))?;
        let path = free_render_path(&renders, &name);
        let index = self
            .lane_ids()
            .iter()
            .position(|lane| *lane == clip.lane)
            .ok_or("that clip is on no row")?;
        Ok((
            VocalBounce {
                prep,
                player,
                gain_db: channel.gain_db,
                pan: channel.pan,
                from,
                to,
                sample_rate: self.options.sample_rate,
                path,
            },
            AfterBounce::Vocal(VocalAfter::Row {
                index,
                at: from,
                name,
                track: channel.mixer_track,
                channel: id,
            }),
        ))
    }

    /// Freezes vocal track `index` here and now (waiting for its final
    /// takes on this thread) — the window's own goes through a job.
    pub fn freeze_vocal_now(&mut self, index: usize) -> Result<String, String> {
        let (bounce, then) = self.prepare_freeze(index)?;
        let path = bounce.path.clone();
        let clipped = bounce.run(&mut |_| {}, &AtomicU64::new(0))?;
        self.finish_bounce(then, &path, clipped)
    }

    /// Renders the roll's selection here and now — see
    /// [`freeze_vocal_now`](Self::freeze_vocal_now).
    pub fn render_vocal_selection_now(&mut self) -> Result<String, String> {
        let (bounce, then) = self.prepare_render_selection()?;
        let path = bounce.path.clone();
        let clipped = bounce.run(&mut |_| {}, &AtomicU64::new(0))?;
        self.finish_bounce(then, &path, clipped)
    }

    /// Unfreezes vocal track `index`: the render's clip goes, the part
    /// sings again. One undo step.
    pub fn unfreeze_vocal_now(&mut self, index: usize) -> Result<String, String> {
        let id = *self
            .channel_ids()
            .get(index)
            .ok_or("that track is not there")?;
        self.apply_for::<UnfreezeVocal>(Box::new(UnfreezeVocal::new(id)))?;
        self.let_go();
        self.rebuild_graph();
        self.republish();
        self.touch();
        Ok(format!(
            "Unfroze \u{201c}{}\u{201d} \u{2014} it sings from its part again",
            self.project.channels[id].name
        ))
    }

    /// Sends a vocal render to a thread of its own, the way
    /// [`start_bounce`](Self::start_bounce) does an export.
    pub(super) fn start_vocal_bounce(
        &mut self,
        prepare: impl FnOnce(&mut Self) -> Result<(VocalBounce, AfterBounce), String>,
        verb: &str,
    ) -> Result<String, String> {
        if self.job.is_some() {
            return Err("a render is already running \u{2014} wait for it to finish".to_string());
        }
        let (bounce, then) = prepare(self)?;
        let singing = Arc::new(AtomicU64::new(0));
        let sung = Arc::clone(&singing);
        let path = bounce.path.clone();
        let total = (bounce.to - bounce.from).max(1);
        let done = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&done);
        let worker = std::thread::Builder::new()
            .name("fontelle-vocal-render".to_string())
            .spawn(move || {
                bounce.run(
                    &mut |frames| counter.store(frames, Ordering::Relaxed),
                    &sung,
                )
            })
            .map_err(|e| format!("could not start the render: {e}"))?;
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let label = format!("{verb} {file}");
        self.job = Some(RunningJob {
            label: label.clone(),
            total,
            done,
            path,
            then,
            worker,
            singing,
        });
        Ok(format!("{label}\u{2026}"))
    }

    /// A vocal render's file is written: freeze with it, or put it on its
    /// row.
    pub(super) fn finish_vocal(
        &mut self,
        after: VocalAfter,
        path: &Path,
    ) -> Result<String, String> {
        match after {
            VocalAfter::Freeze {
                channel,
                at,
                row,
                name,
            } => {
                // Asked before the freeze: a frozen track sings nothing.
                let unsung = self.unsung_note(channel);
                let imported = self.library.import_audio(path).map_err(|e| e.to_string())?;
                if imported.frames == 0 || imported.sample_rate == 0 {
                    return Err("the freeze rendered no sound".to_string());
                }
                let mut data = fontelle_types::AudioClipData::whole(
                    imported.asset.clone(),
                    imported.frames as fontelle_types::Sample,
                    imported.sample_rate,
                );
                data.mixer_track = self
                    .project
                    .channels
                    .get(channel)
                    .and_then(|c| c.mixer_track);
                let start = self.project.tempo_map.sample_to_tick(at.max(0)).max(0);
                let length =
                    self.footprint_ticks(imported.frames as u64, imported.sample_rate, start);
                self.apply_for::<FreezeVocal>(Box::new(FreezeVocal::new(
                    channel,
                    format!("{name} (frozen)"),
                    data,
                    start,
                    length,
                    row,
                )))?;
                self.let_go();
                self.rebuild_graph();
                self.republish();
                self.touch();
                Ok(format!(
                    "Froze \u{201c}{name}\u{201d} \u{2014} it plays its render now; its part \
                     is kept, unfreeze to edit it{unsung}"
                ))
            }
            VocalAfter::Row {
                index,
                at,
                name,
                track,
                channel,
            } => {
                self.import_audio_named(
                    path,
                    at,
                    track,
                    Landing::NewRow(index + 1),
                    Some(format!("{name} (rendered)")),
                )?;
                self.let_go();
                self.dirty = true;
                self.republish();
                self.touch();
                Ok(format!(
                    "Rendered \u{2014} {}{}",
                    path.display(),
                    self.unsung_note(channel)
                ))
            }
        }
    }

    /// What a render of vocal track `channel` is missing: "" when every
    /// phrase was sung, else how many are silent in it and why.
    fn unsung_note(&self, channel: ChannelId) -> String {
        match self.vocal.unsung(channel) {
            (0, _) => String::new(),
            (n, why) => format!(
                ". {n} phrase{} could not be sung and {} silent in it: {}",
                if n == 1 { "" } else { "s" },
                if n == 1 { "is" } else { "are" },
                why.unwrap_or_default()
            ),
        }
    }
}
