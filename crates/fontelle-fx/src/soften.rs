#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftenPreset {
    Gentle,
    Standard,
    Aggressive,
    VintageRompler,
}

/// The soundfont-harshness effect (TDD §13.5), complementing the voice-side
/// mitigations in `fontelle-core` (§7.8): dynamic high shelf, adaptive resonance
/// suppressor (1-6kHz honk), transient softener, and an air-restore shelf so the
/// result reads as smoothed rather than merely darkened.
#[derive(Debug, Clone, Copy)]
pub struct SoftenConfig {
    pub shelf_amount: f32,
    pub suppressor_amount: f32,
    pub transient_amount: f32,
    pub air_restore_amount: f32,
}

impl SoftenConfig {
    pub fn from_preset(preset: SoftenPreset) -> Self {
        let _ = preset;
        todo!("named preset -> concrete parameter values")
    }
}

pub struct Soften;

impl Soften {
    pub fn process(&mut self, _block: &mut [f32], _config: &SoftenConfig) {
        todo!("dynamic shelf -> resonance suppressor -> transient softener -> air shelf")
    }
}
