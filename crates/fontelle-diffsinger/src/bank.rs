//! A voicebank folder in OpenUtau's DiffSinger layout:
//!
//! ```text
//! <bank>/character.txt, character.yaml    name, portrait, voice colours
//! <bank>/dsconfig.yaml                    the acoustic model, inventory,
//!                                         languages, speakers, vocoder name
//! <bank>/dsdur/      dsconfig.yaml, dsdict*.yaml   linguistic + duration
//! <bank>/dspitch/    dsconfig.yaml                 linguistic + pitch
//! <bank>/dsvariance/ dsconfig.yaml                 linguistic + variance
//! <bank>/dsvocoder/  vocoder.yaml                  a bank's own vocoder
//! <dependencies>/<vocoder name>/vocoder.yaml       a shared vocoder
//!                                                  (an installed .oudep)
//! ```
//!
//! Loading a bank reads only its text files; the models are opened when it
//! first sings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fontelle_sing::{Capabilities, VoiceInfo};

use crate::config::{Character, DsConfig, VocoderConfig};
use crate::dict::Dictionary;
use crate::inventory::{Inventory, Languages};

/// Sampling steps a final render takes unless the part says otherwise.
pub const DEFAULT_STEPS: u32 = 20;
/// The setting that holds them.
pub const SETTING_STEPS: &str = "steps";
/// The setting that turns the bank's pitch model off (0) or on (1).
pub const SETTING_NATURAL_PITCH: &str = "natural_pitch";
/// The extra lane of a bank whose acoustic model takes energy (dB).
pub const LANE_ENERGY: &str = "energy";
/// The extra lane of a pitch model that takes expressiveness (%).
pub const LANE_EXPRESSIVENESS: &str = "expressiveness";
/// Speaker mix lanes are this followed by the speaker's id (%).
pub const LANE_SPEAKER_PREFIX: &str = "speaker:";
/// The per-note option that picks a speaker.
pub const OPTION_SPEAKER: &str = "speaker";

/// One model folder: the root, `dsdur`, `dspitch` or `dsvariance`.
#[derive(Debug, Clone)]
pub struct SubModel {
    pub dir: PathBuf,
    pub config: DsConfig,
    pub inventory: Inventory,
    pub languages: Option<Languages>,
}

impl SubModel {
    /// A file the config names, inside this folder.
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn load(dir: &Path) -> Result<SubModel, String> {
        let config = DsConfig::parse(&read_text(&dir.join("dsconfig.yaml"))?)
            .map_err(|e| format!("{}: {e}", dir.join("dsconfig.yaml").display()))?;
        if config.hidden_size == 0 || config.hidden_size > crate::config::MAX_HIDDEN_SIZE {
            return Err(format!(
                "{}: hidden_size {} is out of range",
                dir.display(),
                config.hidden_size
            ));
        }
        let phonemes = dir.join(&config.phonemes);
        let json = phonemes
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let inventory = Inventory::parse(&read_text(&phonemes)?, json)
            .map_err(|e| format!("{}: {e}", phonemes.display()))?;
        let languages = match (&config.languages, config.use_lang_id) {
            (Some(file), _) => {
                let path = dir.join(file);
                match read_text(&path) {
                    Ok(text) => Some(
                        Languages::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?,
                    ),
                    Err(e) if config.use_lang_id => return Err(e),
                    Err(_) => None,
                }
            }
            (None, true) => {
                return Err(format!(
                    "{}: use_lang_id is set but no languages file is named",
                    dir.display()
                ));
            }
            (None, false) => None,
        };
        Ok(SubModel {
            dir: dir.to_path_buf(),
            config,
            inventory,
            languages,
        })
    }

    /// The models the config names, each checked to exist.
    fn require(&self, names: &[Option<&String>]) -> Result<(), String> {
        for name in names {
            let Some(name) = name else {
                return Err(format!(
                    "{}/dsconfig.yaml does not name every model it needs",
                    self.dir.display()
                ));
            };
            if !self.file(name).is_file() {
                return Err(format!("{} is missing", self.file(name).display()));
            }
        }
        Ok(())
    }

