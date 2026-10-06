//! Analyze Musically's half of the session (`docs/analyze-musically-plan.md`
//! §3, §6.1, P2–P5): what the window is open on, the study it edits, the
//! preview, the renders that run off the window's thread, and the takes an
//! insert or an input records.
//!
//! The window sees none of this: it reads an `AnalyzeView` and hands back
//! whole values (the edits, the clean, the markers, the takes), each one
//! command (INVARIANT 9). What a study *sounds* like is
//! [`crate::analyze::process_study`], pure; here is only who asked for it and
//! where the result goes.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use fontelle_types::{
    AnalyzeConfig, AssetRef, EffectConfig, PersistentId, Study, StudyClean, StudyCompSpan, StudyId,
    StudyMarker, StudySource, StudyTake,
};
use fontelle_ui::canvas::{
    AnalyzeRecordOp, AnalyzeRecordView, AnalyzeSliceKey, AnalyzeSliceLayout, AnalyzeSource,
    AnalyzeStudyRow, AnalyzeTakeOp,
};

use super::*;

/// What the Analyze Musically window is open on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StudyTarget {
    /// An arrangement clip: the span it plays.
    Clip(ClipId),
    /// A study with no clip: a recording, a file, an insert's whose effect
    /// has gone.
    Study(StudyId),
    /// The Analyze Musically insert in `slot` of `track` (Ty, plan §6.1).
    Insert { track: MixerTrackId, slot: usize },
    /// *Record into Analyze Musically…*: a study that is made by its first
    /// take, found by this key until then.
    Recording(PersistentId),
}

/// Where a render's result goes once it is done.
#[derive(Debug, Clone)]
pub(crate) enum RenderPurpose {
    /// Render to clip: the clip's audio replaced (or a new clip below).
    Clip { clip: ClipId, below: bool },
    /// Send to arrangement: a new clip at this song sample.
    Arrangement { at: Sample },
    /// Send to sampler: cut at these frames of the render.
    Sampler {
        cuts: Vec<i64>,
        layout: AnalyzeSliceLayout,
        replay: Option<Tick>,
    },
}

/// A render running off the window's thread (plan §3.7: *"renders
/// off-thread through the job strip"*).
pub(crate) struct RenderJob {
    worker: Option<std::thread::JoinHandle<Vec<f32>>>,
    progress: crate::analyze::Progress,
    study: StudyId,
    name: String,
    channels: u16,
    rate: u32,
    trim: Option<(Sample, Sample)>,
    purpose: RenderPurpose,
}

impl RenderJob {
    pub(crate) fn fraction(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }
}

/// A study's recorder: the insert's capture drained into takes, or an input
/// device's blocks.
pub(crate) struct Recorder {
    /// `None` records the insert's own track.
    pub input: Option<String>,
    /// The input devices, asked once when the recorder was made.
    pub inputs: Vec<String>,
    insert_writer: Option<crate::insert_takes::InsertTakeWriter>,
    input_writer: Option<crate::insert_takes::InputTakeWriter>,
    /// A stream this recorder opened itself (no track had the device open).
    _device: Option<fontelle_engine::AudioDevice>,
    /// The arm settings when there is no insert to keep them.
    config: AnalyzeConfig,
    /// Why it cannot record, said on the card.
    pub problem: Option<String>,
}

impl Recorder {
    fn new(inputs: Vec<String>, input: Option<String>) -> Self {
        Self {
            input,
            inputs,
            insert_writer: None,
            input_writer: None,
            _device: None,
            config: AnalyzeConfig::new(),
            problem: None,
        }
    }

    /// The takes finished since the last poll.
    fn poll(&self) -> Vec<Result<crate::insert_takes::InsertTake, String>> {
        let mut out = Vec::new();
        if let Some(w) = &self.insert_writer {
            out.extend(w.poll());
        }
        if let Some(w) = &self.input_writer {
            out.extend(w.poll());
        }
        out
    }

    /// Stops both writers, closing a take still running; what they held.
    fn stop(&mut self) -> Vec<Result<crate::insert_takes::InsertTake, String>> {
        let mut out = Vec::new();
        if let Some(w) = self.insert_writer.take() {
            out.extend(w.stop());
        }
        if let Some(w) = self.input_writer.take() {
            out.extend(w.stop());
        }
        self._device = None;
        out
    }
}

/// The frames of `len` a list of cut points makes slices of, within `span`.
pub(crate) fn slices_within(
    cuts: &[i64],
    span: (i64, i64),
) -> Vec<fontelle_analysis::slice::Slice> {
    let (a, b) = (span.0.max(0), span.1.max(span.0.max(0)));
    let mut edges: Vec<i64> = std::iter::once(a)
        .chain(cuts.iter().copied().filter(|c| *c > a && *c < b))
        .chain(std::iter::once(b))
        .collect();
    edges.sort_unstable();
    edges.dedup();
    edges
        .windows(2)
        .filter(|w| w[1] > w[0])
        .map(|w| fontelle_analysis::slice::Slice {
            start: w[0] as usize,
            end: w[1] as usize,
            pitch: None,
        })
        .collect()
}

fn layout_of(layout: AnalyzeSliceLayout) -> fontelle_analysis::slice::SliceLayout {
    match layout {
        AnalyzeSliceLayout::Chop => fontelle_analysis::slice::SliceLayout::Chop,
        AnalyzeSliceLayout::ByPitch => fontelle_analysis::slice::SliceLayout::ByPitch,
        AnalyzeSliceLayout::DrumMap => fontelle_analysis::slice::SliceLayout::DrumMap,
    }
}

impl Session {
    // ------------------------------------------------------- the target ---

    /// The Analyze Musically insert's settings in `slot` of `track`.
    fn analyze_config(&self, track: MixerTrackId, slot: usize) -> Option<AnalyzeConfig> {
        match &self
            .project
            .mixer
            .tracks
            .get(track)?
            .inserts
            .get(slot)?
            .config
        {
            EffectConfig::Analyze(config) => Some(*config),
            _ => None,
        }
    }

    /// The study found by a recorder's key.
    fn study_by_key(&self, key: PersistentId) -> Option<StudyId> {
        self.project
            .studies
            .iter()
            .find(|(_, s)| s.insert == Some(key))
            .map(|(id, _)| id)
    }

    /// The study of `clip`, if anything has been done to it.
    pub(super) fn study_of(&self, clip: ClipId) -> Option<StudyId> {
        self.project
            .studies
            .iter()
            .find(|(_, s)| s.source == StudySource::Clip(clip))
            .map(|(id, _)| id)
    }

    /// The study the window edits, when there is one yet.
    pub(super) fn open_study_id(&self) -> Option<StudyId> {
        match self.analysis_target? {
            StudyTarget::Clip(clip) => self.study_of(clip),
            StudyTarget::Study(id) => self.project.studies.contains_key(id).then_some(id),
            StudyTarget::Insert { track, slot } => {
                self.study_by_key(self.analyze_config(track, slot)?.study?)
            }
            StudyTarget::Recording(key) => self.study_by_key(key),
        }
    }

    fn open_study(&self) -> Option<&Study> {
        self.project.studies.get(self.open_study_id()?)
    }

    /// The audio the lane shows: the asset and the span of it.
    fn target_audio(&self) -> Option<(AssetRef, i64, Option<i64>, String)> {
        match self.analysis_target? {
            StudyTarget::Clip(clip) => {
                let ClipSource::Audio(data) = &self.project.clips.get(clip)?.source else {
                    return None;
                };
                let mut asset = data.asset.clone();
                // A clip playing a render of its study's edits is studied as
                // it was recorded (plan §3.7).
                if let Some(study) = self
                    .study_of(clip)
                    .and_then(|s| self.project.studies.get(s))
                    && study.rendered.as_ref() == Some(&data.asset)
                {
                    asset = study.original.clone();
                }
                let name = self
                    .clips()
                    .into_iter()
                    .find(|info| info.id == clip)
                    .map_or_else(|| "Audio".to_string(), |info| info.name);
                Some((asset, data.source_start, Some(data.source_end), name))
            }
            _ => {
                let study = self.open_study()?;
                Some((study.original.clone(), 0, None, study.name.clone()))
            }
        }
    }

