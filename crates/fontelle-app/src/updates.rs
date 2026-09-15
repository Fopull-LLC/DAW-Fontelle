//! Checking for a newer Fontelle, and installing one.
//!
//! The start menu asks this module one question at launch — *is there a
//! newer release?* — and, if the answer is yes and the user presses the
//! button, one more: *put it in place*. Both run on their own thread and
//! report back through [`Updater::status`], which the window reads once a
//! frame like everything else it draws.
//!
//! # Where a release comes from
//!
//! GitHub Releases on the project's own repository, which is the one place
//! a build is published from. `releases/latest` answers with the newest
//! non-draft, non-prerelease tag and its attached files; the release
//! workflow (`.github/workflows/release.yml`) attaches one archive per
//! target, named by [`asset_name`], and a `SHA256SUMS` beside them. Those two
//! conventions are the whole contract between the workflow and this file,
//! and `tests/updates.rs` pins the names so neither side can drift.
//!
//! # Why the transfer is `curl`'s
//!
//! For the same reason the folder picker is the desktop's own
//! (`desktop.rs`): an HTTP client with TLS is thirty crates and a C library
//! for two requests a launch, and `curl` is on every Linux, every macOS and
//! every Windows 10 there is. Its absence is reported as "could not check",
//! which is exactly what it is. The one thing done here rather than there is
//! the checksum — a download the updater cannot verify is not one it should
//! run.
//!
//! # The swap
//!
//! [`install`] unpacks the archive next to the running binary, renames the
//! old binary aside and the new one into place. Renaming is what makes it
//! safe: the running program keeps its inode, the new one is complete before
//! it has a name, and a machine that cannot delete a running executable
//! (Windows) still lets it be renamed. The `.old` left behind on such a
//! machine is removed by [`tidy`] on the next launch. A binary somewhere the
//! user cannot write — `/usr/bin` — is an error with the path in it, and the
//! menu falls back to opening the release page.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use fontelle_ui::UpdateStatus;
use sha2::{Digest, Sha256};

/// The version this binary was built as — the workspace's, so the window,
/// the release tag and the comparison are one number.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// The repository releases are published from.
pub const REPO: &str = "Fopull-LLC/DAW-Fontelle";

/// Where the newest release is asked for.
pub const LATEST_URL: &str = "https://api.github.com/repos/Fopull-LLC/DAW-Fontelle/releases/latest";

/// The page a person is sent to when the updater cannot do it for them.
pub const RELEASES_PAGE: &str = "https://github.com/Fopull-LLC/DAW-Fontelle/releases";

/// The name of the checksum file the release workflow attaches.
pub const CHECKSUMS: &str = "SHA256SUMS";

/// How long a request may take before it is a failure. The start menu is
/// drawn immediately either way; this only bounds how long the line says
/// "checking".
const TIMEOUT_SECONDS: u32 = 15;

