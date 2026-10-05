//! A shared song over the relay (`docs/collab-plan.md` §9, §12.3).
//!
//! Every test here runs its own relay in-process on `127.0.0.1:0` — the
//! lifted `RelayServer`, the same code Floptle Cloud runs — and never touches
//! the internet, except the one marked `#[ignore]`, which is the manual check
//! against `relay.fopull.com` (F34).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fontelle_net::{
    Channel, FONTELLE_CLOUD_KEY, FRAME, Framed, HostAdmission, Incoming, MemoryHub, Paced, PeerId,
    Relay, RelayLimits, RelayPolicy, RelayServer, SERVER, Transport,
};

/// A relay on a thread of its own, stepping until it is dropped.
struct InProcessRelay {
    addr: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl InProcessRelay {
    fn start(policy: Option<Box<dyn RelayPolicy>>, limits: Option<RelayLimits>) -> Self {
        Self::start_on(0, policy, limits)
    }

    /// On a named port — `port` again is a relay restarted, an upgrade as a
    /// host sees one. Tried for a few seconds: the old socket is closed by a
    /// thread of its own, and some systems hold the port a moment after.
    fn start_on(
        port: u16,
        policy: Option<Box<dyn RelayPolicy>>,
        limits: Option<RelayLimits>,
    ) -> Self {
        let started = Instant::now();
        let mut server = loop {
            match RelayServer::bind(port) {
                Ok(server) => break server,
                Err(e) if started.elapsed() < Duration::from_secs(5) && port != 0 => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("a relay binds to port {port}: {e}"),
            }
        };
        if let Some(policy) = policy {
            server.set_policy(policy);
        }
        if let Some(limits) = limits {
            server.set_limits(limits);
        }
        let addr = format!("127.0.0.1:{}", server.port());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                server.step();
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        Self {
            addr,
            stop,
            thread: Some(thread),
        }
    }

    /// A self-hosted relay: anybody may host, codes are five letters.
    fn open() -> Self {
        Self::start(None, None)
    }

    fn relay(&self) -> Relay {
        Relay::Open(self.addr.clone())
    }

    fn port(&self) -> u16 {
        self.addr.rsplit(':').next().unwrap().parse().unwrap()
    }
}

impl Drop for InProcessRelay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The managed rules as far as these tests need them: Fontelle's key may
/// host, and a host may have back **the code it was given** — the policy hub
/// card `tasks/fontelle/0266` asks Floptle Cloud's relay to take (F58).
#[derive(Default)]
struct OwnCodes {
    given: HashMap<String, String>,
}

impl RelayPolicy for OwnCodes {
    fn admit_host(&mut self, key: Option<&str>, _build: Option<&str>) -> HostAdmission {
        if key == Some(FONTELLE_CLOUD_KEY) {
            HostAdmission::Allow { prefix: Some('U') }
        } else {
            HostAdmission::Refuse {
                reason: "not Fontelle".into(),
            }
        }
    }

    fn claim_code(&mut self, key: Option<&str>, code: &str) -> bool {
        key.is_some_and(|key| self.given.get(code).is_some_and(|owner| owner == key))
    }

