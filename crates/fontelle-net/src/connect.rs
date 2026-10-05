//! Hosting and joining a shared song over the relay (`docs/collab-plan.md`
//! §9.2–9.3).
//!
//! **Floptle Cloud** is the default: Fontelle is registered there as a game
//! (slug `fontelle`), and hosting presents its key — public by design, the way
//! every game's key ships inside the game. Joining needs nothing. A code is
//! six characters, the first naming the region whose relay holds it. A
//! **self-hosted** relay (`Settings::relay`) is the other choice: open, no
//! key, five-letter codes, and wherever the setting says.
//!
//! Every transport made here is framed ([`Framed`]) and paced ([`Paced`]):
//! a shared song's messages come in every size, and the relay closes a
//! connection that sends too fast.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::framed::Framed;
use crate::paced::{PACE, Paced};
use crate::relay::{RelayClient, RelayHost};
use crate::transport::{Channel, Incoming, LinkStats, PeerId, SERVER, Transport};

/// Fontelle's Floptle Cloud game key (`docs/collab-plan.md` §9.3). Public by
/// design, like every game's; revoking and reissuing it is Ty's, on the
/// website, and a new one is a release.
pub const FONTELLE_CLOUD_KEY: &str = "fk_live_U3JJ95XPGFZ69XZJM73SARMMHF3GZEMC";

/// Which relay a code's region letter means. Compiled in rather than asked
/// of the regions API, which would be one more HTTP client in a tree that has
/// none; a new region is a release (F41).
pub const REGIONS: &[(char, &str)] = &[('U', "us-east.relay.fopull.com:7788")];

/// Where a studio hosts, and joins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Relay {
    /// Floptle Cloud, with Fontelle's key.
    Cloud,
    /// A relay somebody runs themselves, at `host:port`.
    Open(String),
}

impl Relay {
    /// The relay the setting names: blank is Floptle Cloud.
    pub fn from_setting(setting: Option<&str>) -> Self {
        match setting.map(str::trim).filter(|s| !s.is_empty()) {
            Some(addr) => Self::Open(addr.to_string()),
            None => Self::Cloud,
        }
    }

    /// Where a host registers.
    pub fn host_address(&self) -> String {
        match self {
            Self::Cloud => REGIONS[0].1.to_string(),
            Self::Open(addr) => addr.clone(),
        }
    }

    /// Where the lobby `code` is: its region's relay, or the open relay.
    pub fn address_for(&self, code: &str) -> Result<String, String> {
        let code = code.trim().to_uppercase();
        if code.len() < 5 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(format!(
                "\u{201c}{code}\u{201d} is not a code \u{2014} a code is six letters and numbers"
            ));
        }
        match self {
            Self::Open(addr) => Ok(addr.clone()),
            Self::Cloud => {
                let region = code.chars().next().expect("checked");
                REGIONS
                    .iter()
                    .find(|(letter, _)| *letter == region)
                    .map(|(_, addr)| addr.to_string())
                    .ok_or_else(|| {
                        format!(
                            "\u{201c}{code}\u{201d} is on a relay this Fontelle does not know \
                             \u{2014} update from the start menu, or check the code"
                        )
                    })
            }
        }
    }
}

/// A lobby this studio hosts: the relay leg, framed and paced, and what to do
/// when the relay goes away.
///
/// **A drop is answered by asking for the same lobby back, with its token**
/// (§9.2, F38, F58). The relay hands a host a secret with its code
/// ([`RelayHost::reclaim_token`], hub card `tasks/fontelle/0266`) and holds
/// the lobby, joiners and all, for [`crate::HOST_GRACE`] after the host's
/// connection goes; a host that comes back inside that with the token gets
/// the same lobby. So on a lost leg this drops it — its own recovery backs
/// off 1, 2, 4, 8, 16 s and would miss most of the window — and re-hosts on a
/// thread every second or so until the relay answers, since each attempt
/// waits for it. Back under the same code, the joiners the relay still holds
/// are announced again and passed on (the session brings them up to date);
/// any it no longer holds are let go of. Back under a new code — the relay
/// restarted, or the host was away too long — every joiner of the old lobby
/// is let go of, and a notice says the new code. Meanwhile the "lost the
/// connection to the relay" the client raises for each joiner is kept from
/// the session, which would otherwise forget them.
struct Hosting {
    inner: Option<Framed<Paced<RelayHost>>>,
    relay: Relay,
    build: String,
    code: String,
    /// What proves the lobby is this studio's, once the relay has sent it.
    token: Option<[u8; 16]>,
    /// The joiners the relay has said are in the lobby.
    peers: BTreeSet<PeerId>,
    /// Back under the same code: the next poll is the relay's roll call.
    roll_call: bool,
    /// Why the lobby ended, once it has — a code that has lapsed is never
    /// offered as live (F40).
    lapsed: Option<String>,
    reclaiming: Option<Reclaiming>,
    notices: Vec<String>,
    /// Handed to the leg, and to the one got back after a drop (F48).
    wake: Option<crate::transport::Wake>,
}

