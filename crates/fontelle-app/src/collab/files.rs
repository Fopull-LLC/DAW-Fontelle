//! The files a shared song plays, moving between studios by what is in them
//! (`docs/collab-plan.md` §7.2).
//!
//! **Pulled, never pushed.** A studio that finds a file it has not got asks
//! for it by hash — a joiner asks the host, and the host asks whoever made the
//! edit that named it — and the answer comes back in pieces under the relay's
//! frame cap, then a last word saying it is all there. One request in flight
//! per studio, smallest first, so a 300 MB soundfont does not keep a drum hit
//! waiting behind it.
//!
//! **Nothing arrives where it could do harm.** A piece goes into a `.part`
//! file in the bundle's `cache/`; the whole is checked against the hash it
//! was asked for before it is renamed into place; and the place is chosen
//! here, from the file's own name and nothing else the other side said — a
//! sound into `assets/<name>.<hash>.<ext>` in this studio's bundle, a
//! soundfont into this studio's bank, and only once the person has said yes.

use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fontelle_model::Project;
use fontelle_model::wire::{AssetEntry, Msg};
use fontelle_net::{Channel, PeerId, Transport};
use fontelle_types::{AssetKind, AssetRef};

use super::CHUNK;

/// How many pieces go to one studio in one turn. The relay's budget is the
/// real limit (§8.4, paced in Phase 3); this keeps one turn short.
const PIECES_PER_TURN: usize = 16;

/// What the fetch question asks about: files that are big enough, or go
/// somewhere other than the song's own folder, to ask before fetching.
#[derive(Clone, Debug)]
pub struct FetchQuestion {
    pub lines: Vec<String>,
    pub entries: Vec<AssetEntry>,
}

/// A file being fetched.
struct Fetching {
    entry: AssetEntry,
    from: PeerId,
    part: PathBuf,
    written: u64,
}

/// A file being sent.
struct Upload {
    hash: u64,
    path: PathBuf,
    next: u32,
    of: u32,
}

#[derive(Default)]
pub(crate) struct Files {
    /// Not asked for yet, smallest first, and who to ask.
    wanted: VecDeque<(AssetEntry, PeerId)>,
    fetching: Option<Fetching>,
    /// Waiting on the person (see [`FetchQuestion`]).
    held: Vec<(AssetEntry, PeerId)>,
    /// Asked about and turned down, or not there to be had.
    declined: Vec<u64>,
    received: Vec<u64>,
    uploads: BTreeMap<PeerId, VecDeque<Upload>>,
    /// Asked of this studio before it had them itself: sent when they land.
    owed: Vec<(PeerId, u64)>,
}

impl Files {
    pub fn received(&self) -> &[u64] {
        &self.received
    }

    pub fn question(&self) -> Option<FetchQuestion> {
        if self.held.is_empty() {
            return None;
        }
        let entries: Vec<AssetEntry> = self.held.iter().map(|(e, _)| e.clone()).collect();
        let size: u64 = entries.iter().map(|e| e.size).sum();
        let names: Vec<String> = entries.iter().map(|e| display_name(&e.file_name)).collect();
        let what = match names.as_slice() {
            [one] => format!("\u{201c}{one}\u{201d}"),
            many => format!("{} files ({})", many.len(), many.join(", ")),
        };
        Some(FetchQuestion {
            lines: vec![
                format!(
                    "This song uses {what} ({}), which you don\u{2019}t have.",
                    megabytes(size)
                ),
                format!("Fetch it? {}.", how_long(size)),
            ],
            entries,
        })
    }

    pub fn answer(&mut self, fetch: bool) {
        let held = std::mem::take(&mut self.held);
        if fetch {
            self.wanted.extend(held);
            self.wanted.make_contiguous().sort_by_key(|(e, _)| e.size);
        } else {
            self.declined.extend(held.iter().map(|(e, _)| e.hash));
        }
    }

    /// How far along `hash` is, if it is on its way or waiting to be.
    pub fn progress(&self, hash: u64) -> Option<f32> {
        if hash == 0 || self.received.contains(&hash) || self.declined.contains(&hash) {
            return None;
        }
        if let Some(fetching) = &self.fetching
            && fetching.entry.hash == hash
        {
            return Some((fetching.written as f32 / fetching.entry.size.max(1) as f32).min(1.0));
        }
        let queued = self
            .wanted
            .iter()
            .chain(&self.held)
            .any(|(e, _)| e.hash == hash);
        queued.then_some(0.0)
    }