    /// The index in this folder's `speakers` of a speaker id: the entry
    /// itself, or the entry whose last path segments are the id; the first
    /// speaker otherwise.
    pub fn speaker_index(&self, id: &str) -> usize {
        let Some(speakers) = &self.config.speakers else {
            return 0;
        };
        if let Some(i) = speakers.iter().position(|s| s == id) {
            return i;
        }
        let want: Vec<&str> = id.split(['/', '\\']).collect();
        speakers
            .iter()
            .position(|s| {
                let have: Vec<&str> = s.split(['/', '\\']).collect();
                want.len() <= have.len() && have[have.len() - want.len()..] == want[..]
            })
            .unwrap_or(0)
    }

    /// Every speaker's embedding, in `speakers` order.
    pub fn embeddings(&self) -> Result<Vec<Vec<f32>>, String> {
        let size = self.config.hidden_size;
        let mut out = Vec::new();
        for speaker in self.config.speakers.iter().flatten() {
            let path = self.dir.join(format!("{speaker}.emb"));
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if bytes.len() < size * 4 {
                return Err(format!(
                    "{} holds {} values, the model wants {size}",
                    path.display(),
                    bytes.len() / 4
                ));
            }
            out.push(
                bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .take(size)
                    .map(|b| f32::from_le_bytes(*b))
                    .collect(),
            );
        }
        Ok(out)
    }
}

/// A vocoder found for a bank.
#[derive(Debug, Clone)]
pub struct VocoderRef {
    pub dir: PathBuf,
    pub config: VocoderConfig,
}

impl VocoderRef {
    pub fn model_path(&self) -> PathBuf {
        self.dir.join(&self.config.model)
    }
}

/// A speaker a bank offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaker {
    /// What the bank's configs call it (an embedding path or its suffix).
    pub id: String,
    pub label: String,
}

#[derive(Debug)]
pub struct Bank {
    /// The folder's name; stable, saved in songs.
    pub id: String,
    pub dir: PathBuf,
    pub character: Character,
    pub acoustic: SubModel,
    pub dur: Option<SubModel>,
    pub pitch: Option<SubModel>,
    pub variance: Option<SubModel>,
    /// The vocoder, or why there is none.
    pub vocoder: Result<VocoderRef, String>,
    pub speakers: Vec<Speaker>,
    /// BCP 47 codes; the first is the default.
    pub languages: Vec<String>,
    pub terms: Option<String>,
    /// What is wrong with the bank short of not loading (a model folder
    /// whose files are missing, say).
    pub problems: Vec<String>,
    dictionaries: Mutex<HashMap<PathBuf, Arc<Dictionary>>>,
}