    /// What the window is called while there is no audio yet.
    fn target_name(&self) -> String {
        match self.analysis_target {
            Some(StudyTarget::Insert { track, .. }) => self
                .project
                .mixer
                .tracks
                .get(track)
                .map_or_else(|| "Recording".to_string(), |t| t.name.clone()),
            Some(StudyTarget::Recording(_)) => "Recording".to_string(),
            _ => self
                .open_study()
                .map_or_else(|| "Audio".to_string(), |s| s.name.clone()),
        }
    }

    /// Opens Analyze Musically on `clip`: refused for anything but audio,
    /// and for audio still loading.
    pub(super) fn open_analysis(&mut self, id: ClipId) -> Result<String, String> {
        let clip = self.project.clips.get(id).ok_or("that clip is not there")?;
        if !matches!(clip.source, ClipSource::Audio(_)) {
            return Err("only an audio clip can be analysed musically".to_string());
        }
        self.retarget(StudyTarget::Clip(id));
        self.open_audio()
    }

    /// The window on another target: what was open is let go.
    fn retarget(&mut self, target: StudyTarget) {
        self.close_analysis_audio();
        self.stop_recorder();
        self.analysis_target = Some(target);
    }

    /// (Re)opens the analysis on the target's audio: the clip's span, or
    /// the study's whole file. No audio (a recording before its first take)
    /// is no analysis, and not an error.
    fn open_audio(&mut self) -> Result<String, String> {
        self.close_analysis_audio();
        let Some((asset, from, to, name)) = self.target_audio() else {
            return Ok(format!("Analyze Musically \u{2014} {}", self.target_name()));
        };
        let buffer = self
            .library
            .audio_store()
            .get(asset.id)
            .cloned()
            .ok_or("that audio is still loading \u{2014} try again in a moment")?;
        let from = from.max(0) as usize;
        let to = to.map_or(buffer.frames(), |t| {
            (t.max(0) as usize).min(buffer.frames())
        });
        if to <= from {
            return Err(format!("there is no sound in \u{201c}{name}\u{201d}"));
        }
        let mono = crate::analyze::mono_span(&buffer, from, to);
        let span = (asset.id, from as i64, to as i64);
        let cache = crate::analyze::cache_dir_here(self.bundle.as_deref());
        let known = self.analysis_keys.get(&span).cloned();
        let open = crate::analyze::OpenAnalysis::start(
            name.clone(),
            span,
            mono,
            buffer.sample_rate,
            cache,
            known,
        );
        let said = if open.finished().is_some() {
            format!("Analyze Musically \u{2014} \u{201c}{name}\u{201d}")
        } else {
            format!("Analyze Musically: analysing \u{201c}{name}\u{201d}\u{2026}")
        };
        self.analysis = Some(open);
        self.study_player.stop();
        self.study_player.set_original(false);
        self.preview = Some(crate::analyze::Preview::new(&buffer));
        self.service_preview();
        Ok(said)
    }

    /// The analysis and the preview let go (the target kept).
    fn close_analysis_audio(&mut self) {
        self.study_player.stop();
        self.preview = None;
        if let Some(open) = self.analysis.take()
            && let Some(key) = open.key()
            && open.finished().is_some()
        {
            self.analysis_keys.insert(open.span, key);
        }
    }

    /// The window closed: everything let go.
    pub(super) fn close_study_window(&mut self) {
        self.close_analysis_audio();
        self.stop_recorder();
        self.analysis_target = None;
        self.listen_removed = false;
        self.sweep_takes();
    }

    /// The lane's audio changed under the analysis (a take loaded, an undo
    /// of one): it is analysed again.
    fn follow_target_audio(&mut self) {
        let wanted = self
            .target_audio()
            .map(|(asset, from, _, _)| (asset.id, from));
        let open = self.analysis.as_ref().map(|o| (o.span.0, o.span.1));
        if wanted != open {
            let _ = self.open_audio();
        }
    }

    // ------------------------------------------------------ the preview ---

    /// The preview follows the song's study (an undo is an edit too).
    pub(super) fn service_preview(&mut self) {
        let want = match self.open_study() {
            Some(study) => crate::analyze::Want {
                edits: study.pitch_edits.clone(),
                clean: study.clean.clone(),
                removed: self.listen_removed && study.clean.denoise.active(),
            },
            None => crate::analyze::Want::default(),
        };
        if let Some(preview) = &mut self.preview {
            preview.want(want);
            preview.service(&self.study_player);
        }
    }

    /// Seconds into the analysed audio, as a frame of its file.
    pub(super) fn analysis_frame(&self, seconds: f64) -> Option<i64> {
        let open = self.analysis.as_ref()?;
        let rate = self.preview.as_ref()?.rate;
        Some(open.span.1 + (seconds.max(0.0) * f64::from(rate)).round() as i64)
    }

    // -------------------------------------------------- one command each ---

    /// Runs `command`, refused with the reason rather than a status line.
    fn try_run(&mut self, command: Box<dyn Command>) -> Result<(), String> {
        self.history
            .apply(command, &mut self.project)
            .map_err(|e| e.to_string())?;
        self.dirty = true;
        if self.project.lane_routing.mode == fontelle_model::RoutingMode::Lane {
            self.lanes_unsettled = Some((self.history.generation(), std::time::Instant::now()));
        }
        self.republish();
        self.touch();
        Ok(())
    }

    /// A change to the open study: `existing` makes the command for a study
    /// that is there; on a clip with none yet, `fresh` is done to a new one
    /// and it is started (`AddStudy`, into which the rest of a drag merges).
    fn change_study(
        &mut self,
        merge: bool,
        existing: impl FnOnce(StudyId) -> Box<dyn Command>,
        fresh: impl FnOnce(&mut Study),
    ) -> Result<(), String> {
        if !merge {
            self.history.break_gesture();
        }
        let command = match self.open_study_id() {
            Some(id) => existing(id),
            None => {
                let Some(StudyTarget::Clip(clip)) = self.analysis_target else {
                    return Err(
                        "record a take first \u{2014} there is no audio here yet".to_string()
                    );
                };
                let Some(ClipSource::Audio(data)) = self.project.clips.get(clip).map(|c| &c.source)
                else {
                    return Err("only an audio clip can be studied".to_string());
                };
                let name = self
                    .analysis
                    .as_ref()
                    .map_or_else(|| "Audio".to_string(), |o| o.name.clone());
                let mut study = Study::new(name, StudySource::Clip(clip), data.asset.clone());
                fresh(&mut study);
                Box::new(fontelle_model::AddStudy::new(study))
            }
        };
        self.try_run(command)?;
        self.service_preview();
        Ok(())
    }

    pub(super) fn set_study_edits(
        &mut self,
        changes: &[fontelle_ui::canvas::AnalyzeEditChange],
        merge: bool,
    ) -> Result<String, String> {
        if self.analysis.is_none() {
            return Err("nothing is open in Analyze Musically".to_string());
        }
        let mut edits = self
            .open_study()
            .map(|s| s.pitch_edits.clone())
            .unwrap_or_default();
        for change in changes {
            let (Some(a), Some(b)) = (
                self.analysis_frame(change.start),
                self.analysis_frame(change.end),
            ) else {
                continue;
            };
            edits.retain(|e| !e.overlaps((a, b)));
            if let Some(edit) = change.edit.filter(|e| !e.is_identity()) {
                edits.push(crate::analyze::pitch_edit((a, b), &edit));
            }
        }
        edits.sort_by_key(|e| e.span.0);
        let fresh = edits.clone();
        self.change_study(
            merge,
            |id| Box::new(fontelle_model::SetStudyEdits::new(id, edits)),
            |s| s.pitch_edits = fresh,
        )?;
        Ok(String::new())
    }