    fn knows(&self, hash: u64) -> bool {
        self.received.contains(&hash)
            || self.declined.contains(&hash)
            || self.fetching.as_ref().is_some_and(|f| f.entry.hash == hash)
            || self
                .wanted
                .iter()
                .chain(&self.held)
                .any(|(e, _)| e.hash == hash)
    }

    /// Asks for whichever of `entries` this studio has not got, from `from`,
    /// and hands back the ones it has — to be loaded now, since whatever named
    /// them is a song this studio's library has not seen yet.
    ///
    /// A soundfont — which goes into the bank, outside the song — and
    /// anything over `ask_above` bytes wait for the person first (§7.1, F29).
    pub fn want(
        &mut self,
        entries: impl IntoIterator<Item = AssetEntry>,
        from: PeerId,
        here: &Here,
        ask_above: u64,
    ) -> Vec<u64> {
        let mut here_already = Vec::new();
        for entry in entries {
            if entry.hash == 0 || self.knows(entry.hash) {
                continue;
            }
            if here.has(&entry) {
                here_already.push(entry.hash);
                continue;
            }
            let big = entry.size > ask_above
                || matches!(entry.kind, AssetKind::Sf2 | AssetKind::Sf3 | AssetKind::Sfz);
            if big {
                self.held.push((entry, from));
            } else {
                self.wanted.push_back((entry, from));
            }
        }
        self.wanted.make_contiguous().sort_by_key(|(e, _)| e.size);
        here_already
    }

    /// One turn: the next request goes out if none is in flight, and every
    /// upload in hand sends its next pieces.
    pub fn pump(&mut self, transport: &mut dyn Transport, here: &Here) {
        if self.fetching.is_none()
            && let Some((entry, from)) = self.wanted.pop_front()
        {
            let part = here
                .cache()
                .join(format!("incoming-{:016x}.part", entry.hash));
            if let Some(dir) = part.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::remove_file(&part);
            send(transport, from, &Msg::AssetRequest { hash: entry.hash });
            self.fetching = Some(Fetching {
                entry,
                from,
                part,
                written: 0,
            });
        }
        for (peer, queue) in &mut self.uploads {
            let mut budget = PIECES_PER_TURN;
            while budget > 0
                && let Some(upload) = queue.front_mut()
            {
                if upload.next >= upload.of {
                    send(transport, *peer, &Msg::AssetDone { hash: upload.hash });
                    queue.pop_front();
                    continue;
                }
                match piece(&upload.path, upload.next) {
                    Ok(bytes) => send(
                        transport,
                        *peer,
                        &Msg::AssetChunk {
                            hash: upload.hash,
                            index: upload.next,
                            of: upload.of,
                            bytes,
                        },
                    ),
                    Err(_) => {
                        send(transport, *peer, &Msg::AssetMissing { hash: upload.hash });
                        queue.pop_front();
                        continue;
                    }
                }
                upload.next += 1;
                budget -= 1;
            }
        }
        self.uploads.retain(|_, queue| !queue.is_empty());
    }

    /// Somebody asked this studio for `hash`.
    pub fn asked(&mut self, transport: &mut dyn Transport, peer: PeerId, hash: u64, here: &Here) {
        if let Some(path) = here.find(hash) {
            let size = std::fs::metadata(&path).map_or(0, |m| m.len());
            let of = size.div_ceil(CHUNK as u64).max(1) as u32;
            self.uploads.entry(peer).or_default().push_back(Upload {
                hash,
                path,
                next: 0,
                of,
            });
        } else if self.knows(hash) && !self.declined.contains(&hash) {
            // On its way here from somebody else: passed on when it lands.
            self.owed.push((peer, hash));
        } else {
            send(transport, peer, &Msg::AssetMissing { hash });
        }
    }

