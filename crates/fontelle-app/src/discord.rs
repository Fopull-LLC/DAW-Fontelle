//! Telling a running Discord that Fontelle is open — the "Discord status".
//!
//! Two switches in Settings, both off until somebody turns them on: one for
//! the status at all, one for naming the song in it. A song's title is
//! something people may not want broadcast (`docs/next-update-plan.md` §6),
//! so the plain status says only *"Making music in Fontelle"*.
//!
//! # Why no crate
//!
//! Discord's local protocol is an 8-byte header and some JSON over a Unix
//! socket, or a named pipe on Windows. The crates that speak it bring a
//! runtime and a dozen dependencies for what [`encode`], [`decode`] and
//! [`Connection`] do in a page, with `serde_json`, which was here already.
//!
//! # The application ID
//!
//! Discord shows a status under an *application*, registered in its
//! developer portal, which carries the name and the picture people see. That
//! is Fopull's to create, and it has not been yet — so [`DISCORD_CLIENT_ID`]
//! is `None`, and while it is, [`DiscordPresence`] never starts its thread
//! and never touches a socket. The two rows are there all the same, and the
//! first one says under it that this build cannot do it yet: hiding them
//! would mean a second change to the settings page later for no gain, and a
//! switch that flips and silently does nothing would send people looking
//! for a fault in Discord that is not there.
//!
//! # Never on the UI thread
//!
//! [`DiscordPresence::update`] is called every [`Session::pump`] and does no
//! more than compare what it was last told; a change goes down a channel to
//! a thread that owns the socket. That thread is thin on purpose: what it
//! should do next is [`next_step`], a pure function of what Discord is known
//! to show, what is wanted and the time, and the tests hold it to the rules
//! — ask a Discord that is not running at most every fifteen seconds, send
//! at most every four (Discord's own limit is five a twenty seconds), send
//! only a change, and clear at once when turned off.
//!
//! [`Session::pump`]: crate::Session

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// The Discord application the status is shown under. Filled in once Ty has
/// created the application in Discord's developer portal (he is to be walked
/// through it); until then the whole feature is inert — see the module's
/// comment for why the settings rows exist anyway.
pub const DISCORD_CLIENT_ID: Option<&str> = None;

/// The first message on a connection: who is asking.
pub const OP_HANDSHAKE: u32 = 0;
/// A command, or Discord's answer to one.
pub const OP_FRAME: u32 = 1;
/// Goodbye, from either end.
pub const OP_CLOSE: u32 = 2;
/// Discord asking whether the other end is still there…
const OP_PING: u32 = 3;
/// …and the answer.
const OP_PONG: u32 = 4;

/// How long between asking a Discord that was not there.
const RETRY: Duration = Duration::from_secs(15);
/// The least time between two statuses.
const MIN_SEND_GAP: Duration = Duration::from_secs(4);
/// Discord refuses a whole activity whose details are longer.
const MAX_DETAILS: usize = 128;
/// How long a read waits for Discord's answer before going on without it.
/// Unix only: a Windows pipe opened as a file has no timeout to set.
#[cfg(unix)]
const READ_WAIT: Duration = Duration::from_secs(2);
/// More than this unread with no whole message in it is not Discord.
const MAX_PENDING: usize = 1 << 16;

/// One message as it goes down the socket: the opcode and the body's length,
/// both little-endian `u32`s, then the body.
pub fn encode(opcode: u32, body: &Value) -> Vec<u8> {
    let text = body.to_string();
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// The first whole message in `bytes`, and how many bytes it took. `None`
/// while the message is still arriving, or when its body is not JSON.
pub fn decode(bytes: &[u8]) -> Option<(u32, Value, usize)> {
    let opcode = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
    let len = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let end = 8usize.checked_add(len)?;
    let body = serde_json::from_slice(bytes.get(8..end)?).ok()?;
    Some((opcode, body, end))
}

/// What the status says. Without a song it says only that somebody is
/// making music in Fontelle — the second switch is what adds the name.
pub fn activity(song: Option<&str>, playing: bool, started_unix_secs: u64) -> Value {
    let details = match song.map(str::trim).filter(|s| !s.is_empty()) {
        Some(song) => format!("Working on {song}")
            .chars()
            .take(MAX_DETAILS)
            .collect(),
        None => "Making music in Fontelle".to_string(),
    };
    let mut activity = json!({
        "details": details,
        "timestamps": {"start": started_unix_secs},
        "assets": {"large_image": "fontelle", "large_text": "Fontelle"},
    });
    if playing {
        activity["state"] = json!("Playing");
    }
    activity
}

/// A status as the session wants it: everything the activity is made of
/// except the start time, which is the presence's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub song: Option<String>,
    pub playing: bool,
}