    pub(super) fn set_study_clean(
        &mut self,
        clean: StudyClean,
        merge: bool,
    ) -> Result<String, String> {
        if self.analysis.is_none() {
            return Err("nothing is open in Analyze Musically".to_string());
        }
        let fresh = clean.clone();
        self.change_study(
            merge,
            |id| Box::new(fontelle_model::SetStudyClean::new(id, clean)),
            |s| s.clean = fresh,
        )?;
        Ok(String::new())
    }

    pub(super) fn set_study_markers(
        &mut self,
        mut markers: Vec<StudyMarker>,
        merge: bool,
    ) -> Result<String, String> {
        if self.analysis.is_none() {
            return Err("nothing is open in Analyze Musically".to_string());
        }
        markers.sort_by_key(|m| (m.at, m.id));
        let fresh = markers.clone();
        self.change_study(
            merge,
            |id| Box::new(fontelle_model::SetStudyMarkers::new(id, markers)),
            |s| s.markers = fresh,
        )?;
        Ok(String::new())
    }

    /// The Noise tool: the span `from..to` seconds captured as the profile,
    /// and the denoiser switched on with it.
    pub(super) fn capture_study_noise(&mut self, from: f64, to: f64) -> Result<String, String> {
        let (a, b) = (
            self.analysis_frame(from.min(to)).ok_or("nothing is open")?,
            self.analysis_frame(from.max(to)).ok_or("nothing is open")?,
        );
        let preview = self.preview.as_ref().ok_or("nothing is open")?;
        let noise = crate::analyze::capture_noise(
            &preview.original,
            preview.channels,
            preview.rate,
            a.max(0) as usize,
            b.max(0) as usize,
        )?;
        let level = noise.level_db;
        let mut clean = self
            .open_study()
            .map(|s| s.clean.clone())
            .unwrap_or_default();
        clean.denoise.noise = Some(noise);
        clean.denoise.on = true;
        self.set_study_clean(clean, false)?;
        self.let_go();
        Ok(format!(
            "Noise captured: {} dB \u{2014} the Denoise card takes it out",
            level.round() as i32
        ))
    }

    pub(super) fn set_listen_removed(&mut self, on: bool) {
        self.listen_removed = on;
        self.service_preview();
    }

    // ------------------------------------------------------ the renders ---

    /// Starts a render of the study's audio, through its clean and edits,
    /// off the window's thread; where it goes is `purpose`.
    fn start_render(&mut self, purpose: RenderPurpose) -> Result<String, String> {
        if self.render_job.is_some() {
            return Err("a render is already running \u{2014} one moment".to_string());
        }
        let study_id = self
            .open_study_id()
            .ok_or("move a note or clean something first \u{2014} nothing is done yet")?;
        let study = self.project.studies[study_id].clone();
        let buffer = self
            .library
            .audio_store()
            .get(study.original.id)
            .cloned()
            .ok_or("the original audio is not loaded")?;
        let progress: crate::analyze::Progress =
            Arc::new(std::sync::atomic::AtomicU32::new(0.0f32.to_bits()));
        let (original, channels, rate) = (
            Arc::clone(&buffer.data),
            usize::from(buffer.channels.max(1)),
            buffer.sample_rate,
        );
        let (edits, clean, report) = (
            study.pitch_edits.clone(),
            study.clean.clone(),
            Arc::clone(&progress),
        );
        let worker = std::thread::Builder::new()
            .name("fontelle-study-render".to_string())
            .spawn(move || {
                crate::analyze::process_study(
                    &original,
                    channels,
                    rate,
                    &edits,
                    &clean,
                    false,
                    Some(&report),
                )
            })
            .map_err(|e| format!("could not start the render: {e}"))?;
        let label = match &purpose {
            RenderPurpose::Clip { .. } => "Rendering",
            RenderPurpose::Arrangement { .. } => "Sending to the arrangement",
            RenderPurpose::Sampler { .. } => "Slicing",
        };
        self.render_job = Some(RenderJob {
            worker: Some(worker),
            progress,
            study: study_id,
            name: study.name.clone(),
            channels: buffer.channels.max(1),
            rate,
            trim: study.clean.trim,
            purpose,
        });
        self.touch();
        Ok(format!("{label} \u{201c}{}\u{201d}\u{2026}", study.name))
    }

    /// Render to clip (plan §3.7), or as a new clip below.
    pub(super) fn render_study(&mut self, below: bool) -> Result<String, String> {
        let Some(StudyTarget::Clip(clip)) = self.analysis_target else {
            return Err(
                "this audio has no clip \u{2014} Send to arrangement makes one".to_string(),
            );
        };
        let study = self
            .open_study()
            .ok_or("move a note or clean something first \u{2014} nothing is done yet")?;
        if study.pitch_edits.is_empty() && study.clean.is_identity() {
            return Err(
                "move a note or clean something first \u{2014} nothing is done yet".to_string(),
            );
        }
        self.start_render(RenderPurpose::Clip { clip, below })
    }

    /// Where a render goes: `<bundle>/renders/<name> (<what> N).wav`, the
    /// first N free. A song not saved yet is saved first, as a take does.
    fn render_path(&mut self, name: &str, what: &str) -> Result<PathBuf, String> {
        let dir = self.make_real()?.join("renders");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for n in 1..10_000 {
            let path = dir.join(format!("{name} ({what} {n}).wav"));
            if !path.exists() {
                return Ok(path);
            }
        }
        Err("the renders folder is full".to_string())
    }

    /// How the render is getting on, and — once — how it went; where it
    /// went is done here, one undo.
    pub(super) fn poll_study_render(&mut self) -> JobPoll {
        let Some(job) = &self.render_job else {
            return JobPoll::Idle;
        };
        if !job.worker.as_ref().is_none_or(|w| w.is_finished()) {
            return JobPoll::Running(JobProgress {
                label: format!("Rendering \u{201c}{}\u{201d}", job.name),
                fraction: Some(job.fraction()),
            });
        }
        let mut job = self.render_job.take().expect("checked");
        let result = match job.worker.take().map(|w| w.join()) {
            Some(Ok(audio)) => self.land_render(&job, &audio),
            _ => Err("the render stopped".to_string()),
        };
        self.touch();
        JobPoll::Finished(result)
    }

    fn land_render(&mut self, job: &RenderJob, audio: &[f32]) -> Result<String, String> {
        let what = match job.purpose {
            RenderPurpose::Clip { .. } => "edited",
            RenderPurpose::Arrangement { .. } => "sent",
            RenderPurpose::Sampler { .. } => "slices",
        };
        let path = self.render_path(&job.name, what)?;
        let mut writer = fontelle_assets::WavWriter::create(&path, job.rate, job.channels)
            .map_err(|e| e.to_string())?;
        writer
            .write(audio)
            .and_then(|()| writer.finish())
            .map_err(|e| e.to_string())?;
        let file = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let frames = (audio.len() / usize::from(job.channels.max(1))) as i64;
        let span = job.trim.map_or((0, frames), |(a, b)| {
            (a.clamp(0, frames), b.clamp(a.clamp(0, frames), frames))
        });
        let name = job.name.clone();
        match job.purpose.clone() {
            RenderPurpose::Clip { clip, below } => {
                self.land_clip_render(&path, job.study, clip, below, span)?;
                Ok(if below {
                    format!(
                        "Rendered to \u{201c}{file}\u{201d}, on a new row under \u{201c}{name}\u{201d}"
                    )
                } else {
                    format!(
                        "\u{201c}{name}\u{201d} plays your edits ({file}) \u{2014} Revert to original undoes it"
                    )
                })
            }
            RenderPurpose::Arrangement { at } => {
                self.history.break_gesture();
                self.import_audio_span(&path, at, span)?;
                self.let_go();
                Ok(format!(
                    "\u{201c}{name}\u{201d} is on the arrangement ({file})"
                ))
            }
            RenderPurpose::Sampler {
                cuts,
                layout,
                replay,
            } => {
                let slices = slices_within(&cuts, span);
                if slices.is_empty() {
                    return Err("there is nothing between the cuts to slice".to_string());
                }
                self.history.break_gesture();
                let added = self.add_sampler_slices(&path, &slices, layout_of(layout), replay)?;
                let mut said = format!(
                    "{} slices on \u{201c}{}\u{201d}, a new Sampler",
                    slices.len(),
                    added.name
                );
                if added.unmapped > 0 {
                    said.push_str(&format!(
                        " ({} slice(s) had no key to go on)",
                        added.unmapped
                    ));
                }
                Ok(said)
            }
        }
    }