    fn lobby_opened(&mut self, code: &str, key: Option<&str>, _from: Option<std::net::IpAddr>) {
        if let Some(key) = key {
            self.given.insert(code.to_string(), key.to_string());
        }
    }
}

/// Polls until `found` says yes, or a few seconds pass.
fn poll_until(
    transport: &mut dyn Transport,
    mut found: impl FnMut(&Incoming) -> bool,
) -> Option<Incoming> {
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        for incoming in transport.poll() {
            if found(&incoming) {
                return Some(incoming);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

fn message(incoming: &Incoming) -> Option<(PeerId, &[u8])> {
    match incoming {
        Incoming::Message(peer, Channel::Reliable, bytes) => Some((*peer, bytes)),
        _ => None,
    }
}

/// F33. A host registers, a joiner joins by the code, and bytes cross both
/// ways — through a relay that understands none of them.
#[test]
fn host_join_and_echo_through_an_in_process_relay() {
    let relay = InProcessRelay::open();
    let (mut host, code) = fontelle_net::host(&relay.relay(), "test").expect("hosts");
    assert_eq!(
        code.len(),
        5,
        "an open relay's codes are five letters: {code}"
    );
    assert_eq!(host.lobby_code().as_deref(), Some(code.as_str()));

    let mut joiner = fontelle_net::join(&relay.relay(), &code).expect("joins");
    joiner.send(SERVER, Channel::Reliable, b"hello");
    let Some(Incoming::Message(peer, _, bytes)) =
        poll_until(host.as_mut(), |i| message(i).is_some())
    else {
        panic!("the host never heard the joiner");
    };
    assert_eq!(bytes, b"hello");

    host.send(peer, Channel::Reliable, b"hello back");
    let back = poll_until(joiner.as_mut(), |i| message(i).is_some()).expect("an answer");
    assert_eq!(message(&back).unwrap().1, b"hello back");
}

/// F48. A message arriving is announced the moment it lands, on the network
/// thread, so the window can wake for it rather than finding it on its next
/// look — both ways round, and without anybody polling in between.
#[test]
fn a_message_wakes_whoever_is_waiting() {
    use std::sync::atomic::AtomicUsize;
    let relay = InProcessRelay::open();
    let (mut host, code) = fontelle_net::host(&relay.relay(), "test").expect("hosts");
    let mut joiner = fontelle_net::join(&relay.relay(), &code).expect("joins");
    let counter = |count: &Arc<AtomicUsize>| -> fontelle_net::Wake {
        let count = count.clone();
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        })
    };
    let host_woke = Arc::new(AtomicUsize::new(0));
    let joiner_woke = Arc::new(AtomicUsize::new(0));
    host.set_wake(counter(&host_woke));
    joiner.set_wake(counter(&joiner_woke));

    joiner.send(SERVER, Channel::Reliable, b"hello");
    let start = Instant::now();
    while host_woke.load(Ordering::SeqCst) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the host was never woken"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let Some(Incoming::Message(peer, _, _)) = poll_until(host.as_mut(), |i| message(i).is_some())
    else {
        panic!("woken, and then nothing to read");
    };

    let before = joiner_woke.load(Ordering::SeqCst);
    host.send(peer, Channel::Reliable, b"hello back");
    let start = Instant::now();
    while joiner_woke.load(Ordering::SeqCst) == before {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the joiner was never woken"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let back = poll_until(joiner.as_mut(), |i| message(i).is_some()).expect("an answer");
    assert_eq!(message(&back).unwrap().1, b"hello back");
}

/// Keeps what an inner transport was asked to send, with when.
struct Recorder<T> {
    inner: T,
    sent: Arc<Mutex<Vec<(Duration, usize)>>>,
    clock: Arc<Mutex<Duration>>,
}

impl<T: Transport> Transport for Recorder<T> {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        let now = *self.clock.lock().unwrap();
        self.sent.lock().unwrap().push((now, bytes.len()));
        self.inner.send(peer, channel, bytes);
    }
    fn poll(&mut self) -> Vec<Incoming> {
        self.inner.poll()
    }
    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.stats(peer)
    }
}

/// F35. A message bigger than the relay lets through in one frame is sent in
/// frames that fit, and arrives as the message it was — over the hub and over
/// the relay, whose client-to-host cap is 64 KB.
#[test]
fn a_frame_over_48kb_is_chunked_and_reassembled() {
    let big: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();

    let hub = MemoryHub::new();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let clock = Arc::new(Mutex::new(Duration::ZERO));
    let mut host = Framed::new(hub.server_endpoint());
    let mut joiner = Framed::new(Recorder {
        inner: hub.connect(),
        sent: sent.clone(),
        clock,
    });
    joiner.send(SERVER, Channel::Reliable, &big);
    let arrived = poll_until(&mut host, |i| message(i).is_some()).expect("it arrives");
    assert_eq!(
        message(&arrived).unwrap().1,
        big.as_slice(),
        "whole, and in order"
    );
    let frames = sent.lock().unwrap().clone();
    assert!(frames.len() >= 6, "{} frames", frames.len());
    assert!(
        frames.iter().all(|(_, len)| *len <= FRAME + 16),
        "every frame fits: {frames:?}"
    );

    let relay = InProcessRelay::open();
    let (mut host, code) = fontelle_net::host(&relay.relay(), "test").unwrap();
    let mut joiner = fontelle_net::join(&relay.relay(), &code).unwrap();
    joiner.send(SERVER, Channel::Reliable, &big);
    // What is paced goes out as the joiner is polled, as a session does
    // every tick.
    let until = Instant::now() + Duration::from_secs(10);
    let arrived = loop {
        assert!(Instant::now() < until, "never arrived across the relay");
        joiner.poll();
        if let Some(arrived) = host.poll().into_iter().find(|i| message(i).is_some()) {
            break arrived;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(message(&arrived).unwrap().1, big.as_slice());
}

/// Passes on only as many frames as it is let, into a link it shares.
struct Gate {
    inner: Arc<Mutex<fontelle_net::MemoryTransport>>,
    pass: Arc<std::sync::atomic::AtomicUsize>,
}

impl Transport for Gate {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        let left = self.pass.load(Ordering::SeqCst);
        if left > 0 {
            self.pass.store(left - 1, Ordering::SeqCst);
            self.inner.lock().unwrap().send(peer, channel, bytes);
        }
    }
    fn poll(&mut self) -> Vec<Incoming> {
        self.inner.lock().unwrap().poll()
    }
    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.lock().unwrap().stats(peer)
    }
}

/// A message cut off half way — its sender's connection dropped between two
/// of its frames — does not cost the next message that happens to carry its
/// number. A host back on the relay frames afresh and numbers from nought
/// again, and the joiner, still holding the first frame of a message that
/// will never be finished, threw away the first big message after the
/// reconnect: usually the fresh copy of the song that was meant to mend it.
#[test]
fn a_message_cut_off_half_way_does_not_cost_the_next_one() {
    use std::sync::atomic::AtomicUsize;
    let hub = MemoryHub::new();
    let mut host = Framed::new(hub.server_endpoint());
    let link = Arc::new(Mutex::new(hub.connect()));
    let pass = Arc::new(AtomicUsize::new(1));
    let big: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();

    let mut cut = Framed::new(Gate {
        inner: link.clone(),
        pass: pass.clone(),
    });
    cut.send(SERVER, Channel::Reliable, &big);
    assert!(
        gather_until(&mut host, Duration::from_millis(100), |_| false)
            .iter()
            .all(|i| message(i).is_none()),
        "half a message is not a message"
    );

    // Back again, framing afresh.
    pass.store(usize::MAX, Ordering::SeqCst);
    let mut again = Framed::new(Gate { inner: link, pass });
    let next: Vec<u8> = big.iter().rev().copied().collect();
    again.send(SERVER, Channel::Reliable, &next);
    let arrived = poll_until(&mut host, |i| message(i).is_some()).expect("the next one arrives");
    assert_eq!(message(&arrived).unwrap().1, next.as_slice());
}

/// F36. However much is handed over at once, what leaves never goes over
/// the pace in any second — the relay closes a connection three seconds over
/// its budget, and the sender, not the relay, is the one that must hold back.
#[test]
fn the_sender_never_exceeds_the_budget() {
    const RATE: u64 = 480 * 1024;
    let hub = MemoryHub::new();
    let _host = hub.server_endpoint();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let clock = Arc::new(Mutex::new(Duration::ZERO));
    let ticking = clock.clone();
    let mut paced = Paced::with_clock(
        Recorder {
            inner: hub.connect(),
            sent: sent.clone(),
            clock: clock.clone(),
        },
        RATE,
        Box::new(move || *ticking.lock().unwrap()),
    );
    let piece = vec![0u8; 48 * 1024];
    let pieces = 214; // a little over 10 MB
    let total = pieces * piece.len();
    for _ in 0..pieces {
        paced.send(SERVER, Channel::Reliable, &piece);
    }
    for _ in 0..40_000 {
        *clock.lock().unwrap() += Duration::from_millis(10);
        paced.poll();
        if sent.lock().unwrap().iter().map(|(_, n)| n).sum::<usize>() >= total {
            break;
        }
    }
    let sent = sent.lock().unwrap().clone();
    assert_eq!(
        sent.iter().map(|(_, n)| n).sum::<usize>(),
        total,
        "all of it went"
    );
    for (start, _) in &sent {
        let in_second: usize = sent
            .iter()
            .filter(|(at, _)| *at >= *start && *at < *start + Duration::from_secs(1))
            .map(|(_, n)| n)
            .sum();
        assert!(
            in_second as u64 <= RATE,
            "{in_second} bytes in the second from {start:?}"
        );
    }
    let last = sent.last().unwrap().0;
    assert!(
        last >= Duration::from_secs(20),
        "10 MB at 480 KiB/s takes about 21 s, not {last:?}"
    );
}

/// Hub card 0264: Floptle Cloud grants Fontelle's key 2 MiB a second per
/// connection (relay 0.107 and later; us-east runs 0.109.1), four times the
/// 512 KiB any other relay allows. A song loads at the pace of its relay —
/// about three and a half times faster on Floptle Cloud than it did — and
/// never at the edge of what the relay allows: over it, the relay drops what
/// was sent (on the reliable channel, a message lost) and, three seconds
/// running, closes the connection.
#[test]
fn each_relay_is_paced_under_what_it_allows() {
    const FONTELLE_GRANT: u64 = 2 * 1024 * 1024;
    let default = RelayLimits::default().bytes_per_window;
    assert_eq!(default, 512 * 1024, "the relay's own budget moved");
    for (relay, budget) in [
        (Relay::Cloud, FONTELLE_GRANT),
        (Relay::Open("10.0.0.2:7788".into()), default),
    ] {
        let pace = relay.pace();
        // What arrives in one of the relay's seconds is at most a second's
        // pace, one helping and the frame that may finish it, and whatever a
        // delay on the way bunched up: room for 80 ms of that.
        let delay = pace * 8 / 100;
        let worst = pace + fontelle_net::pace_burst(pace) + FRAME as u64 + delay;
        assert!(worst <= budget, "{relay:?}: {worst} over {budget}");
    }
    assert!(
        Relay::Cloud.pace() * 10 >= 480 * 1024 * 34,
        "a song loads more than three times faster on Floptle Cloud than at the old pace"
    );
}

/// What leaves goes in small helpings, not a whole second's budget at once:
/// a second's worth handed over in one go arrived at the relay inside one of
/// its seconds with the start of the next one, and went over.
#[test]
fn the_sender_spreads_a_second_over_the_second() {
    const RATE: u64 = 1664 * 1024;
    let hub = MemoryHub::new();
    let _host = hub.server_endpoint();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let clock = Arc::new(Mutex::new(Duration::ZERO));
    let ticking = clock.clone();
    let mut paced = Paced::with_clock(
        Recorder {
            inner: hub.connect(),
            sent: sent.clone(),
            clock: clock.clone(),
        },
        RATE,
        Box::new(move || *ticking.lock().unwrap()),
    );
    let piece = vec![0u8; 48 * 1024];
    let pieces = 214;
    let total = pieces * piece.len();
    for _ in 0..pieces {
        paced.send(SERVER, Channel::Reliable, &piece);
    }
    // The window looks every tenth of a second while a session is open.
    for _ in 0..4_000 {
        *clock.lock().unwrap() += Duration::from_millis(100);
        paced.poll();
        if sent.lock().unwrap().iter().map(|(_, n)| n).sum::<usize>() >= total {
            break;
        }
    }
    let sent = sent.lock().unwrap().clone();
    assert_eq!(sent.iter().map(|(_, n)| n).sum::<usize>(), total);
    let eighth = Duration::from_millis(125);
    for (start, _) in &sent {
        let in_eighth: u64 = sent
            .iter()
            .filter(|(at, _)| *at >= *start && *at < *start + eighth)
            .map(|(_, n)| *n as u64)
            .sum();
        assert!(
            in_eighth <= RATE / 4,
            "{in_eighth} bytes in the eighth of a second from {start:?}"
        );
    }
    // And spreading it costs nothing: it still goes at the pace.
    let last = sent.last().unwrap().0.as_secs_f64();
    let ideal = total as f64 / RATE as f64;
    assert!(
        last <= ideal * 1.1,
        "{last} s for what the pace sends in {ideal} s"
    );
}

/// F38 and F58. A host that drops asks for its own code back; a relay that
/// agrees gives it the same lobby, with the joiner still in it.
#[test]
fn a_lost_host_reconnects_inside_the_grace() {
    let relay = InProcessRelay::start(Some(Box::new(OwnCodes::default())), None);
    let (host, code) = fontelle_net::host(&relay.relay(), "test").expect("hosts");
    assert_eq!(
        code.len(),
        6,
        "a managed relay's codes carry a region: {code}"
    );
    let mut joiner = fontelle_net::join(&relay.relay(), &code).expect("joins");
    joiner.send(SERVER, Channel::Reliable, b"here");
    let mut host = host;
    let first = poll_until(host.as_mut(), |i| message(i).is_some()).expect("the joiner is in");
    let peer = message(&first).unwrap().0;

    // The host's connection goes; the relay holds the lobby.
    drop(host);
    std::thread::sleep(Duration::from_millis(200));

    let (mut again, same) = fontelle_net::reclaim(&relay.relay(), "test", &code).expect("re-hosts");
    assert_eq!(
        same, code,
        "the same code, so nobody has to be told a new one"
    );
    // The same lobby, with the joiner still in it under the id it had: the
    // session above this keeps its roster across the reconnect, so the
    // relay's re-announcement of who is there is not needed (and the lifted
    // client's handshake loop does not pass it on).
    again.send(peer, Channel::Reliable, b"back");
    let back = poll_until(joiner.as_mut(), |i| message(i).is_some()).expect("it reaches");
    assert_eq!(message(&back).unwrap().1, b"back");
    joiner.send(SERVER, Channel::Reliable, b"still here");
    let still = poll_until(again.as_mut(), |i| message(i).is_some()).expect("and back");
    assert_eq!(message(&still).unwrap(), (peer, &b"still here"[..]));
}

/// Polls until `found` has said yes to something, keeping everything seen.
fn gather_until(
    transport: &mut dyn Transport,
    within: Duration,
    mut found: impl FnMut(&[Incoming]) -> bool,
) -> Vec<Incoming> {
    let until = Instant::now() + within;
    let mut seen = Vec::new();
    while Instant::now() < until {
        seen.extend(transport.poll());
        if found(&seen) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    seen
}

/// Ty: *"i really do want it to work well so users can work on songs
/// together"*. A host's connection that drops — a Wi-Fi blip, a laptop
/// changing networks — comes back **under the same code, on its own**, and
/// the joiner held in the lobby meanwhile is still there: nobody is told a
/// new code, nobody joins again.
///
/// On an open relay, which has no policy to vouch for a code: what proves
/// the lobby is this host's is the token the relay handed it with the code
/// (hub card 0266). The relay here closes the host for going over a budget
/// set low on purpose — the one way to cut a live leg from outside it.
#[test]
fn a_host_whose_connection_drops_comes_back_under_its_own_code() {
    let relay = InProcessRelay::start(
        None,
        Some(RelayLimits {
            bytes_per_window: 32 * 1024,
            strikes: 1,
            ..RelayLimits::default()
        }),
    );
    let (mut host, code) = fontelle_net::host(&relay.relay(), "test").expect("hosts");
    let mut joiner = fontelle_net::join(&relay.relay(), &code).expect("joins");
    joiner.send(SERVER, Channel::Reliable, b"here");
    let first = poll_until(host.as_mut(), |i| message(i).is_some()).expect("the joiner is in");
    let peer = message(&first).unwrap().0;

    // Over the budget: the relay closes the host's connection.
    host.send(peer, Channel::Reliable, &vec![0u8; 100_000]);
    let start = Instant::now();
    let mut gone = false;
    let mut seen = Vec::new();
    while start.elapsed() < Duration::from_secs(15) {
        seen.extend(host.poll());
        match host.lobby_code() {
            None => gone = true,
            Some(back) if gone => {
                assert_eq!(
                    back, code,
                    "the same code, so nobody has to be told a new one"
                );
                break;
            }
            Some(_) => {}
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(gone, "the host's connection was never cut");
    assert_eq!(
        host.lobby_code().as_deref(),
        Some(code.as_str()),
        "the host is back on the relay"
    );
    // The joiner is still in the lobby, under the id it had: the relay says
    // so, and it is passed on, so the session can bring it up to date.
    seen.extend(gather_until(
        host.as_mut(),
        Duration::from_secs(2),
        |seen| {
            seen.iter()
                .any(|i| matches!(i, Incoming::Connected(p) if *p == peer))
        },
    ));
    assert!(
        seen.iter()
            .any(|i| matches!(i, Incoming::Connected(p) if *p == peer)),
        "the host was not told who is still here: {seen:?}"
    );
    assert!(
        !seen
            .iter()
            .any(|i| matches!(i, Incoming::Disconnected(p, _) if *p == peer)),
        "the joiner was let go of: {seen:?}"
    );
    let said = host.take_notices().join(" ");
    assert!(
        said.contains(&code),
        "the host is told what happened: {said}"
    );

    host.send(peer, Channel::Reliable, b"back");
    let back = poll_until(joiner.as_mut(), |i| {
        message(i).is_some() || matches!(i, Incoming::Disconnected(..))
    })
    .expect("it reaches");
    assert_eq!(message(&back).map(|m| m.1), Some(&b"back"[..]), "{back:?}");
    joiner.send(SERVER, Channel::Reliable, b"still here");
    let still = poll_until(host.as_mut(), |i| message(i).is_some()).expect("and back");
    assert_eq!(message(&still).unwrap(), (peer, &b"still here"[..]));
}

/// The other end of the same story: a relay that has forgotten the lobby — it
/// restarted, or the host was away past the grace — gives the host a new
/// code. The host keeps sharing under it and is told it, and the joiners of
/// the old lobby, who are gone with it, are let go of rather than kept as
/// people nobody can reach.
#[test]
fn a_host_back_under_a_new_code_lets_the_old_lobbys_joiners_go() {
    let relay = InProcessRelay::open();
    let port = relay.port();
    let (mut host, code) = fontelle_net::host(&relay.relay(), "test").expect("hosts");
    let mut joiner = fontelle_net::join(&relay.relay(), &code).expect("joins");
    joiner.send(SERVER, Channel::Reliable, b"here");
    let first = poll_until(host.as_mut(), |i| message(i).is_some()).expect("the joiner is in");
    let peer = message(&first).unwrap().0;

    drop(relay);
    let again = InProcessRelay::start_on(port, None, None);
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < Duration::from_secs(20) {
        seen.extend(host.poll());
        if host.lobby_code().is_some_and(|c| c != code) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let new = host.lobby_code().expect("back on the relay");
    assert_ne!(new, code, "a restarted relay knows no lobby to give back");
    seen.extend(gather_until(
        host.as_mut(),
        Duration::from_millis(200),
        |_| false,
    ));
    assert!(
        seen.iter()
            .any(|i| matches!(i, Incoming::Disconnected(p, _) if *p == peer)),
        "the old lobby's joiner is let go of: {seen:?}"
    );
    let said = host.take_notices().join(" ");
    assert!(said.contains(&new), "the new code is said: {said}");
    drop(again);
}

/// The joiner's side of the same thing: its own connection drops — the
/// laptop changed networks — and it joins the same lobby again by itself,
/// saying so with a second `Connected`, so the session can say hello again.
/// The host sees the old connection go and a new one come.
#[test]
fn a_joiner_whose_connection_drops_joins_again_on_its_own() {
    let relay = InProcessRelay::start(
        None,
        Some(RelayLimits {
            bytes_per_window: 32 * 1024,
            strikes: 1,
            ..RelayLimits::default()
        }),
    );
    let (mut host, code) = fontelle_net::host(&relay.relay(), "test").expect("hosts");
    let mut joiner = fontelle_net::join(&relay.relay(), &code).expect("joins");
    let first = gather_until(joiner.as_mut(), Duration::from_secs(5), |seen| {
        seen.iter()
            .any(|i| matches!(i, Incoming::Connected(SERVER)))
    });
    assert!(
        first
            .iter()
            .any(|i| matches!(i, Incoming::Connected(SERVER))),
        "{first:?}"
    );
    joiner.send(SERVER, Channel::Reliable, b"here");
    let heard = poll_until(host.as_mut(), |i| message(i).is_some()).expect("the joiner is in");
    let old = message(&heard).unwrap().0;

    // Over the budget: the relay closes the joiner's connection.
    joiner.send(SERVER, Channel::Reliable, &vec![0u8; 100_000]);
    let again = gather_until(joiner.as_mut(), Duration::from_secs(10), |seen| {
        seen.iter()
            .any(|i| matches!(i, Incoming::Connected(SERVER)))
    });
    assert!(
        again
            .iter()
            .any(|i| matches!(i, Incoming::Connected(SERVER))),
        "the joiner did not join again: {again:?}"
    );
    assert!(
        !again
            .iter()
            .any(|i| matches!(i, Incoming::Disconnected(..))),
        "a blip is not the end: {again:?}"
    );

    joiner.send(SERVER, Channel::Reliable, b"back");
    let mut seen = Vec::new();
    let back = poll_until(host.as_mut(), |i| {
        seen.push(format!("{i:?}"));
        message(i).is_some()
    })
    .expect("it reaches the host");
    let new = message(&back).unwrap().0;
    assert_ne!(new, old, "a new connection is a new peer to the relay");
    host.send(new, Channel::Reliable, b"welcome back");
    let reply = poll_until(joiner.as_mut(), |i| message(i).is_some()).expect("and back");
    assert_eq!(message(&reply).unwrap().1, b"welcome back");
    let said = joiner.take_notices().join(" ");
    assert!(said.contains("reconnect"), "{said}");
}

/// A join the relay turns away is told so at once, and is not tried again:
/// a wrong code stays wrong.
#[test]
fn a_join_the_relay_turns_away_is_told_and_not_retried() {
    let relay = InProcessRelay::open();
    let mut joiner = fontelle_net::join(&relay.relay(), "ZZZZZ").expect("connects");
    let seen = gather_until(joiner.as_mut(), Duration::from_secs(5), |seen| {
        seen.iter().any(|i| matches!(i, Incoming::Disconnected(..)))
    });
    assert!(
        seen.iter().any(
            |i| matches!(i, Incoming::Disconnected(SERVER, Some(why)) if why.contains("ZZZZZ"))
        ),
        "{seen:?}"
    );
}

/// F40. A lobby nobody joins ends on the relay after its idle window; the
/// host is told, in words, and its code is no longer offered as if it worked.
#[test]
fn an_idle_lobby_lapse_is_reported_not_hidden() {
    let relay = InProcessRelay::start(
        None,
        Some(RelayLimits {
            idle_lobby: Duration::from_millis(150),
            ..RelayLimits::default()
        }),
    );
    let (mut host, _code) = fontelle_net::host(&relay.relay(), "test").expect("hosts");
    std::thread::sleep(Duration::from_millis(400));
    let ended = poll_until(
        host.as_mut(),
        |i| matches!(i, Incoming::Disconnected(peer, Some(_)) if *peer == SERVER),
    )
    .expect("the host is told the lobby ended");
    let Incoming::Disconnected(_, Some(why)) = ended else {
        unreachable!()
    };
    assert!(why.contains("nobody joined"), "{why}");
    assert_eq!(
        host.lobby_code(),
        None,
        "a lapsed code is not offered as live"
    );
}

/// F41. A code says which relay it is on: the letter in front is a region,
/// looked up in a table compiled in rather than fetched; a self-hosted relay
/// is wherever the setting says, whatever the code.
#[test]
fn a_code_names_its_relay() {
    assert_eq!(
        Relay::Cloud.address_for("UABCDE"),
        Ok("us-east.relay.fopull.com:7788".to_string())
    );
    assert_eq!(
        Relay::Cloud.address_for("uabcde"),
        Ok("us-east.relay.fopull.com:7788".to_string()),
        "a code is not case-sensitive"
    );
    let unknown = Relay::Cloud.address_for("QABCDE").unwrap_err();
    assert!(unknown.contains("update"), "{unknown}");
    assert!(
        Relay::Cloud.address_for("ABC").is_err(),
        "too short to be a code"
    );
    assert_eq!(
        Relay::Open("10.0.0.2:7788".into()).address_for("ABCDE"),
        Ok("10.0.0.2:7788".to_string())
    );
}

/// F34, by hand: hosting on Floptle Cloud with Fontelle's key gets a code.
/// `cargo test -p fontelle-net --test relay -- --ignored`.
#[test]
#[ignore = "talks to relay.fopull.com"]
fn fontelle_hosts_on_floptle_cloud() {
    let (host, code) = fontelle_net::host(&Relay::Cloud, env!("CARGO_PKG_VERSION"))
        .expect("the managed relay admits Fontelle's key");
    assert_eq!(code.len(), 6, "{code}");
    assert!(code.starts_with('U'), "{code}");
    assert_eq!(host.lobby_code().as_deref(), Some(code.as_str()));
    println!("hosted on Floptle Cloud as {code}");

    // And a joiner reaches it by the code alone, bytes both ways.
    let mut host = host;
    let mut joiner = fontelle_net::join(&Relay::Cloud, &code).expect("joins by the code");
    joiner.send(SERVER, Channel::Reliable, b"from the joiner");
    let heard = poll_until(host.as_mut(), |i| message(i).is_some()).expect("the host hears");
    let (peer, bytes) = message(&heard).unwrap();
    assert_eq!(bytes, b"from the joiner");
    host.send(peer, Channel::Reliable, b"from the host");
    let back = poll_until(joiner.as_mut(), |i| message(i).is_some()).expect("the joiner hears");
    assert_eq!(message(&back).unwrap().1, b"from the host");
    println!("joined {code} through Floptle Cloud; bytes crossed both ways");
}