/// What the worker knows about its connection.
#[derive(Debug, Clone, Default)]
pub struct Link {
    pub connected: bool,
    /// What Discord was last told and took; `None` when it shows nothing of
    /// ours.
    pub shown: Option<Shown>,
    pub last_send: Option<Instant>,
    pub last_attempt: Option<Instant>,
}

/// The worker's next move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing to do yet.
    Wait,
    /// Try the sockets.
    Connect,
    /// Tell Discord this.
    Send(Shown),
    /// Take the status down.
    Clear,
    /// Say goodbye and let go of the socket.
    Close,
}

/// What to do, given what Discord shows, what is wanted (`None` when the
/// switch is off) and the time.
pub fn next_step(link: &Link, desired: Option<&Shown>, now: Instant) -> Step {
    let since = |t: Option<Instant>| t.map(|t| now.saturating_duration_since(t));
    match desired {
        // Not held to the four seconds: the person asked for it to go.
        None if link.connected && link.shown.is_some() => Step::Clear,
        None if link.connected => Step::Close,
        None => Step::Wait,
        Some(_) if !link.connected => match since(link.last_attempt) {
            Some(gap) if gap < RETRY => Step::Wait,
            _ => Step::Connect,
        },
        Some(want) if link.shown.as_ref() == Some(want) => Step::Wait,
        Some(want) => match since(link.last_send) {
            Some(gap) if gap < MIN_SEND_GAP => Step::Wait,
            _ => Step::Send(want.clone()),
        },
    }
}

/// Every place a Unix Discord's socket may be, in the order to try them:
/// each temporary folder the environment names, then `/tmp`; in each, the
/// folder itself and the three a Flatpak or a Snap Discord puts it under;
/// in each of those, sockets 0 to 9 (a second Discord takes the next).
pub fn unix_socket_paths(env: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    let named = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .filter_map(&env)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    for base in named.chain([PathBuf::from("/tmp")]) {
        // `PathBuf`'s equality goes by components, so `/tmp/` is `/tmp`.
        if !bases.contains(&base) {
            bases.push(base);
        }
    }
    let places = [
        "",
        "app/com.discordapp.Discord",
        "snap.discord",
        ".flatpak/com.discordapp.Discord/xdg-run",
    ];
    let mut paths = Vec::with_capacity(bases.len() * places.len() * 10);
    for base in &bases {
        for place in places {
            for n in 0..10 {
                paths.push(base.join(place).join(format!("discord-ipc-{n}")));
            }
        }
    }
    paths
}

/// Windows Discord's named pipes, 0 to 9.
pub fn windows_pipe_paths() -> Vec<PathBuf> {
    (0..10)
        .map(|n| PathBuf::from(format!(r"\\.\pipe\discord-ipc-{n}")))
        .collect()
}

/// Where this machine's Discord would be listening.
pub fn socket_paths() -> Vec<PathBuf> {
    if cfg!(windows) {
        windows_pipe_paths()
    } else {
        unix_socket_paths(|name| std::env::var(name).ok())
    }
}

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

/// An open, greeted connection to Discord.
pub struct Connection {
    stream: Box<dyn Stream>,
    pending: Vec<u8>,
    nonce: u64,
}