    /// The clip on the render: its audio swapped, and with a trim its span,
    /// its place and its length with it — one compound with the study's
    /// stamp; or the same as a new clip on the row under.
    fn land_clip_render(
        &mut self,
        path: &Path,
        study: StudyId,
        clip_id: ClipId,
        below: bool,
        span: (i64, i64),
    ) -> Result<(), String> {
        let clip = self
            .project
            .clips
            .get(clip_id)
            .cloned()
            .ok_or("the analysed clip is not there any more")?;
        let ClipSource::Audio(data) = &clip.source else {
            return Err("only an audio clip can take a render".to_string());
        };
        let imported = self.library.import_audio(path).map_err(|e| e.to_string())?;
        let mut swapped = data.clone();
        swapped.asset = imported.asset.clone();
        let mut placed = clip.clone();
        let trimmed = self
            .project
            .studies
            .get(study)
            .and_then(|s| s.clean.trim)
            .is_some();
        if trimmed {
            // The trim is the clip's span: its start moves with the head cut
            // off, so what is left plays where it played.
            let rate = f64::from(data.sample_rate.max(1));
            let song = f64::from(self.options.sample_rate);
            let speed = data.speed.max(1e-6);
            let start_sample = self.project.tempo_map.tick_to_sample(clip.start)
                + (((span.0 - data.source_start) as f64 / rate / speed) * song).round() as i64;
            placed.start = self.project.tempo_map.sample_to_tick(start_sample).max(0);
            let frames = (span.1 - span.0).max(0) as f64 / speed;
            placed.length = self.footprint_ticks(frames as u64, data.sample_rate, placed.start);
            swapped.source_start = span.0;
            swapped.source_end = span.1;
        }
        placed.source = ClipSource::Audio(swapped.clone());
        let name = self.project.studies[study].name.clone();
        let command: Box<dyn Command> = if below {
            let row = self
                .lane_ids()
                .iter()
                .position(|lane| *lane == clip.lane)
                .map_or(0, |row| row + 1);
            Box::new(
                fontelle_model::AddAudioClip::new(
                    format!("{name} (edited)"),
                    swapped,
                    placed.start,
                    placed.length,
                )
                .at_row(row),
            )
        } else {
            Box::new(fontelle_model::Compound::new(
                format!("Render edits to {name}"),
                vec![
                    Box::new(fontelle_model::ReplaceClip::new(clip_id, placed)),
                    Box::new(fontelle_model::SetStudyRender::new(
                        study,
                        Some(imported.asset),
                    )),
                ],
            ))
        };
        self.history.break_gesture();
        self.try_run(command)?;
        self.let_go();
        self.rebuild_graph();
        Ok(())
    }

    /// A file onto the arrangement as one clip playing `span` of it, its
    /// first frame at song sample `at`.
    fn import_audio_span(
        &mut self,
        path: &Path,
        at: Sample,
        span: (i64, i64),
    ) -> Result<(), String> {
        let name = file_label(path);
        let imported = self.library.import_audio(path).map_err(|e| e.to_string())?;
        if imported.frames == 0 || imported.sample_rate == 0 {
            return Err(format!("there is no sound in {name}"));
        }
        let (a, b) = (
            span.0.clamp(0, imported.frames as i64),
            span.1.clamp(0, imported.frames as i64),
        );
        // Where frame `a` was heard: the take's song position moved on by
        // the head the trim cut off.
        let head = (a as f64 * f64::from(self.options.sample_rate)
            / f64::from(imported.sample_rate))
        .round() as i64;
        let start = self
            .project
            .tempo_map
            .sample_to_tick((at + head).max(0))
            .max(0);
        let length = self.footprint_ticks((b - a).max(1) as u64, imported.sample_rate, start);
        let mut data = fontelle_types::AudioClipData::whole(
            imported.asset.clone(),
            imported.frames as Sample,
            imported.sample_rate,
        );
        data.source_start = a;
        data.source_end = b.max(a + 1);
        let clip = fontelle_model::AddAudioClip::new(name, data, start, length);
        let clip = match self.selected_lane_id() {
            Some(lane) if self.lane_is_free(lane, start, start + length) => clip.on_lane(lane),
            _ => clip.at_row(self.arrival_index()),
        };
        self.try_run(Box::new(clip))?;
        self.rebuild_graph();
        self.collect_if_shared();
        Ok(())
    }

    pub(super) fn revert_study(&mut self) -> Result<String, String> {
        let Some(StudyTarget::Clip(clip)) = self.analysis_target else {
            return Err("this audio has no clip to revert".to_string());
        };
        let id = self.study_of(clip).ok_or("this clip has no edits")?;
        let study = self.project.studies[id].clone();
        let name = study.name.clone();
        let Some(ClipSource::Audio(data)) = self.project.clips.get(clip).map(|c| &c.source) else {
            return Err("the analysed clip is not there any more".to_string());
        };
        if study.rendered.is_none() || study.rendered.as_ref() != Some(&data.asset) {
            return Err(format!("\u{201c}{name}\u{201d} already plays the original"));
        }
        let mut back = data.clone();
        back.asset = study.original.clone();
        self.history.break_gesture();
        self.run(Box::new(fontelle_model::Compound::new(
            format!("Revert {name} to the original"),
            vec![
                Box::new(fontelle_model::SetAudioClip::new(clip, back)),
                Box::new(fontelle_model::SetStudyRender::new(id, None)),
            ],
        )));
        self.let_go();
        self.rebuild_graph();
        Ok(format!(
            "\u{201c}{name}\u{201d} plays the original again (your edits are kept)"
        ))
    }

    // ------------------------------------------------------- the slices ---

    /// Where the slices cut at `cuts` would land, for the keyboard preview.
    pub(super) fn study_slice_keys(
        &mut self,
        cuts: &[f64],
        layout: AnalyzeSliceLayout,
    ) -> Vec<AnalyzeSliceKey> {
        let frames: Vec<i64> = cuts
            .iter()
            .filter_map(|c| self.analysis_frame(*c))
            .collect();
        if let Some((held, how, keys)) = &self.slice_keys_memo
            && *held == frames
            && *how == layout
        {
            return keys.clone();
        }
        let Some(preview) = &self.preview else {
            return Vec::new();
        };
        let channels = preview.channels.max(1);
        let mono: Vec<f32> = preview
            .original
            .chunks(channels)
            .map(|f| f.iter().sum::<f32>() / channels as f32)
            .collect();
        let span = self.slice_span(mono.len() as i64);
        let slices = slices_within(&frames, span);
        let laid = fontelle_analysis::slice::layout_slices(
            &mono,
            preview.rate,
            &slices,
            layout_of(layout),
        );
        let keys: Vec<AnalyzeSliceKey> = laid
            .zones
            .iter()
            .map(|z| AnalyzeSliceKey {
                slice: z.slice,
                low: z.key_range.0,
                high: z.key_range.1,
                root: z.root_key,
            })
            .collect();
        self.slice_keys_memo = Some((frames, layout, keys.clone()));
        keys
    }

    /// The frames slicing covers: the trim, or the span the lane shows.
    fn slice_span(&self, frames: i64) -> (i64, i64) {
        if let Some(trim) = self.open_study().and_then(|s| s.clean.trim) {
            return trim;
        }
        match &self.analysis {
            Some(open) => (open.span.1, open.span.2),
            None => (0, frames),
        }
    }

