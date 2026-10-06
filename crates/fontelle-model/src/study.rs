//! Analyze Musically's studies, edited (`docs/analyze-musically-plan.md`
//! §3.9).
//!
//! A study is what a person did to a piece of audio: the pitch edits, by
//! span of samples, and the file the last render of them made. These are the
//! commands that change one, each with its inverse and its wire form, so a
//! study travels and undoes like every other edit (INVARIANT 9). The
//! analysis is never here: it is derived, and cached beside the song.

use fontelle_types::{AssetRef, PitchEdit, Study, StudyId};

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
        "Analyze audio"
    }

    fn merge_with(&mut self, _next: &dyn Command) -> bool {
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
