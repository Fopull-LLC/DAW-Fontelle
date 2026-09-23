//! Writes `assets/presets/` from the recipes that used to be constructor code
//! (`docs/flopsynth-plan.md` §P.9).
//!
//! # Why a tool rather than a constructor
//!
//! Before the preset system, a preset was a `match` arm in the effect that
//! owned it: `DistortionConfig::from_preset(2)` wrote seven fields and the
//! panel drew a chip that called it. That works for one effect and does not
//! scale to a DAW — every new device would have to grow its own bank, its own
//! chip row, its own "which one am I on" recogniser, and none of it would be
//! searchable, favouritable or savable-over by the person using it.
//!
//! So the recipes **stay** — they are the authoring tool, the same position
//! `DrumKitStyle` keeps — and what changes is where their output lives. This
//! runs them once and writes each result as a preset file; the files are
//! committed, embedded by `fontelle-app/build.rs`, and read by the same bank
//! that reads the user's own. A person can then rename one, star it, save
//! over a copy of it, or delete it, none of which a `match` arm can do.
//!
//! # Idempotent
//!
//! Running it twice writes the same bytes, and running it after a recipe
//! changes rewrites exactly the file that changed — which is what makes the
//! diff of a sound-design change reviewable. Files that are no longer
//! generated are **left alone**: this tool owns what it writes, not the
//! folder, because a user preset that somebody dropped into the tree by hand
//! is not ours to delete.

use fontelle_types::{
    BitcrushConfig, BitcrushPreset, ChorusConfig, ChorusPreset, CompressorConfig, CompressorPreset,
    DelayConfig, DelayPreset, DeviceKind, DistortionConfig, DistortionPreset, EffectConfig,
    EqConfig, EqPreset, FilterConfig, FilterPreset, FlangerConfig, FlangerPreset, FoldConfig,
    FoldPreset, GateConfig, GatePreset, HyperConfig, HyperPreset, InstrumentKind, LimiterConfig,
    LimiterPreset, MultibandConfig, MultibandPreset, NotepadConfig, NotepadPreset, PhaserConfig,
    PhaserPreset, Preset, PresetPayload, ReverbConfig, ReverbPreset, ShifterConfig, ShifterPreset,
    SoftenConfig, SoftenPreset, TrackChain, TrackPreset, TuneConfig, TunePreset, UtilityConfig,
    UtilityPreset, WidthConfig, WidthPreset,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn export() -> Result<String, String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/presets");
    let mut written = 0usize;
    let mut unchanged = 0usize;

    for preset in every_preset()? {
        match write(&root, &preset)? {
            true => written += 1,
            false => unchanged += 1,
        }
    }

    Ok(format!(
        "{} presets under {} ({written} written, {unchanged} already current)",
        written + unchanged,
        root.display()
    ))
}

/// Every factory preset this build knows how to make.
fn every_preset() -> Result<Vec<Preset>, String> {
    let mut out = Vec::new();
    out.extend(effect_presets());
    out.extend(disgusting_beat_presets());
    out.extend(drum_kits()?);
    out.extend(flopsynth()?);
    Ok(out)
}