/// A re-host under way on its own thread.
struct Reclaiming {
    answer: std::sync::mpsc::Receiver<Result<(RelayHost, String), String>>,
    stop: Arc<AtomicBool>,
}

/// How long a host that lost the relay keeps trying to get back: well past
/// the relay's grace, after which coming back still keeps the song shared,
/// under a new code.
const RECLAIM_FOR: Duration = Duration::from_secs(60);

/// What the lifted client says, for each joiner, when the host's own leg to
/// the relay goes.
const LOST_THE_RELAY: &str = "lost the connection to the relay";

impl Hosting {
    fn new(inner: RelayHost, relay: &Relay, build: &str, code: &str) -> Self {
        let token = inner.reclaim_token();
        Self {
            inner: Some(Framed::new(Paced::new(inner, PACE))),
            relay: relay.clone(),
            build: build.to_string(),
            code: code.to_string(),
            token,
            peers: BTreeSet::new(),
            roll_call: false,
            lapsed: None,
            reclaiming: None,
            notices: Vec::new(),
            wake: None,
        }
    }

    fn start_reclaiming(&mut self) {
        let (tx, answer) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (addr, build, code, token) = (
            self.relay.host_address(),
            self.build.clone(),
            self.code.clone(),
            self.token,
        );
        std::thread::spawn(move || {
            let until = Instant::now() + RECLAIM_FOR;
            loop {
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
                let attempt = match token {
                    Some(token) => RelayHost::host_reclaiming(
                        &addr,
                        Some(FONTELLE_CLOUD_KEY),
                        Some(&build),
                        &code,
                        token,
                    ),
                    // A relay too old to hand out tokens: asking for the code
                    // is all there is.
                    None => RelayHost::host_keyed_reclaiming(
                        &addr,
                        FONTELLE_CLOUD_KEY,
                        Some(&build),
                        &code,
                    ),
                };
                match attempt {
                    Ok(back) => {
                        // Nobody is waiting for it any more: let it go, so the
                        // relay does not hold a lobby for nobody.
                        if !stopped.load(Ordering::Relaxed) {
                            let _ = tx.send(Ok(back));
                        }
                        return;
                    }
                    Err(why) if Instant::now() >= until => {
                        let _ = tx.send(Err(why));
                        return;
                    }
                    Err(_) => std::thread::sleep(Duration::from_secs(1)),
                }
            }
        });
        self.reclaiming = Some(Reclaiming { answer, stop });
        self.notices.push(format!(
            "Lost the connection to the relay \u{2014} reconnecting; {} stays yours for a \
             little while",
            self.code
        ));
    }

    /// The relay's answer to a re-host, when it has come. `Err` is the end.
    fn land_reclaim(&mut self, out: &mut Vec<Incoming>) -> Result<(), String> {
        let Some(answer) = self
            .reclaiming
            .as_ref()
            .and_then(|r| r.answer.try_recv().ok())
        else {
            return Ok(());
        };
        self.reclaiming = None;
        let (host, code) =
            answer.map_err(|why| format!("could not get back onto the relay: {why}"))?;
        if code == self.code {
            self.notices.push(format!(
                "Back on the relay \u{2014} {code} is still yours, and everybody in it still is"
            ));
            self.roll_call = true;
        } else {
            self.notices.push(format!(
                "Back on the relay under a new code, {code} \u{2014} anybody holding {} has to \
                 be told it",
                self.code
            ));
            // Gone with the old lobby: nobody can reach them now.
            for peer in std::mem::take(&mut self.peers) {
                out.push(Incoming::refused(
                    peer,
                    "the relay lost the lobby they were in",
                ));
            }
            self.code = code;
        }
        if let Some(token) = host.reclaim_token() {
            self.token = Some(token);
        }
        let mut inner = Framed::new(Paced::new(host, PACE));
        if let Some(wake) = &self.wake {
            inner.set_wake(wake.clone());
        }
        self.inner = Some(inner);
        Ok(())
    }
}

