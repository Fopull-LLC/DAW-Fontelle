//! The install ping: one anonymous request a day, so installs can be counted
//! rather than guessed from downloads (hub card 0428, the floptle-platform
//! hub's `contracts/install-ping.md`).
//!
//! What it sends is the contract's body and nothing else: a random id made
//! here, the product, the version, the OS, the CPU and the update channel.
//! It is said on the start menu before anything is sent — the first launch
//! that shows the menu shows the notice and sends nothing, so anybody who
//! wants it off can turn it off before a single request — and it is off
//! under `FOPULL_NO_PING` or the Settings switch, which also deletes the id.
//!
//! Like the update check (`updates.rs`) it goes through `curl`: one more
//! subprocess and nothing more linked. It runs on a thread of its own and
//! never shows an error.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Where the pings go.
pub const ENDPOINT: &str = "https://fopull.com/api/v1/installs/ping";
/// What the start menu's notice links to.
pub const PRIVACY_URL: &str = "https://fopull.com/privacy#install-ping";
/// The file beside the settings that keeps the id and the last day sent.
pub const FILE_NAME: &str = "install-ping.json";

/// The contract's waits before a second and a third try, after a network
/// error or a 5xx: about a minute, then about ten.
const RETRIES: [Duration; 2] = [Duration::from_secs(60), Duration::from_secs(600)];

/// The id and the last UTC day a ping was recorded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PingFile {
    pub install_id: String,
    #[serde(default)]
    pub last_sent: Option<String>,
}

impl PingFile {
    pub fn read(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
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
}

/// One day's ping, ready to go.
pub struct Ping {
    pub path: PathBuf,
    pub version: String,
    pub channel: String,
    /// The UTC day it is, `YYYY-MM-DD`.
    pub today: String,
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
        let file = match PingFile::read(&self.path) {
            Some(file) => file,
            None => {
                let Some(install_id) = new_install_id() else {
                    return PingAnswer::GaveUp;
                };
                let file = PingFile {
                    install_id,
                    last_sent: None,
                };
                file.write(&self.path);
                file
            }
        };
        let request = body(&file.install_id, &self.version, &self.channel);
        for attempt in 0..=RETRIES.len() {
            if attempt > 0 {
                wait(RETRIES[attempt - 1]);
            }
            match send(&request) {
                Ok(204) | Ok(200) => {
                    PingFile {
                        last_sent: Some(self.today.clone()),
                        ..file
                    }
                    .write(&self.path);
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

/// The request body, exactly the contract's (§1).
pub fn body(install_id: &str, version: &str, channel: &str) -> String {
    serde_json::json!({
        "install_id": install_id,
        "product": "fontelle",
        "version": version,
        "os": os_name(),
        "arch": std::env::consts::ARCH,
        "channel": channel,
    })
    .to_string()
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
    let mut child = std::process::Command::new("curl")
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