/// The built-in effects' own presets.
///
/// One category, "Factory", and deliberately: a distortion's seven presets are
/// seven points on one control surface, not seven kinds of thing. Inventing
/// families for them here would be a taxonomy that exists in this file and
/// nowhere else.
fn effect_presets() -> Vec<Preset> {
    let mut out = Vec::new();
    for preset in DistortionPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Distortion(DistortionConfig::from_preset(preset)),
        ));
    }
    for preset in BitcrushPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Bitcrush(BitcrushConfig::from_preset(preset)),
        ));
    }
    for preset in SoftenPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Soften(SoftenConfig::from_preset(preset)),
        ));
    }
    // The pitch corrector's forty (`docs/tune-plan.md` §6 and §4.8). One category
    // like the rest: these are points on one control surface, and "hard tune"
    // and "cheap plastic" are the same three knobs at different settings.
    for preset in TunePreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Tune(TuneConfig::from_preset(preset)),
        ));
    }
    // The eight banks the other effects shipped without
    // (`fontelle-types/src/effect_presets.rs`). Same one category, same
    // reason: each is one control surface with named places on it.
    for preset in CompressorPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Compressor(CompressorConfig::from_preset(preset)),
        ));
    }
    for preset in LimiterPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Limiter(LimiterConfig::from_preset(preset)),
        ));
    }
    for preset in GatePreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Gate(GateConfig::from_preset(preset)),
        ));
    }
    for preset in ChorusPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Chorus(ChorusConfig::from_preset(preset)),
        ));
    }
    for preset in DelayPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Delay(DelayConfig::from_preset(preset)),
        ));
    }
    for preset in ReverbPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Reverb(ReverbConfig::from_preset(preset)),
        ));
    }
    for preset in FilterPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Filter(FilterConfig::from_preset(preset)),
        ));
    }
    for preset in EqPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Eq(EqConfig::from_preset(preset)),
        ));
    }
    for preset in UtilityPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Utility(UtilityConfig::from_preset(preset)),
        ));
    }
    // The seven of `docs/flopsynth-next.md` §4.5, with banks from the day
    // they exist.
    for preset in PhaserPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Phaser(PhaserConfig::from_preset(preset)),
        ));
    }
    for preset in FlangerPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Flanger(FlangerConfig::from_preset(preset)),
        ));
    }
    for preset in FoldPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Fold(FoldConfig::from_preset(preset)),
        ));
    }
    for preset in ShifterPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Shifter(ShifterConfig::from_preset(preset)),
        ));
    }
    for preset in HyperPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Hyper(HyperConfig::from_preset(preset)),
        ));
    }
    for preset in MultibandPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Multiband(MultibandConfig::from_preset(preset)),
        ));
    }
    for preset in WidthPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Width(WidthConfig::from_preset(preset)),
        ));
    }
    // The notepad's bank is its **looks**: the pad has nothing to say about
    // the sound, so a preset here is a palette, and there is one per theme
    // (`fontelle-types/src/notepad.rs`).
    for preset in NotepadPreset::ALL {
        out.push(effect_preset(
            preset.label(),
            EffectConfig::Notepad(NotepadConfig::from_preset(preset)),
        ));
    }
    // The sixteen **track** chains — a whole mixer strip rather than one
    // device. Three categories rather than one, because unlike the corrector's
    // forty these are not points on one control surface: "Spoken Word" and
    // "Hyperpop Lead" have nothing in common to be different settings of, and
    // a shelf somebody can skip past is worth more than a single long list.
    for preset in TrackPreset::ALL {
        out.push(Preset::new(
            DeviceKind::Track,
            preset.label(),
            preset.category(),
            PresetPayload::Track(TrackChain::from_preset(preset)),
        ));
    }
    out
}

/// DisgustingBeat's sixty-four rows and six kits
/// (`docs/disgusting-beat-plan.md` §8).
///
/// The **only** effect with real categories: every other one's presets are
/// points on one control surface, and a scratch and a sidechain pump are not
/// that. Each row carries the sentence that says what it is for, which the
/// browser shows.
fn disgusting_beat_presets() -> Vec<Preset> {
    fontelle_types::DisgustingBeatFactoryPreset::ALL
        .iter()
        .map(|row| {
            let (config, bank) = row.build();
            Preset::new(
                DeviceKind::Effect(fontelle_types::EffectKind::DisgustingBeat),
                row.name,
                row.category,
                PresetPayload::DisgustingBeat(fontelle_types::DisgustingBeatPreset {
                    config,
                    bank,
                }),
            )
            .with_words(Vec::new(), row.notes.to_string(), "Fontelle")
        })
        .collect()
}

fn effect_preset(name: &str, config: EffectConfig) -> Preset {
    Preset::new(
        DeviceKind::Effect(config.kind()),
        name,
        "Factory",
        PresetPayload::Effect(config),
    )
}

