//! The vocal track's render pipeline.
//!
//! ```text
//! the song's notes and lyrics on a vocal channel
//!   → phrases (split at rests of a fifth of a second, capped in length)   phrases.rs
//!   → a render key per phrase: what is sung, in phrase-relative time      phrases.rs
//!   → the scheduler: worker threads singing through the engine, nearest
//!     the playhead first, every preview before any final, cancelling what
//!     is no longer wanted                                                  render.rs
//!   → a cache of takes by key: a move or an undo sings nothing
//!   → a table of placed phrases for `fontelle_engine::VocalNode`          table_for
//! ```
//!
//! Nothing here runs on the audio thread, and nothing here knows which
//! engine is installed: an engine is anything that is a [`Singer`], which a
//! loaded `fontelle_host::SingerEngine` is.

mod phrases;
mod render;

pub use phrases::{
    MAX_PHRASE_SECONDS, MIN_REST_SECONDS, NoteRef, Phrase, RenderKey, phrases_of, render_key,
};
pub use render::{PhraseState, Rendered, Singer, SingerVoice, VocalRenderer, Wanted};

use fontelle_engine::{SungPhrase, VocalTable};

/// The table a channel's node plays: each wanted phrase's best take so far,
/// placed at its song second. A phrase not sung yet is left out — silence,
/// which the roll draws as "rendering".
pub fn table_for(renderer: &VocalRenderer, wanted: &[Wanted]) -> VocalTable {
    VocalTable::new(
        wanted
            .iter()
            .filter_map(|w| {
                let take = renderer.best(w)?;
                Some(SungPhrase {
                    start: w.start + take.start,
                    sample_rate: take.sample_rate,
                    samples: std::sync::Arc::clone(&take.samples),
                })
            })
            .collect(),
    )
}
