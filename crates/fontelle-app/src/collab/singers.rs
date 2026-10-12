//! The singers a shared song uses, travelling like its sounds and
//! soundfonts where their terms let them (`docs/collab-plan.md` §18, F93).
//!
//! Ty: *"collaborating doesn't share singer files currently like it does
//! with sound fonts and audio so you cannot sync singers you don't share.
//! please add this."*
//!
//! **The rule.** A song names a singer by engine and voice id
//! (`fontelle_model::VocalVoice`), never by a file. What a friend without it
//! can do depends on whose it is:
//!
//! - **A Fontelle Singer bank** (UTAU, DiffSinger) is *sent* only when its
//!   catalogue entry says its author lets it be passed on
//!   (`Terms::redistribute == Yes`). Terms read off a bank's own readme are
//!   prose, in any language: a studio cannot tell "you may share this" from
//!   "you may not", so a bank the catalogue does not vouch for is **not
//!   sent** — the friend is told which bank, and where to get it (the
//!   catalogue's download or the author's page) when anybody knows.
//! - **A voice made for research only** is never sent, whatever else is
//!   true: its output must not be shared, let alone the voice.
//! - **Another engine's singer** cannot be sent at all: the friend is told
//!   which engine and which singer.
//!
//! A bank that is sent arrives only after the friend has read its terms and
//! agreed, and is installed — and agreed in Fontelle's own record — before
//! it sings. It travels as one file: the bank's folder packed by [`pack`]
//! (Fontelle's own few lines, not an archive format with a reader of its
//! own to trust), installed from the folder [`unpack`] makes.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use fontelle_model::wire::{SingerNeed, SingerOffer};
use fontelle_vocal::catalogue::{CatalogueSinger, Get, Permission};

/// What a packed bank starts with.
const MAGIC: &[u8; 8] = b"FTSINGR1";

/// The most one packed bank may unpack to: past this it is not a voicebank.
const MOST: u64 = 8 * 1024 * 1024 * 1024;

/// How a bank this studio has is offered to a friend who has not got it.
pub(crate) fn offer_for_bank(research_only: bool, catalogue: Option<&CatalogueSinger>) -> Rule {
    if research_only {
        return Rule::ResearchOnly;
    }
    match catalogue {
        Some(entry) if entry.terms.redistribute == Permission::Yes => Rule::Send,
        Some(entry) => Rule::GetIt(link_of(entry)),
        None => Rule::GetIt(None),
    }
}

/// Where the catalogue says a singer is got: its download, or its author's
/// page.
fn link_of(entry: &CatalogueSinger) -> Option<String> {
    let link = match entry.get() {
        Get::Download(download) => download.url,
        Get::Page(page) => page,
    };
    (!link.trim().is_empty()).then_some(link)
}

/// The decision for one bank, before it is packed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    /// Its terms let it be passed on: pack it and send it.
    Send,
    /// Tell the friend where to get it.
    GetIt(Option<String>),
    ResearchOnly,
}

/// Every singer `song` uses, once each, with how a friend without it gets
/// it — and the packed banks this studio may send, by hash. A bank is packed
/// into `<voicebanks>/.collab-packs/` the first time it is offered (a folder
/// whose name starts with a dot is Fontelle's, never listed as a bank).
pub(crate) fn needs_of(
    song: &fontelle_model::Project,
    vocal: &crate::vocal::VocalSession,
) -> (Vec<SingerNeed>, Vec<(u64, PathBuf)>) {
    use fontelle_vocal::catalogue::CatalogueKind;
    use fontelle_vocal::voicebanks::{self, PackageKind};
    let mut needs: Vec<SingerNeed> = Vec::new();
    let mut packs = Vec::new();
    let voices = vocal.registry().voices();
    let dir = vocal.voicebanks_dir();
    let installed = voicebanks::list(&dir);
    let catalogue = vocal.catalogue();
    for voice in song.channels.values().filter_map(|c| c.vocal.as_ref()) {
        if voice.singer.is_empty()
            || needs
                .iter()
                .any(|n| n.engine == voice.engine && n.singer == voice.singer)
        {
            continue;
        }
        let known = voices
            .iter()
            .find(|v| v.backend == voice.engine && v.info.id == voice.singer);
        let name = known.map_or_else(|| voice.singer.clone(), |v| v.info.name.clone());
        let need = |offer| SingerNeed {
            engine: voice.engine.clone(),
            singer: voice.singer.clone(),
            name: name.clone(),
            offer,
        };
        if !fontelle_model::is_builtin_engine(&voice.engine) {
            needs.push(need(SingerOffer::Engine));
            continue;
        }
        let image = known.and_then(|v| v.info.image.as_deref()).map(Path::new);
        let Some(bank) = voicebanks::bank_for_voice(&installed, &voice.singer, image) else {
            needs.push(need(SingerOffer::GetIt { link: None }));
            continue;
        };
        let kind = match bank.kind {
            PackageKind::Utau => CatalogueKind::Utau,
            PackageKind::DiffSinger => CatalogueKind::DiffSinger,
            PackageKind::Dependency => CatalogueKind::Dependency,
        };
        let entry =
            fontelle_vocal::catalogue::match_bank(&catalogue, kind, &[&bank.name, &bank.id]);
        let research = known.is_some_and(|v| v.info.research_only);
        let offer = match offer_for_bank(research, entry) {
            Rule::ResearchOnly => SingerOffer::ResearchOnly,
            Rule::GetIt(link) => SingerOffer::GetIt { link },
            Rule::Send => {
                let packed = dir.join(".collab-packs").join(format!("{}.pack", bank.id));
                let made = if packed.is_file() {
                    Ok(())
                } else {
                    pack(&bank.path, &packed)
                };
                match made.map_err(|e| e.to_string()).and_then(|()| {
                    fontelle_assets::content_hash::hash_file(&packed).map_err(|e| e.to_string())
                }) {
                    Ok(hash) => {
                        packs.push((hash.low, packed));
                        SingerOffer::Send {
                            hash: hash.low,
                            size: hash.size,
                            file_name: format!("{}.pack", bank.id),
                            terms: voicebanks::read_terms(&bank.root),
                        }
                    }
                    Err(e) => {
                        eprintln!("Fontelle: could not pack {}: {e}", bank.name);
                        SingerOffer::GetIt {
                            link: link_of_entry(entry),
                        }
                    }
                }
            }
        };
        needs.push(need(offer));
    }
    (needs, packs)
}