    /// A piece of the file in flight — from the studio it was asked of, and
    /// nobody else.
    pub fn piece(&mut self, from: PeerId, hash: u64, bytes: &[u8]) {
        let Some(fetching) = &mut self.fetching else {
            return;
        };
        if fetching.entry.hash != hash || fetching.from != from {
            return;
        }
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&fetching.part)
            .and_then(|mut file| file.write_all(bytes));
        if written.is_ok() {
            fetching.written += bytes.len() as u64;
        }
    }

    /// The last word on the file in flight. `Ok(Some(hash))` when it landed
    /// whole, `Err` with a sentence when it did not.
    pub fn done(&mut self, from: PeerId, hash: u64, here: &Here) -> Result<Option<u64>, String> {
        let Some(fetching) = self
            .fetching
            .take_if(|f| f.entry.hash == hash && f.from == from)
        else {
            return Ok(None);
        };
        let name = display_name(&fetching.entry.file_name);
        let landed = fontelle_assets::content_hash::hash_file(&fetching.part)
            .map_err(|e| e.to_string())
            .and_then(|found| {
                if found.low == hash {
                    Ok(())
                } else {
                    Err("it arrived damaged".to_string())
                }
            })
            .and_then(|()| here.keep(&fetching.part, &fetching.entry));
        match landed {
            Ok(path) => {
                self.received.push(hash);
                // Anybody who asked this studio for it while it was coming.
                let owed: Vec<PeerId> = self
                    .owed
                    .iter()
                    .filter(|(_, h)| *h == hash)
                    .map(|(p, _)| *p)
                    .collect();
                self.owed.retain(|(_, h)| *h != hash);
                let size = std::fs::metadata(&path).map_or(0, |m| m.len());
                let of = size.div_ceil(CHUNK as u64).max(1) as u32;
                for peer in owed {
                    self.uploads.entry(peer).or_default().push_back(Upload {
                        hash,
                        path: path.clone(),
                        next: 0,
                        of,
                    });
                }
                Ok(Some(hash))
            }
            Err(why) => {
                let _ = std::fs::remove_file(&fetching.part);
                self.declined.push(hash);
                Err(format!("\u{201c}{name}\u{201d} could not be kept: {why}"))
            }
        }
    }

    /// The other side has not got it either.
    pub fn missing(&mut self, transport: &mut dyn Transport, hash: u64) -> Option<String> {
        let fetching = self.fetching.take_if(|f| f.entry.hash == hash)?;
        let _ = std::fs::remove_file(&fetching.part);
        self.declined.push(hash);
        for (peer, _) in self.owed.iter().filter(|(_, h)| *h == hash) {
            send(transport, *peer, &Msg::AssetMissing { hash });
        }
        self.owed.retain(|(_, h)| *h != hash);
        Some(format!(
            "\u{201c}{}\u{201d} is not on the other machine either \u{2014} its clips stay silent.",
            display_name(&fetching.entry.file_name)
        ))
    }
}

/// Where this studio keeps things: its bundle and its soundfont folders.
#[derive(Clone, Debug, Default)]
pub(crate) struct Place {
    pub bundle: Option<PathBuf>,
    pub banks: Vec<PathBuf>,
}

impl Place {
    /// With the song, for a question about a file.
    pub fn with<'a>(&'a self, song: &'a Project) -> Here<'a> {
        Here {
            bundle: self.bundle.as_deref(),
            banks: &self.banks,
            song,
        }
    }
}

/// [`Place`] and the song, for finding a file somebody asks for by its hash.
pub(crate) struct Here<'a> {
    pub bundle: Option<&'a Path>,
    pub banks: &'a [PathBuf],
    pub song: &'a Project,
}

