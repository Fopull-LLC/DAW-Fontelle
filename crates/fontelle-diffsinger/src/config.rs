//! The voicebank's own files: `dsconfig.yaml` (the bank root and each of
//! `dsdur/`, `dspitch/`, `dsvariance/`), a vocoder's `vocoder.yaml`, a
//! dependency's `oudep.yaml`, and `character.txt` / `character.yaml`.
//!
//! The keys are the ones DiffSinger's exporters write and OpenUtau reads
//! (snake case). Every key is optional; a missing one takes the value the
//! format documents. Unknown keys are ignored, so a newer bank still loads.

use serde::Deserialize;

/// The largest mel bin count accepted from a bank or vocoder. Banks use 80
/// or 128; a larger number is a file that is not what it claims to be.
pub const MAX_MEL_BINS: usize = 255;

/// The largest speaker embedding accepted.
pub const MAX_HIDDEN_SIZE: usize = 4096;

/// One `dsconfig.yaml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct DsConfig {
    /// The phoneme inventory (`.txt`, one per line, or `.json`, symbol to id).
    pub phonemes: String,
    /// Language ids (`.json`, code to id) of a multi-language bank.
    pub languages: Option<String>,
    pub acoustic: Option<String>,
    /// The vocoder, by the name of an installed dependency.
    pub vocoder: Option<String>,
    /// Speaker embedding files, without `.emb`, relative to the folder.
    pub speakers: Option<Vec<String>>,
    pub hidden_size: usize,
    pub use_key_shift_embed: bool,
    pub use_speed_embed: bool,
    pub use_energy_embed: bool,
    pub use_breathiness_embed: bool,
    pub use_voicing_embed: bool,
    pub use_tension_embed: bool,
    pub augmentation_args: Option<AugmentationArgs>,
    pub use_continuous_acceleration: bool,
    pub use_lang_id: bool,
    /// The older name of `use_variable_depth`.
    pub use_shallow_diffusion: Option<bool>,
    pub use_variable_depth: Option<bool>,
    /// Steps out of 1000, or 0..1 with continuous acceleration.
    pub max_depth: Option<f64>,
    pub dur: Option<String>,
    pub linguistic: Option<String>,
    pub pitch: Option<String>,
    pub variance: Option<String>,
    /// The linguistic encoder takes words (division and duration) rather
    /// than phoneme durations.
    pub predict_dur: bool,
    pub predict_energy: bool,
    pub predict_breathiness: bool,
    pub predict_voicing: bool,
    pub predict_tension: bool,
    pub use_expr: bool,
    pub use_note_rest: bool,
    pub sample_rate: u32,
    pub hop_size: u32,
    pub win_size: u32,
    pub fft_size: u32,
    pub num_mel_bins: usize,
    pub mel_fmin: f64,
    pub mel_fmax: f64,
    pub mel_base: String,
    pub mel_scale: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct AugmentationArgs {
    pub random_pitch_shifting: Option<RandomPitchShifting>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct RandomPitchShifting {
    /// Semitones, `[lowest, highest]`.
    pub range: Vec<f32>,
}

impl Default for DsConfig {
    fn default() -> Self {
        Self {
            phonemes: "phonemes.txt".into(),
            languages: None,
            acoustic: None,
            vocoder: None,
            speakers: None,
            hidden_size: 256,
            use_key_shift_embed: false,
            use_speed_embed: false,
            use_energy_embed: false,
            use_breathiness_embed: false,
            use_voicing_embed: false,
            use_tension_embed: false,
            augmentation_args: None,
            use_continuous_acceleration: false,
            use_lang_id: false,
            use_shallow_diffusion: None,
            use_variable_depth: None,
            max_depth: None,
            dur: None,
            linguistic: None,
            pitch: None,
            variance: None,
            predict_dur: true,
            predict_energy: true,
            predict_breathiness: true,
            predict_voicing: false,
            predict_tension: false,
            use_expr: false,
            use_note_rest: false,
            sample_rate: 44100,
            hop_size: 512,
            win_size: 2048,
            fft_size: 2048,
            num_mel_bins: 128,
            mel_fmin: 40.0,
            mel_fmax: 16000.0,
            mel_base: "10".into(),
            mel_scale: "slaney".into(),
        }
    }
}

impl DsConfig {
    pub fn parse(text: &str) -> Result<Self, String> {
        parse_yaml_or_default(text)
    }

    /// Seconds a frame lasts.
    pub fn frame_seconds(&self) -> f64 {
        f64::from(self.hop_size) / f64::from(self.sample_rate.max(1))
    }

    /// Whether the acoustic model renders from a lowered depth (shallow
    /// diffusion).
    pub fn variable_depth(&self) -> bool {
        self.use_variable_depth
            .or(self.use_shallow_diffusion)
            .unwrap_or(false)
    }

    /// The deepest render as a fraction of the whole process (0..1), when
    /// the depth is variable.
    pub fn max_depth_fraction(&self) -> Option<f64> {
        if !self.variable_depth() {
            return None;
        }
        let depth = self.max_depth?;
        if !depth.is_finite() || depth < 0.0 {
            return None;
        }
        Some(if self.use_continuous_acceleration {
            depth
        } else {
            depth / 1000.0
        })
    }

    /// The key shift range the acoustic model was trained with, semitones.
    pub fn key_shift_range(&self) -> Option<(f32, f32)> {
        let range = &self
            .augmentation_args
            .as_ref()?
            .random_pitch_shifting
            .as_ref()?
            .range;
        match range[..] {
            [low, high, ..] => Some((low, high)),
            _ => None,
        }
    }

    /// The variance curves the variance model predicts, in the order its
    /// `retake` input lists them.
    pub fn predicted_variances(&self) -> Vec<Variance> {
        [
            (self.predict_energy, Variance::Energy),
            (self.predict_breathiness, Variance::Breathiness),
            (self.predict_voicing, Variance::Voicing),
            (self.predict_tension, Variance::Tension),
        ]
        .into_iter()
        .filter_map(|(on, v)| on.then_some(v))
        .collect()
    }

    /// The variance curves the acoustic model takes.
    pub fn embedded_variances(&self) -> Vec<Variance> {
        [
            (self.use_energy_embed, Variance::Energy),
            (self.use_breathiness_embed, Variance::Breathiness),
            (self.use_voicing_embed, Variance::Voicing),
            (self.use_tension_embed, Variance::Tension),
        ]
        .into_iter()
        .filter_map(|(on, v)| on.then_some(v))
        .collect()
    }
}

/// The curves a variance model predicts and an acoustic model may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variance {
    Energy,
    Breathiness,
    Voicing,
    Tension,
}

