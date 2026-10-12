//! Voicebanks: dropped in as a `.zip`, an `.oudep` or a folder, recognised
//! (a classic UTAU bank by its `oto.ini`, a DiffSinger bank by its
//! `dsconfig.yaml`, a dependency such as a vocoder by its own marker),
//! unpacked into Fontelle's voicebanks folder whatever the archive's names
//! were written in (Shift-JIS from an old Japanese Windows, GBK, UTF-8),
//! with its author's terms shown and acknowledged before first use, and
//! removed again.

mod common;

use std::path::{Path, PathBuf};

use common::zip::{Entry, write};
use fontelle_vocal::voicebanks::{
    self, InstallError, PackageKind, acknowledge_terms, decode_text, install, list,
    terms_acknowledged, uninstall,
};

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("fontelle-voicebanks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sjis(text: &str) -> Vec<u8> {
    let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(text);
    assert!(!bad);
    bytes.into_owned()
}

fn gbk(text: &str) -> Vec<u8> {
    let (bytes, _, bad) = encoding_rs::GBK.encode(text);
    assert!(!bad);
    bytes.into_owned()
}

fn archive(dir: &Path, name: &str, entries: &[Entry]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, write(entries)).unwrap();
    path
}

const OTO: &[u8] = b"a.wav=- a,20,100,-200,50,30\n";

#[test]
fn a_shift_jis_utau_bank_in_a_zip_installs_with_its_names_and_terms() {
    let dir = scratch("sjis");
    let banks = dir.join("voicebanks");
    // As a Japanese Windows zipped it: one top folder, Shift-JIS names, no
    // UTF-8 flag, a Shift-JIS readme and character.txt.
    let top = "重音テト音声ライブラリー";
    let zip = archive(
        &dir,
        "teto.zip",
        &[
            Entry::raw(sjis(&format!("{top}/oto.ini")), OTO),
            Entry::raw(sjis(&format!("{top}/あ.wav")), b"RIFF"),
            Entry::raw(
                sjis(&format!("{top}/character.txt")),
                &sjis("name=重音テト\nimage=teto.bmp\nauthor=線の人\n"),
            ),
            Entry::raw(
                sjis(&format!("{top}/readme.txt")),
                &sjis("利用規約：非営利・営利問わず使用できます。"),
            ),
            Entry::raw(sjis(&format!("{top}/teto.bmp")), b"BM"),
        ],
    );
    let bank = install(&zip, &banks).expect("installed");
    assert_eq!(bank.kind, PackageKind::Utau);
    assert_eq!(bank.name, "重音テト");
    assert!(bank.path.starts_with(&banks));
    assert!(
        bank.path.join("oto.ini").is_file(),
        "the top folder is the bank"
    );
    assert!(
        bank.path.join("あ.wav").is_file(),
        "names decoded from Shift-JIS"
    );
    assert!(bank.terms.as_deref().unwrap().contains("営利"));
    assert_eq!(bank.info.author.as_deref(), Some("線の人"));
    assert_eq!(bank.info.portrait, Some(bank.path.join("teto.bmp")));
    // Listed, and its terms not yet agreed to.
    let all = list(&banks);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "重音テト");
    assert!(!terms_acknowledged(&bank.path));
    acknowledge_terms(&bank.path).unwrap();
    assert!(terms_acknowledged(&bank.path));
}

#[test]
fn utf8_and_gbk_names_are_read_as_written() {
    let dir = scratch("names");
    let banks = dir.join("voicebanks");
    let zip = archive(
        &dir,
        "utf8.zip",
        &[
            Entry::utf8("Défoko/oto.ini", OTO),
            Entry::utf8("Défoko/ア.wav", b"RIFF"),
        ],
    );
    let bank = install(&zip, &banks).unwrap();
    assert!(bank.path.join("ア.wav").is_file());
    assert_eq!(bank.name, "Défoko", "no character.txt: the folder's name");
    let zip = archive(
        &dir,
        "gbk.zip",
        &[
            Entry::raw(gbk("女声音源/oto.ini"), OTO),
            Entry::raw(gbk("女声音源/啊.wav"), b"RIFF"),
        ],
    );
    let bank = install(&zip, &banks).unwrap();
    assert!(bank.path.join("啊.wav").is_file(), "{:?}", bank.path);
}

#[test]
fn a_multi_pitch_bank_keeps_its_pitch_folders() {
    let dir = scratch("multipitch");
    let banks = dir.join("voicebanks");
    let zip = archive(
        &dir,
        "multi.zip",
        &[
            Entry::utf8("Release/Bank/character.txt", b"name=Multi\n"),
            Entry::utf8("Release/Bank/prefix.map", b""),
            Entry::utf8("Release/Bank/A3/oto.ini", OTO),
            Entry::utf8("Release/Bank/C4/oto.ini", OTO),
            Entry::utf8("Release/notes.txt", b"thanks"),
        ],
    );
    let bank = install(&zip, &banks).unwrap();
    assert_eq!(bank.name, "Multi");
    assert!(bank.path.join("A3/oto.ini").is_file());
    assert!(bank.path.join("character.txt").is_file());
}

