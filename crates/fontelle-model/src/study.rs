//! Analyze Musically's studies, edited (`docs/analyze-musically-plan.md`
//! §3.9).
//!
//! A study is what a person did to a piece of audio: the pitch edits, by
//! span of samples, and the file the last render of them made. These are the
//! commands that change one, each with its inverse and its wire form, so a
//! study travels and undoes like every other edit (INVARIANT 9). The
//! analysis is never here: it is derived, and cached beside the song.

use fontelle_types::{
    AssetRef, PitchEdit, Study, StudyClean, StudyCompSpan, StudyId, StudyMarker, StudyTake,
};

use crate::command::{Command, CommandError};
use crate::commands::NotApplied;
use crate::project::Project;

fn no_study() -> CommandError {
    CommandError("that study is not there".into())
}

/// Starts a study: what opening Analyze Musically on a clip does the first
/// time anything is done in it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AddStudy {
    study: Study,
    made: Option<StudyId>,
}

impl AddStudy {
    pub fn new(study: Study) -> Self {
        Self { study, made: None }
    }

    /// The study, once this has been applied.
    pub fn id(&self) -> Option<StudyId> {
        self.made
    }
}

impl Command for AddStudy {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::AddStudy(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        match self.made {
            Some(id) => {
                if !doc.studies.insert_at(id, self.study.clone()) {
                    return Err(CommandError("that study id is taken".into()));
                }
            }
            None => self.made = Some(doc.studies.insert(self.study.clone())),
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match self.made {
            Some(id) => Box::new(RemoveStudy::new(id)),
            None => Box::new(NotApplied::new("starting a study")),
        }
    }

    fn label(&self) -> &str {
        "Move notes"
    }

    /// The first edit of a clip starts its study, and a drag that began it
    /// carries on as edits of it: one entry, whose redo makes the study with
    /// the drag's last edits.
    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let any = next.as_any();
        if let Some(next) = any.downcast_ref::<SetStudyEdits>()
            && Some(next.study) == self.made
        {
            self.study.pitch_edits = next.edits.clone();
            return true;
        }
        // The Clean and Slice pages start a study the same way.
        if let Some(next) = any.downcast_ref::<SetStudyClean>()
            && Some(next.study) == self.made
        {
            self.study.clean = next.clean.clone();
            return true;
        }
        if let Some(next) = any.downcast_ref::<SetStudyMarkers>()
            && Some(next.study) == self.made
        {
            self.study.markers = next.markers.clone();
            return true;
        }
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.study.pitch_edits.len() * std::mem::size_of::<PitchEdit>()
    }
}

/// Forgets a study, keeping it for the undo.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RemoveStudy {
    study: StudyId,
    removed: Option<Study>,
}

impl RemoveStudy {
    pub fn new(study: StudyId) -> Self {
        Self {
            study,
            removed: None,
        }
    }
}

impl Command for RemoveStudy {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::RemoveStudy(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let removed = doc.studies.remove(self.study).ok_or_else(no_study)?;
        self.removed.get_or_insert(removed);
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.removed {
            Some(study) => Box::new(RestoreStudy {
                id: self.study,
                study: study.clone(),
            }),
            None => Box::new(NotApplied::new("forgetting a study")),
        }
    }

    fn label(&self) -> &str {
        "Forget analysis"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.removed.as_ref().map_or(0, |s| {
                s.pitch_edits.len() * std::mem::size_of::<PitchEdit>()
            })
    }
}

/// [`RemoveStudy`]'s inverse: the study, back under its own id.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RestoreStudy {
    id: StudyId,
    study: Study,
}

impl Command for RestoreStudy {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::RestoreStudy(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        if !doc.studies.insert_at(self.id, self.study.clone()) {
            return Err(CommandError("that study id is taken".into()));
        }
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        Box::new(RemoveStudy::new(self.id))
    }

