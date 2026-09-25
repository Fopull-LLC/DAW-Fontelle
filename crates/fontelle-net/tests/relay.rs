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
        let mut server = RelayServer::bind(0).expect("a relay binds to a free port");
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
    let arrived = poll_until(host.as_mut(), |i| message(i).is_some()).expect("across the relay");
    assert_eq!(message(&arrived).unwrap().1, big.as_slice());
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