impl Bank {
    pub fn load(dir: &Path, dependency_dirs: &[PathBuf]) -> Result<Bank, String> {
        let acoustic = SubModel::load(dir)?;
        acoustic.require(&[acoustic.config.acoustic.as_ref()])?;
        let id = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string());
        let character = Character::parse(
            read_text(&dir.join("character.txt")).ok().as_deref(),
            read_text(&dir.join("character.yaml")).ok().as_deref(),
        );
        let mut problems = Vec::new();
        let mut optional = |folder: &str, needs: &dyn Fn(&DsConfig) -> Vec<Option<String>>| {
            let sub_dir = dir.join(folder);
            if !sub_dir.join("dsconfig.yaml").is_file() {
                return None;
            }
            let loaded = SubModel::load(&sub_dir).and_then(|sub| {
                let names = needs(&sub.config);
                sub.require(&names.iter().map(Option::as_ref).collect::<Vec<_>>())?;
                Ok(sub)
            });
            match loaded {
                Ok(sub) => Some(sub),
                Err(e) => {
                    problems.push(format!("{folder} is not used: {e}"));
                    None
                }
            }
        };
        let dur = optional("dsdur", &|c| vec![c.linguistic.clone(), c.dur.clone()]);
        let pitch = optional("dspitch", &|c| vec![c.linguistic.clone(), c.pitch.clone()]);
        let variance = optional("dsvariance", &|c| {
            vec![c.linguistic.clone(), c.variance.clone()]
        });
        let vocoder = match &acoustic.config.vocoder {
            Some(name) => find_vocoder(name, dir, dependency_dirs),
            None => {
                find_vocoder("", dir, &[]).map_err(|_| "dsconfig.yaml names no vocoder".to_string())
            }
        };
        if let Ok(v) = &vocoder
            && let Some(why) = v.config.mismatch(&acoustic.config)
        {
            problems.push(format!("the vocoder does not fit: {why}"));
        }
        let speakers = speakers_of(&character, &acoustic.config);
        let mut languages: Vec<String> = Vec::new();
        if let Some(code) = character.phonemizer_language() {
            languages.push(code.to_string());
        }
        if let Some(l) = &acoustic.languages {
            languages.extend(l.codes());
        }
        for folder in [dir.join("dsdur"), dir.to_path_buf()] {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            let mut found: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let code = name.strip_prefix("dsdict-")?.strip_suffix(".yaml")?;
                    Some(code.to_string())
                })
                .collect();
            found.sort();
            languages.extend(found);
        }
        let mut seen = std::collections::HashSet::new();
        languages.retain(|l| seen.insert(l.clone()));
        if languages.is_empty() {
            languages.push("ja".into());
        }
        Ok(Bank {
            id,
            dir: dir.to_path_buf(),
            terms: terms_of(dir),
            character,
            acoustic,
            dur,
            pitch,
            variance,
            vocoder,
            speakers,
            languages,
            problems,
            dictionaries: Mutex::new(HashMap::new()),
        })
    }

    /// The bank's language for a phrase's BCP 47 tag: its own code when the
    /// bank sings it, else the bank's default.
    pub fn language_for(&self, tag: &str) -> String {
        let tag = tag.trim().to_ascii_lowercase().replace('_', "-");
        let primary = tag.split('-').next().unwrap_or("");
        let primary = match primary {
            "cmn" => "zh",
            "jpn" => "ja",
            "eng" => "en",
            other => other,
        };
        let wanted = [tag.as_str(), primary];
        for want in wanted {
            if let Some(l) = self.languages.iter().find(|l| l.eq_ignore_ascii_case(want)) {
                return l.clone();
            }
        }
        self.languages[0].clone()
    }

    /// The dictionary file for a language: `dsdur/dsdict-<lang>.yaml`, then
    /// `dsdur/dsdict.yaml`, then the same at the bank's root.
    pub fn dictionary_path(&self, language: &str) -> Option<PathBuf> {
        let named = format!("dsdict-{language}.yaml");
        [self.dir.join("dsdur"), self.dir.clone()]
            .iter()
            .flat_map(|d| [d.join(&named), d.join("dsdict.yaml")])
            .find(|p| p.is_file())
    }

    /// The dictionary for a language, read once and kept.
    pub fn dictionary(&self, language: &str) -> Result<Arc<Dictionary>, String> {
        let path = self.dictionary_path(language).ok_or_else(|| {
            format!(
                "{} has no phoneme dictionary (dsdict.yaml) for \"{language}\"",
                self.id
            )
        })?;
        let mut cache = self.dictionaries.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(d) = cache.get(&path) {
            return Ok(d.clone());
        }
        let inventory = &self.acoustic.inventory;
        let d = Arc::new(
            Dictionary::parse(&read_text(&path)?)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .with_unprefixed(language, |s| inventory.contains(s)),
        );
        cache.insert(path, d.clone());
        Ok(d)
    }

    pub fn capabilities(&self) -> Capabilities {
        use fontelle_sing::{Lane, LaneInfo, NoteOptionInfo, SettingInfo};
        let a = &self.acoustic.config;
        let predicts = |v: crate::config::Variance| {
            self.variance
                .as_ref()
                .is_some_and(|m| m.config.predicted_variances().contains(&v))
        };
        let mut lanes = vec![Lane::PitchBend, Lane::Dynamics];
        for (embedded, v, lane) in [
            (
                a.use_breathiness_embed,
                crate::config::Variance::Breathiness,
                Lane::Breathiness,
            ),
            (
                a.use_voicing_embed,
                crate::config::Variance::Voicing,
                Lane::Voicing,
            ),
            (
                a.use_tension_embed,
                crate::config::Variance::Tension,
                Lane::Tension,
            ),
        ] {
            if embedded && predicts(v) {
                lanes.push(lane);
            }
        }
        if a.use_key_shift_embed && a.key_shift_range().is_some() {
            lanes.push(Lane::Gender);
        }
        let mut extra_lanes = Vec::new();
        if a.use_energy_embed && predicts(crate::config::Variance::Energy) {
            extra_lanes.push(LaneInfo {
                id: LANE_ENERGY.into(),
                name: "Energy".into(),
                min: -12.0,
                max: 12.0,
                default: 0.0,
                unit: "dB".into(),
            });
        }
        if self.pitch.as_ref().is_some_and(|p| p.config.use_expr) {
            extra_lanes.push(LaneInfo {
                id: LANE_EXPRESSIVENESS.into(),
                name: "Pitch expressiveness".into(),
                min: 0.0,
                max: 100.0,
                default: 100.0,
                unit: "%".into(),
            });
        }
        let mut note_options = Vec::new();
        if self.speakers.len() > 1 {
            for s in &self.speakers {
                extra_lanes.push(LaneInfo {
                    id: format!("{LANE_SPEAKER_PREFIX}{}", s.id),
                    name: format!("Mix in {}", s.label),
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    unit: "%".into(),
                });
            }
            note_options.push(NoteOptionInfo {
                id: OPTION_SPEAKER.into(),
                name: "Voice colour".into(),
                choices: self
                    .speakers
                    .iter()
                    .map(|s| (s.id.clone(), s.label.clone()))
                    .collect(),
            });
        }
        let mut settings = vec![SettingInfo {
            id: SETTING_STEPS.into(),
            name: "Sampling steps (final render)".into(),
            min: 1.0,
            max: 100.0,
            default: DEFAULT_STEPS as f32,
            unit: String::new(),
            choices: Vec::new(),
        }];
        if self.pitch.is_some() {
            settings.push(SettingInfo {
                id: SETTING_NATURAL_PITCH.into(),
                name: "Natural pitch".into(),
                min: 0.0,
                max: 1.0,
                default: 1.0,
                unit: String::new(),
                // Off: the notes' own line; on: the voice's pitch model
                // where nothing is drawn.
                choices: vec!["Off".into(), "On".into()],
            });
        }
        let phonemes = self
            .acoustic
            .inventory
            .symbols()
            .iter()
            .filter(|s| !(s.starts_with('<') && s.ends_with('>')))
            .cloned()
            .collect();
        Capabilities {
            lanes,
            vibrato: true,
            portamento: true,
            velocity: true,
            phoneme_timing: true,
            phonemes,
            extra_lanes,
            settings,
            note_options,
        }
    }

    pub fn voice_info(&self) -> VoiceInfo {
        let image = [&self.character.image, &self.character.portrait]
            .into_iter()
            .flatten()
            .map(|i| self.dir.join(i))
            .find(|p| p.is_file())
            .map(|p| p.display().to_string());
        VoiceInfo {
            id: self.id.clone(),
            name: self
                .character
                .name
                .clone()
                .unwrap_or_else(|| self.id.clone()),
            languages: self.languages.clone(),
            // DiffSinger banks do not declare a range; this is a typical
            // one, C3 to G5.
            lowest: 48,
            highest: 79,
            research_only: false,
            terms: self.terms.clone(),
            image,
        }
    }
}