    fn label(&self) -> &str {
        "Restore analysis"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// Every pitch edit of one study at once: the window hands back the whole
/// list, as the audio clip editor hands back a whole clip, so "unchanged"
/// needs no definition and the inverse is the list that was there.
///
/// A drag is a run of these on one study, merged into one entry (the
/// gesture break on mouse-up ends it), as a note drag is.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetStudyEdits {
    study: StudyId,
    edits: Vec<PitchEdit>,
    before: Option<Vec<PitchEdit>>,
}

impl SetStudyEdits {
    pub fn new(study: StudyId, edits: Vec<PitchEdit>) -> Self {
        Self {
            study,
            edits,
            before: None,
        }
    }
}

impl Command for SetStudyEdits {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetStudyEdits(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let study = doc.studies.get_mut(self.study).ok_or_else(no_study)?;
        if self.before.is_none() {
            self.before = Some(study.pitch_edits.clone());
        }
        study.pitch_edits = self.edits.clone();
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.before {
            Some(edits) => Box::new(SetStudyEdits::new(self.study, edits.clone())),
            None => Box::new(NotApplied::new("moving notes")),
        }
    }

    fn label(&self) -> &str {
        "Move notes"
    }

    /// The later list with the earlier `before`: one undo reaches back to
    /// before the drag began.
    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetStudyEdits>() else {
            return false;
        };
        if next.study != self.study {
            return false;
        }
        self.edits = next.edits.clone();
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + (self.edits.len() + self.before.as_ref().map_or(0, Vec::len))
                * std::mem::size_of::<PitchEdit>()
    }
}

/// Stamps (or clears) the file a study's edits were rendered to. Render to
/// clip is this and the clip's `SetAudioClip` in one compound (plan §3.7).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetStudyRender {
    study: StudyId,
    rendered: Option<AssetRef>,
    #[serde(with = "crate::wire::nested")]
    before: Option<Option<AssetRef>>,
}

impl SetStudyRender {
    pub fn new(study: StudyId, rendered: Option<AssetRef>) -> Self {
        Self {
            study,
            rendered,
            before: None,
        }
    }
}

impl Command for SetStudyRender {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetStudyRender(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let study = doc.studies.get_mut(self.study).ok_or_else(no_study)?;
        if self.before.is_none() {
            self.before = Some(study.rendered.clone());
        }
        study.rendered = self.rendered.clone();
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.before {
            Some(before) => Box::new(SetStudyRender::new(self.study, before.clone())),
            None => Box::new(NotApplied::new("rendering edits")),
        }
    }

    fn label(&self) -> &str {
        "Render edits"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// Every marker of one study at once (plan §3.9): the window hands back the
/// list, and the inverse is the list that was there. A drag of one merges.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetStudyMarkers {
    study: StudyId,
    markers: Vec<StudyMarker>,
    before: Option<Vec<StudyMarker>>,
}

impl SetStudyMarkers {
    pub fn new(study: StudyId, markers: Vec<StudyMarker>) -> Self {
        Self {
            study,
            markers,
            before: None,
        }
    }
}

impl Command for SetStudyMarkers {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetStudyMarkers(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let study = doc.studies.get_mut(self.study).ok_or_else(no_study)?;
        if self.before.is_none() {
            self.before = Some(study.markers.clone());
        }
        study.markers = self.markers.clone();
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.before {
            Some(markers) => Box::new(SetStudyMarkers::new(self.study, markers.clone())),
            None => Box::new(NotApplied::new("setting markers")),
        }
    }

    fn label(&self) -> &str {
        "Markers"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetStudyMarkers>() else {
            return false;
        };
        if next.study != self.study {
            return false;
        }
        self.markers = next.markers.clone();
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + (self.markers.len() + self.before.as_ref().map_or(0, Vec::len))
                * std::mem::size_of::<StudyMarker>()
    }
}

/// The Clean page's settings, whole (plan §3.9): trim, fades, gain and the
/// denoiser. A knob's drag merges into one entry.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetStudyClean {
    study: StudyId,
    clean: StudyClean,
    before: Option<StudyClean>,
}

impl SetStudyClean {
    pub fn new(study: StudyId, clean: StudyClean) -> Self {
        Self {
            study,
            clean,
            before: None,
        }
    }
}