/// A release version: three numbers, nothing else.
///
/// Pre-releases are deliberately not a version this type can hold — a
/// `-beta` tag is not something the start menu should push at people, and
/// refusing to parse it is simpler than ranking it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Reads `1.2.3` or `v1.2.3`. Anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self::new(major, minor, patch))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// One file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// A published release: its version, its page, and what is attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The human page for it — the fallback when the updater cannot install.
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// Reads GitHub's `releases/latest` answer.
    ///
    /// Only the fields used are read, and a missing tag is an error rather
    /// than a release at 0.0.0: the same endpoint answers `{"message":"Not
    /// Found"}` for a repository it cannot see, which is what a private
    /// repository looks like from outside.
    pub fn from_github_json(text: &str) -> Result<Self, String> {
        let json: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| format!("the release listing was not JSON: {e}"))?;
        let tag = json
            .get("tag_name")
            .and_then(|t| t.as_str())
            .ok_or_else(|| match json.get("message").and_then(|m| m.as_str()) {
                Some(message) => format!("no release found ({message})"),
                None => "the release listing has no tag".to_string(),
            })?;
        let version = Version::parse(tag)
            .ok_or_else(|| format!("the release tag {tag:?} is not a version"))?;
        let page = json
            .get("html_url")
            .and_then(|u| u.as_str())
            .unwrap_or(RELEASES_PAGE)
            .to_string();
        let assets = json
            .get("assets")
            .and_then(|a| a.as_array())
            .map(|assets| {
                assets
                    .iter()
                    .filter_map(|asset| {
                        Some(Asset {
                            name: asset.get("name")?.as_str()?.to_string(),
                            url: asset.get("browser_download_url")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            version,
            page,
            assets,
        })
    }

    /// The archive built for `target`, if the release has one.
    pub fn asset_for(&self, target: &str) -> Option<&Asset> {
        let wanted = asset_name(self.version, target);
        self.assets.iter().find(|a| a.name == wanted)
    }

    /// The checksum file, if the release has one.
    pub fn checksums(&self) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == CHECKSUMS)
    }
}

/// What the release workflow names the archive for one target.
///
/// Windows gets a zip because that is what Windows opens; everything else a
/// tarball, which keeps the executable bit.
pub fn asset_name(version: Version, target: &str) -> String {
    let extension = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("fontelle-{version}-{target}.{extension}")
}

/// The target triple this binary was built for, spelled the way the release
/// workflow spells it.
///
/// Composed from `cfg` rather than read from the build, so no build script
/// is needed for it; the four the workflow builds are listed in
/// `deny.toml`'s targets and covered here.
pub fn target_triple() -> String {
    let arch = std::env::consts::ARCH;
    let (vendor, os) = match std::env::consts::OS {
        "linux" => ("unknown", "linux-gnu"),
        "windows" => ("pc", "windows-msvc"),
        "macos" => ("apple", "darwin"),
        other => ("unknown", other),
    };
    format!("{arch}-{vendor}-{os}")
}

/// The SHA-256 of `bytes`, as lowercase hex — `sha256sum`'s spelling.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The checksum `sums` (in `sha256sum` format) records for `file`.
pub fn expected_sha256(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hex, name) = line.split_once("  ")?;
        // `sha256sum -b` writes a `*` before the name; either form reads.
        let name = name.trim().trim_start_matches('*');
        (name == file).then(|| hex.trim().to_ascii_lowercase())
    })
}

/// The `curl` invocation that fetches `url` to stdout.
///
/// `--fail` so an HTTP error is an error rather than an error page read as
/// JSON; `-L` because a release asset is a redirect to a CDN; `--max-time` so
/// a machine with no route gives up rather than holding the menu at
/// "checking" for ever. The user agent is required by GitHub's API and is
/// the honest one.
pub fn fetch_command(url: &str) -> (&'static str, Vec<String>) {
    (
        "curl",
        vec![
            "-sS".to_string(),
            "-L".to_string(),
            "--fail".to_string(),
            "--max-time".to_string(),
            TIMEOUT_SECONDS.to_string(),
            "-A".to_string(),
            format!("fontelle/{CURRENT}"),
            "-H".to_string(),
            "Accept: application/vnd.github+json".to_string(),
            url.to_string(),
        ],
    )
}

/// Fetches `url` with `curl`, returning the body.
pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let (program, args) = fetch_command(url);
    let output = Command::new(program)
        .args(&args)
        .output()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(plain_curl_error(stderr.trim(), output.status.code()))
    }
}

/// How long a **download** may take, against [`TIMEOUT_SECONDS`] for a
/// request whose answer is a page of JSON: an archive over a slow link is
/// minutes, and giving up at fifteen seconds was a bar that never filled.
const DOWNLOAD_TIMEOUT_SECONDS: u32 = 900;