impl Here<'_> {
    fn cache(&self) -> PathBuf {
        match &self.bundle {
            Some(bundle) => bundle.join("cache"),
            None => std::env::temp_dir(),
        }
    }

    /// Whether this studio has the file `entry` names **of its own** — in its
    /// bundle or its soundfont folders, found by what is in it.
    ///
    /// Never by the path a reference carries: a path in somebody else's edit
    /// names a place on *their* disk, and a machine that happened to have
    /// something there (two studios on one computer, or two people with the
    /// same user name and a file of the same name) would otherwise decide it
    /// has a file it has not.
    pub fn has(&self, entry: &AssetEntry) -> bool {
        self.own(entry.hash, entry.size).is_some()
    }

    fn own(&self, hash: u64, size: u64) -> Option<PathBuf> {
        let tail = format!("{hash:016x}");
        if let Some(bundle) = self.bundle
            && let Ok(listing) = std::fs::read_dir(bundle.join("assets"))
        {
            let found = listing.flatten().map(|e| e.path()).find(|path| {
                path.file_stem()
                    .is_some_and(|stem| stem.to_string_lossy().ends_with(&tail))
            });
            if found.is_some() {
                return found;
            }
        }
        fontelle_assets::content_hash::find_by_hash(self.banks, hash, size)
    }

    /// The file on this machine whose contents are `hash`, if there is one —
    /// to send to somebody who asked. This studio's own first, then anywhere
    /// the song's own references reach on this machine (an import from the
    /// desktop that was never collected is still this studio's to send).
    pub fn find(&self, hash: u64) -> Option<PathBuf> {
        let files = self.song.files();
        let size = files
            .iter()
            .find(|file| file.content_hash == hash)
            .map_or(0, |file| file.size);
        self.own(hash, size).or_else(|| {
            files
                .iter()
                .filter(|file| file.content_hash == hash)
                .find_map(|file| crate::bundle::resolve(self.bundle, file, self.banks))
        })
    }

    /// Puts a file that arrived whole where it belongs, and says where.
    fn keep(&self, part: &Path, entry: &AssetEntry) -> Result<PathBuf, String> {
        let name = crate::projects::safe_name(&display_name(&entry.file_name));
        let ext = Path::new(&entry.file_name)
            .extension()
            .map(|e| crate::projects::safe_name(&e.to_string_lossy()).to_lowercase())
            .unwrap_or_else(|| "bin".to_string());
        let target = match entry.kind {
            AssetKind::Sf2 | AssetKind::Sf3 | AssetKind::Sfz => {
                let bank = self
                    .banks
                    .first()
                    .ok_or("there is no soundfont folder to put it in")?;
                std::fs::create_dir_all(bank).map_err(|e| format!("{}: {e}", bank.display()))?;
                let mut target = bank.join(format!("{name}.{ext}"));
                let mut n = 2;
                while target.exists() {
                    target = bank.join(format!("{name} {n}.{ext}"));
                    n += 1;
                }
                target
            }
            AssetKind::Sample => {
                let bundle = self.bundle.ok_or("this studio has no song open")?;
                let assets = bundle.join("assets");
                std::fs::create_dir_all(&assets)
                    .map_err(|e| format!("{}: {e}", assets.display()))?;
                assets.join(format!("{name}.{:016x}.{ext}", entry.hash))
            }
        };
        if target.is_file() {
            // Already here under its own name: the same bytes, by the name.
            let _ = std::fs::remove_file(part);
            return Ok(target);
        }
        std::fs::rename(part, &target)
            .or_else(|_| std::fs::copy(part, &target).map(|_| ()))
            .map_err(|e| format!("{}: {e}", target.display()))?;
        let _ = std::fs::remove_file(part);
        Ok(target)
    }
}

/// What a file is called, for a person: its own name, without a collected
/// copy's hash — from nothing but the last part of whatever path it came as.
fn display_name(file_name: &str) -> String {
    let last = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    crate::library::sound_name(Path::new(last))
}

/// Every file an edit names: an `AssetRef` wherever it sits in the edit.
pub(crate) fn named_by(edit: &fontelle_model::Edit) -> Vec<AssetEntry> {
    let Ok(json) = serde_json::to_value(edit) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk(&json, &mut out);
    out
}

fn walk(value: &serde_json::Value, out: &mut Vec<AssetEntry>) {
    match value {
        serde_json::Value::Object(map)
            if ["id", "path", "content_hash", "size", "kind"]
                .iter()
                .all(|key| map.contains_key(*key)) =>
        {
            if let Ok(file) = serde_json::from_value::<AssetRef>(value.clone())
                && file.content_hash != 0
                && !out.iter().any(|e: &AssetEntry| e.hash == file.content_hash)
            {
                out.push(entry_of(&file));
            }
        }
        serde_json::Value::Object(map) => map.values().for_each(|v| walk(v, out)),
        serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
        _ => {}
    }
}

/// The manifest's line for one reference.
pub(crate) fn entry_of(file: &AssetRef) -> AssetEntry {
    let file_name = if file.path.is_relative() {
        file.path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        file.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    AssetEntry {
        hash: file.content_hash,
        size: file.size,
        file_name,
        kind: file.kind,
    }
}

/// Every file `song` names that this studio has not got.
pub(crate) fn missing(song: &Project, here: &Here) -> Vec<AssetEntry> {
    let mut out: Vec<AssetEntry> = Vec::new();
    for file in song.files() {
        if file.content_hash == 0 || out.iter().any(|e| e.hash == file.content_hash) {
            continue;
        }
        let entry = entry_of(&file);
        if !here.has(&entry) {
            out.push(entry);
        }
    }
    out
}

fn piece(path: &Path, index: u32) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(index as u64 * CHUNK as u64))?;
    let mut bytes = Vec::with_capacity(CHUNK);
    file.take(CHUNK as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn send(transport: &mut dyn Transport, to: PeerId, msg: &Msg) {
    transport.send(to, Channel::Reliable, &msg.to_bytes());
}

fn megabytes(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{:.0} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Roughly how long `bytes` takes at the relay's pace (§8.4: 480 KiB/s).
fn how_long(bytes: u64) -> String {
    let seconds = bytes / (480 * 1024);
    match seconds {
        0..60 => "It takes less than a minute".to_string(),
        60..120 => "It takes about a minute".to_string(),
        _ => format!("It takes about {} minutes", seconds.div_ceil(60)),
    }
}