    pub(super) fn send_study_to_sampler(
        &mut self,
        cuts: &[f64],
        layout: AnalyzeSliceLayout,
        replay: bool,
        playhead: Sample,
    ) -> Result<String, String> {
        if self.analysis.is_none() {
            return Err("nothing is open in Analyze Musically".to_string());
        }
        let frames: Vec<i64> = cuts
            .iter()
            .filter_map(|c| self.analysis_frame(*c))
            .collect();
        // Nothing done to it yet is still something to slice: the study is
        // started so the render has its audio.
        if self.open_study_id().is_none() {
            self.set_study_markers(Vec::new(), false)?;
            self.let_go();
        }
        let mut cut_frames = frames;
        // Slices are cut within the span the lane shows (or the trim).
        let span = self.slice_span(i64::MAX);
        if self.open_study().and_then(|s| s.clean.trim).is_none() {
            cut_frames.push(span.0);
        }
        let replay_at = replay.then(|| match self.analysis_target {
            Some(StudyTarget::Clip(clip)) => self.project.clips.get(clip).map_or(0, |c| c.start),
            _ => self.project.tempo_map.sample_to_tick(playhead.max(0)),
        });
        let mut said = self.start_render(RenderPurpose::Sampler {
            cuts: cut_frames,
            layout,
            replay: replay_at,
        })?;
        let untrimmed = self.open_study().and_then(|s| s.clean.trim).is_none();
        if let Some(job) = &mut self.render_job
            && untrimmed
        {
            // The clip's own span stands in for the trim.
            job.trim = Some(span);
        }
        said.push_str(" the slices go on a new Sampler");
        Ok(said)
    }

    // --------------------------------------------------------- recording ---

    /// The insert the window is open on, if it is one.
    fn target_insert(&self) -> Option<(MixerTrackId, usize)> {
        match self.analysis_target? {
            StudyTarget::Insert { track, slot } => Some((track, slot)),
            _ => None,
        }
    }

    /// Opens the window on the Analyze Musically insert in `slot` of mixer
    /// strip `strip`: the first time, the insert is given the key of the
    /// study its takes will make.
    pub(super) fn open_insert_study(
        &mut self,
        strip: usize,
        slot: usize,
    ) -> Result<String, String> {
        let track = self
            .mixer_track_ids()
            .get(strip)
            .copied()
            .ok_or("that mixer track is not there")?;
        let config = self
            .analyze_config(track, slot)
            .ok_or("that insert is not Analyze Musically")?;
        if config.study.is_none() {
            let bound = AnalyzeConfig {
                study: Some(PersistentId::new()),
                ..config
            };
            self.history.break_gesture();
            self.try_run(Box::new(fontelle_model::RestoreInsertConfig::new(
                track,
                slot,
                EffectConfig::Analyze(bound),
            )))?;
            self.let_go();
        }
        self.retarget(StudyTarget::Insert { track, slot });
        let inputs = StudioHost::audio_inputs(self);
        self.recorder = Some(Recorder::new(inputs, None));
        self.open_audio()
    }

    /// *Record into Analyze Musically…*: a new study recording from the
    /// input device (the one a track has open, else the first there is).
    pub(super) fn record_into_study(&mut self) -> Result<String, String> {
        let inputs = StudioHost::audio_inputs(self);
        let input = self
            .input_open
            .as_ref()
            .map(|(_, name)| name.clone())
            .or_else(|| inputs.first().cloned());
        self.retarget(StudyTarget::Recording(PersistentId::new()));
        let mut recorder = Recorder::new(inputs, input.clone());
        recorder.config = AnalyzeConfig {
            arm: fontelle_types::ArmMode::Now,
            ..AnalyzeConfig::new()
        };
        if input.is_none() {
            recorder.problem = Some("No input device \u{2014} plug one in".to_string());
        }
        self.recorder = Some(recorder);
        self.open_audio()?;
        Ok("Record into Analyze Musically \u{2014} arm and record a take".to_string())
    }

    fn stop_recorder(&mut self) {
        if let Some(mut recorder) = self.recorder.take() {
            let left = recorder.stop();
            self.land_takes(left);
            if let Some((track, slot)) = self.target_insert()
                && let Some(capture) = self.analyze_capture(track, slot)
            {
                capture.arm(false);
            }
        }
    }

    /// The arm settings in force: the insert's, or the recorder's own.
    fn record_config(&self) -> AnalyzeConfig {
        match self.target_insert() {
            Some((track, slot)) => self.analyze_config(track, slot).unwrap_or_default(),
            None => self
                .recorder
                .as_ref()
                .map_or_else(AnalyzeConfig::new, |r| r.config),
        }
    }

    pub(super) fn study_record(&mut self, op: AnalyzeRecordOp) -> Result<String, String> {
        if self.recorder.is_none() {
            return Err(
                "record from a mixer insert or Record into Analyze Musically\u{2026}".to_string(),
            );
        }
        let mut config = self.record_config();
        let param = match &op {
            AnalyzeRecordOp::Arm(on) => return self.arm_recorder(*on),
            AnalyzeRecordOp::Source(input) => {
                let input = input.clone();
                let armed = self.recorder_armed();
                if armed {
                    self.arm_recorder(false)?;
                }
                let no_insert = self.target_insert().is_none();
                if let Some(r) = &mut self.recorder {
                    if input.is_none() && no_insert {
                        return Err("only an insert records its own track".to_string());
                    }
                    r.input = input;
                    r.problem = None;
                }
                return Ok(match self.recorder.as_ref().and_then(|r| r.input.clone()) {
                    Some(name) => format!("Recording from \u{201c}{name}\u{201d}"),
                    None => "Recording what plays through this track".to_string(),
                });
            }
            AnalyzeRecordOp::Mode(mode) => {
                config.arm = *mode;
                ("arm", mode.index() as f32)
            }
            AnalyzeRecordOp::Threshold(db) => {
                config.threshold_db = db.clamp(-80.0, 0.0);
                ("threshold", config.threshold_db)
            }
            AnalyzeRecordOp::Release(ms) => {
                config.release_ms = ms.clamp(50.0, 10_000.0);
                ("release", config.release_ms)
            }
            AnalyzeRecordOp::PostFader(on) => {
                config.post_fader = *on;
                ("post_fader", if *on { 1.0 } else { 0.0 })
            }
        };
        match self.target_insert() {
            Some((track, slot)) => {
                // The parameter's normalised value, as the insert's panel
                // would send it; a knob's drag merges, a choice is its own.
                let value = EffectConfig::Analyze(config)
                    .normalised(param.0)
                    .unwrap_or(param.1);
                let knob = matches!(
                    op,
                    AnalyzeRecordOp::Threshold(_) | AnalyzeRecordOp::Release(_)
                );
                if !knob {
                    self.history.break_gesture();
                }
                self.try_run(Box::new(fontelle_model::SetInsertParam::new(
                    track, slot, param.0, value,
                )))?;
                if !knob {
                    self.let_go();
                }
            }
            None => {
                if let Some(r) = &mut self.recorder {
                    r.config = config;
                }
            }
        }
        if let Some(w) = self.recorder.as_ref().and_then(|r| r.input_writer.as_ref()) {
            w.control.configure(&config);
        }
        Ok(String::new())
    }

    fn recorder_armed(&self) -> bool {
        let Some(recorder) = &self.recorder else {
            return false;
        };
        if let Some(w) = &recorder.input_writer {
            return w.control.armed.load(Ordering::Relaxed);
        }
        if recorder.insert_writer.is_some()
            && let Some((track, slot)) = self.target_insert()
        {
            return self
                .analyze_capture(track, slot)
                .is_some_and(|c| c.is_armed());
        }
        false
    }