impl Variance {
    /// The input name both models use.
    pub fn name(self) -> &'static str {
        match self {
            Variance::Energy => "energy",
            Variance::Breathiness => "breathiness",
            Variance::Voicing => "voicing",
            Variance::Tension => "tension",
        }
    }
}

/// A vocoder's `vocoder.yaml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct VocoderConfig {
    pub name: String,
    pub model: String,
    pub sample_rate: u32,
    pub hop_size: u32,
    pub win_size: u32,
    pub fft_size: u32,
    pub num_mel_bins: usize,
    pub mel_fmin: f64,
    pub mel_fmax: f64,
    pub mel_base: String,
    pub mel_scale: String,
    pub pitch_controllable: bool,
}

impl Default for VocoderConfig {
    fn default() -> Self {
        Self {
            name: "vocoder".into(),
            model: "model.onnx".into(),
            sample_rate: 44100,
            hop_size: 512,
            win_size: 2048,
            fft_size: 2048,
            num_mel_bins: 128,
            mel_fmin: 40.0,
            mel_fmax: 16000.0,
            mel_base: "10".into(),
            mel_scale: "slaney".into(),
            pitch_controllable: false,
        }
    }
}

impl VocoderConfig {
    pub fn parse(text: &str) -> Result<Self, String> {
        parse_yaml_or_default(text)
    }

    /// Why this vocoder cannot voice that acoustic model's mels, if it
    /// cannot (a different mel base is converted, not refused).
    pub fn mismatch(&self, acoustic: &DsConfig) -> Option<String> {
        for (who, bins) in [
            ("vocoder", self.num_mel_bins),
            ("acoustic model", acoustic.num_mel_bins),
        ] {
            if bins == 0 || bins > MAX_MEL_BINS {
                return Some(format!(
                    "the {who} declares {bins} mel bins (1 to {MAX_MEL_BINS} are accepted)"
                ));
            }
        }
        for (who, base) in [
            ("vocoder", &self.mel_base),
            ("acoustic model", &acoustic.mel_base),
        ] {
            if base != "10" && base != "e" {
                return Some(format!("the {who}'s mel base is \"{base}\" (10 or e)"));
            }
        }
        let pairs: [(&str, f64, f64); 7] = [
            (
                "sample rate",
                f64::from(self.sample_rate),
                f64::from(acoustic.sample_rate),
            ),
            (
                "hop size",
                f64::from(self.hop_size),
                f64::from(acoustic.hop_size),
            ),
            (
                "window size",
                f64::from(self.win_size),
                f64::from(acoustic.win_size),
            ),
            (
                "FFT size",
                f64::from(self.fft_size),
                f64::from(acoustic.fft_size),
            ),
            (
                "mel bins",
                self.num_mel_bins as f64,
                acoustic.num_mel_bins as f64,
            ),
            ("mel fmin", self.mel_fmin, acoustic.mel_fmin),
            ("mel fmax", self.mel_fmax, acoustic.mel_fmax),
        ];
        for (what, v, a) in pairs {
            if (v - a).abs() > 1e-5 {
                return Some(format!(
                    "the vocoder's {what} ({v}) is not the acoustic model's ({a})"
                ));
            }
        }
        if self.mel_scale != acoustic.mel_scale {
            return Some(format!(
                "the vocoder's mel scale ({}) is not the acoustic model's ({})",
                self.mel_scale, acoustic.mel_scale
            ));
        }
        None
    }
}

