//! A frozen vocal track plays its render: its part is kept and not sung.

mod common;

use common::{BEAT, note, song};
use fontelle_model::FrozenVocal;
use fontelle_vocal::phrases_of;

#[test]
fn a_frozen_track_sings_nothing_and_sings_again_unfrozen() {
    let mut s = song("en", vec![note(0, BEAT, 60, "la")]);
    assert_eq!(phrases_of(&s.project, s.voice, &s.tempo()).len(), 1);
    s.project.frozen.push(FrozenVocal {
        channel: s.voice,
        clip: s.clip,
        lane: s.lane,
    });
    assert!(phrases_of(&s.project, s.voice, &s.tempo()).is_empty());
    s.project.frozen.clear();
    assert_eq!(phrases_of(&s.project, s.voice, &s.tempo()).len(), 1);
}