    /// Arms (a writer for the takes, and the capture armed) or disarms (the
    /// take running is closed and lands).
    fn arm_recorder(&mut self, on: bool) -> Result<String, String> {
        if !on {
            if let Some((track, slot)) = self.target_insert()
                && let Some(capture) = self.analyze_capture(track, slot)
            {
                capture.arm(false);
            }
            if let Some(w) = self.recorder.as_ref().and_then(|r| r.input_writer.as_ref()) {
                w.control.armed.store(false, Ordering::Relaxed);
            }
            return Ok("Disarmed".to_string());
        }
        let dir = self.make_real()?.join("recordings");
        let input = self.recorder.as_ref().and_then(|r| r.input.clone());
        let config = self.record_config();
        match (input, self.target_insert()) {
            (None, Some((track, slot))) => {
                let capture = self.analyze_capture(track, slot).ok_or(
                    "the insert is not playing yet \u{2014} start the audio and try again",
                )?;
                if let Some(r) = &mut self.recorder
                    && r.insert_writer.is_none()
                {
                    r.insert_writer = Some(crate::insert_takes::InsertTakeWriter::spawn(
                        Arc::clone(&capture),
                        dir,
                        self.options.sample_rate,
                    ));
                }
                capture.arm(true);
                Ok(format!(
                    "Armed \u{2014} records {}",
                    match config.arm {
                        fontelle_types::ArmMode::OnPlay => "while the song plays",
                        fontelle_types::ArmMode::OnInput => "when the sound crosses the threshold",
                        fontelle_types::ArmMode::Now => "from now",
                    }
                ))
            }
            (Some(name), _) => {
                if self
                    .recorder
                    .as_ref()
                    .is_some_and(|r| r.input_writer.is_none())
                {
                    self.start_input_writer(&name, dir)?;
                }
                if let Some(w) = self.recorder.as_ref().and_then(|r| r.input_writer.as_ref()) {
                    w.control.configure(&config);
                    w.control.armed.store(true, Ordering::Relaxed);
                }
                Ok(format!(
                    "Armed \u{2014} recording from \u{201c}{name}\u{201d}"
                ))
            }
            (None, None) => Err("choose an input to record from".to_string()),
        }
    }

    /// A thread turning `name`'s blocks into takes: through the tap on the
    /// stream a track already has open (§5 R4), or a stream of its own.
    fn start_input_writer(&mut self, name: &str, dir: PathBuf) -> Result<(), String> {
        let control = Arc::new(crate::insert_takes::InputControl::default());
        let transport = self.transport.clone();
        let shared = self
            .input_open
            .as_ref()
            .is_some_and(|(_, open)| open == name);
        let (source, channels, rate, device): (
            crate::insert_takes::InputSource,
            u16,
            u32,
            Option<fontelle_engine::AudioDevice>,
        ) = if shared && let Some(mut tap) = self.input_tap.take() {
            tap.open();
            let (channels, rate) = (self.input_take.channels().max(1), self.input_rate.max(1));
            (
                Box::new(move |out: &mut Vec<f32>| {
                    tap.drain_into(out);
                    tap.dropped() as u64
                }),
                channels,
                rate,
                None,
            )
        } else {
            let mut device = fontelle_engine::AudioDevice::default_host();
            let (writer, mut reader) = fontelle_engine::input_capture_channel(96_000 * 2);
            let (rate, channels) = device
                .start_input_stream(Some(name), writer)
                .map_err(|e| format!("could not open \u{201c}{name}\u{201d}: {e}"))?;
            (
                Box::new(move |out: &mut Vec<f32>| {
                    reader.drain_into(out);
                    reader.dropped() as u64
                }),
                channels,
                rate,
                Some(device),
            )
        };
        let writer = crate::insert_takes::InputTakeWriter::spawn(
            source, channels, rate, dir, control, transport,
        );
        if let Some(r) = &mut self.recorder {
            r.input_writer = Some(writer);
            r._device = device;
        }
        Ok(())
    }

    /// Once a frame: finished takes land in the study.
    pub(super) fn service_recorder(&mut self) {
        let finished = match &self.recorder {
            Some(r) => r.poll(),
            None => return,
        };
        if !finished.is_empty() {
            self.land_takes(finished);
        }
    }

    /// Takes that finished, into the study (made by the first one): each its
    /// own undo, and the lane on the first.
    fn land_takes(&mut self, takes: Vec<Result<crate::insert_takes::InsertTake, String>>) {
        for take in takes {
            match take {
                Ok(take) => {
                    if let Err(e) = self.land_take(&take) {
                        self.message = Some(e);
                    }
                }
                Err(e) => self.message = Some(format!("a take could not be written: {e}")),
            }
        }
    }

    fn land_take(&mut self, take: &crate::insert_takes::InsertTake) -> Result<(), String> {
        if take.frames == 0 {
            let _ = std::fs::remove_file(&take.path);
            return Ok(());
        }
        let imported = self
            .library
            .import_audio(&take.path)
            .map_err(|e| e.to_string())?;
        self.take_files.push(take.path.clone());
        let fresh = |id: u32| StudyTake {
            id,
            asset: imported.asset.clone(),
            name: format!("Take {id}"),
            song_sample: take.song_sample,
            frames: take.frames as i64,
            sample_rate: take.sample_rate,
            starred: false,
            dropped_frames: take.dropped_frames,
        };
        self.history.break_gesture();
        match self.open_study_id() {
            Some(id) => {
                let study = &self.project.studies[id];
                let next = study.takes.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                let mut takes = study.takes.clone();
                takes.push(fresh(next));
                let comp = study.comp.clone();
                self.try_run(Box::new(fontelle_model::SetStudyTakes::new(
                    id, takes, comp,
                )))?;
            }
            None => {
                let key = match self.analysis_target {
                    Some(StudyTarget::Insert { track, slot }) => {
                        self.analyze_config(track, slot).and_then(|c| c.study)
                    }
                    Some(StudyTarget::Recording(key)) => Some(key),
                    _ => None,
                };
                let name = format!("{} takes", self.target_name());
                let mut study = Study::new(name, StudySource::Standalone, imported.asset.clone());
                study.insert = key;
                study.takes = vec![fresh(1)];
                study.current_take = Some(1);
                self.try_run(Box::new(fontelle_model::AddStudy::new(study)))?;
            }
        }
        self.let_go();
        self.follow_target_audio();
        Ok(())
    }

    pub(super) fn study_take(&mut self, op: AnalyzeTakeOp) -> Result<String, String> {
        let id = self.open_study_id().ok_or("there are no takes yet")?;
        let study = self.project.studies[id].clone();
        let find = |take: u32| {
            study
                .takes
                .iter()
                .position(|t| t.id == take)
                .ok_or_else(|| "that take is not there".to_string())
        };
        self.history.break_gesture();
        let said = match op {
            AnalyzeTakeOp::Load(take) => {
                let at = find(take)?;
                let chosen = study.takes[at].clone();
                if study.current_take == Some(take) {
                    return Ok(format!("{} is in the lane", chosen.name));
                }
                // The edits name spans of the audio that was there: they go
                // with it, in the same entry, so one undo brings them back.
                let mut clean = study.clean.clone();
                clean.trim = None;
                clean.fade_in = 0;
                clean.fade_out = 0;
                let had = !study.pitch_edits.is_empty() || !study.markers.is_empty();
                self.try_run(Box::new(fontelle_model::Compound::new(
                    format!("Load {}", chosen.name),
                    vec![
                        Box::new(fontelle_model::SetStudyOriginal::new(
                            id,
                            chosen.asset.clone(),
                            Some(take),
                        )),
                        Box::new(fontelle_model::SetStudyEdits::new(id, Vec::new())),
                        Box::new(fontelle_model::SetStudyMarkers::new(id, Vec::new())),
                        Box::new(fontelle_model::SetStudyClean::new(id, clean)),
                    ],
                )))?;
                self.follow_target_audio();
                if had {
                    format!(
                        "{} in the lane \u{2014} the last take's edits are one undo away",
                        chosen.name
                    )
                } else {
                    format!("{} in the lane", chosen.name)
                }
            }
            AnalyzeTakeOp::Star(take) => {
                let at = find(take)?;
                let mut takes = study.takes.clone();
                takes[at].starred = !takes[at].starred;
                let said = if takes[at].starred {
                    format!("{} starred", takes[at].name)
                } else {
                    format!("{} unstarred", takes[at].name)
                };
                self.try_run(Box::new(fontelle_model::SetStudyTakes::new(
                    id,
                    takes,
                    study.comp.clone(),
                )))?;
                said
            }
            AnalyzeTakeOp::Rename(take, name) => {
                let at = find(take)?;
                let mut takes = study.takes.clone();
                let name = name.trim();
                takes[at].name = if name.is_empty() {
                    format!("Take {take}")
                } else {
                    name.to_string()
                };
                self.try_run(Box::new(fontelle_model::SetStudyTakes::new(
                    id,
                    takes,
                    study.comp.clone(),
                )))?;
                String::new()
            }
            AnalyzeTakeOp::Discard(take) => {
                let at = find(take)?;
                let mut takes = study.takes.clone();
                let gone = takes.remove(at);
                let comp: Vec<StudyCompSpan> = study
                    .comp
                    .iter()
                    .copied()
                    .filter(|s| s.take != take)
                    .collect();
                self.try_run(Box::new(fontelle_model::SetStudyTakes::new(
                    id, takes, comp,
                )))?;
                self.sweep_takes();
                format!("{} discarded \u{2014} Ctrl+Z brings it back", gone.name)
            }
            AnalyzeTakeOp::UseComp => return self.use_comp(id),
        };
        self.let_go();
        Ok(said)
    }

