//! Reading voicebanks: encodings, oto.ini, prefix.map, character files,
//! terms and voice colours.

mod common;

use std::path::Path;

use fontelle_utau::bank::{self, Subbank, Voicebank};
use fontelle_utau::oto::{self, OtoEntry};
use fontelle_utau::text::{self, TextEncoding};

fn sjis(s: &str) -> Vec<u8> {
    encoding_rs::SHIFT_JIS.encode(s).0.into_owned()
}

fn gbk(s: &str) -> Vec<u8> {
    encoding_rs::GBK.encode(s).0.into_owned()
}

#[test]
fn text_is_read_in_utf8_shift_jis_or_gbk() {
    let s = "あ.wav=- あ,100,200,-300,150,50\n";
    assert_eq!(
        text::decode(s.as_bytes(), None),
        (s.to_string(), TextEncoding::Utf8)
    );
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(s.as_bytes());
    assert_eq!(text::decode(&bom, None).0, s, "a BOM is dropped");
    assert_eq!(
        text::decode(&sjis(s), None),
        (s.to_string(), TextEncoding::ShiftJis)
    );
    // Chinese banks: hanzi with no kana is GBK.
    let zh = "啊.wav=a,100,200,-300,150,50\n名字=测试声库\n";
    assert_eq!(
        text::decode(&gbk(zh), None),
        (zh.to_string(), TextEncoding::Gbk)
    );
    // A hint wins.
    assert_eq!(
        text::decode(&gbk(zh), Some(TextEncoding::Gbk)).1,
        TextEncoding::Gbk
    );
    assert_eq!(
        text::encoding_from_name("shift_jis"),
        Some(TextEncoding::ShiftJis)
    );
    assert_eq!(text::encoding_from_name("UTF-8"), Some(TextEncoding::Utf8));
    assert_eq!(text::encoding_from_name("gb2312"), Some(TextEncoding::Gbk));
}

#[test]
fn oto_lines_parse_with_utau_defaults() {
    let dir = Path::new("/bank");
    let entries = oto::parse_oto(
        "_ああ.wav=- あ,1000,200,-500,150,50\r\n\
         ka.wav=,10.5,20,30,40,5\n\
         \n\
         broken line\n\
         short.wav=sh,5\n\
         sub/x.wav=x,1,2,3,4,5\n",
        dir,
        "low",
    );
    assert_eq!(entries.len(), 4, "{entries:?}");
    let e = &entries[0];
    assert_eq!(e.alias, "- あ");
    assert_eq!(e.file, dir.join("_ああ.wav"));
    assert_eq!(e.folder, "low");
    assert_eq!(
        (e.offset, e.consonant, e.cutoff, e.preutterance, e.overlap),
        (1000.0, 200.0, -500.0, 150.0, 50.0)
    );
    // An empty alias is the file's name.
    assert_eq!(entries[1].alias, "ka");
    assert_eq!(entries[1].offset, 10.5);
    // Missing numbers are zero.
    assert_eq!(entries[2].alias, "sh");
    assert_eq!((entries[2].offset, entries[2].consonant), (5.0, 0.0));
    assert_eq!(entries[3].file, dir.join("sub").join("x.wav"));
}

#[test]
fn cutoff_is_from_the_end_or_a_length() {
    let mut e = OtoEntry {
        alias: "a".into(),
        file: "a.wav".into(),
        folder: String::new(),
        offset: 100.0,
        consonant: 50.0,
        cutoff: 200.0,
        preutterance: 60.0,
        overlap: 10.0,
    };
    assert_eq!(e.end_ms(1000.0), 800.0);
    e.cutoff = -300.0;
    assert_eq!(e.end_ms(1000.0), 400.0);
    e.cutoff = 0.0;
    assert_eq!(e.end_ms(1000.0), 1000.0);
}