impl Drop for Hosting {
    fn drop(&mut self) {
        if let Some(reclaiming) = &self.reclaiming {
            reclaiming.stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Transport for Hosting {
    fn set_wake(&mut self, wake: crate::transport::Wake) {
        if let Some(inner) = &mut self.inner {
            inner.set_wake(wake.clone());
        }
        self.wake = Some(wake);
    }

    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        // Nothing to send it on while the leg is being got back: the session
        // sends each joiner a fresh copy of the song once it is.
        if let Some(inner) = &mut self.inner {
            inner.send(peer, channel, bytes);
        }
    }

    fn poll(&mut self) -> Vec<Incoming> {
        let mut out = Vec::new();
        if let Err(why) = self.land_reclaim(&mut out) {
            self.lapsed = Some(why.clone());
            out.push(Incoming::Disconnected(SERVER, Some(why)));
            return out;
        }
        let Some(inner) = &mut self.inner else {
            return out;
        };
        let mut incoming = inner.poll();
        if let Some(token) = inner.get_ref().get_ref().reclaim_token() {
            self.token = Some(token);
        }
        // The relay itself ended the lobby — nobody joined it for half an
        // hour, or it refused the host — and that is the end, not a blip.
        for event in &incoming {
            if let Incoming::Disconnected(peer, Some(why)) = event
                && *peer == SERVER
            {
                self.lapsed = Some(why.clone());
            }
        }
        if inner.lobby_code().is_none() && self.lapsed.is_none() {
            incoming.retain(|event| {
                !matches!(event, Incoming::Disconnected(peer, Some(why))
                    if *peer != SERVER && why == LOST_THE_RELAY)
            });
            // Dropped, so its own recovery never mints a code nobody has.
            self.inner = None;
            self.start_reclaiming();
            out.extend(incoming);
            return out;
        }
        if std::mem::take(&mut self.roll_call) {
            // The relay announces everybody it still holds in the lobby
            // before it hands the code back, so they are all here.
            let here: BTreeSet<PeerId> = incoming
                .iter()
                .filter_map(|event| match event {
                    Incoming::Connected(peer) => Some(*peer),
                    _ => None,
                })
                .collect();
            for peer in self.peers.difference(&here) {
                out.push(Incoming::dropped(*peer));
            }
            self.peers = here;
        }
        for event in &incoming {
            match event {
                Incoming::Connected(peer) if *peer != SERVER => {
                    self.peers.insert(*peer);
                }
                Incoming::Disconnected(peer, _) if *peer != SERVER => {
                    self.peers.remove(peer);
                }
                _ => {}
            }
        }
        out.extend(incoming);
        out
    }

    fn stats(&self, peer: PeerId) -> LinkStats {
        self.inner
            .as_ref()
            .map(|inner| inner.stats(peer))
            .unwrap_or_default()
    }

    fn disconnect(&mut self, peer: PeerId) {
        if let Some(inner) = &mut self.inner {
            inner.disconnect(peer);
        }
    }

    fn take_notices(&mut self) -> Vec<String> {
        let mut notices = std::mem::take(&mut self.notices);
        if let Some(inner) = &mut self.inner {
            notices.extend(inner.take_notices());
        }
        notices
    }

    fn lobby_code(&self) -> Option<String> {
        if self.lapsed.is_some() {
            return None;
        }
        self.inner.as_ref().and_then(|inner| inner.lobby_code())
    }
}

/// Registers a lobby for a song this studio shares, and says its code.
///
/// **Blocks for up to about three seconds** while the relay answers — the
/// window runs it off its own thread. `build` is this Fontelle's version,
/// which a managed relay records beside the key.
pub fn host(relay: &Relay, build: &str) -> Result<(Box<dyn Transport>, String), String> {
    let (inner, code) =
        RelayHost::host_keyed(&relay.host_address(), FONTELLE_CLOUD_KEY, Some(build))?;
    Ok((Box::new(Hosting::new(inner, relay, build, &code)), code))
}

/// Registers a lobby asking for `code` back — the code this studio had before
/// its connection to the relay dropped. A relay that agrees gives the same
/// lobby, with whoever was in it; one that does not gives a new code, and
/// the caller has to say so (§9.2, F38, F58).
pub fn reclaim(
    relay: &Relay,
    build: &str,
    code: &str,
) -> Result<(Box<dyn Transport>, String), String> {
    let (inner, code) = RelayHost::host_keyed_reclaiming(
        &relay.host_address(),
        FONTELLE_CLOUD_KEY,
        Some(build),
        code,
    )?;
    Ok((Box::new(Hosting::new(inner, relay, build, &code)), code))
}

/// Joins the lobby `code`. Does not block: the join rides the same ordered
/// stream as everything after it, and a refusal ("no such lobby") arrives as
/// a disconnect with the relay's own words.
pub fn join(relay: &Relay, code: &str) -> Result<Box<dyn Transport>, String> {
    let addr = relay.address_for(code)?;
    let inner = RelayClient::join(&addr, &code.trim().to_uppercase())?;
    Ok(Box::new(Framed::new(Paced::new(inner, PACE))))
}