    pub(super) fn set_study_comp(
        &mut self,
        comp: Vec<StudyCompSpan>,
        merge: bool,
    ) -> Result<String, String> {
        let id = self.open_study_id().ok_or("there are no takes yet")?;
        let takes = self.project.studies[id].takes.clone();
        if !merge {
            self.history.break_gesture();
        }
        self.try_run(Box::new(fontelle_model::SetStudyTakes::new(
            id, takes, comp,
        )))?;
        Ok(String::new())
    }

    /// The comp made into a take of its own (crossfaded spans, plan P5),
    /// and loaded into the lane.
    fn use_comp(&mut self, id: StudyId) -> Result<String, String> {
        let study = self.project.studies[id].clone();
        if study.comp.is_empty() {
            return Err("drag over the takes to choose spans for the comp first".to_string());
        }
        let store = self.library.audio_store();
        let mut buffers = Vec::new();
        for take in &study.takes {
            let buffer = store
                .get(take.asset.id)
                .cloned()
                .ok_or_else(|| format!("{} is not loaded", take.name))?;
            buffers.push((take.id, buffer));
        }
        let channels = buffers
            .iter()
            .map(|(_, b)| usize::from(b.channels.max(1)))
            .max()
            .unwrap_or(1);
        let rate = buffers
            .first()
            .map_or(self.options.sample_rate, |(_, b)| b.sample_rate);
        let len = study
            .comp
            .iter()
            .map(|s| s.end.max(0) as usize)
            .max()
            .unwrap_or(0);
        let index_of = |take: u32| buffers.iter().position(|(id, _)| *id == take);
        let spans: Vec<fontelle_analysis::comp::CompSpan> = study
            .comp
            .iter()
            .filter_map(|s| {
                Some(fontelle_analysis::comp::CompSpan {
                    take: index_of(s.take)?,
                    start: s.start.max(0) as usize,
                    end: s.end.max(0) as usize,
                })
            })
            .collect();
        let mut out = vec![0.0f32; len * channels];
        for ch in 0..channels {
            let per_take: Vec<Vec<f32>> = buffers
                .iter()
                .map(|(_, b)| {
                    let c = usize::from(b.channels.max(1));
                    b.data
                        .iter()
                        .skip(ch.min(c - 1))
                        .step_by(c)
                        .copied()
                        .collect()
                })
                .collect();
            let refs: Vec<&[f32]> = per_take.iter().map(Vec::as_slice).collect();
            let mono = fontelle_analysis::comp::comp(
                &refs,
                &spans,
                len,
                fontelle_analysis::comp::default_crossfade(rate),
                fontelle_analysis::edit::FadeShape::EqualPower,
            );
            for (i, v) in mono.into_iter().enumerate() {
                out[i * channels + ch] = v;
            }
        }
        let dir = self.make_real()?.join("recordings");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = (1..10_000)
            .map(|n| dir.join(format!("Comp {n}.wav")))
            .find(|p| !p.exists())
            .ok_or("the recordings folder is full")?;
        let mut writer = fontelle_assets::WavWriter::create(&path, rate, channels as u16)
            .map_err(|e| e.to_string())?;
        writer
            .write(&out)
            .and_then(|()| writer.finish())
            .map_err(|e| e.to_string())?;
        let imported = self
            .library
            .import_audio(&path)
            .map_err(|e| e.to_string())?;
        self.take_files.push(path.clone());
        let next = study.takes.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let first = study
            .comp
            .iter()
            .min_by_key(|s| s.start)
            .and_then(|s| study.takes.iter().find(|t| t.id == s.take));
        let name = format!(
            "Comp {}",
            study
                .takes
                .iter()
                .filter(|t| t.name.starts_with("Comp"))
                .count()
                + 1
        );
        let mut takes = study.takes.clone();
        takes.push(StudyTake {
            id: next,
            asset: imported.asset.clone(),
            name: name.clone(),
            song_sample: first.and_then(|t| t.song_sample),
            frames: len as i64,
            sample_rate: rate,
            starred: false,
            dropped_frames: 0,
        });
        let mut clean = study.clean.clone();
        clean.trim = None;
        clean.fade_in = 0;
        clean.fade_out = 0;
        self.history.break_gesture();
        self.try_run(Box::new(fontelle_model::Compound::new(
            format!("Make {name}"),
            vec![
                Box::new(fontelle_model::SetStudyTakes::new(
                    id,
                    takes,
                    study.comp.clone(),
                )),
                Box::new(fontelle_model::SetStudyOriginal::new(
                    id,
                    imported.asset,
                    Some(next),
                )),
                Box::new(fontelle_model::SetStudyEdits::new(id, Vec::new())),
                Box::new(fontelle_model::SetStudyMarkers::new(id, Vec::new())),
                Box::new(fontelle_model::SetStudyClean::new(id, clean)),
            ],
        )))?;
        self.let_go();
        self.follow_target_audio();
        Ok(format!("{name} made from the comp, in the lane"))
    }