fn link_of_entry(entry: Option<&CatalogueSinger>) -> Option<String> {
    entry.and_then(link_of)
}

/// Packs the folder `bank` into the file `into`: every file under it, by
/// its path inside the bank.
pub(crate) fn pack(bank: &Path, into: &Path) -> std::io::Result<()> {
    let mut files = Vec::new();
    walk(bank, bank, &mut files)?;
    files.sort();
    if let Some(dir) = into.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = into.with_extension("part");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&part)?);
    out.write_all(MAGIC)?;
    for (name, path) in files {
        let size = std::fs::metadata(&path)?.len();
        out.write_all(&(name.len() as u32).to_le_bytes())?;
        out.write_all(name.as_bytes())?;
        out.write_all(&size.to_le_bytes())?;
        std::io::copy(&mut std::fs::File::open(&path)?.take(size), &mut out)?;
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&part, into)
}

/// Every file under `dir`, by its path inside `root` written with `/`.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else if path.is_file() {
            let inside = path.strip_prefix(root).unwrap_or(&path);
            let name: Vec<String> = inside
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            // Fontelle's own marks (the terms agreed, an install in
            // progress) are this computer's, not the bank's.
            if name.iter().any(|part| part.starts_with(".fontelle")) {
                continue;
            }
            out.push((name.join("/"), path));
        }
    }
    Ok(())
}

/// A path out of a pack, if it stays inside the folder it is unpacked into.
fn inside(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(part) => out.push(part),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Unpacks a file [`pack`] made into the folder `into`, refusing any path
/// that would leave it.
pub(crate) fn unpack(packed: &Path, into: &Path) -> std::io::Result<()> {
    let refused = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_string());
    let result = (|| {
        let mut input = std::io::BufReader::new(std::fs::File::open(packed)?);
        let mut magic = [0u8; 8];
        input.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(refused("not a packed voicebank"));
        }
        let mut total = 0u64;
        loop {
            let mut len = [0u8; 4];
            match input.read_exact(&mut len) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e),
            }
            let len = u32::from_le_bytes(len) as usize;
            if len == 0 || len > 4096 {
                return Err(refused("a name in the pack is not a name"));
            }
            let mut name = vec![0u8; len];
            input.read_exact(&mut name)?;
            let name = String::from_utf8(name).map_err(|_| refused("a name is not text"))?;
            let path = inside(&name).ok_or_else(|| refused("a path leaves the bank"))?;
            let mut size = [0u8; 8];
            input.read_exact(&mut size)?;
            let size = u64::from_le_bytes(size);
            total = total.saturating_add(size);
            if total > MOST {
                return Err(refused("bigger than any voicebank"));
            }
            let target = into.join(path);
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut file = std::fs::File::create(&target)?;
            let copied = std::io::copy(&mut (&mut input).take(size), &mut file)?;
            if copied != size {
                return Err(refused("the pack ends in the middle of a file"));
            }
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(into);
    }
    result
}