impl Command for SetStudyClean {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetStudyClean(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let study = doc.studies.get_mut(self.study).ok_or_else(no_study)?;
        if self.before.is_none() {
            self.before = Some(study.clean.clone());
        }
        study.clean = self.clean.clone();
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.before {
            Some(clean) => Box::new(SetStudyClean::new(self.study, clean.clone())),
            None => Box::new(NotApplied::new("cleaning")),
        }
    }

    fn label(&self) -> &str {
        "Clean"
    }

    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetStudyClean>() else {
            return false;
        };
        if next.study != self.study {
            return false;
        }
        self.clean = next.clean.clone();
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        let profile = |c: &StudyClean| {
            c.denoise
                .noise
                .as_ref()
                .map_or(0, |n| n.magnitudes.len() * 4)
        };
        std::mem::size_of::<Self>() + profile(&self.clean) + self.before.as_ref().map_or(0, profile)
    }
}

/// The Record page's takes and the comp built from them, whole (plan P5):
/// a take arriving, starred, renamed or discarded, and a comp span chosen.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetStudyTakes {
    study: StudyId,
    takes: Vec<StudyTake>,
    comp: Vec<StudyCompSpan>,
    before: Option<(Vec<StudyTake>, Vec<StudyCompSpan>)>,
}

impl SetStudyTakes {
    pub fn new(study: StudyId, takes: Vec<StudyTake>, comp: Vec<StudyCompSpan>) -> Self {
        Self {
            study,
            takes,
            comp,
            before: None,
        }
    }
}

impl Command for SetStudyTakes {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetStudyTakes(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let study = doc.studies.get_mut(self.study).ok_or_else(no_study)?;
        if self.before.is_none() {
            self.before = Some((study.takes.clone(), study.comp.clone()));
        }
        study.takes = self.takes.clone();
        study.comp = self.comp.clone();
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.before {
            Some((takes, comp)) => {
                Box::new(SetStudyTakes::new(self.study, takes.clone(), comp.clone()))
            }
            None => Box::new(NotApplied::new("changing takes")),
        }
    }

    fn label(&self) -> &str {
        "Takes"
    }

    /// A comp span dragged out is a run of these: one entry.
    fn merge_with(&mut self, next: &dyn Command) -> bool {
        let Some(next) = next.as_any().downcast_ref::<SetStudyTakes>() else {
            return false;
        };
        if next.study != self.study || next.takes != self.takes {
            return false;
        }
        self.comp = next.comp.clone();
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        let takes = self.takes.len() + self.before.as_ref().map_or(0, |b| b.0.len());
        std::mem::size_of::<Self>() + takes * std::mem::size_of::<StudyTake>()
    }
}

/// What the study studies: a take loaded into the lane, or the comp made
/// into one. The edits name spans of whatever is there; the window clears
/// them in the same compound when the audio changes under them.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SetStudyOriginal {
    study: StudyId,
    original: AssetRef,
    take: Option<u32>,
    before: Option<(AssetRef, Option<u32>)>,
}

impl SetStudyOriginal {
    pub fn new(study: StudyId, original: AssetRef, take: Option<u32>) -> Self {
        Self {
            study,
            original,
            take,
            before: None,
        }
    }
}

impl Command for SetStudyOriginal {
    fn to_edit(&self) -> crate::wire::Edit {
        crate::wire::Edit::SetStudyOriginal(self.clone())
    }

    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError> {
        let study = doc.studies.get_mut(self.study).ok_or_else(no_study)?;
        if self.before.is_none() {
            self.before = Some((study.original.clone(), study.current_take));
        }
        study.original = self.original.clone();
        study.current_take = self.take;
        Ok(())
    }

    fn invert(&self) -> Box<dyn Command> {
        match &self.before {
            Some((original, take)) => {
                Box::new(SetStudyOriginal::new(self.study, original.clone(), *take))
            }
            None => Box::new(NotApplied::new("loading a take")),
        }
    }

    fn label(&self) -> &str {
        "Load take"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}