#[test]
fn a_diffsinger_bank_is_told_from_a_utau_one() {
    let dir = scratch("ds");
    let banks = dir.join("voicebanks");
    let zip = archive(
        &dir,
        "qixuan.zip",
        &[
            Entry::utf8(
                "Qixuan/character.yaml",
                b"name: Qixuan\nsinger_type: diffsinger\nportrait: portrait.png\n",
            ),
            Entry::utf8("Qixuan/dsconfig.yaml", b"phonemes: dsmain/phonemes.txt\n"),
            Entry::utf8("Qixuan/portrait.png", b"\x89PNG"),
            Entry::utf8("Qixuan/README.md", b"CC BY-NC-SA 4.0"),
        ],
    );
    let bank = install(&zip, &banks).unwrap();
    assert_eq!(bank.kind, PackageKind::DiffSinger);
    assert_eq!(bank.name, "Qixuan");
    assert_eq!(bank.info.portrait, Some(bank.path.join("portrait.png")));
    assert!(bank.terms.unwrap().contains("CC BY-NC-SA"));
}

#[test]
fn a_dependency_package_goes_beside_the_banks_not_among_them() {
    let dir = scratch("dep");
    let banks = dir.join("voicebanks");
    let oudep = archive(
        &dir,
        "nsf_hifigan.oudep",
        &[
            Entry::utf8("vocoder.yaml", b"name: nsf_hifigan\nmodel: model.onnx\n"),
            Entry::utf8("model.onnx", b"onnx"),
        ],
    );
    let dep = install(&oudep, &banks).unwrap();
    assert_eq!(dep.kind, PackageKind::Dependency);
    assert_eq!(dep.name, "nsf_hifigan");
    assert_eq!(
        dep.path,
        banks.join(voicebanks::DEPENDENCIES).join("nsf_hifigan")
    );
    assert!(list(&banks).is_empty(), "a vocoder is not a singer");
    assert_eq!(voicebanks::dependencies(&banks).len(), 1);
}

#[test]
fn a_folder_installs_as_a_copy() {
    let dir = scratch("folder");
    let src = dir.join("Momo");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("oto.ini"), OTO).unwrap();
    std::fs::write(src.join("a.wav"), b"RIFF").unwrap();
    let banks = dir.join("voicebanks");
    let bank = install(&src, &banks).unwrap();
    assert!(bank.path.join("a.wav").is_file());
    assert!(src.join("a.wav").is_file(), "the original is left alone");
}

#[test]
fn a_second_bank_of_the_same_name_does_not_overwrite_the_first() {
    let dir = scratch("twice");
    let banks = dir.join("voicebanks");
    let zip = archive(&dir, "a.zip", &[Entry::utf8("Bank/oto.ini", OTO)]);
    let one = install(&zip, &banks).unwrap();
    let two = install(&zip, &banks).unwrap();
    assert_ne!(one.path, two.path);
    assert_eq!(list(&banks).len(), 2);
    uninstall(&banks, &two.id).unwrap();
    assert_eq!(list(&banks).len(), 1);
    assert!(!two.path.exists());
    assert!(
        uninstall(&banks, "../outside").is_err(),
        "only a bank's own folder"
    );
}

#[test]
fn what_is_not_a_voicebank_is_refused_and_says_why() {
    let dir = scratch("refuse");
    let banks = dir.join("voicebanks");
    let zip = archive(&dir, "song.zip", &[Entry::utf8("song.mp3", b"ID3")]);
    match install(&zip, &banks) {
        Err(InstallError::NotAVoicebank) => {}
        other => panic!("{other:?}"),
    }
    assert!(InstallError::NotAVoicebank.to_string().contains("oto.ini"));
    // A path that climbs out of the folder is refused, not written.
    let zip = archive(
        &dir,
        "evil.zip",
        &[
            Entry::utf8("Bank/oto.ini", OTO),
            Entry::utf8("../../escaped.txt", b"x"),
        ],
    );
    assert!(matches!(
        install(&zip, &banks),
        Err(InstallError::Unsafe(_))
    ));
    assert!(!dir.join("escaped.txt").exists());
    // Not a zip at all.
    let junk = dir.join("junk.zip");
    std::fs::write(&junk, b"not a zip").unwrap();
    assert!(matches!(
        install(&junk, &banks),
        Err(InstallError::Archive(_))
    ));
    // Nothing half-installed is left behind.
    assert!(list(&banks).is_empty());
}

