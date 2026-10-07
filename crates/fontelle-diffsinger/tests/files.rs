//! Reading a voicebank's own files: configs, inventories, dictionaries.
//! Every fixture is written here, by hand, in the layout the published
//! format and OpenUtau use; no voicebank's data is in the tree.

use fontelle_diffsinger::config::{Character, DependencyInfo, DsConfig, Variance, VocoderConfig};
use fontelle_diffsinger::dict::{Dictionary, SymbolKind};
use fontelle_diffsinger::inventory::{Inventory, Languages, symbol_language};

const ROOT_CONFIG: &str = r#"
phonemes: acoustic.phonemes.json
languages: acoustic.languages.json
use_lang_id: true
acoustic: acoustic.onnx
hidden_size: 384
vocoder: some_vocoder_2025.02
speakers:
- embeds/alto
- embeds/soprano
augmentation_args:
  random_pitch_shifting:
    range:
    - -5.0
    - 7.0
use_key_shift_embed: true
use_speed_embed: true
use_breathiness_embed: true
use_voicing_embed: true
use_continuous_acceleration: true
use_variable_depth: true
max_depth: 0.6
sample_rate: 44100
hop_size: 512
num_mel_bins: 128
mel_base: e
some_key_from_a_newer_exporter: 3
"#;

#[test]
fn a_bank_config_reads_its_keys_and_defaults_the_rest() {
    let c = DsConfig::parse(ROOT_CONFIG).unwrap();
    assert_eq!(c.phonemes, "acoustic.phonemes.json");
    assert_eq!(c.languages.as_deref(), Some("acoustic.languages.json"));
    assert!(c.use_lang_id);
    assert_eq!(c.hidden_size, 384);
    assert_eq!(c.vocoder.as_deref(), Some("some_vocoder_2025.02"));
    assert_eq!(
        c.speakers.as_deref(),
        Some(&["embeds/alto".to_string(), "embeds/soprano".to_string()][..])
    );
    assert_eq!(c.key_shift_range(), Some((-5.0, 7.0)));
    assert!(c.variable_depth());
    assert_eq!(c.max_depth_fraction(), Some(0.6));
    assert_eq!(c.mel_base, "e");
    // Not written: the format's defaults.
    assert_eq!(c.win_size, 2048);
    assert_eq!(c.mel_scale, "slaney");
    assert!(c.predict_dur);
    assert!((c.frame_seconds() - 512.0 / 44100.0).abs() < 1e-12);
    assert_eq!(
        c.embedded_variances(),
        vec![Variance::Breathiness, Variance::Voicing]
    );
}

#[test]
fn discrete_depth_is_out_of_a_thousand_steps() {
    let c = DsConfig::parse("use_shallow_diffusion: true\nmax_depth: 400\n").unwrap();
    assert!(c.variable_depth(), "the older key still counts");
    assert_eq!(c.max_depth_fraction(), Some(0.4));
    let c = DsConfig::parse("max_depth: 400\n").unwrap();
    assert!(!c.variable_depth());
    assert_eq!(c.max_depth_fraction(), None);
}

#[test]
fn a_variance_config_lists_what_it_predicts_in_retake_order() {
    let c = DsConfig::parse(
        "linguistic: l.onnx\nvariance: v.onnx\npredict_energy: false\npredict_breathiness: true\npredict_voicing: true\npredict_tension: true\n",
    )
    .unwrap();
    assert_eq!(
        c.predicted_variances(),
        vec![Variance::Breathiness, Variance::Voicing, Variance::Tension]
    );
    // Defaults: energy and breathiness.
    let c = DsConfig::parse("variance: v.onnx\n").unwrap();
    assert_eq!(
        c.predicted_variances(),
        vec![Variance::Energy, Variance::Breathiness]
    );
}

#[test]
fn an_empty_or_broken_config_says_so() {
    assert!(DsConfig::parse("phonemes: [unclosed\n").is_err());
    // An empty file is all defaults.
    assert_eq!(DsConfig::parse("").unwrap(), DsConfig::default());
}