/// A speaker for each voice colour (`character.yaml` subbanks), else one
/// for each embedding the acoustic model lists.
fn speakers_of(character: &Character, config: &DsConfig) -> Vec<Speaker> {
    let Some(listed) = &config.speakers else {
        return Vec::new();
    };
    let from_colours: Vec<Speaker> = character
        .subbanks
        .iter()
        .filter(|s| !s.suffix.is_empty())
        .map(|s| Speaker {
            id: s.suffix.clone(),
            label: if s.color.is_empty() {
                s.suffix.clone()
            } else {
                s.color.clone()
            },
        })
        .collect();
    if !from_colours.is_empty() {
        return from_colours;
    }
    listed
        .iter()
        .map(|s| Speaker {
            id: s.clone(),
            label: s.rsplit(['/', '\\']).next().unwrap_or(s).to_string(),
        })
        .collect()
}

/// The author's terms: the bank's readme and any terms or licence file,
/// as text; for terms only in a document this cannot read, where they are.
fn terms_of(dir: &Path) -> Option<String> {
    const NAMES: [&str; 12] = [
        "readme", "terms", "license", "licence", "rule", "利用", "規約", "规约", "条款", "使用",
        "readme", "copying",
    ];
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            NAMES.iter().any(|n| name.contains(n))
        })
        .collect();
    files.sort();
    let mut text = String::new();
    let mut elsewhere = Vec::new();
    for f in &files {
        let ext = f
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let name = f.file_name().unwrap_or_default().to_string_lossy();
        if ext == "txt" || ext == "md" || ext.is_empty() {
            let Ok(bytes) = std::fs::read(f) else {
                continue;
            };
            let body = String::from_utf8_lossy(&bytes);
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            if files.len() > 1 {
                text.push_str(&format!("— {name} —\n"));
            }
            text.push_str(crate::config::strip_bom(body.trim()));
        } else {
            elsewhere.push(name.into_owned());
        }
    }
    if !elsewhere.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&format!(
            "The voicebank's terms are also in {} in its folder.",
            elsewhere.join(", ")
        ));
    }
    const LIMIT: usize = 64 * 1024;
    if text.len() > LIMIT {
        let mut cut = LIMIT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push('…');
    }
    (!text.is_empty()).then_some(text)
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Bank folders under each folder (a folder with a `dsconfig.yaml` naming
/// an acoustic model), up to three levels down.
pub fn find_banks(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let config = dir.join("dsconfig.yaml");
        if config.is_file()
            && let Ok(text) = read_text(&config)
            && DsConfig::parse(&text).is_ok_and(|c| c.acoustic.is_some())
        {
            out.push(dir.to_path_buf());
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut subdirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for d in subdirs {
            walk(&d, depth - 1, out);
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        walk(d, 3, &mut out);
    }
    out
}

/// The vocoder a bank names: its own `dsvocoder/`, else a dependency folder
/// of that name, else a dependency whose `oudep.yaml` id or `vocoder.yaml`
/// name is it.
pub fn find_vocoder(
    name: &str,
    bank_dir: &Path,
    dependency_dirs: &[PathBuf],
) -> Result<VocoderRef, String> {
    let load = |dir: &Path| -> Result<VocoderRef, String> {
        let config = VocoderConfig::parse(&read_text(&dir.join("vocoder.yaml"))?)
            .map_err(|e| format!("{}: {e}", dir.join("vocoder.yaml").display()))?;
        let r = VocoderRef {
            dir: dir.to_path_buf(),
            config,
        };
        if !r.model_path().is_file() {
            return Err(format!("{} is missing", r.model_path().display()));
        }
        Ok(r)
    };
    let own = bank_dir.join("dsvocoder");
    if own.join("vocoder.yaml").is_file() {
        return load(&own);
    }
    if name.is_empty() {
        return Err("no vocoder is named".into());
    }
    for deps in dependency_dirs {
        let named = deps.join(name);
        if named.join("vocoder.yaml").is_file() {
            return load(&named);
        }
    }
    for deps in dependency_dirs {
        let Ok(entries) = std::fs::read_dir(deps) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            if !dir.join("vocoder.yaml").is_file() {
                continue;
            }
            let id = read_text(&dir.join("oudep.yaml"))
                .ok()
                .and_then(|t| crate::config::DependencyInfo::parse(&t).ok())
                .map(|d| d.id);
            let vocoder_name = read_text(&dir.join("vocoder.yaml"))
                .ok()
                .and_then(|t| VocoderConfig::parse(&t).ok())
                .map(|v| v.name);
            if id.as_deref() == Some(name) || vocoder_name.as_deref() == Some(name) {
                return load(&dir);
            }
        }
    }
    Err(format!(
        "the vocoder \"{name}\" this voicebank needs is not installed \u{2014} drop its .oudep \
         (the bank\u{2019}s readme says where to get it) on the window to install it"
    ))
}
