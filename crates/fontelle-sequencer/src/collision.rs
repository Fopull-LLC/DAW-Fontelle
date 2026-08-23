/// Per-clip voice contexts (TDD §11.4): two overlapping clips referencing the same
/// channel and key get distinct tags, so a note-off only matches the note-on with
/// the same tag. Costs one small integer per voice and eliminates the "my notes are
/// cutting each other off" failure mode — at the cost of a voice each, which is the
/// correct, visually-expected behaviour.
pub fn voice_context_for_clip(clip_index: u32) -> u32 {
    clip_index
}