/// What a studio without the singer `need` is told, for the list of what
/// is missing.
pub(crate) fn missing_words(need: &SingerNeed, from: &str) -> (String, String) {
    let what = if fontelle_model::is_builtin_engine(&need.engine) {
        format!("{} \u{2014} Fontelle Singer voice", need.name)
    } else {
        format!("{} \u{2014} {} singer", need.name, need.engine)
    };
    let detail = match &need.offer {
        SingerOffer::Send { .. } => {
            format!("{from} can send it \u{2014} read its terms and say yes to fetch it")
        }
        SingerOffer::GetIt { link: Some(link) } => {
            format!("its terms do not let it be passed on \u{2014} get it at {link}")
        }
        SingerOffer::GetIt { link: None } => {
            format!("its terms do not let it be passed on \u{2014} ask {from} where to get it")
        }
        SingerOffer::Engine => format!(
            "sung by {}, which is not installed here \u{2014} an engine cannot be sent",
            need.engine
        ),
        SingerOffer::ResearchOnly => {
            "a voice for research only \u{2014} it is never passed on".to_string()
        }
    };
    (what, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(redistribute: Permission, page: Option<&str>) -> CatalogueSinger {
        let mut json = serde_json::json!({
            "id": "teto",
            "name": "Kasane Teto",
            "kind": "utau",
            "page": page,
        });
        json["terms"] = serde_json::json!({
            "redistribute": match redistribute {
                Permission::Yes => "yes",
                Permission::No => "no",
                Permission::Ask => "ask",
                Permission::Unknown => "unknown",
            }
        });
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn only_a_bank_the_catalogue_says_may_be_passed_on_is_sent() {
        let page = Some("https://kasaneteto.jp/");
        assert_eq!(
            offer_for_bank(false, Some(&entry(Permission::Yes, page))),
            Rule::Send
        );
        for no in [Permission::No, Permission::Ask, Permission::Unknown] {
            assert_eq!(
                offer_for_bank(false, Some(&entry(no, page))),
                Rule::GetIt(Some("https://kasaneteto.jp/".into())),
                "{no:?} is not a yes"
            );
        }
        assert_eq!(
            offer_for_bank(false, None),
            Rule::GetIt(None),
            "nobody vouches for a bank the catalogue does not know"
        );
        assert_eq!(
            offer_for_bank(true, Some(&entry(Permission::Yes, page))),
            Rule::ResearchOnly,
            "a research voice is never sent"
        );
    }

    #[test]
    fn a_packed_bank_unpacks_as_it_was_and_nowhere_else() {
        let dir = std::env::temp_dir().join(format!("fontelle-pack-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let bank = dir.join("Teto");
        std::fs::create_dir_all(bank.join("A3")).unwrap();
        std::fs::write(bank.join("character.txt"), "name=Teto\r\n").unwrap();
        std::fs::write(bank.join("A3").join("oto.ini"), "a.wav=- a,1,2,3,4,5\n").unwrap();
        std::fs::write(bank.join("A3").join("a.wav"), vec![7u8; 70_000]).unwrap();
        let packed = dir.join("teto.pack");
        pack(&bank, &packed).unwrap();
        let out = dir.join("out");
        unpack(&packed, &out).unwrap();
        for file in ["character.txt", "A3/oto.ini", "A3/a.wav"] {
            assert_eq!(
                std::fs::read(out.join(file)).unwrap(),
                std::fs::read(bank.join(file)).unwrap(),
                "{file}"
            );
        }

        // A pack that names a path out of the folder is refused whole.
        let mut evil = MAGIC.to_vec();
        let path = b"../escaped.txt";
        evil.extend((path.len() as u32).to_le_bytes());
        evil.extend(path);
        evil.extend(3u64.to_le_bytes());
        evil.extend(b"bad");
        std::fs::write(dir.join("evil.pack"), evil).unwrap();
        assert!(unpack(&dir.join("evil.pack"), &dir.join("evil")).is_err());
        assert!(!dir.join("escaped.txt").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_friend_is_told_which_singer_and_where_to_get_it() {
        let need = |offer| SingerNeed {
            engine: "fontelle-utau".into(),
            singer: "utau/Teto".into(),
            name: "Kasane Teto".into(),
            offer,
        };
        let (what, detail) = missing_words(
            &need(SingerOffer::GetIt {
                link: Some("https://kasaneteto.jp/".into()),
            }),
            "Alice",
        );
        assert!(what.contains("Kasane Teto"), "{what}");
        assert!(detail.contains("https://kasaneteto.jp/"), "{detail}");
        let (_, detail) = missing_words(&need(SingerOffer::GetIt { link: None }), "Alice");
        assert!(detail.contains("Alice"), "ask whoever has it: {detail}");
        let engine = SingerNeed {
            engine: "Some Engine".into(),
            ..need(SingerOffer::Engine)
        };
        let (what, detail) = missing_words(&engine, "Alice");
        assert!(
            what.contains("Kasane Teto") && detail.contains("Some Engine"),
            "{what} / {detail}"
        );
    }
}