/// A dependency's `oudep.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct DependencyInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

impl DependencyInfo {
    pub fn parse(text: &str) -> Result<Self, String> {
        parse_yaml_or_default(text)
    }
}

/// What `character.txt` and `character.yaml` say about the singer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Character {
    pub name: Option<String>,
    pub image: Option<String>,
    pub portrait: Option<String>,
    pub author: Option<String>,
    pub voice: Option<String>,
    pub web: Option<String>,
    pub version: Option<String>,
    /// OpenUtau's phonemizer class name, which says the bank's main
    /// language.
    pub default_phonemizer: Option<String>,
    pub singer_type: Option<String>,
    /// Voice colours; for a DiffSinger bank each names a speaker by its
    /// suffix.
    pub subbanks: Vec<Subbank>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Subbank {
    /// The label shown for the colour.
    pub color: String,
    /// The speaker it selects.
    pub suffix: String,
}

impl Character {
    /// Reads `character.txt` (`key=value` lines) then `character.yaml`
    /// over it; either may be absent (`None`).
    pub fn parse(txt: Option<&str>, yaml: Option<&str>) -> Self {
        let mut c = Character::default();
        for line in txt.unwrap_or("").lines() {
            let line = line.trim_start_matches('\u{feff}').trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            let slot = match key.trim().to_ascii_lowercase().as_str() {
                "name" => &mut c.name,
                "image" => &mut c.image,
                "author" => &mut c.author,
                "voice" => &mut c.voice,
                "web" => &mut c.web,
                "version" => &mut c.version,
                _ => continue,
            };
            *slot = Some(value.to_string());
        }
        if let Some(yaml) = yaml
            && let Ok(y) = serde_yaml_ng::from_str::<CharacterYaml>(strip_bom(yaml))
        {
            let over = |slot: &mut Option<String>, v: Option<String>| {
                if let Some(v) = v.filter(|v| !v.trim().is_empty()) {
                    *slot = Some(v);
                }
            };
            over(&mut c.name, y.name);
            over(&mut c.image, y.image);
            over(&mut c.portrait, y.portrait);
            over(&mut c.author, y.author);
            over(&mut c.voice, y.voice);
            over(&mut c.web, y.web);
            over(&mut c.version, y.version);
            over(&mut c.default_phonemizer, y.default_phonemizer);
            over(&mut c.singer_type, y.singer_type);
            c.subbanks = y
                .subbanks
                .unwrap_or_default()
                .into_iter()
                .map(|s| Subbank {
                    color: s.color.unwrap_or_default(),
                    suffix: s.suffix.unwrap_or_default(),
                })
                .collect();
        }
        c
    }

    /// The language the default phonemizer is for, as a BCP 47 code.
    pub fn phonemizer_language(&self) -> Option<&'static str> {
        let class = self.default_phonemizer.as_deref()?.to_ascii_lowercase();
        // OpenUtau's DiffSinger phonemizers, by the language in their
        // class names.
        const BY_NAME: [(&str, &str); 14] = [
            ("japanese", "ja"),
            ("english", "en"),
            ("arpa", "en"),
            ("chinese", "zh"),
            ("mandarin", "zh"),
            ("jyutping", "yue"),
            ("cantonese", "yue"),
            ("korean", "ko"),
            ("spanish", "es"),
            ("portuguese", "pt"),
            ("italian", "it"),
            ("german", "de"),
            ("french", "fr"),
            ("russian", "ru"),
        ];
        BY_NAME
            .iter()
            .find(|(name, _)| class.contains(name))
            .map(|(_, code)| *code)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CharacterYaml {
    name: Option<String>,
    image: Option<String>,
    portrait: Option<String>,
    author: Option<String>,
    voice: Option<String>,
    web: Option<String>,
    version: Option<String>,
    default_phonemizer: Option<String>,
    singer_type: Option<String>,
    subbanks: Option<Vec<SubbankYaml>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SubbankYaml {
    color: Option<String>,
    suffix: Option<String>,
}

pub(crate) fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Parses a YAML mapping; an empty document is every default.
fn parse_yaml_or_default<T: for<'de> Deserialize<'de> + Default>(text: &str) -> Result<T, String> {
    let text = strip_bom(text);
    if text.trim().is_empty() {
        return Ok(T::default());
    }
    serde_yaml_ng::from_str(text).map_err(|e| e.to_string())
}