#[test]
fn changed_terms_are_asked_again() {
    let dir = scratch("terms");
    let banks = dir.join("voicebanks");
    let zip = archive(
        &dir,
        "a.zip",
        &[
            Entry::utf8("B/oto.ini", OTO),
            Entry::utf8("B/readme.txt", b"Free for any use."),
        ],
    );
    let bank = install(&zip, &banks).unwrap();
    acknowledge_terms(&bank.path).unwrap();
    std::fs::write(bank.path.join("readme.txt"), "Non-commercial only.").unwrap();
    assert!(!terms_acknowledged(&bank.path));
}

#[test]
fn text_is_read_in_whichever_encoding_it_was_written() {
    assert_eq!(decode_text("あいう".as_bytes()), "あいう");
    assert_eq!(decode_text(&sjis("音声ライブラリ")), "音声ライブラリ");
    assert_eq!(decode_text(b"\xEF\xBB\xBFname=x"), "name=x");
    assert_eq!(decode_text(&gbk("女声")), "女声");
}

#[test]
fn the_voicebanks_folder_follows_the_apps_folder_rule() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    };
    assert_eq!(
        voicebanks::voicebanks_dir_from(&env(&[("HOME", "/home/ty")]), false, &|_| false),
        Some(PathBuf::from("/home/ty/.local/share/fontelle/voicebanks"))
    );
    assert_eq!(
        voicebanks::voicebanks_dir_from(
            &env(&[("XDG_DATA_HOME", "/data"), ("HOME", "/home/ty")]),
            false,
            &|_| false
        ),
        Some(PathBuf::from("/data/fontelle/voicebanks"))
    );
    assert_eq!(
        voicebanks::voicebanks_dir_from(
            &env(&[("LOCALAPPDATA", "C:\\Users\\ty\\AppData\\Local")]),
            true,
            &|_| false
        ),
        Some(
            PathBuf::from("C:\\Users\\ty\\AppData\\Local")
                .join("fontelle")
                .join("voicebanks")
        )
    );
}

#[test]
fn a_shift_jis_character_txt_names_its_sample_and_picture_with_its_own_backslash() {
    // Teto's: `sample=重音テト単独音\_にゃ.wav` in Shift-JIS, whose backslash
    // byte reads as ¥; and a DiffSinger bank's icon (`image`) is its face,
    // the yaml's `portrait` the large picture behind the roll.
    let dir = scratch("sjis-sample");
    let bank = dir.join("teto");
    std::fs::create_dir_all(bank.join("単独音")).unwrap();
    std::fs::write(bank.join("oto.ini"), OTO).unwrap();
    std::fs::write(bank.join("単独音/_にゃ.wav"), b"RIFF").unwrap();
    std::fs::write(bank.join("teto.bmp"), b"BM").unwrap();
    let mut text = sjis("name=重音テト\r\nimage=teto.bmp\r\nsample=単独音");
    text.push(b'\\');
    text.extend(sjis("_にゃ.wav\r\n"));
    std::fs::write(bank.join("character.txt"), text).unwrap();
    let info = voicebanks::read_info(&bank, PackageKind::Utau);
    assert_eq!(info.sample, Some(bank.join("単独音/_にゃ.wav")));
    assert_eq!(info.portrait, Some(bank.join("teto.bmp")));

    let ds = dir.join("ds");
    std::fs::create_dir_all(&ds).unwrap();
    std::fs::write(ds.join("dsconfig.yaml"), b"phonemes: p.txt\n").unwrap();
    std::fs::write(ds.join("face.png"), b"\x89PNG").unwrap();
    std::fs::write(ds.join("big.png"), b"\x89PNG").unwrap();
    std::fs::write(ds.join("character.txt"), b"image=face.png\n").unwrap();
    std::fs::write(ds.join("character.yaml"), b"portrait: big.png\n").unwrap();
    let info = voicebanks::read_info(&ds, PackageKind::DiffSinger);
    assert_eq!(
        info.portrait,
        Some(ds.join("face.png")),
        "the icon, not the backdrop"
    );
}

/// A bank installed with its files one folder down (as a `.uar`'s
/// `contentsdir=` leaves them, and as Canary's and Teto's packages are):
/// the Installed page reads its name, picture and sample from the folder
/// with the `character.txt`, as the singer picker does — reported as
/// *"the picture is missing on some in the installed section"*.
#[test]
fn a_bank_one_folder_down_is_described_from_its_own_folder() {
    let dir = scratch("wrapped");
    let inner = dir
        .join("KasaneTeto_English")
        .join("重音テト音声ライブラリー");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("oto.ini"), OTO).unwrap();
    std::fs::write(inner.join("teto.bmp"), b"BM").unwrap();
    std::fs::write(
        inner.join("character.txt"),
        sjis("name=重音テト\r\nimage=teto.bmp\r\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("KasaneTeto_English").join("install.txt"),
        b"type=voiceset\n",
    )
    .unwrap();
    let banks = list(&dir);
    assert_eq!(banks.len(), 1);
    let teto = &banks[0];
    assert_eq!(
        teto.id, "KasaneTeto_English",
        "the folder is still what is uninstalled"
    );
    assert_eq!(teto.name, "重音テト");
    assert_eq!(teto.root, inner);
    assert_eq!(teto.info.portrait, Some(inner.join("teto.bmp")));
}