#[test]
fn a_vocoder_config_is_checked_against_the_acoustic_model() {
    let v = VocoderConfig::parse(
        "name: \"v\"\nmodel: \"v.onnx\"\nsample_rate: 44100\nhop_size: 512\nnum_mel_bins: 128\nmel_base: e\npitch_controllable: true\nforce_on_cpu: false\n",
    )
    .unwrap();
    assert_eq!(v.model, "v.onnx");
    assert!(v.pitch_controllable);
    let acoustic = DsConfig::parse(ROOT_CONFIG).unwrap();
    assert_eq!(v.mismatch(&acoustic), None);
    let mut other = acoustic.clone();
    other.hop_size = 256;
    assert!(v.mismatch(&other).unwrap().contains("hop"));
    let mut other = acoustic.clone();
    other.num_mel_bins = 80;
    assert!(v.mismatch(&other).unwrap().contains("mel"));
    // A different log base is converted, not refused.
    let mut other = acoustic.clone();
    other.mel_base = "10".into();
    assert_eq!(v.mismatch(&other), None);
    // Mel bins past the sane limit are refused outright.
    let mut huge = v.clone();
    huge.num_mel_bins = 4000;
    let mut huge_acoustic = acoustic.clone();
    huge_acoustic.num_mel_bins = 4000;
    assert!(huge.mismatch(&huge_acoustic).is_some());
}

#[test]
fn a_dependency_names_itself() {
    let d = DependencyInfo::parse(
        "id: \"some_vocoder_2025.02\"\nname: \"Some vocoder\"\nversion: \"2025.02\"\nclass: X\n",
    )
    .unwrap();
    assert_eq!(d.id, "some_vocoder_2025.02");
    assert_eq!(d.version, "2025.02");
}

#[test]
fn character_files_merge_with_yaml_over_txt() {
    let c = Character::parse(
        Some("name=Test Singer\nimage=icon.png\nauthor=Someone\nweb=https://example.org\n"),
        Some(
            "\u{feff}text_file_encoding: utf-8\nportrait: big.png\ndefault_phonemizer: OpenUtau.Core.DiffSinger.DiffSingerJapanesePhonemizer\nsinger_type: diffsinger\nsubbanks:\n- color: Soft\n  suffix: embeds/alto\n- color: Bright\n  prefix: ''\n  suffix: soprano\n",
        ),
    );
    assert_eq!(c.name.as_deref(), Some("Test Singer"));
    assert_eq!(c.image.as_deref(), Some("icon.png"));
    assert_eq!(c.portrait.as_deref(), Some("big.png"));
    assert_eq!(c.author.as_deref(), Some("Someone"));
    assert_eq!(c.singer_type.as_deref(), Some("diffsinger"));
    assert_eq!(c.phonemizer_language(), Some("ja"));
    assert_eq!(c.subbanks.len(), 2);
    assert_eq!(c.subbanks[1].color, "Bright");
    assert_eq!(c.subbanks[1].suffix, "soprano");
    // The yaml's name wins when it has one.
    let c = Character::parse(Some("name=A\n"), Some("name: B\n"));
    assert_eq!(c.name.as_deref(), Some("B"));
    assert_eq!(Character::parse(None, None), Character::default());
}

#[test]
fn inventories_read_as_text_or_json() {
    let txt = Inventory::parse("AP\nSP\na\nk\n", false).unwrap();
    assert_eq!(txt.token("AP"), Some(0));
    assert_eq!(txt.token("k"), Some(3));
    assert_eq!(txt.token("x"), None);
    assert_eq!(txt.symbols(), &["AP", "SP", "a", "k"]);
    // Windows line ends and a trailing blank line are not symbols.
    let crlf = Inventory::parse("AP\r\nSP\r\n\r\n", false).unwrap();
    assert_eq!(crlf.len(), 2);
    // Merged phonemes share an id.
    let json = Inventory::parse(
        r#"{"AP": 1, "SP": 4, "en/b": 13, "ja/b": 13, "<PAD>": 0}"#,
        true,
    )
    .unwrap();
    assert_eq!(json.token("ja/b"), Some(13));
    assert_eq!(json.token("en/b"), Some(13));
    assert!(json.contains("<PAD>"));
}