/// [`fetch`], reporting how much has arrived as it arrives.
///
/// > *"make it so theres a progress bar when installing an update"*
///
/// Two requests: a `HEAD` for the size, which GitHub's CDN answers with a
/// `content-length` after its redirects, then the transfer itself with
/// `curl` writing to a pipe this reads in chunks — so `progress` is
/// called with the bytes so far and the total, if the server gave one. A
/// server that did not gets `None`, which the bar draws as indeterminate
/// rather than as a lie.
pub fn fetch_with_progress(
    url: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let total = content_length(url);
    progress(0, total);
    let (program, args) = fetch_command(url);
    // The same command, with the download's timeout in place of the page's.
    let args: Vec<String> = {
        let mut args = args;
        if let Some(at) = args.iter().position(|a| a == "--max-time") {
            args[at + 1] = DOWNLOAD_TIMEOUT_SECONDS.to_string();
        }
        args.push("--connect-timeout".to_string());
        args.push(TIMEOUT_SECONDS.to_string());
        args
    };
    let mut child = Command::new(program)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    let mut body = Vec::with_capacity(total.unwrap_or(0) as usize);
    if let Some(mut stdout) = child.stdout.take() {
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    body.extend_from_slice(&chunk[..n]);
                    progress(body.len() as u64, total);
                }
                Err(e) => return Err(format!("the download stopped: {e}")),
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("could not wait for {program}: {e}"))?;
    if output.status.success() {
        Ok(body)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(plain_curl_error(stderr.trim(), output.status.code()))
    }
}

/// The size `url` says it is, from a `HEAD` — the last `content-length`
/// after the redirects, which is the CDN's and the real one.
fn content_length(url: &str) -> Option<u64> {
    let output = Command::new("curl")
        .args([
            "-sI",
            "-L",
            "--max-time",
            &TIMEOUT_SECONDS.to_string(),
            "-A",
            &format!("fontelle/{CURRENT}"),
            url,
        ])
        .output()
        .ok()?;
    parse_content_length(&String::from_utf8_lossy(&output.stdout))
}

/// The last `content-length` in a run of response headers.
pub fn parse_content_length(headers: &str) -> Option<u64> {
    headers
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<u64>().ok())
                .flatten()
        })
        .next_back()
}

/// What `curl` said, as a sentence for the start menu.
///
/// `curl: (22) The requested URL returned error: 404` is true and not
/// what anybody wants to read under the logo. The two cases a person will
/// actually meet — no release published yet, and no network — get their
/// own words; the rest keep curl's, minus its prefix.
pub fn plain_curl_error(stderr: &str, code: Option<i32>) -> String {
    let said = stderr.strip_prefix("curl: ").unwrap_or(stderr);
    // The "(22) " that follows the prefix.
    let said = match said.split_once(") ") {
        Some((number, rest)) if number.starts_with('(') => rest,
        _ => said,
    };
    if said.contains("returned error: 404") {
        return "no release has been published yet".to_string();
    }
    if said.contains("Could not resolve host") || code == Some(6) || code == Some(7) {
        return "no connection".to_string();
    }
    if said.is_empty() {
        format!("curl failed ({})", code.unwrap_or(-1))
    } else {
        said.to_string()
    }
}

/// The name of the binary inside an archive.
fn binary_name() -> &'static str {
    if cfg!(windows) {
        "fontelle.exe"
    } else {
        "fontelle"
    }
}

/// Unpacks `archive` and puts the binary in it where `exe` is.
///
/// The staging folder is beside the binary rather than in `/tmp`, so the
/// final rename is on one filesystem and therefore atomic. Everything else
/// in the archive — licences, the desktop file, the installer — is left in
/// staging and removed with it: a person who installed by unpacking a
/// tarball has those already, and one who did not has a binary and nothing
/// scattered around it.
pub fn install(archive: &Path, exe: &Path) -> Result<(), String> {
    let dir = exe
        .parent()
        .ok_or_else(|| format!("{} has no folder", exe.display()))?;
    let staging = dir.join(".fontelle-update");
    std::fs::remove_dir_all(&staging).ok();
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("cannot write to {}: {e}", dir.display()))?;
    let result = unpack_and_swap(archive, exe, &staging);
    std::fs::remove_dir_all(&staging).ok();
    result
}

