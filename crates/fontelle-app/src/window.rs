//! The window's view of the engine (item 7 of `docs/first-usable-plan.md`).
//!
//! `fontelle-app` is the one layer allowed to see both the model and the
//! engine, so the `fontelle_ui::TransportHost` seam is implemented here rather
//! than in `fontelle-ui` — which keeps the UI crate off `fontelle-engine`
//! entirely, and keeps its own tests running against a fake.
//!
//! The whole interface is atomics. TDD §2.2's shape, in two directions:
//!
//! - **Commands down.** `play`, `stop`, `seek` and `set_looping` are each a
//!   relaxed store or two. Nothing blocks, nothing allocates, and nothing waits
//!   for the audio thread to acknowledge anything.
//! - **State up.** [`EngineHost::view`] is one read of each atomic, taken once
//!   per frame, so the window draws one consistent picture rather than several
//!   sampled microseconds apart.

use std::sync::Arc;

use fontelle_engine::{MasterMeter, Transport, TransportState};
use fontelle_model::TempoMap;
use fontelle_types::PPQN;
use fontelle_ui::{TransportHost, TransportView};

pub struct EngineHost {
    transport: Arc<Transport>,
    master: Arc<MasterMeter>,
    /// The song's own map, for turning the published sample position into the
    /// bars and beats the read-out shows. A copy rather than a borrow: the
    /// window outlives any one borrow of the document, and the tempo curve
    /// only changes through a command.
    tempo: TempoMap,
    length_samples: i64,
    sample_rate: u32,
}

impl EngineHost {
    pub fn new(
        transport: Arc<Transport>,
        master: Arc<MasterMeter>,
        tempo: TempoMap,
        length_samples: i64,
        sample_rate: u32,
    ) -> Self {
        Self {
            transport,
            master,
            tempo,
            length_samples,
            sample_rate,
        }
    }
}

impl TransportHost for EngineHost {
    fn view(&mut self) -> TransportView {
        let state = self.transport.state();
        let position = self.transport.position_sample();
        // Taking resets, so this must happen exactly once per frame — which is
        // why the window reads a whole view rather than asking for pieces.
        let peaks = self.master.take_peaks();

        TransportView {
            available: true,
            playing: state.is_processing(),
            recording: state == TransportState::Recording,
            position_sample: position,
            // Through the map, never by arithmetic on a BPM: a song with a
            // tempo change has no single BPM to divide by (INVARIANT 5).
            position_beats: self.tempo.sample_to_tick(position) as f64 / PPQN as f64,
            length_samples: self.length_samples,
            sample_rate: self.sample_rate as f64,
            looping: self.transport.is_looping(),
            loop_range_samples: self.transport.loop_range_sample(),
            peaks: [
                peaks.first().copied().unwrap_or(0.0),
                peaks.get(1).copied().unwrap_or(0.0),
            ],
            reduction_db: self.master.take_max_reduction_db(),
        }
    }

    fn play(&mut self) {
        self.transport.play();
    }

    fn stop(&mut self) {
        self.transport.stop();
    }

    fn seek(&mut self, sample: i64) {
        self.transport.seek(sample);
    }

    fn set_looping(&mut self, on: bool) {
        self.transport.set_looping(on);
    }
}