/// The drum machine's twenty-two kits.
fn drum_kits() -> Result<Vec<Preset>, String> {
    let mut out = Vec::new();
    for style in fontelle_core::DrumKitStyle::ALL {
        let patch = fontelle_core::drum_kit(style);
        out.push(Preset::new(
            DeviceKind::Instrument(InstrumentKind::DrumMachine),
            style.label(),
            "Kits",
            PresetPayload::Patch(patch_data(&patch)?),
        ));
    }
    Ok(out)
}

/// Flopsynth's bank, by the category each row declares.
fn flopsynth() -> Result<Vec<Preset>, String> {
    let mut out = Vec::new();
    for row in fontelle_core::flopsynth::presets::FACTORY {
        let patch = (row.build)();
        // The words (§5.1): the tags the row derives and wrote, and its
        // showcase phrase — into the file, where the browser reads them.
        let tags = fontelle_core::flopsynth::presets::tags_of(row, &patch);
        let notes = fontelle_core::flopsynth::presets::notes_of(row, &patch);
        out.push(
            Preset::new(
                DeviceKind::Instrument(InstrumentKind::Flopsynth),
                row.name,
                row.category.label(),
                PresetPayload::Patch(patch_data(&patch)?),
            )
            .with_words(tags, notes, "Fontelle"),
        );
    }
    Ok(out)
}

/// A patch as a preset file stores it.
///
/// The provenance map is empty because none of these patches has audio behind
/// it: a synth's layers are oscillators and a drum kit's are synthesised
/// voices. A preset that *did* name a sample would carry a `SampleRef` here
/// and relink like a project does (§P.3), which is exactly why this goes
/// through the same function a project save does rather than around it.
fn patch_data(patch: &fontelle_core::Patch) -> Result<fontelle_types::PatchData, String> {
    patch.to_data(&HashMap::new()).map_err(|e| format!("{e:?}"))
}

/// Writes one preset, and says whether the file changed.
fn write(root: &Path, preset: &Preset) -> Result<bool, String> {
    let folder = root.join(preset.device.slug()).join(&preset.category);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!("{}.json", preset.name));
    // Pretty, and with a trailing newline: these files are committed, and a
    // diff of a sound-design change should read as one.
    let mut text = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
    text.push('\n');
    if std::fs::read_to_string(&path).is_ok_and(|current| current == text) {
        return Ok(false);
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows that would write to the same file are the one way this tool
    /// can be non-idempotent: whichever ran last would win, and the *pair*
    /// would flip the committed bytes on alternate runs depending on nothing.
    #[test]
    fn no_two_presets_want_the_same_file() {
        let mut seen: Vec<(String, String, String)> = Vec::new();
        for preset in every_preset().unwrap() {
            let at = (
                preset.device.slug(),
                preset.category.clone(),
                preset.name.clone(),
            );
            assert!(!seen.contains(&at), "two presets both want {at:?}");
            seen.push(at);
        }
    }

    #[test]
    fn every_generated_preset_is_a_preset_for_the_device_it_claims() {
        for preset in every_preset().unwrap() {
            assert!(
                preset.is_consistent(),
                "{} is a {} holding somebody else's payload",
                preset.name,
                preset.device.label()
            );
        }
    }

    /// A name that cannot be a file name is a preset that either fails to
    /// write or writes somewhere else — the same rule `PresetBank::save`
    /// enforces for the user's own, checked here for the ones we generate.
    #[test]
    fn every_generated_name_can_be_a_file_name() {
        for preset in every_preset().unwrap() {
            for part in [&preset.name, &preset.category] {
                assert!(!part.trim().is_empty(), "{preset:?} has a blank name");
                assert!(
                    !part.contains(['/', '\\', ':']),
                    "{part:?} cannot be a file name"
                );
            }
        }
    }

    #[test]
    fn running_it_twice_writes_nothing_the_second_time() {
        let root = std::env::temp_dir().join(format!("fontelle-xtask-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let presets = every_preset().unwrap();
        let first = presets.iter().filter(|p| write(&root, p).unwrap()).count();
        assert_eq!(first, presets.len(), "a fresh folder is all new files");
        let second = presets.iter().filter(|p| write(&root, p).unwrap()).count();
        assert_eq!(second, 0, "the same recipes wrote different bytes");
        std::fs::remove_dir_all(&root).ok();
    }
}