/// Which installed bank a built-in runtime's voice is — so the picker's
/// card and the Installed page's row show the one name and the one picture.
#[test]
fn a_built_in_voice_is_found_among_the_installed_banks() {
    let dir = scratch("voice-to-bank");
    let make = |folder: &str, inner: Option<&str>, character: &str| {
        let root = match inner {
            Some(i) => dir.join(folder).join(i),
            None => dir.join(folder),
        };
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("oto.ini"), OTO).unwrap();
        std::fs::write(root.join("character.txt"), character).unwrap();
        root
    };
    let canary = make(
        "Canary_ARPAsing",
        Some("Canary Arpasing 1.0"),
        "name=Canary\nimage=c.bmp\n",
    );
    std::fs::write(canary.join("c.bmp"), b"BM").unwrap();
    make("Teto_A", Some("Teto"), "name=Teto A\n");
    make("Teto_B", Some("Teto"), "name=Teto B\n");
    make("plain", None, "name=Plain\n");
    let ds = dir.join("Qixuan");
    std::fs::create_dir_all(&ds).unwrap();
    std::fs::write(ds.join("dsconfig.yaml"), b"phonemes: p.txt\n").unwrap();
    let banks = list(&dir);
    let find = |id: &str, image: Option<&Path>| {
        voicebanks::bank_for_voice(&banks, id, image).map(|b| b.id.clone())
    };
    // By its picture, wherever it is.
    assert_eq!(
        find("anything", Some(&canary.join("c.bmp"))).as_deref(),
        Some("Canary_ARPAsing")
    );
    // A UTAU id: `utau/` and the folder with the bank's files, and that
    // folder's parent when two share a name.
    assert_eq!(
        find("utau/Canary Arpasing 1.0", None).as_deref(),
        Some("Canary_ARPAsing")
    );
    assert_eq!(find("utau/Teto", None).as_deref(), Some("Teto_A"));
    assert_eq!(find("utau/Teto (Teto_B)", None).as_deref(), Some("Teto_B"));
    // A DiffSinger id is its folder's name; a test runtime's is the folder.
    assert_eq!(find("Qixuan", None).as_deref(), Some("Qixuan"));
    assert_eq!(find("plain", None).as_deref(), Some("plain"));
    assert_eq!(find("utau/Nobody", None), None);
}

/// Banks that call themselves the same are told apart: Teto's 単独音 and
/// 統合 libraries are both 重音テト音声ライブラリー, and the picker showed two
/// cards of one name. A name nobody else has is left alone; two of one
/// kind are told apart by folder; two of different kinds by kind.
#[test]
fn banks_of_one_name_are_told_apart_by_kind_or_folder() {
    use fontelle_vocal::voicebanks::told_apart;
    let teto = "重音テト音声ライブラリー";
    let names = told_apart(&[
        (teto, "UTAU", "KasaneTeto_tandoku_100619"),
        ("Yoko", "DiffSinger", "Yoko_DiffSinger"),
        (teto, "UTAU", "KasaneTeto_tougou_110401"),
        ("Ritsu", "UTAU", "NamineRitsu_VCV"),
        ("Ritsu", "DiffSinger", "NamineRitsu_DiffSinger"),
    ]);
    assert_eq!(
        names,
        vec![
            format!("{teto} (KasaneTeto_tandoku_100619)"),
            "Yoko".to_string(),
            format!("{teto} (KasaneTeto_tougou_110401)"),
            "Ritsu (UTAU)".to_string(),
            "Ritsu (DiffSinger)".to_string(),
        ]
    );
    // Three of one name, two of them one kind: the folder for all three,
    // so no two lines read alike.
    let names = told_apart(&[
        ("Ritsu", "UTAU", "a"),
        ("Ritsu", "UTAU", "b"),
        ("Ritsu", "DiffSinger", "c"),
    ]);
    assert_eq!(names, vec!["Ritsu (a)", "Ritsu (b)", "Ritsu (c)"]);
    // Names that differ only in spacing at the ends are one name.
    let names = told_apart(&[("Kumi ", "UTAU", "x"), ("Kumi", "DiffSinger", "y")]);
    assert_eq!(names, vec!["Kumi (UTAU)", "Kumi (DiffSinger)"]);
}