fn unpack_and_swap(archive: &Path, exe: &Path, staging: &Path) -> Result<(), String> {
    // `tar` reads both a tarball and a zip (bsdtar, which is what Windows
    // and macOS ship, does; GNU tar does not read zips, but Linux gets a
    // tarball).
    let status = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(staging)
        .status()
        .map_err(|e| format!("could not run tar: {e}"))?;
    if !status.success() {
        return Err(format!("could not unpack {}", archive.display()));
    }
    let fresh = find_binary(staging)
        .ok_or_else(|| format!("{} holds no {}", archive.display(), binary_name()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("could not mark {} executable: {e}", fresh.display()))?;
    }
    let old = exe.with_extension("old");
    std::fs::remove_file(&old).ok();
    if exe.exists() {
        std::fs::rename(exe, &old).map_err(|e| format!("cannot replace {}: {e}", exe.display()))?;
    }
    if let Err(e) = std::fs::rename(&fresh, exe) {
        // Put the old one back rather than leave no binary at all.
        std::fs::rename(&old, exe).ok();
        return Err(format!("cannot replace {}: {e}", exe.display()));
    }
    // Gone where the platform allows it; `tidy` gets the rest next launch.
    std::fs::remove_file(&old).ok();
    Ok(())
}

/// The binary inside an unpacked archive: at the top, or — as the release
/// workflow packs it — inside the one folder the archive holds.
fn find_binary(staging: &Path) -> Option<PathBuf> {
    let direct = staging.join(binary_name());
    if direct.is_file() {
        return Some(direct);
    }
    let mut folders = std::fs::read_dir(staging)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_dir());
    let folder = folders.next()?;
    if folders.next().is_some() {
        // Two folders is not a release archive; refuse to guess.
        return None;
    }
    let nested = folder.join(binary_name());
    nested.is_file().then_some(nested)
}

/// Removes the previous binary an upgrade left beside `exe`, if one is there.
pub fn tidy(exe: &Path) {
    std::fs::remove_file(exe.with_extension("old")).ok();
}

/// What the updater's network is: a URL in, the body out, and the bytes so
/// far reported along the way — [`fetch_with_progress`], or a table in a
/// test.
pub type Fetcher =
    Box<dyn Fn(&str, &mut dyn FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> + Send + Sync>;

/// The check and the install, on their own thread, with the answer in a
/// cell the window reads.
pub struct Updater {
    status: Arc<Mutex<UpdateStatus>>,
    release: Arc<Mutex<Option<Release>>>,
    fetch: Arc<Fetcher>,
    target: String,
    enabled: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self::new()
    }
}

impl Updater {
    /// The real one: `curl` to GitHub, this machine's target.
    pub fn new() -> Self {
        Self::with_fetcher(Box::new(fetch_with_progress))
    }

    /// One that never asks. [`Updater::status`] answers [`UpdateStatus::Off`].
    pub fn disabled() -> Self {
        let mut updater = Self::new();
        updater.enabled = false;
        *updater.status.lock().unwrap() = UpdateStatus::Off;
        updater
    }

    /// One whose network is `fetch` — for tests.
    pub fn with_fetcher(fetch: Fetcher) -> Self {
        Self {
            status: Arc::new(Mutex::new(UpdateStatus::Unchecked)),
            release: Arc::new(Mutex::new(None)),
            fetch: Arc::new(fetch),
            target: target_triple(),
            enabled: true,
        }
    }