#[test]
fn prefix_map_gives_each_note_its_suffix() {
    assert_eq!(oto::note_number("C4"), Some(60));
    assert_eq!(oto::note_number("C#4"), Some(61));
    assert_eq!(oto::note_number("Bb3"), Some(58));
    assert_eq!(oto::note_number("B7"), Some(107));
    assert_eq!(oto::note_number("x"), None);
    let map = oto::parse_prefix_map("C4\t\t_low\nC#4\t\t_low\nD4\tp\t_high\r\nE4\t\t\n");
    assert_eq!(map.get(60), ("", "_low"));
    assert_eq!(map.get(62), ("p", "_high"));
    assert_eq!(map.get(64), ("", ""));
    assert_eq!(map.get(30), ("", ""), "a note not in the map is plain");
}

fn entry(alias: &str, folder: &str) -> OtoEntry {
    OtoEntry {
        alias: alias.into(),
        file: format!("{folder}/{alias}.wav").into(),
        folder: folder.into(),
        offset: 0.0,
        consonant: 100.0,
        cutoff: -300.0,
        preutterance: 50.0,
        overlap: 10.0,
    }
}

#[test]
fn find_applies_the_subbank_for_the_note_then_falls_back() {
    let entries = vec![entry("あ", ""), entry("あ_H", "high"), entry("か", "")];
    let subbanks = vec![
        Subbank {
            colour: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            tones: vec![(0, 63)],
        },
        Subbank {
            colour: String::new(),
            prefix: String::new(),
            suffix: "_H".into(),
            tones: vec![(64, 127)],
        },
    ];
    let b = Voicebank::from_parts(Path::new("/b"), "b", entries, subbanks);
    assert_eq!(b.find("あ", 60, "").unwrap().alias, "あ");
    assert_eq!(b.find("あ", 70, "").unwrap().alias, "あ_H");
    // No high か: the plain one.
    assert_eq!(b.find("か", 70, "").unwrap().alias, "か");
    assert!(b.find("さ", 60, "").is_none());
    assert!(b.has("か"));
    assert_eq!(b.base_aliases(), vec!["あ".to_string(), "か".to_string()]);
}

#[test]
fn suffix_appends_become_voice_colours() {
    let base = [
        "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し",
    ];
    let mut entries = Vec::new();
    for a in base {
        entries.push(entry(a, ""));
        entries.push(entry(&format!("{a}強"), ""));
        entries.push(entry(&format!("{a}_soft"), ""));
    }
    let b = Voicebank::from_parts(Path::new("/b"), "b", entries, vec![]);
    let ids: Vec<&str> = b.colours.iter().map(|c| c.id.as_str()).collect();
    assert!(
        ids.contains(&"強") && ids.contains(&"_soft"),
        "{:?}",
        b.colours
    );
    let strong = b.colours.iter().find(|c| c.id == "強").unwrap();
    assert_eq!(b.find("か", 60, &strong.id).unwrap().alias, "か強");
    assert_eq!(b.find("か", 60, "").unwrap().alias, "か");
    assert_eq!(b.base_aliases().len(), base.len());
}

#[test]
fn subfolders_with_the_same_aliases_are_voice_colours() {
    let base = ["あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ"];
    let mut entries = Vec::new();
    for folder in ["normal", "whisper"] {
        for a in base {
            entries.push(entry(a, folder));
        }
    }
    let b = Voicebank::from_parts(Path::new("/b"), "b", entries, vec![]);
    let w = b
        .colours
        .iter()
        .find(|c| c.folder.as_deref() == Some("whisper"))
        .expect("whisper is a colour");
    assert_eq!(b.find("か", 60, &w.id).unwrap().folder, "whisper");
    assert_eq!(b.find("か", 60, "").unwrap().folder, "normal");
}