#[test]
fn languages_map_a_symbols_prefix_to_its_id() {
    let l = Languages::parse(r#"{"en": 1, "ja": 2, "zh": 3}"#).unwrap();
    assert_eq!(l.id_of_symbol("ja/k"), 2);
    assert_eq!(l.id_of_symbol("SP"), 0);
    assert_eq!(l.id_of_symbol("fr/a"), 0);
    assert_eq!(l.codes(), vec!["en", "ja", "zh"]);
    assert_eq!(symbol_language("en/hh"), Some("en"));
    assert_eq!(symbol_language("AP"), None);
}

const DICT: &str = "symbols:
- symbol: AP
  type: vowel
- symbol: SP
  type: vowel
- symbol: en/aa
  type: vowel
- symbol: en/ow
  type: vowel
- symbol: en/ax
  type: vowel
- symbol: en/hh
  type: fricative
- symbol: en/l
  type: liquid
- symbol: en/w
  type: semivowel
- symbol: \"en/k\"
  type: stop
entries:
- grapheme: hello
  phonemes:
  - en/hh
  - en/ax
  - en/l
  - en/ow
- grapheme: 'flow seq'
  phonemes: [en/hh, en/aa]
- grapheme: dropped
  phonemes:
  - en/hh
  - not/declared
- grapheme: Hi
  phonemes:
  - en/hh
  - en/aa
replacements:
- from: aa
  to: en/aa
";

#[test]
fn a_dictionary_classifies_symbols_and_says_words() {
    let d = Dictionary::parse(DICT).unwrap();
    assert_eq!(d.kind("en/aa"), Some(SymbolKind::Vowel));
    assert_eq!(d.kind("en/l"), Some(SymbolKind::Glide));
    assert_eq!(d.kind("en/w"), Some(SymbolKind::Glide));
    assert_eq!(d.kind("en/k"), Some(SymbolKind::Consonant));
    assert_eq!(d.kind("en/hh"), Some(SymbolKind::Consonant));
    assert!(d.is_vowel("SP") && d.is_vowel("AP"));
    assert_eq!(
        d.query("hello").unwrap(),
        &["en/hh", "en/ax", "en/l", "en/ow"]
    );
    assert_eq!(d.query("HELLO").unwrap().len(), 4, "lower case is tried");
    assert_eq!(
        d.query("Hi").unwrap(),
        &["en/hh", "en/aa"],
        "as written first"
    );
    assert_eq!(d.query("flow seq").unwrap(), &["en/hh", "en/aa"]);
    // Undeclared symbols are dropped from an entry.
    assert_eq!(d.query("dropped").unwrap(), &["en/hh"]);
    assert_eq!(d.query("nowhere"), None);
    assert_eq!(
        d.replacements().get("aa").map(String::as_str),
        Some("en/aa")
    );
    assert_eq!(d.entry_count(), 4);
}

#[test]
fn the_scanner_and_the_yaml_parser_read_alike() {
    let fast = Dictionary::parse(DICT).unwrap();
    let slow = Dictionary::parse_yaml(DICT).unwrap();
    assert_eq!(fast, slow);
    // Flow-style mappings are beyond the scanner and go to the parser.
    let flow = "symbols:\n- {symbol: a, type: vowel}\nentries:\n- {grapheme: x, phonemes: [a]}\nreplacements:\n- {from: b, to: a}\n";
    let d = Dictionary::parse(flow).unwrap();
    assert_eq!(d.query("x").unwrap(), &["a"]);
    assert!(d.is_vowel("a"));
    assert_eq!(d.replacements().get("b").map(String::as_str), Some("a"));
    // Entries before symbols (order is not fixed by the format).
    let reversed =
        "entries:\n- grapheme: x\n  phonemes:\n  - a\nsymbols:\n- symbol: a\n  type: vowel\n";
    assert_eq!(
        Dictionary::parse(reversed).unwrap().query("x").unwrap(),
        &["a"]
    );
    assert_eq!(
        Dictionary::parse("entries: []\nsymbols: []\n").unwrap(),
        Dictionary::parse_yaml("entries: []\nsymbols: []\n").unwrap()
    );
}

#[test]
fn a_large_dictionary_reads_quickly() {
    let mut text =
        String::from("symbols:\n- symbol: a\n  type: vowel\n- symbol: k\n  type: stop\nentries:\n");
    for i in 0..200_000 {
        text.push_str(&format!(
            "- grapheme: w{i}\n  phonemes:\n  - k\n  - a\n  - k\n"
        ));
    }
    let t = std::time::Instant::now();
    let d = Dictionary::parse(&text).unwrap();
    assert_eq!(d.entry_count(), 200_000);
    assert_eq!(d.query("w199999").unwrap(), &["k", "a", "k"]);
    // A release build reads this in a fraction of that; the bound is for
    // debug builds on a slow runner.
    assert!(t.elapsed().as_secs_f64() < 5.0, "{:?}", t.elapsed());
}