    /// Looks for another platform's archive — for tests, which run on one
    /// machine and want to prove the naming for the others.
    pub fn for_target(mut self, target: &str) -> Self {
        self.target = target.to_string();
        self
    }

    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap().clone()
    }

    /// The newest release's page, once a check has found one.
    pub fn release_page(&self) -> Option<String> {
        self.release
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| r.page.clone())
    }

    /// Asks GitHub for the newest release, in the background.
    pub fn check(&self) {
        if !self.enabled {
            return;
        }
        *self.status.lock().unwrap() = UpdateStatus::Checking;
        let status = Arc::clone(&self.status);
        let release = Arc::clone(&self.release);
        let fetch = Arc::clone(&self.fetch);
        std::thread::spawn(move || {
            let answer = fetch(LATEST_URL, &mut |_, _| {})
                .and_then(|bytes| {
                    String::from_utf8(bytes)
                        .map_err(|_| "the release listing was not text".to_string())
                })
                .and_then(|text| Release::from_github_json(&text));
            let next = match answer {
                Ok(latest) => {
                    let current = Version::parse(CURRENT).unwrap_or(Version::new(0, 0, 0));
                    let next = if latest.version > current {
                        UpdateStatus::Available {
                            version: latest.version.to_string(),
                        }
                    } else {
                        UpdateStatus::UpToDate
                    };
                    *release.lock().unwrap() = Some(latest);
                    next
                }
                Err(why) => {
                    UpdateStatus::Failed(format!("Could not check for updates \u{2014} {why}"))
                }
            };
            *status.lock().unwrap() = next;
        });
    }

    /// Downloads the release a check found, verifies it, and puts its binary
    /// where `exe` is — in the background.
    pub fn upgrade(&self, exe: PathBuf) {
        let Some(latest) = self.release.lock().unwrap().clone() else {
            *self.status.lock().unwrap() =
                UpdateStatus::Failed("no release to install — check first".to_string());
            return;
        };
        *self.status.lock().unwrap() = UpdateStatus::Downloading {
            version: latest.version.to_string(),
            done: 0,
            total: None,
        };
        let status = Arc::clone(&self.status);
        let fetch = Arc::clone(&self.fetch);
        let target = self.target.clone();
        std::thread::spawn(move || {
            let version = latest.version.to_string();
            let progress_status = Arc::clone(&status);
            let mut progress = move |done: u64, total: Option<u64>| {
                *progress_status.lock().unwrap() = UpdateStatus::Downloading {
                    version: version.clone(),
                    done,
                    total,
                };
            };
            let result = download_and_install(&latest, &target, &exe, &fetch, &mut progress);
            *status.lock().unwrap() = match result {
                Ok(()) => UpdateStatus::Installed {
                    version: latest.version.to_string(),
                },
                Err(why) => UpdateStatus::Failed(format!("Could not install \u{2014} {why}")),
            };
        });
    }
}

fn download_and_install(
    latest: &Release,
    target: &str,
    exe: &Path,
    fetch: &Fetcher,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let asset = latest
        .asset_for(target)
        .ok_or_else(|| format!("{} has no build for {target}", latest.version))?;
    let sums = latest
        .checksums()
        .ok_or_else(|| format!("{} was published without {CHECKSUMS}", latest.version))?;
    let sums = fetch(&sums.url, &mut |_, _| {})?;
    let sums = String::from_utf8_lossy(&sums);
    let expected = expected_sha256(&sums, &asset.name)
        .ok_or_else(|| format!("{CHECKSUMS} does not list {}", asset.name))?;
    let bytes = fetch(&asset.url, progress)?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!(
            "the checksum of {} did not match — the download was not installed",
            asset.name
        ));
    }
    let dir = exe
        .parent()
        .ok_or_else(|| format!("{} has no folder", exe.display()))?;
    let archive = dir.join(&asset.name);
    std::fs::write(&archive, &bytes)
        .map_err(|e| format!("cannot write to {}: {e}", dir.display()))?;
    let result = install(&archive, exe);
    std::fs::remove_file(&archive).ok();
    result
}