#[test]
fn a_bank_on_disk_loads_its_character_terms_and_every_oto() {
    let dir = common::temp_dir("bank_load");
    std::fs::write(
        dir.join("character.txt"),
        sjis("name=テスト歌手\nimage=icon.bmp\nsample=sample.wav\nauthor=作者\nweb=https://example.org\n"),
    )
    .unwrap();
    std::fs::write(dir.join("readme.txt"), sjis("利用規約：商用利用禁止\n")).unwrap();
    std::fs::write(
        dir.join("oto.ini"),
        sjis("あ.wav=- あ,100,200,-300,150,50\n"),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("強")).unwrap();
    std::fs::write(
        dir.join("強").join("oto.ini"),
        "か.wav=か強,10,20,-30,15,5\n",
    )
    .unwrap();
    std::fs::write(dir.join("prefix.map"), "C4\t\t\nC5\t\t_H\n").unwrap();
    assert!(bank::is_voicebank(&dir));
    let b = Voicebank::load(&dir).unwrap();
    assert_eq!(b.name, "テスト歌手");
    assert_eq!(b.author.as_deref(), Some("作者"));
    assert_eq!(b.web.as_deref(), Some("https://example.org"));
    assert_eq!(b.image, Some(dir.join("icon.bmp")));
    assert_eq!(b.sample, Some(dir.join("sample.wav")));
    assert_eq!(
        b.terms.as_deref().map(str::trim),
        Some("利用規約：商用利用禁止")
    );
    assert_eq!(b.encoding, TextEncoding::ShiftJis);
    assert_eq!(b.entries.len(), 2);
    let strong = b.entries.iter().find(|e| e.alias == "か強").unwrap();
    assert_eq!(strong.folder, "強");
    assert_eq!(strong.file, dir.join("強").join("か.wav"));
    // prefix.map became a subbank per suffix.
    assert!(
        b.subbanks
            .iter()
            .any(|s| s.suffix == "_H" && s.tones.contains(&(72, 72)))
    );
    assert_eq!(
        bank::scan(dir.parent().unwrap())
            .iter()
            .filter(|p| **p == dir)
            .count(),
        1
    );
}

#[test]
fn character_yaml_names_subbanks_and_colours() {
    let dir = common::temp_dir("bank_yaml");
    std::fs::write(
        dir.join("character.yaml"),
        "name: Yaml Singer\nimage: face.png\nauthor: Someone\ntext_file_encoding: utf-8\n\
         default_phonemizer: OpenUtau.Plugin.Builtin.JapaneseCVVCPhonemizer\n\
         subbanks:\n\
         - color: ''\n  prefix: ''\n  suffix: ''\n  tone_ranges:\n  - C1-B4\n\
         - color: ''\n  suffix: _H\n  tone_ranges: [C5-B7]\n\
         - color: Power\n  suffix: P\n  tone_ranges:\n  - C1-B7\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("oto.ini"),
        "a.wav=あ,1,2,3,4,5\nb.wav=あP,1,2,3,4,5\nc.wav=あ_H,1,2,3,4,5\n",
    )
    .unwrap();
    let b = Voicebank::load(&dir).unwrap();
    assert_eq!(b.name, "Yaml Singer");
    assert_eq!(b.image, Some(dir.join("face.png")));
    assert_eq!(
        b.default_phonemizer.as_deref(),
        Some("OpenUtau.Plugin.Builtin.JapaneseCVVCPhonemizer")
    );
    assert_eq!(b.find("あ", 60, "").unwrap().alias, "あ");
    assert_eq!(b.find("あ", 74, "").unwrap().alias, "あ_H");
    assert_eq!(b.find("あ", 60, "Power").unwrap().alias, "あP");
    assert!(b.colours.iter().any(|c| c.id == "Power"));
    assert_eq!(b.range(), (24, 107));
}

#[test]
fn a_bank_without_character_files_is_named_after_its_folder() {
    let dir = common::temp_dir("bank_bare").join("Bare Bank");
    std::fs::create_dir_all(dir.join("A3")).unwrap();
    std::fs::write(dir.join("A3").join("oto.ini"), "a.wav=a,1,2,3,4,5\n").unwrap();
    assert!(bank::is_voicebank(&dir));
    let b = Voicebank::load(&dir).unwrap();
    assert_eq!(b.name, "Bare Bank");
    assert!(b.terms.is_none());
    assert_eq!(b.entries[0].folder, "A3");
}
