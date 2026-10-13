//! Fixtures written by the tests themselves: a voicebank folder in the
//! OpenUtau layout with no real model in it.
#![allow(dead_code)]

pub mod proto;

use std::path::{Path, PathBuf};

/// A fresh folder for one test, under the target directory.
pub fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fontelle-diffsinger")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub fn write_embedding(path: &Path, values: &[f32]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(path, bytes).unwrap();
}

pub const PHONEMES: &str = r#"{"<PAD>": 0, "AP": 1, "SP": 2, "en/aa": 3, "en/hh": 4, "en/l": 5, "en/ow": 6, "ja/a": 7, "ja/k": 8}"#;

pub const DICT_EN: &str = "symbols:
- symbol: en/aa
  type: vowel
- symbol: en/ow
  type: vowel
- symbol: en/hh
  type: fricative
- symbol: en/l
  type: liquid
entries:
- grapheme: hello
  phonemes: [en/hh, en/aa, en/l, en/ow]
- grapheme: la
  phonemes: [en/l, en/aa]
";

pub const DICT_ALL: &str = "symbols:
- symbol: ja/a
  type: vowel
- symbol: ja/k
  type: stop
entries:
- grapheme: ka
  phonemes: [ja/k, ja/a]
";

/// A two-speaker, two-language bank with every model folder, and its
/// vocoder installed as a dependency named differently from its id.
/// Returns (bank folder, dependencies folder).
pub fn write_bank(root: &Path) -> (PathBuf, PathBuf) {
    let bank = root.join("singers").join("Test Bank");
    let deps = root.join("Dependencies");
    write(
        &bank.join("character.txt"),
        "name=Test Singer\nimage=icon.png\nauthor=Tests\n",
    );
    write(
        &bank.join("character.yaml"),
        "default_phonemizer: OpenUtau.Core.DiffSinger.DiffSingerJapanesePhonemizer\nsinger_type: diffsinger\nsubbanks:\n- color: Soft\n  suffix: alto\n- color: Bright\n  suffix: soprano\n",
    );
    write(&bank.join("icon.png"), "not really a png");
    write(
        &bank.join("readme.txt"),
        "Free for any use. Credit appreciated.",
    );
    write(
        &bank.join("dsconfig.yaml"),
        "phonemes: a.phonemes.json\nlanguages: a.languages.json\nuse_lang_id: true\nacoustic: a.onnx\nhidden_size: 4\nvocoder: test_vocoder\nspeakers:\n- emb/alto\n- emb/soprano\nuse_key_shift_embed: true\nuse_speed_embed: true\nuse_breathiness_embed: true\nuse_voicing_embed: true\naugmentation_args:\n  random_pitch_shifting:\n    range: [-5.0, 5.0]\nuse_continuous_acceleration: true\nuse_variable_depth: true\nmax_depth: 0.6\n",
    );
    write(&bank.join("a.onnx"), "");
    write(&bank.join("a.phonemes.json"), PHONEMES);
    write(&bank.join("a.languages.json"), r#"{"en": 1, "ja": 2}"#);
    write_embedding(&bank.join("emb/alto.emb"), &[1.0, 0.0, 0.0, 0.0]);
    write_embedding(&bank.join("emb/soprano.emb"), &[0.0, 1.0, 0.0, 0.0]);
    for (sub, extra) in [
        (
            "dsdur",
            "linguistic: l.onnx\ndur: d.onnx\npredict_dur: true\n",
        ),
        (
            "dspitch",
            "linguistic: l.onnx\npitch: p.onnx\npredict_dur: false\nuse_expr: true\nuse_note_rest: true\n",
        ),
        (
            "dsvariance",
            "linguistic: l.onnx\nvariance: v.onnx\npredict_dur: false\npredict_energy: false\npredict_breathiness: true\npredict_voicing: true\n",
        ),
    ] {
        let d = bank.join(sub);
        write(
            &d.join("dsconfig.yaml"),
            &format!(
                "phonemes: p.json\nlanguages: l.json\nuse_lang_id: true\nhidden_size: 4\nspeakers:\n- emb/soprano\n- emb/alto\nuse_continuous_acceleration: true\n{extra}"
            ),
        );
        write(&d.join("p.json"), PHONEMES);
        write(&d.join("l.json"), r#"{"en": 1, "ja": 2}"#);
        for f in ["l.onnx", "d.onnx", "p.onnx", "v.onnx"] {
            write(&d.join(f), "");
        }
        write_embedding(&d.join("emb/alto.emb"), &[1.0, 0.0, 0.0, 0.0]);
        write_embedding(&d.join("emb/soprano.emb"), &[0.0, 1.0, 0.0, 0.0]);
    }
    write(&bank.join("dsdur/dsdict-en.yaml"), DICT_EN);
    write(&bank.join("dsdur/dsdict.yaml"), DICT_ALL);
    let vocoder = deps.join("installed under another name");
    write(
        &vocoder.join("oudep.yaml"),
        "id: \"test_vocoder\"\nname: \"Test vocoder\"\nversion: \"1\"\n",
    );
    write(
        &vocoder.join("vocoder.yaml"),
        "name: \"test_vocoder\"\nmodel: \"v.onnx\"\nsample_rate: 44100\nhop_size: 512\nnum_mel_bins: 128\nmel_base: e\n",
    );
    write(&vocoder.join("v.onnx"), "");
    (bank, deps)
}