impl Connection {
    /// Connects to the first of `paths` that answers and says hello as
    /// `client_id`. An error when none does — Discord is not running — or
    /// when Discord turns the hello down.
    pub fn open(paths: &[PathBuf], client_id: &str) -> io::Result<Self> {
        let stream = paths
            .iter()
            .find_map(|p| connect(p).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Discord is not running"))?;
        let mut connection = Connection {
            stream,
            pending: Vec::new(),
            nonce: 0,
        };
        connection.write(OP_HANDSHAKE, &json!({"v": 1, "client_id": client_id}))?;
        // Discord answers a good hello with READY and a bad one (an unknown
        // application) with a goodbye; anything else is not Discord.
        match connection.read()? {
            Some((OP_FRAME, _)) | None => Ok(connection),
            Some((_, body)) => Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                body.to_string(),
            )),
        }
    }

    /// Shows `activity`, or takes the status down when it is `None`.
    pub fn set_activity(&mut self, activity: Option<&Value>) -> io::Result<()> {
        self.nonce += 1;
        let nonce = format!("{}-{}", std::process::id(), self.nonce);
        self.write(
            OP_FRAME,
            &json!({
                "cmd": "SET_ACTIVITY",
                "args": {"pid": std::process::id(), "activity": activity},
                "nonce": nonce,
            }),
        )?;
        // Read up to the answer, so answers never pile up unread in the
        // socket. A late one is passed over on the next command by its
        // nonce; an answer that is an error (a status Discord would not
        // take) is no reason to drop the connection.
        while let Some((_, body)) = self.read()? {
            if body["nonce"] == nonce.as_str() {
                break;
            }
        }
        Ok(())
    }

    /// Says goodbye. Discord takes the status down with the connection.
    pub fn close(mut self) {
        let _ = self.write(OP_CLOSE, &json!({}));
    }

    fn write(&mut self, opcode: u32, body: &Value) -> io::Result<()> {
        self.stream.write_all(&encode(opcode, body))?;
        self.stream.flush()
    }

    /// The next message, answering pings on the way; `None` when Discord has
    /// not said anything in time (a socket's read timeout).
    fn read(&mut self) -> io::Result<Option<(u32, Value)>> {
        loop {
            if let Some((opcode, body, used)) = decode(&self.pending) {
                self.pending.drain(..used);
                match opcode {
                    OP_PING => self.write(OP_PONG, &body)?,
                    OP_CLOSE => {
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            body.to_string(),
                        ));
                    }
                    _ => return Ok(Some((opcode, body))),
                }
                continue;
            }
            if self.pending.len() > MAX_PENDING {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "not Discord"));
            }
            let mut chunk = [0u8; 4096];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => self.pending.extend_from_slice(&chunk[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(unix)]
fn connect(path: &Path) -> io::Result<Box<dyn Stream>> {
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    stream.set_read_timeout(Some(READ_WAIT))?;
    stream.set_write_timeout(Some(READ_WAIT))?;
    Ok(Box::new(stream))
}

/// A named pipe opens as a file. It has no read timeout, so a Discord that
/// never answers holds the worker — never the window, which only ever sends
/// down a channel.
#[cfg(windows)]
fn connect(path: &Path) -> io::Result<Box<dyn Stream>> {
    let pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    Ok(Box::new(pipe))
}

#[cfg(not(any(unix, windows)))]
fn connect(_path: &Path) -> io::Result<Box<dyn Stream>> {
    Err(io::ErrorKind::Unsupported.into())
}

/// The session's end of the Discord status.
pub struct DiscordPresence {
    /// `None` in a build without the application ID: inert.
    client_id: Option<String>,
    paths: Vec<PathBuf>,
    /// When this Fontelle started, for the status's elapsed time.
    started: u64,
    /// What the worker was last told; `None` before it was told anything.
    told: Option<Option<Shown>>,
    worker: Option<Sender<Option<Shown>>>,
}

impl Default for DiscordPresence {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordPresence {
    /// This build's presence: inert without [`DISCORD_CLIENT_ID`].
    pub fn new() -> Self {
        let paths = match DISCORD_CLIENT_ID {
            Some(_) => socket_paths(),
            None => Vec::new(),
        };
        Self::build(DISCORD_CLIENT_ID.map(str::to_string), paths)
    }

    /// A presence that says hello as `client_id` at exactly these `paths` —
    /// for a test's fake Discord.
    pub fn with_paths(client_id: &str, paths: Vec<PathBuf>) -> Self {
        Self::build(Some(client_id.to_string()), paths)
    }

    fn build(client_id: Option<String>, paths: Vec<PathBuf>) -> Self {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        DiscordPresence {
            client_id,
            paths,
            started,
            told: None,
            worker: None,
        }
    }

    /// What the status should be now. Cheap when nothing has changed — no
    /// allocation, no I/O — so it is called every pump.
    pub fn update(&mut self, enabled: bool, song: Option<&str>, playing: bool) {
        let Some(client_id) = &self.client_id else {
            return;
        };
        let same = match &self.told {
            Some(None) => !enabled,
            Some(Some(s)) => enabled && s.playing == playing && s.song.as_deref() == song,
            None => false,
        };
        if same {
            return;
        }
        let desired = enabled.then(|| Shown {
            song: song.map(str::to_string),
            playing,
        });
        self.told = Some(desired.clone());
        // No thread is started just to be told the switch is off.
        if self.worker.is_none() && desired.is_some() {
            let (tx, rx) = std::sync::mpsc::channel();
            let (id, paths, started) = (client_id.clone(), self.paths.clone(), self.started);
            let spawned = std::thread::Builder::new()
                .name("discord".to_string())
                .spawn(move || run(&id, &paths, started, rx));
            if spawned.is_ok() {
                self.worker = Some(tx);
            }
        }
        if let Some(tx) = &self.worker
            && tx.send(desired).is_err()
        {
            // The thread is gone; the next change starts another.
            self.worker = None;
            self.told = None;
        }
    }
}

/// The worker: does what [`next_step`] says until the presence is dropped
/// and Discord no longer shows anything of ours.
fn run(client_id: &str, paths: &[PathBuf], started: u64, rx: Receiver<Option<Shown>>) {
    let mut desired: Option<Shown> = None;
    let mut alive = true;
    let mut link = Link::default();
    let mut connection: Option<Connection> = None;
    loop {
        // The newest state wins; a dropped presence means off.
        loop {
            match rx.try_recv() {
                Ok(d) => desired = d,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    alive = false;
                    desired = None;
                    break;
                }
            }
        }
        let now = Instant::now();
        match next_step(&link, desired.as_ref(), now) {
            Step::Wait => {
                if !alive {
                    return;
                }
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(d) => desired = d,
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        alive = false;
                        desired = None;
                    }
                }
            }
            Step::Connect => {
                link.last_attempt = Some(now);
                if let Ok(c) = Connection::open(paths, client_id) {
                    connection = Some(c);
                    link.connected = true;
                    link.shown = None;
                }
            }
            Step::Send(shown) => {
                link.last_send = Some(now);
                let a = activity(shown.song.as_deref(), shown.playing, started);
                match connection.as_mut().map(|c| c.set_activity(Some(&a))) {
                    Some(Ok(())) => link.shown = Some(shown),
                    _ => drop_link(&mut link, &mut connection),
                }
            }
            Step::Clear => match connection.as_mut().map(|c| c.set_activity(None)) {
                Some(Ok(())) => link.shown = None,
                _ => drop_link(&mut link, &mut connection),
            },
            Step::Close => {
                if let Some(c) = connection.take() {
                    c.close();
                }
                link.connected = false;
                link.shown = None;
            }
        }
    }
}

/// A connection that failed: Discord quit, or it stopped making sense.
/// Asked again after [`RETRY`], like one that was never there.
fn drop_link(link: &mut Link, connection: &mut Option<Connection>) {
    *connection = None;
    link.connected = false;
    link.shown = None;
    link.last_attempt = Some(Instant::now());
}
