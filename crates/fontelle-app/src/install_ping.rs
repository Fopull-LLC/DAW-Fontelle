//! The install ping: one anonymous request a day, so installs can be counted
//! rather than guessed from downloads (hub card 0428, the floptle-platform
//! hub's `contracts/install-ping.md`).
//!
//! What it sends is the contract's body and nothing else: a random id made
//! here, the product, the version, the OS, the CPU and the update channel.
//! It is said before anything is sent — on the start menu, or as a toast
//! when a launch shows none — and the first launch's ping follows a minute
//! after, or when the menu is left (hub card 0645, Ty: *opt out, counted on
//! the first open*), so anybody who wants it off has had the moment to turn
//! it off. It is off under `FOPULL_NO_PING` or the Settings switch, which
//! also deletes the id; either, read when it goes, means no request.
//!
//! It also says how far a person has got (hub card 0646, the contract's
//! §9): **milestones**, firsts like a note played or a song saved, each sent
//! until it is acknowledged and then never again; and **features used
//! today**, sent again when the day's set grows. Fixed keys only
//! ([`Milestone`], [`Feature`]) — never a name, a file or a note from the
//! person's work. The day's first ping carries what was done before it;
//! later sends follow a milestone after [`MILESTONE_BATCH`], or a grown set
//! of features after [`USED_EVERY`], [`SENDS_A_DAY`] at most.
//!
//! Like the update check (`updates.rs`) it goes through `curl`: one more
//! subprocess and nothing more linked. It runs on a thread of its own and
//! never shows an error.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Where the pings go.
pub const ENDPOINT: &str = "https://fopull.com/api/v1/installs/ping";
/// What the start menu's notice links to.
pub const PRIVACY_URL: &str = fontelle_ui::support::INSTALL_PING_URL;
/// How long the first launch's ping waits after its notice is shown (hub
/// card 0645, the contract's §4): time to read it and turn it off.
pub const NOTICE_GRACE: Duration = Duration::from_secs(60);
/// The notice where no start menu is shown — the start menu's words, with
/// the address the menu's link would open, since a toast is not a link.
pub const NOTICE_WITHOUT_MENU: &str = "Fontelle counts installs with one anonymous ping a day \u{2014} \
     Settings turns it off. What is sent: fopull.com/privacy";
/// The file beside the settings that keeps the id and the last day sent.
pub const FILE_NAME: &str = "install-ping.json";

/// The contract's waits before a second and a third try, after a network
/// error or a 5xx: about a minute, then about ten.
const RETRIES: [Duration; 2] = [Duration::from_secs(60), Duration::from_secs(600)];

/// How long a milestone waits for others to go with it (§9: *about 30
/// seconds*), so a first song's five firsts are one request, not five.
pub const MILESTONE_BATCH: Duration = Duration::from_secs(30);
/// The least time between two sends for features alone (§9).
pub const USED_EVERY: Duration = Duration::from_secs(30 * 60);
/// Requests a day, retries and all (§4).
pub const SENDS_A_DAY: u32 = 8;