    /// Takes' files nothing names any more — not the song, not anything an
    /// undo or a redo could bring back — are deleted (plan P5: *"discard
    /// deletes its file only if no command references it"*).
    pub(super) fn sweep_takes(&mut self) {
        if self.take_files.is_empty() {
            return;
        }
        let used: Vec<PathBuf> = self.project.files().into_iter().map(|f| f.path).collect();
        let mut kept = Vec::new();
        for path in std::mem::take(&mut self.take_files) {
            let named = used.contains(&path) || self.history.references(&path.to_string_lossy());
            if named {
                kept.push(path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
        self.take_files = kept;
    }

    /// Send to arrangement: the lane's audio, cleaned and edited, as a new
    /// clip where it was recorded — or at the playhead.
    pub(super) fn send_study_to_arrangement(&mut self, playhead: Sample) -> Result<String, String> {
        let study = self.open_study().ok_or("record a take first")?;
        let at = study
            .current_take
            .and_then(|id| study.takes.iter().find(|t| t.id == id))
            .and_then(|t| t.song_sample)
            .unwrap_or(playhead)
            .max(0);
        self.start_render(RenderPurpose::Arrangement { at })
    }

    // ----------------------------------------------------- the listings ---

    pub(super) fn study_rows(&self) -> Vec<AnalyzeStudyRow> {
        let open = self.open_study_id();
        let inserts: Vec<(PersistentId, String)> = self
            .project
            .mixer
            .tracks
            .values()
            .flat_map(|t| {
                t.inserts.iter().filter_map(|i| match &i.config {
                    EffectConfig::Analyze(c) => c.study.map(|k| (k, t.name.clone())),
                    _ => None,
                })
            })
            .collect();
        self.project
            .studies
            .iter()
            .map(|(id, s)| {
                let place = match s.source {
                    StudySource::Clip(_) => "clip".to_string(),
                    StudySource::Standalone => {
                        match s
                            .insert
                            .and_then(|k| inserts.iter().find(|(key, _)| *key == k))
                        {
                            Some((_, track)) => format!("insert on {track}"),
                            None if !s.takes.is_empty() => {
                                format!("recording, {} take(s)", s.takes.len())
                            }
                            None => "file".to_string(),
                        }
                    }
                };
                AnalyzeStudyRow {
                    id,
                    name: s.name.clone(),
                    place,
                    open: Some(id) == open,
                }
            })
            .collect()
    }

    pub(super) fn open_study_window(&mut self, id: StudyId) -> Result<String, String> {
        let study = self
            .project
            .studies
            .get(id)
            .ok_or("that study is not there")?;
        if let StudySource::Clip(clip) = study.source
            && self.project.clips.contains_key(clip)
        {
            return self.open_analysis(clip);
        }
        // An insert's study opens on its insert, so it can record again.
        if let Some(key) = study.insert {
            let at = self
                .mixer_track_ids()
                .into_iter()
                .enumerate()
                .find_map(|(strip, track)| {
                    let inserts = &self.project.mixer.tracks.get(track)?.inserts;
                    inserts.iter().position(|i| {
                    matches!(&i.config, EffectConfig::Analyze(c) if c.study == Some(key))
                })
                .map(|slot| (strip, slot))
                });
            if let Some((strip, slot)) = at {
                return self.open_insert_study(strip, slot);
            }
        }
        self.retarget(StudyTarget::Study(id));
        if !study_records(&self.project.studies[id]) {
            return self.open_audio();
        }
        let inputs = StudioHost::audio_inputs(self);
        let input = inputs.first().cloned();
        let mut recorder = Recorder::new(inputs, input);
        recorder.config.arm = fontelle_types::ArmMode::Now;
        self.recorder = Some(recorder);
        self.open_audio()
    }

    pub(super) fn is_analyze_slot(&self, strip: usize, slot: usize) -> bool {
        self.mixer_track_ids()
            .get(strip)
            .is_some_and(|track| self.analyze_config(*track, slot).is_some())
    }

    /// Analyze Musically's preview player, shared with the node on the
    /// master of every graph.
    pub fn study_player(&self) -> std::sync::Arc<fontelle_engine::StudyPlayer> {
        std::sync::Arc::clone(&self.study_player)
    }

    /// *Notes under the audio*: the analysed notes as a note clip on a new
    /// row directly under the clip, playing the selected channel, as long as
    /// the clip. One command, so one undo.
    pub(super) fn analysis_clip(
        &mut self,
        mode: fontelle_ui::canvas::AnalyzeMode,
        selection: &[usize],
        keep_bends: bool,
    ) -> Result<String, String> {
        let open = self.analysis.as_ref().ok_or("nothing is being analysed")?;
        let Some(StudyTarget::Clip(clip)) = self.analysis_target else {
            return Err(
                "this audio has no clip to put notes under \u{2014} Copy notes instead".to_string(),
            );
        };
        let audio = self
            .project
            .clips
            .get(clip)
            .cloned()
            .ok_or("the analysed clip is not there any more")?;
        let (notes, _) = StudioHost::analysis_notes(self, mode, selection, keep_bends)
            .ok_or("there are no notes to put under it yet")?;
        let channel = self
            .selected_channel_id()
            .ok_or("there is no instrument to play the notes \u{2014} add one first")?;
        let row = self
            .lane_ids()
            .iter()
            .position(|lane| *lane == audio.lane)
            .ok_or("the analysed clip is on no row")?;
        let name = format!("{} notes", open.name);
        let mut arena = Arena::default();
        for note in notes {
            // The clip starts where the audio does; a note heard before that
            // has nowhere to go.
            let start = note.start - audio.start;
            if start >= 0 {
                arena.insert(fontelle_model::Note { start, ..note });
            }
        }
        let count = arena.len();
        let clip = fontelle_model::Clip {
            name: None,
            lane: audio.lane,
            start: audio.start,
            length: audio.length,
            source: ClipSource::Notes(fontelle_model::NoteData {
                channel,
                notes: arena,
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        };
        let command = fontelle_model::AddClip::on_new_row_at(
            clip,
            name.clone(),
            [0x8a, 0xa4, 0xe8, 0xff],
            row + 1,
        );
        self.apply_for::<fontelle_model::AddClip>(Box::new(command))?;
        self.let_go();
        self.dirty = true;
        self.republish();
        self.touch();
        Ok(format!(
            "{count} note(s) on \u{201c}{name}\u{201d}, under the audio"
        ))
    }

    // ------------------------------------------------------- the view ---

    /// What the window shows besides the analysis: the study, where it is
    /// from, the recorder, the render.
    pub(super) fn fill_study_view(&self, view: &mut fontelle_ui::canvas::AnalyzeView) {
        let (rate, offset) = match (&self.analysis, &self.preview) {
            (Some(open), Some(preview)) => (preview.rate, open.span.1),
            _ => (self.options.sample_rate, 0),
        };
        view.rate = rate;
        view.offset = offset;
        view.has_audio = self.analysis.is_some();
        view.voice_denoise = crate::analyze::VOICE_DENOISE;
        let beat = f64::from(crate::beat_samples(&self.project))
            / f64::from(self.options.sample_rate.max(1));
        view.beat_seconds = (beat > 0.0).then_some(beat);
        view.source = match self.analysis_target {
            Some(StudyTarget::Clip(_)) => AnalyzeSource::Clip,
            Some(StudyTarget::Insert { track, .. }) => AnalyzeSource::Insert {
                track: self
                    .project
                    .mixer
                    .tracks
                    .get(track)
                    .map_or_else(String::new, |t| t.name.clone()),
            },
            _ => AnalyzeSource::Standalone,
        };
        if view.name.is_empty() {
            view.name = self.target_name();
        }
        if let Some(study) = self.open_study() {
            view.clean = study.clean.clone();
            view.markers = study.markers.clone();
            view.takes = study.takes.clone();
            view.comp = study.comp.clone();
            view.current_take = study.current_take;
        }
        view.rendering = self.render_job.as_ref().map(RenderJob::fraction);
        view.record = self.recorder.as_ref().map(|r| {
            let config = self.record_config();
            let mut record = AnalyzeRecordView {
                input: r.input.clone(),
                inputs: r.inputs.clone(),
                arm: config.arm,
                threshold_db: config.threshold_db,
                release_ms: config.release_ms,
                post_fader: config.post_fader,
                problem: r.problem.clone(),
                ..Default::default()
            };
            if let Some(w) = &r.input_writer {
                let c = &w.control;
                record.armed = c.armed.load(Ordering::Relaxed);
                record.recording = c.recording.load(Ordering::Relaxed);
                record.level = c.level();
                record.dropped_frames = c.dropped.load(Ordering::Relaxed);
                record.take_seconds = c.take_frames.load(Ordering::Relaxed) as f64
                    / f64::from(self.input_rate.max(1));
            } else if r.input.is_none()
                && let Some((track, slot)) = self.target_insert()
                && let Some(capture) = self.analyze_capture(track, slot)
            {
                record.armed = capture.is_armed();
                record.recording = capture.is_recording();
                record.dropped_frames = capture.dropped_frames();
                record.level = self.strip_peak(track);
            }
            record
        });
    }

    /// A mixer track's peak now, linear, for the Record page's meter.
    fn strip_peak(&self, track: MixerTrackId) -> f32 {
        let _ = track;
        0.0
    }

    /// Whether the window's view would read differently: moved by the
    /// recorder and the render, which change without a command.
    pub(super) fn study_revision(&self) -> u64 {
        let render = self
            .render_job
            .as_ref()
            .map_or(0, |j| u64::from(j.progress.load(Ordering::Relaxed)) + 1);
        let record = self.recorder.as_ref().map_or(0, |r| {
            let w = r.input_writer.as_ref();
            let armed = self.recorder_armed() as u64;
            let level = w.map_or(0, |w| u64::from(w.control.level().to_bits() >> 16));
            let recording = w.map_or(0, |w| w.control.take_frames.load(Ordering::Relaxed) / 4800);
            armed + (level << 1) + (recording << 20) + 7
        });
        render.wrapping_mul(31).wrapping_add(record)
    }
}

/// Whether a study is one that records (it has takes, or was made by a
/// recorder).
fn study_records(study: &Study) -> bool {
    study.insert.is_some() || !study.takes.is_empty()
}
