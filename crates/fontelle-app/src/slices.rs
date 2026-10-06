//! Send to sampler (Analyze Musically, plan §3.8, P4): what a set of slices
//! of one file becomes — a sampler `Patch` and the notes that replay them.
//!
//! The layout is `fontelle_analysis::slice`'s; this turns its description
//! into the instrument the rack plays. [`Session::add_sampler_slices`]
//! (crate::Session::add_sampler_slices) puts it on a channel.

use fontelle_analysis::slice::{ReplayNote, SlicerPatch};
use fontelle_types::{AssetId, Tick};

/// What `Session::add_sampler_slices` made.
#[derive(Debug, Clone, PartialEq)]
pub struct SlicesAdded {
    pub channel: fontelle_types::ChannelId,
    /// The replay clip, when one was asked for.
    pub clip: Option<fontelle_types::ClipId>,
    /// The channel's name.
    pub name: String,
    /// Slices that got no key (see `SlicerPatch::unmapped`).
    pub unmapped: usize,
}

/// The sampler patch for `layout` over the file `file` (already imported).
///
/// One layer a zone, `start_offset`/`end_offset` the zone's frames of the
/// file (the units the voice steps through a sample in), the file's own
/// pitch at the root. The amp envelope opens at once and lets go in 2 ms:
/// a slice is played as it was cut, and the de-click is the release, not an
/// attack that would soften every transient it was cut at. The filter is
/// off, for the same reason.
pub fn slice_patch(file: AssetId, layout: &SlicerPatch) -> fontelle_core::Patch {
    let mut patch = fontelle_core::Patch::basic_synth();
    patch.layers = layout
        .zones
        .iter()
        .map(|zone| fontelle_core::Layer {
            source: fontelle_core::Source::Sample { file },
            key_range: zone.key_range,
            vel_range: (0, 127),
            root_key: zone.root_key,
            fine_tune_cents: zone.fine_tune_cents,
            playback: fontelle_core::PlaybackConfig {
                start_offset: zone.start as f64,
                end_offset: zone.end as f64,
                ..Default::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        })
        .collect();
    if let Some(amp) = patch.envelopes.first_mut() {
        amp.attack_s = 0.0;
        amp.release_s = 0.002;
    }
    for filter in &mut patch.filters {
        filter.enabled = false;
    }
    patch
}

/// The replay clip's notes: `replay` (in frames of a file at `file_rate`)
/// placed so the first slice's note starts where it did in the file, on a
/// clip starting at `clip_start`, through `tempo`.
pub fn replay_notes(
    replay: &[ReplayNote],
    file_rate: u32,
    clip_start: Tick,
    tempo: &fontelle_model::TempoMap,
) -> Vec<fontelle_model::Note> {
    if file_rate == 0 {
        return Vec::new();
    }
    let origin = tempo.tick_to_sample(clip_start);
    let to_tick = |frames: usize| {
        // The file's frames at the song's rate, from where the clip starts.
        let at =
            origin + (frames as f64 * tempo.sample_rate_hz() / f64::from(file_rate)).round() as i64;
        tempo.sample_to_tick(at) - clip_start
    };
    replay
        .iter()
        .map(|note| {
            let start = to_tick(note.start);
            let end = to_tick(note.start + note.length);
            fontelle_model::Note {
                start,
                length: (end - start).max(1),
                key: note.key,
                velocity: note.velocity,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                slide: false,
                path: Vec::new(),
                channel: None,
            }
        })
        .collect()
}
