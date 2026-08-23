/// External clock in/out, MMC transport, and Song Position Pointer (TDD §14.5).
/// When slaved, the tempo map is driven by the incoming clock through a PLL to
/// smooth jitter, and tempo automation is disabled with a clear UI indication of
/// why — the two must never fight over the same tempo map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSource {
    Internal,
    ExternalMidi,
}

pub struct ClockSync {
    source: ClockSource,
    pll_phase: f64,
}

impl ClockSync {
    pub fn new() -> Self {
        Self {
            source: ClockSource::Internal,
            pll_phase: 0.0,
        }
    }

    pub fn source(&self) -> ClockSource {
        self.source
    }

    pub fn on_midi_clock_tick(&mut self) {
        let _ = self.pll_phase;
        todo!("PLL phase correction per incoming 24-ppqn clock tick")
    }
}

impl Default for ClockSync {
    fn default() -> Self {
        Self::new()
    }
}
