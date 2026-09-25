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
/// **A drop is answered by asking for the same code back** (§9.2, F38). The
/// lifted client would, left to itself, re-host with backoff under a *new*
/// code — which strands everybody holding the old one — so on a lost leg
/// this drops it and re-hosts asking for its own code, on a thread, since
/// that call waits for the relay. A relay that agrees hands back the lobby
/// with whoever was in it; one that does not (Floptle Cloud today: hub card
/// `tasks/fontelle/0266`, F58) gives a new code, and a notice says so.
/// Meanwhile the "lost the connection to the relay" the client raises for
/// each joiner is kept from the session, which would otherwise forget them.
struct Hosting {
    inner: Option<Framed<Paced<RelayHost>>>,
    relay: Relay,
    build: String,
    code: String,
    /// Why the lobby ended, once it has — a code that has lapsed is never
    /// offered as live (F40).
    lapsed: Option<String>,
    reclaiming: Option<std::sync::mpsc::Receiver<Result<(RelayHost, String), String>>>,
    notices: Vec<String>,
}

/// What the lifted client says, for each joiner, when the host's own leg to
/// the relay goes.
const LOST_THE_RELAY: &str = "lost the connection to the relay";

impl Hosting {
    fn new(inner: RelayHost, relay: &Relay, build: &str, code: &str) -> Self {
        Self {
            inner: Some(Framed::new(Paced::new(inner, PACE))),
            relay: relay.clone(),
            build: build.to_string(),
            code: code.to_string(),
            lapsed: None,
            reclaiming: None,
            notices: Vec::new(),
        }
    }

    fn start_reclaiming(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (addr, build, code) = (
            self.relay.host_address(),
            self.build.clone(),
            self.code.clone(),
        );
        std::thread::spawn(move || {
            let _ = tx.send(RelayHost::host_keyed_reclaiming(
                &addr,
                FONTELLE_CLOUD_KEY,
                Some(&build),
                &code,
            ));
        });
        self.reclaiming = Some(rx);
        self.notices.push(format!(
            "lost the connection to the relay \u{2014} reconnecting and asking for {} back",
            self.code
        ));
    }
}

impl Transport for Hosting {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        // Nothing to send it on while the leg is being got back: the song's
        // hash says so on the next edit, and a joiner that drifted asks for
        // a fresh copy.
        if let Some(inner) = &mut self.inner {
            inner.send(peer, channel, bytes);
        }
    }

    fn poll(&mut self) -> Vec<Incoming> {
        if let Some(answer) = self.reclaiming.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.reclaiming = None;
            match answer {
                Ok((host, code)) => {
                    if code != self.code {
                        self.notices.push(format!(
                            "back on the relay, under a new code: {code} \u{2014} anybody \
                             holding {} has to be told",
                            self.code
                        ));
                        self.code = code;
                    }
                    self.inner = Some(Framed::new(Paced::new(host, PACE)));
                }
                Err(why) => {
                    let why = format!("could not get back onto the relay: {why}");
                    self.lapsed = Some(why.clone());
                    return vec![Incoming::Disconnected(SERVER, Some(why))];
                }
            }
        }
        let Some(inner) = &mut self.inner else {
            return Vec::new();
        };
        let mut incoming = inner.poll();
        if inner.lobby_code().is_none() && self.lapsed.is_none() {
            incoming.retain(|event| {
                !matches!(event, Incoming::Disconnected(peer, Some(why))
                    if *peer != SERVER && why == LOST_THE_RELAY)
            });
            // Dropped, so its own recovery never mints a code nobody has.
            self.inner = None;
            self.start_reclaiming();
            return incoming;
        }
        for event in &incoming {
            if let Incoming::Disconnected(peer, Some(why)) = event
                && *peer == SERVER
            {
                self.lapsed = Some(why.clone());
            }
        }
        incoming
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
