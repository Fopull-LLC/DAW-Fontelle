//! A relay that never answers.
//!
//! > *"I see your working on a multiplayer function, well it won't let me use
//! > that either."*
//!
//! On a network that drops the relay's UDP port nothing comes back at all,
//! and the handshake times out. That was read as a certificate the build
//! could not verify: for a managed name the client waited out the timeout a
//! second time on the fallback trust, and for any other it reported a
//! certificate fault. A timeout is nobody answering, said once, with no
//! certificate in it — what to tell the person is the caller's, which knows
//! what they were trying to do.

use std::time::{Duration, Instant};

use fontelle_net::{Incoming, QuicClient, Transport};

#[test]
fn a_relay_that_never_answers_is_one_timeout_and_no_certificate_story() {
    // Bound and never read: packets to it vanish, as on a blocked port.
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = silent.local_addr().unwrap().to_string();
    let started = Instant::now();
    let mut client = QuicClient::connect_verified(&addr, "relay.example").unwrap();
    let why = loop {
        let dropped = client
            .poll()
            .into_iter()
            .find_map(|incoming| match incoming {
                Incoming::Disconnected(_, why) => Some(why),
                _ => None,
            });
        if let Some(why) = dropped {
            break why;
        }
        assert!(started.elapsed() < Duration::from_secs(30), "never gave up");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        !why.as_deref().unwrap_or("").contains("certificate"),
        "{why:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(12),
        "waited {:?} — the timeout twice",
        started.elapsed()
    );
}

/// Hosting against a relay that never answers gives up after the relay's own
/// deadline for deciding — not before it, which was a host refused on a
/// cold key lookup that would have been answered — and says what happened
/// in words a person can act on.
#[test]
fn hosting_on_a_relay_that_never_answers_waits_its_deadline_and_says_so() {
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = silent.local_addr().unwrap().to_string();
    let started = Instant::now();
    let refused = fontelle_net::host(&fontelle_net::Relay::Open(addr), "test");
    let why = match refused {
        Ok(_) => panic!("hosted on nothing"),
        Err(why) => why,
    };
    assert!(
        started.elapsed() >= fontelle_net::HOST_DECISION_DEADLINE,
        "gave up after {:?}",
        started.elapsed()
    );
    assert!(why.contains("answer"), "{why}");
}