/// The id, the last UTC day a ping was recorded, and what §9 keeps beside
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct PingFile {
    pub install_id: String,
    #[serde(default)]
    pub last_sent: Option<String>,
    /// Milestones the server has acknowledged: never sent again.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub milestones: BTreeSet<String>,
    /// Milestones reached and not yet acknowledged — kept here so a send
    /// that was refused, or a run that ended first, is not the end of them.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub pending: BTreeSet<String>,
    /// The day [`used`](Self::used) is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_day: Option<String>,
    /// Features acknowledged on `used_day`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub used: BTreeSet<String>,
    /// The day [`sends`](Self::sends) counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sends_day: Option<String>,
    /// Requests made on `sends_day`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sends: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// One writer at a time: the ping's thread and the studio's both change the
/// file, and a read-change-write from each at once would lose one.
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl PingFile {
    pub fn read(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// Requests made today.
    pub fn sends_on(&self, today: &str) -> u32 {
        if self.sends_day.as_deref() == Some(today) {
            self.sends
        } else {
            0
        }
    }

    /// Changes the file as it is now, if there is one.
    fn update(path: &Path, change: impl FnOnce(&mut Self)) -> Option<Self> {
        let _held = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut file = Self::read(path)?;
        change(&mut file);
        file.write(path);
        Some(file)
    }

    fn write(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingAnswer {
    /// 204: counted (a second one the same day included).
    Recorded,
    /// 400 or 429: not tried again today.
    Refused,
    /// The network or the server failed every try: tomorrow, then.
    GaveUp,
    /// Today's [`SENDS_A_DAY`] are used up; nothing was sent.
    Capped,
}

/// One day's ping, ready to go.
pub struct Ping {
    pub path: PathBuf,
    pub version: String,
    pub channel: String,
    /// The UTC day it is, `YYYY-MM-DD`.
    pub today: String,
    /// Milestones reached in this run; the file's own pending ones go too,
    /// and acknowledged ones are left out.
    pub milestones: Vec<String>,
    /// Features used today in this run; the day's acknowledged ones go too.
    pub used: Vec<String>,
}

impl Ping {
    /// Sends it, with `send` doing the request (the status, or why there
    /// was none) and `wait` the pause before a retry. Makes the id if there
    /// is none yet, and writes the day down when it is recorded.
    pub fn run(
        &self,
        mut send: impl FnMut(&str) -> Result<u16, String>,
        mut wait: impl FnMut(Duration),
    ) -> PingAnswer {
        let file = {
            let _held = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            match PingFile::read(&self.path) {
                Some(file) => file,
                None => {
                    let Some(install_id) = new_install_id() else {
                        return PingAnswer::GaveUp;
                    };
                    let file = PingFile {
                        install_id,
                        ..PingFile::default()
                    };
                    file.write(&self.path);
                    file
                }
            }
        };
        // What this run reached is pending before anything is asked, so a
        // refusal or a crash keeps it for the next send.
        let file = PingFile::update(&self.path, |file| {
            for key in &self.milestones {
                if !file.milestones.contains(key) {
                    file.pending.insert(key.clone());
                }
            }
        })
        .unwrap_or(file);
        let milestones: Vec<String> = file.pending.iter().cloned().collect();
        let mut used: BTreeSet<String> = self.used.iter().cloned().collect();
        if file.used_day.as_deref() == Some(self.today.as_str()) {
            used.extend(file.used.iter().cloned());
        }
        let used: Vec<String> = used.into_iter().collect();
        let request = body_with(
            &file.install_id,
            &self.version,
            &self.channel,
            &milestones,
            &used,
        );
        for attempt in 0..=RETRIES.len() {
            if attempt > 0 {
                wait(RETRIES[attempt - 1]);
            }
            // Counted before it goes: a request is one of the day's eight
            // whatever comes back.
            let mut capped = false;
            PingFile::update(&self.path, |file| {
                if file.sends_day.as_deref() != Some(self.today.as_str()) {
                    file.sends_day = Some(self.today.clone());
                    file.sends = 0;
                }
                if file.sends >= SENDS_A_DAY {
                    capped = true;
                } else {
                    file.sends += 1;
                }
            });
            if capped {
                return PingAnswer::Capped;
            }
            match send(&request) {
                Ok(204) | Ok(200) => {
                    PingFile::update(&self.path, |file| {
                        file.last_sent = Some(self.today.clone());
                        for key in &milestones {
                            file.pending.remove(key);
                            file.milestones.insert(key.clone());
                        }
                        if file.used_day.as_deref() != Some(self.today.as_str()) {
                            file.used_day = Some(self.today.clone());
                            file.used.clear();
                        }
                        file.used.extend(used.iter().cloned());
                    });
                    return PingAnswer::Recorded;
                }
                Ok(400) | Ok(429) => return PingAnswer::Refused,
                // A 5xx, anything else, or no answer at all: try again.
                _ => {}
            }
        }
        PingAnswer::GaveUp
    }
}

/// Puts a milestone in the file's pending set, when there is a file — so a
/// run that ends before its batch is sent still has it reported by the
/// next. With no file yet (the first ping has not gone) the run's own set
/// carries it.
pub fn note_reached(path: &Path, key: &str) {
    PingFile::update(path, |file| {
        if !file.milestones.contains(key) {
            file.pending.insert(key.to_string());
        }
    });
}

/// The request body, exactly the contract's (§1).
pub fn body(install_id: &str, version: &str, channel: &str) -> String {
    body_with(install_id, version, channel, &[], &[])
}

/// The body with §9's two fields, each left out when it is empty — so a
/// ping with nothing to report is the old body exactly.
pub fn body_with(
    install_id: &str,
    version: &str,
    channel: &str,
    milestones: &[String],
    used: &[String],
) -> String {
    let mut json = serde_json::json!({
        "install_id": install_id,
        "product": "fontelle",
        "version": version,
        "os": os_name(),
        "arch": std::env::consts::ARCH,
        "channel": channel,
    });
    if !milestones.is_empty() {
        json["milestones"] = serde_json::json!(milestones);
    }
    if !used.is_empty() {
        json["used"] = serde_json::json!(used);
    }
    json.to_string()
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        _ => "other",
    }
}

/// A random UUID v4 from the operating system's secure source; `None` if
/// it has none to give, in which case there is no ping.
pub fn new_install_id() -> Option<String> {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new().fill(&mut bytes).ok()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// Whether `FOPULL_NO_PING`, as read, turns it off: any value but empty or
/// `0` (§5).
pub fn no_ping_from_env(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// The UTC day of `at`, `YYYY-MM-DD` — civil from days, so no date crate
/// for one line.
pub fn utc_date(at: SystemTime) -> String {
    let days = at
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Whether a ping goes today: only once the notice has been shown, only
/// while it is on, and only if today's has not been recorded.
pub fn due(notice_said: bool, off: bool, file: Option<&PingFile>, today: &str) -> bool {
    notice_said && !off && file.is_none_or(|f| f.last_sent.as_deref() != Some(today))
}

/// Deletes the id: what turning the ping off does (§3).
pub fn forget(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// The request, through `curl`: the status, or why there was none.
pub fn send_with_curl(body: &str) -> Result<u16, String> {
    use std::io::Write;
    let mut child = fontelle_host::gui::no_console(&mut std::process::Command::new("curl"))
        .args([
            "-sS",
            "-o",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
            "-w",
            "%{http_code}",
            "--max-time",
            "10",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            ENDPOINT,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(body.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u16>()
        .map_err(|_| "no answer".to_string())
        .and_then(|code| {
            if code == 0 {
                Err("no answer".into())
            } else {
                Ok(code)
            }
        })
}

macro_rules! keys {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident => $key:literal,)* }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vdoc])* $variant,)*
        }

        impl $name {
            /// Every key, in the contract's order.
            pub const ALL: &'static [$name] = &[$($name::$variant,)*];

            /// The key on the wire. **Never renamed** once the server lists
            /// it (§9): a new meaning is a new key.
            pub fn key(self) -> &'static str {
                match self {
                    $($name::$variant => $key,)*
                }
            }
        }
    };
}

keys!(
    /// A first, sent until it is acknowledged and then never again (§9).
    /// What each means is written under the contract's table.
    Milestone {
        /// A new song made.
        ProjectCreated => "project_created",
        /// An existing song opened.
        ProjectOpened => "project_opened",
        /// A song saved to disk.
        ProjectSaved => "project_saved",
        /// A note sounded: a key or a click auditioned, or playback with
        /// notes in it.
        NotePlayed => "note_played",
        /// A note added to a clip.
        NotesWritten => "notes_written",
        /// A clip put on the arrangement.
        ClipPlaced => "clip_placed",
        /// A recorded audio take kept.
        AudioRecorded => "audio_recorded",
        /// A soundfont chosen for an instrument.
        SoundfontLoaded => "soundfont_loaded",
        /// A setting changed in the instrument panel.
        InstrumentEdited => "instrument_edited",
        /// A CLAP, LV2 or VST plugin loaded.
        PluginLoaded => "plugin_loaded",
        /// An effect put on a mixer track.
        EffectAdded => "effect_added",
        /// A song or a lane bounced to a file.
        AudioExported => "audio_exported",
        /// A shared song started.
        CollabHosted => "collab_hosted",
        /// A shared song joined.
        CollabJoined => "collab_joined",
        /// The tour's last step reached.
        TourFinished => "tour_finished",
        /// The audio output could not be opened: the studio ran silent.
        AudioUnavailable => "audio_unavailable",
    }
);

keys!(
    /// A feature used today (§9), sent again when the day's set grows.
    Feature {
        /// The transport played.
        Playback => "playback",
        /// Notes edited in the roll.
        PianoRoll => "piano_roll",
        /// A take recorded.
        Recording => "recording",
        /// The instrument panel changed.
        InstrumentEditor => "instrument_editor",
        /// A Flopsynth patch played or edited.
        Flopsynth => "flopsynth",
        /// A hosted plugin played, or its window opened.
        Plugins => "plugins",
        /// An effect changed.
        Effects => "effects",
        /// The mixer changed.
        Mixer => "mixer",
        /// A bounce made.
        Export => "export",
        /// In a shared song.
        Collab => "collab",
        /// A vocal track sung.
        Vocal => "vocal",
        /// An audio clip placed or edited.
        AudioClips => "audio_clips",
    }
);

/// This run's side of §9: what has been reached and used and not yet
/// known to be acknowledged, and when the next send may go.
#[derive(Debug, Clone)]
pub struct Usage {
    /// Milestones dealt with this run — reached, or found acknowledged —
    /// so a second note costs nothing.
    seen: BTreeSet<Milestone>,
    reached: BTreeSet<Milestone>,
    used_day: String,
    used: BTreeSet<Feature>,
    used_at_last_send: usize,
    milestone_since: Option<Instant>,
    used_since: Option<Instant>,
    last_send: Option<Instant>,
    batch: Duration,
}

impl Default for Usage {
    fn default() -> Self {
        Self::with_batch(MILESTONE_BATCH)
    }
}

impl Usage {
    /// Batched `batch` rather than [`MILESTONE_BATCH`] — for a test.
    pub fn with_batch(batch: Duration) -> Self {
        Self {
            seen: BTreeSet::new(),
            reached: BTreeSet::new(),
            used_day: String::new(),
            used: BTreeSet::new(),
            used_at_last_send: 0,
            milestone_since: None,
            used_since: None,
            last_send: None,
            batch,
        }
    }

    /// Whether this run has dealt with `milestone` already.
    pub fn has_seen(&self, milestone: Milestone) -> bool {
        self.seen.contains(&milestone)
    }

    /// `milestone` reached at `now`; `acknowledged` says whether an earlier
    /// run had it acknowledged (asked once a run). True when it is news, to
    /// be kept in the file and sent.
    pub fn reach(
        &mut self,
        milestone: Milestone,
        acknowledged: impl FnOnce() -> bool,
        now: Instant,
    ) -> bool {
        if !self.seen.insert(milestone) || acknowledged() {
            return false;
        }
        self.reached.insert(milestone);
        self.milestone_since.get_or_insert(now);
        true
    }

    /// `feature` used at `now`, on the UTC day `today`.
    pub fn use_feature(&mut self, feature: Feature, today: &str, now: Instant) {
        if self.used_day != today {
            self.used_day = today.to_string();
            self.used.clear();
            self.used_at_last_send = 0;
        }
        if self.used.insert(feature) {
            self.used_since.get_or_insert(now);
        }
    }

    /// Milestones reached in this run, as keys.
    pub fn reached(&self) -> Vec<String> {
        self.reached.iter().map(|m| m.key().to_string()).collect()
    }

    /// Today's features, as keys.
    pub fn used(&self) -> Vec<String> {
        self.used.iter().map(|f| f.key().to_string()).collect()
    }

    /// Whether a later send is due at `now`: a milestone that has waited
    /// out its batch, or a grown set of features that has waited its batch
    /// and [`USED_EVERY`] since the last send.
    pub fn wants_send(&self, now: Instant) -> bool {
        let waited = |since: Option<Instant>| since.is_some_and(|t| now >= t + self.batch);
        let milestone = !self.reached.is_empty() && waited(self.milestone_since);
        let used = self.used.len() > self.used_at_last_send
            && waited(self.used_since)
            && self.last_send.is_none_or(|t| now >= t + USED_EVERY);
        milestone || used
    }

    /// A send has gone with everything this run knows of.
    pub fn sending(&mut self, now: Instant) {
        self.milestone_since = None;
        self.used_since = None;
        self.used_at_last_send = self.used.len();
        self.last_send = Some(now);
    }

    /// Nothing can go yet (the day's first has not been recorded, or its
    /// eight are spent): asks again a batch after `now`, keeping the news.
    pub fn defer(&mut self, now: Instant) {
        for since in [&mut self.milestone_since, &mut self.used_since] {
            if since.is_some() {
                *since = Some(now);
            }
        }
    }

    /// Drops what the file says is acknowledged.
    pub fn acknowledged(&mut self, file: &PingFile) {
        self.reached.retain(|m| !file.milestones.contains(m.key()));
    }

    /// Turned off: nothing is kept for later (§9).
    pub fn forget(&mut self) {
        *self = Self::with_batch(self.batch);
    }
}

/// Every edit the counting below names, by its wire name — pinned against
/// `Edit::TAGS` by a test, so a renamed command is not silently uncounted.
pub const COUNTED_EDITS: &[&str] = &[
    "AddNotes",
    "RemoveNotes",
    "MoveNotes",
    "ResizeNotes",
    "SetNoteVelocity",
    "SetNoteSlide",
    "SliceNotes",
    "MergeSlices",
    "SetNoteProperty",
    "NudgeNoteProperty",
    "SetNotePropertyEach",
    "SetNoteLengths",
    "SetNoteKeys",
    "SetNotePath",
    "SetNoteMuted",
    "AddClip",
    "DuplicateClip",
    "AddPrefabInstance",
    "AddAudioClip",
    "RemoveAudioClip",
    "SetAudioClip",
    "SetAudioClips",
    "AddInsert",
    "RemoveInsert",
    "MoveInsert",
    "SetInsertBypassed",
    "SetInsertParam",
    "SetInsertNotes",
    "SetInsertKey",
    "SetInsertMix",
    "SetEqBand",
    "EditNotepad",
    "EditLapse",
    "AddPluginInsert",
    "ApplyTrackChain",
    "SetChannelPlugin",
    "SetPluginParam",
    "SetPluginState",
    "SetLyrics",
    "SetSinger",
    "SetNoteVoices",
    "SetVocalLane",
    "SetVocalSettings",
    "SetVocalState",
    "ImportVocal",
    "FreezeVocal",
    "SetVocalStyle",
    "AddMixerTrack",
    "RemoveMixerTrack",
    "RenameMixerTrack",
    "MoveMixerTrack",
    "DuplicateMixerTrack",
    "SetMixerOrder",
    "SetTrackOutput",
    "AddSend",
    "RemoveSend",
    "SetSendLevel",
    "SetSendPreFader",
    "SetMixerTrackColor",
];

/// What one of this studio's own edits says about how far its person has
/// got: by the edit's wire name, so every command is reachable and none is
/// taken apart. Undo's `Restore*` inverses count for nothing.
pub fn usage_of(edit: &fontelle_model::wire::Edit, out: &mut Vec<Counted>) {
    use Feature as F;
    use Milestone as M;
    if let fontelle_model::wire::Edit::Compound { parts, .. } = edit {
        for part in parts {
            usage_of(part, out);
        }
        return;
    }
    let tag = edit.tag();
    let milestone = match tag {
        "AddNotes" => Some(M::NotesWritten),
        "AddClip" | "DuplicateClip" | "AddPrefabInstance" | "AddAudioClip" => Some(M::ClipPlaced),
        "AddInsert" | "ApplyTrackChain" => Some(M::EffectAdded),
        "SetChannelPlugin" => Some(M::PluginLoaded),
        _ => None,
    };
    if let Some(m) = milestone {
        out.push(Counted::Reached(m));
    }
    // A plugin effect is both.
    if tag == "AddPluginInsert" {
        out.push(Counted::Reached(M::EffectAdded));
        out.push(Counted::Reached(M::PluginLoaded));
    }
    let feature = match tag {
        "AddNotes"
        | "RemoveNotes"
        | "MoveNotes"
        | "ResizeNotes"
        | "SetNoteVelocity"
        | "SetNoteSlide"
        | "SliceNotes"
        | "MergeSlices"
        | "SetNoteProperty"
        | "NudgeNoteProperty"
        | "SetNotePropertyEach"
        | "SetNoteLengths"
        | "SetNoteKeys"
        | "SetNotePath"
        | "SetNoteMuted" => Some(F::PianoRoll),
        "AddAudioClip" | "RemoveAudioClip" | "SetAudioClip" | "SetAudioClips" => {
            Some(F::AudioClips)
        }
        "AddInsert" | "RemoveInsert" | "MoveInsert" | "SetInsertBypassed" | "SetInsertParam"
        | "SetInsertNotes" | "SetInsertKey" | "SetInsertMix" | "SetEqBand" | "EditNotepad"
        | "EditLapse" | "AddPluginInsert" | "ApplyTrackChain" => Some(F::Effects),
        "SetChannelPlugin" | "SetPluginParam" | "SetPluginState" => Some(F::Plugins),
        "SetLyrics" | "SetSinger" | "SetNoteVoices" | "SetVocalLane" | "SetVocalSettings"
        | "SetVocalState" | "ImportVocal" | "FreezeVocal" | "SetVocalStyle" => Some(F::Vocal),
        "AddMixerTrack"
        | "RemoveMixerTrack"
        | "RenameMixerTrack"
        | "MoveMixerTrack"
        | "DuplicateMixerTrack"
        | "SetMixerOrder"
        | "SetTrackOutput"
        | "AddSend"
        | "RemoveSend"
        | "SetSendLevel"
        | "SetSendPreFader"
        | "SetMixerTrackColor" => Some(F::Mixer),
        _ if edit.touches_mixer() => Some(F::Mixer),
        _ => None,
    };
    if let Some(f) = feature {
        out.push(Counted::Used(f));
    }
}

/// One thing [`usage_of`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counted {
    Reached(Milestone),
    Used(Feature),
}

/// A milestone reached where there is no studio to carry it — `main`'s
/// start that could not open the audio output. Written into the file's
/// pending set, never sent from there: that exit must not wait (§4), and
/// the next launch's ping reports it. Off, or before the first ping has
/// made the file, nothing is kept.
pub fn reached_without_a_studio(
    settings: &crate::settings::Settings,
    path: &Path,
    milestone: Milestone,
) {
    let off =
        !settings.install_ping || no_ping_from_env(std::env::var("FOPULL_NO_PING").ok().as_deref());
    if off || !settings.ping_notice_said {
        return;
    }
    note_reached(path, milestone.key());
}
