use fontelle_model::Project;
use fontelle_types::CompiledTimeline;

/// Turns the whole document — clips, resolved prefab instances, lane mutes,
/// automation, the tempo map — into a flat, sample-timestamped event list
/// (TDD §11.1). Prefab resolution, override merging, and tempo conversion all
/// happen here, on the model thread, ahead of time: playback cost is therefore
/// independent of how deeply prefabs are nested (INVARIANT 3).
pub fn compile(_project: &Project) -> CompiledTimeline {
    todo!("resolve prefabs -> flatten clips -> tempo-convert to samples -> sort")
}
