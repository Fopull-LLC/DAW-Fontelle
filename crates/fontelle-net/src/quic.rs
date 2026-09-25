//! The quic transport: the same [`Transport`] seam the sessions speak, over a
//! real network.
//!
//! quinn runs on a small background tokio runtime; the sync game loop talks
//! to it through channels, so the editor and runtime never await anything:
//!
//! - [`Channel::Reliable`] rides one ordered unidirectional quic stream per
//!   direction, length-prefix framed (control: hello, spawn, rpc).
//! - [`Channel::Unreliable`] and [`Channel::UnreliableSequenced`] ride quic
//!   datagrams tagged `[tag u8][seq u64 LE][payload]`; the receiver drops
//!   stale sequenced datagrams. A datagram too large for the path MTU falls
//!   back to the reliable stream, correct but not droppable; interest
//!   management and the byte budget keep snapshots under the MTU.
//!
//! Dev trust: by default the server presents a fresh self-signed certificate
//! and clients accept any certificate, so LAN and self-hosted play is
//! zero-config and exactly as trustworthy as a Minecraft server. The
//! connection is encrypted; the server's identity is not verified.
//!
//! Verified identity, for a relay reached by name: a server can instead be
//! handed a certificate ([`ServerCertificate`], PEM as certbot writes it) and
//! a newer one while it runs ([`QuicServer::set_certificate`]). New
//! handshakes present the new chain and every live connection keeps the one
//! it agreed, so a renewal on the box drops nobody. Whether a client checks
//! the chain is [`ClientTrust`]'s call.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use crate::transport::{Channel, Incoming, LinkStats, PeerId, SERVER, Transport};

/// Datagram tags (first byte on the wire).
const DGRAM_UNRELIABLE: u8 = 1;
const DGRAM_SEQUENCED: u8 = 2;

/// Reliable-stream frame cap — a decoder guard, far above any real message
/// (RPC/values are ≤ 1 KB by the §13.2 guardrails; spawns are small RON docs).
const MAX_FRAME: usize = 1 << 20;

/// Keep-alives + a short idle timeout so a vanished peer is detected in
/// seconds, not minutes.
const KEEP_ALIVE: Duration = Duration::from_millis(500);
const IDLE_TIMEOUT: Duration = Duration::from_secs(8);

fn install_crypto_provider() {
    // Idempotent: fails harmlessly if another component installed one already.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn transport_config() -> quinn::TransportConfig {
    let mut t = quinn::TransportConfig::default();
    t.keep_alive_interval(Some(KEEP_ALIVE));
    t.max_idle_timeout(Some(IDLE_TIMEOUT.try_into().expect("valid idle timeout")));
    t
}

/// One live peer connection, shared with the sync `send` path.
struct PeerHandle {
    conn: quinn::Connection,
    /// Reliable messages queue here; a writer task frames them onto the stream.
    reliable: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    /// Datagram sequence counter (both unreliable tags share it — monotonic is
    /// all the sequenced receiver needs).
    seq: AtomicU64,
}

impl PeerHandle {
    fn new(conn: quinn::Connection) -> (Self, tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                conn,
                reliable: tx,
                seq: AtomicU64::new(0),
            },
            rx,
        )
    }

    /// The sync send path (called from the game loop).
    fn send(&self, channel: Channel, bytes: &[u8]) {
        match channel {
            Channel::Reliable => {
                let _ = self.reliable.send(bytes.to_vec());
            }
            Channel::Unreliable | Channel::UnreliableSequenced => {
                let tag = if channel == Channel::UnreliableSequenced {
                    DGRAM_SEQUENCED
                } else {
                    DGRAM_UNRELIABLE
                };
                let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
                let mut b = Vec::with_capacity(9 + bytes.len());
                b.push(tag);
                b.extend_from_slice(&seq.to_le_bytes());
                b.extend_from_slice(bytes);
                let fits = self
                    .conn
                    .max_datagram_size()
                    .is_some_and(|max| b.len() <= max);
                if !fits || self.conn.send_datagram(b.into()).is_err() {
                    // Too large for the path MTU (or congestion-blocked): the
                    // reliable stream is the honest fallback — delivered, just
                    // not droppable.
                    let _ = self.reliable.send(bytes.to_vec());
                }
            }
        }
    }
}

/// Drain the reliable outbox onto the stream, length-prefix framed.
async fn write_frames(
    mut tx: quinn::SendStream,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(b) = rx.recv().await {
        let len = (b.len() as u32).to_le_bytes();
        if tx.write_all(&len).await.is_err() || tx.write_all(&b).await.is_err() {
            return;
        }
    }
}

/// Where the network's tasks put what arrived: the channel [`Transport::poll`]
/// drains, and — Fontelle's own, not in the engine's copy — a wake called once
/// each thing is in it (F48, [`Transport::set_wake`]).
#[derive(Clone)]
struct Events {
    tx: mpsc::Sender<Incoming>,
    wake: Arc<Mutex<Option<crate::transport::Wake>>>,
}

impl Events {
    fn new(tx: mpsc::Sender<Incoming>) -> Self {
        Self {
            tx,
            wake: Arc::new(Mutex::new(None)),
        }
    }

    fn send(&self, event: Incoming) -> Result<(), mpsc::SendError<Incoming>> {
        self.tx.send(event)?;
        // Cloned out, so a wake that takes a moment never holds the lock
        // against the next message.
        let wake = self.wake.lock().unwrap().clone();
        if let Some(wake) = wake {
            wake();
        }
        Ok(())
    }
}

/// Read length-prefixed frames off the peer's reliable stream.
async fn read_frames(mut rx: quinn::RecvStream, peer: PeerId, events: Events) {
    loop {
        let mut len = [0u8; 4];
        if rx.read_exact(&mut len).await.is_err() {
            return;
        }
        let n = u32::from_le_bytes(len) as usize;
        if n > MAX_FRAME {
            return; // corrupt/hostile framing: drop the stream
        }
        let mut buf = vec![0u8; n];
        if rx.read_exact(&mut buf).await.is_err() {
            return;
        }
        if events
            .send(Incoming::Message(peer, Channel::Reliable, buf))
            .is_err()
        {
            return;
        }
    }
}

/// Receive datagrams; sequenced ones drop when stale.
async fn read_datagrams(conn: quinn::Connection, peer: PeerId, events: Events) {
    let mut last_seq = 0u64;
    loop {
        let Ok(d) = conn.read_datagram().await else {
            return;
        };
        if d.len() < 9 {
            continue;
        }
        let tag = d[0];
        let seq = u64::from_le_bytes(d[1..9].try_into().expect("9-byte header"));
        let channel = match tag {
            DGRAM_SEQUENCED => {
                if seq <= last_seq {
                    continue; // stale — a newer one already delivered
                }
                last_seq = seq;
                Channel::UnreliableSequenced
            }
            _ => Channel::Unreliable,
        };
        if events
            .send(Incoming::Message(peer, channel, d[9..].to_vec()))
            .is_err()
        {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// The authoritative host's endpoint: accepts quic clients, assigns peer ids
/// from 1 up. Create with [`QuicServer::bind`]; drop to shut down.
pub struct QuicServer {
    runtime: Option<tokio::runtime::Runtime>,
    events: mpsc::Receiver<Incoming>,
    peers: Arc<Mutex<HashMap<PeerId, Arc<PeerHandle>>>>,
    port: u16,
    /// A handle on the endpoint, kept for [`Self::set_certificate`].
    endpoint: quinn::Endpoint,
    /// What the kernel gave the socket when a size was asked for; `None` when
    /// [`Self::bind`] left the kernel default alone.
    buffers: Option<SocketBuffers>,
}

/// The UDP socket's kernel buffers, as granted.
///
/// A relay's inbox is its receive buffer. At the kernel default (212,992
/// bytes on a stock Linux box, 100–200 datagrams once the kernel charges each
/// one's `skb` overhead) a relay starts dropping between 56 and 104 CCU while
/// carrying under 1% of its link, and a bigger machine with the same socket
/// drops at the same point. The relay's `rx_drops`, the kernel's
/// `UdpRcvbufErrors` and the socket's own `skmem d…` all count it.
///
/// Asked and granted differ, and only the second is a fact. The kernel
/// silently clamps `SO_RCVBUF` to `net.core.rmem_max` (and `SO_SNDBUF` to
/// `wmem_max`), so a relay that asks for 8 MiB on a stock box gets 425,984
/// bytes. The sysctl is the operator's half; this struct lets the relay, the
/// only thing that knows it was clamped, say so at startup. Read back with
/// `getsockopt`, which is what `ss -m` prints as `rb`/`tb`.
///
/// Linux reports double what it applied, booking the `skb` overhead in the
/// same number, so an unclamped 8 MiB reads as 16,777,216 here and in `ss`,
/// and a clamp to exactly half the ask reads as the ask itself: a box with
/// `rmem_max = 4 MiB` answers an 8 MiB ask with `8388608`. [`Self::usable`]
/// undoes the booking; [`Self::clamped`] compares that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketBuffers {
    /// Bytes asked for, on each of receive and send.
    pub asked: usize,
    /// `SO_RCVBUF` as the kernel reports it after the ask.
    pub recv: usize,
    /// `SO_SNDBUF` as the kernel reports it after the ask.
    pub send: usize,
}

impl SocketBuffers {
    /// What a reported figure means in payload terms: half of it on Linux,
    /// all of it elsewhere.
    pub fn usable(reported: usize) -> usize {
        if cfg!(target_os = "linux") {
            reported / 2
        } else {
            reported
        }
    }

    /// The kernel gave less than was asked on either side — the sysctl is
    /// below the ask.
    pub fn clamped(&self) -> bool {
        Self::usable(self.recv) < self.asked || Self::usable(self.send) < self.asked
    }

    /// One line for a startup log: the ask and both grants, in bytes. The
    /// reported figures are the ones `ss -ulnm` shows as `rb`/`tb`; on
    /// Linux the usable half follows, so the line reads honestly beside the
    /// ask without knowing the kernel's bookkeeping.
    pub fn report(&self) -> String {
        let mut line = format!(
            "socket buffers: asked {} B each; kernel reports rx {} B, tx {} B",
            self.asked, self.recv, self.send
        );
        if cfg!(target_os = "linux") {
            line.push_str(&format!(
                " (ss's rb/tb; Linux books double, so usable rx {} B, tx {} B)",
                Self::usable(self.recv),
                Self::usable(self.send)
            ));
        }
        line
    }

    /// The operator's half, when [`Self::clamped`]: which sysctl to raise,
    /// and that a restart is needed because the buffer is fixed at socket
    /// creation. `None` when nothing was clamped.
    pub fn advice(&self) -> Option<String> {
        if !self.clamped() {
            return None;
        }
        let mut which = Vec::new();
        if Self::usable(self.recv) < self.asked {
            which.push(format!(
                "net.core.rmem_max (receive: {} B usable)",
                Self::usable(self.recv)
            ));
        }
        if Self::usable(self.send) < self.asked {
            which.push(format!(
                "net.core.wmem_max (send: {} B usable)",
                Self::usable(self.send)
            ));
        }
        Some(format!(
            "socket buffer CLAMPED below the {} B asked — raise {} to at least {} with sysctl, \
             then restart: the buffer is fixed when the socket is created. Until then the inbox \
             overflows long before the link does (rx_drops)",
            self.asked,
            which.join(" and "),
            self.asked
        ))
    }
}

/// **A certificate and its key, for a server that is reached by name.**
///
/// The dev self-signed certificate is minted at startup and nobody checks it.
/// A managed relay at `us-east.relay.fopull.com` is different: clients verify
/// that name against the public roots ([`ClientTrust::Verify`]), so the relay
/// has to present a chain a CA issued for it — and present the renewed one
/// sixty days later without a restart, because a restart ends every lobby on
/// the box. This is that chain, loaded from disk; [`QuicServer::set_certificate`]
/// is the swap.
pub struct ServerCertificate {
    /// Leaf first, then intermediates — the order `fullchain.pem` has.
    chain: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
}

impl Clone for ServerCertificate {
    fn clone(&self) -> Self {
        Self {
            chain: self.chain.clone(),
            key: self.key.clone_key(),
        }
    }
}

impl std::fmt::Debug for ServerCertificate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key. The fingerprint is what an operator compares.
        f.debug_struct("ServerCertificate")
            .field("chain", &self.chain.len())
            .field("fingerprint", &self.fingerprint())
            .finish()
    }
}

impl ServerCertificate {
    /// Load a PEM certificate chain and a PEM private key — certbot's
    /// `fullchain.pem` and `privkey.pem`, or anything shaped like them. The
    /// key has to belong to the leaf, and the chain has to hold at least one
    /// certificate; either failing is an error here rather than a server that
    /// came up presenting nothing.
    pub fn load_pem(
        cert_path: &std::path::Path,
        key_path: &std::path::Path,
    ) -> Result<Self, String> {
        let cert_pem = std::fs::read(cert_path)
            .map_err(|e| format!("certificate {}: {e}", cert_path.display()))?;
        let key_pem =
            std::fs::read(key_path).map_err(|e| format!("key {}: {e}", key_path.display()))?;
        Self::from_pem(&cert_pem, &key_pem)
            .map_err(|e| format!("{} + {}: {e}", cert_path.display(), key_path.display()))
    }

    /// [`Self::load_pem`] from bytes already read.
    pub fn from_pem(cert_pem: &[u8], key_pem: &[u8]) -> Result<Self, String> {
        use rustls::pki_types::pem::PemObject;
        install_crypto_provider();
        let chain: Vec<rustls::pki_types::CertificateDer<'static>> =
            rustls::pki_types::CertificateDer::pem_slice_iter(cert_pem)
                .collect::<Result<_, _>>()
                .map_err(|e| format!("certificate PEM: {e}"))?;
        if chain.is_empty() {
            return Err("certificate PEM holds no certificate".into());
        }
        let key = rustls::pki_types::PrivateKeyDer::from_pem_slice(key_pem)
            .map_err(|e| format!("key PEM: {e}"))?;
        let out = Self { chain, key };
        // A key that does not match its certificate is refused now, by the
        // same check `with_single_cert` runs, so the mismatch is a load error
        // and never a swap that left the endpoint presenting nothing.
        out.server_config()?;
        Ok(out)
    }

    /// The leaf's sha-256 fingerprint, `AB:CD:…` — what
    /// `openssl x509 -noout -fingerprint -sha256` prints for the same file, so
    /// an operator can tell which certificate a running relay is presenting.
    pub fn fingerprint(&self) -> String {
        fingerprint_of(&self.chain[0])
    }

    fn server_config(&self) -> Result<quinn::ServerConfig, String> {
        let mut server_config =
            quinn::ServerConfig::with_single_cert(self.chain.clone(), self.key.clone_key())
                .map_err(|e| format!("server tls: {e}"))?;
        server_config.transport_config(Arc::new(transport_config()));
        Ok(server_config)
    }

    /// The dev-trust certificate: fresh, self-signed, for the name
    /// `floptle-dev` that no client verifies.
    pub(crate) fn self_signed() -> Result<Self, String> {
        install_crypto_provider();
        let cert = rcgen::generate_simple_self_signed(vec!["floptle-dev".into()])
            .map_err(|e| format!("self-signed cert: {e}"))?;
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()),
        );
        Ok(Self {
            chain: vec![cert.cert.der().clone()],
            key,
        })
    }
}

/// SHA-256 of a DER certificate as `AB:CD:…` (openssl's spelling).
pub fn fingerprint_of(der: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, der);
    digest
        .as_ref()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Bind a UDP socket by hand, sizing its kernel buffers first when asked.
///
/// std's `UdpSocket::bind` cannot set `SO_RCVBUF`, and quinn's own
/// `Endpoint::server` uses it — so the socket is built through `socket2`,
/// sized, read back, and only then handed over. The read-back is the point:
/// the kernel clamps silently, and the number it reports is the only one
/// that is true (see [`SocketBuffers`]).
fn bind_udp(
    addr: SocketAddr,
    buffer: Option<usize>,
) -> Result<(std::net::UdpSocket, Option<SocketBuffers>), String> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock = Socket::new(Domain::for_address(addr), Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| format!("udp socket: {e}"))?;
    let buffers = match buffer {
        None => None,
        Some(asked) => {
            // A refused setsockopt is worth a word but not a dead relay: the
            // socket still works at the kernel default, and `clamped()` will
            // say so through the read-back.
            let _ = sock.set_recv_buffer_size(asked);
            let _ = sock.set_send_buffer_size(asked);
            Some(SocketBuffers {
                asked,
                recv: sock
                    .recv_buffer_size()
                    .map_err(|e| format!("SO_RCVBUF: {e}"))?,
                send: sock
                    .send_buffer_size()
                    .map_err(|e| format!("SO_SNDBUF: {e}"))?,
            })
        }
    };
    sock.bind(&addr.into())
        .map_err(|e| format!("bind {addr}: {e}"))?;
    Ok((sock.into(), buffers))
}

impl QuicServer {
    /// Bind on `0.0.0.0:port` (0 = ephemeral, see [`Self::local_port`]) with a
    /// fresh self-signed certificate (see the module docs' security model).
    pub fn bind(port: u16) -> Result<Self, String> {
        Self::bind_with_certificate(port, &ServerCertificate::self_signed()?)
    }

    /// Present a newer certificate to every handshake from now on. Connections
    /// already up keep the one they agreed — quinn swaps the server config
    /// for incoming handshakes only — so this is how a renewed certificate
    /// reaches a relay without ending a single lobby.
    pub fn set_certificate(&self, cert: &ServerCertificate) -> Result<(), String> {
        self.endpoint.set_server_config(Some(cert.server_config()?));
        Ok(())
    }

    /// [`Self::bind`] presenting `cert` instead of a self-signed one.
    pub fn bind_with_certificate(port: u16, cert: &ServerCertificate) -> Result<Self, String> {
        Self::bind_sized(port, cert, None)
    }

    /// [`Self::bind_with_certificate`] asking the kernel for `buffer` bytes of
    /// receive and send buffer on the socket before quinn takes it. What was
    /// granted is on [`Self::socket_buffers`]; a relay is expected to print
    /// it. `None` keeps the kernel default, which is right for a player's own
    /// listen socket and wrong for a relay (see [`SocketBuffers`]).
    pub fn bind_sized(
        port: u16,
        cert: &ServerCertificate,
        buffer: Option<usize>,
    ) -> Result<Self, String> {
        install_crypto_provider();
        let server_config = cert.server_config()?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| format!("net runtime: {e}"))?;
        let addr: SocketAddr = ([0, 0, 0, 0], port).into();
        let (socket, buffers) = bind_udp(addr, buffer)?;
        let endpoint = runtime
            .block_on(async {
                quinn::Endpoint::new(
                    quinn::EndpointConfig::default(),
                    Some(server_config),
                    socket,
                    Arc::new(quinn::TokioRuntime),
                )
            })
            .map_err(|e| format!("bind {addr}: {e}"))?;
        let local_port = endpoint.local_addr().map_err(|e| e.to_string())?.port();

        let (events_tx, events_rx) = mpsc::channel();
        let events_tx = Events::new(events_tx);
        let peers: Arc<Mutex<HashMap<PeerId, Arc<PeerHandle>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        {
            let peers = peers.clone();
            let endpoint = endpoint.clone();
            runtime.spawn(async move {
                let mut next_peer: PeerId = 1;
                while let Some(incoming) = endpoint.accept().await {
                    let Ok(conn) = incoming.await else { continue };
                    let peer = next_peer;
                    next_peer += 1;
                    let (handle, reliable_rx) = PeerHandle::new(conn.clone());
                    peers.lock().unwrap().insert(peer, Arc::new(handle));
                    if events_tx.send(Incoming::Connected(peer)).is_err() {
                        return; // transport dropped
                    }
                    // Writer: our reliable stream toward this peer.
                    let c = conn.clone();
                    let rrx = reliable_rx;
                    tokio::spawn(async move {
                        let Ok(tx) = c.open_uni().await else { return };
                        write_frames(tx, rrx).await;
                    });
                    // Reader: the peer's reliable stream toward us.
                    let c = conn.clone();
                    let ev = events_tx.clone();
                    tokio::spawn(async move {
                        let Ok(rx) = c.accept_uni().await else { return };
                        read_frames(rx, peer, ev).await;
                    });
                    // Datagrams.
                    tokio::spawn(read_datagrams(conn.clone(), peer, events_tx.clone()));
                    // Death watch.
                    let ev = events_tx.clone();
                    let peers = peers.clone();
                    tokio::spawn(async move {
                        conn.closed().await;
                        peers.lock().unwrap().remove(&peer);
                        let _ = ev.send(Incoming::dropped(peer));
                    });
                }
            });
        }
        Ok(Self {
            runtime: Some(runtime),
            events: events_rx,
            peers,
            port: local_port,
            endpoint,
            buffers,
        })
    }

    /// The actually-bound UDP port (useful with `bind(0)`).
    pub fn local_port(&self) -> u16 {
        self.port
    }

    /// What the kernel granted when [`Self::bind_sized`] asked for a buffer;
    /// `None` when the kernel default was kept.
    pub fn socket_buffers(&self) -> Option<SocketBuffers> {
        self.buffers
    }

    /// Where this peer's connection comes from — the address a relay rates
    /// lobby opens and joins by. `None` once the peer is gone.
    pub fn remote_addr(&self, peer: PeerId) -> Option<SocketAddr> {
        self.peers
            .lock()
            .unwrap()
            .get(&peer)
            .map(|h| h.conn.remote_address())
    }
}

impl Transport for QuicServer {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        let handle = self.peers.lock().unwrap().get(&peer).cloned();
        if let Some(h) = handle {
            h.send(channel, bytes);
        }
    }

    fn poll(&mut self) -> Vec<Incoming> {
        self.events.try_iter().collect()
    }

    fn stats(&self, peer: PeerId) -> LinkStats {
        let rtt = self
            .peers
            .lock()
            .unwrap()
            .get(&peer)
            .map(|h| h.conn.rtt().as_secs_f32() * 1000.0)
            .unwrap_or(0.0);
        LinkStats {
            rtt_ms: rtt,
            loss: 0.0,
        }
    }

    /// Close this peer's connection (a kick). The reason has already gone out
    /// as a reliable `Kicked`, so the CONNECTION_CLOSE frame only has to carry
    /// enough for a packet capture to make sense.
    fn disconnect(&mut self, peer: PeerId) {
        if let Some(h) = self.peers.lock().unwrap().remove(&peer) {
            h.conn.close(0u32.into(), b"kicked");
        }
    }
}

impl Drop for QuicServer {
    fn drop(&mut self) {
        // Say goodbye properly: close every connection (CONNECTION_CLOSE goes
        // out), give the driver a beat to transmit, then drop the runtime.
        for h in self.peers.lock().unwrap().values() {
            h.conn.close(0u32.into(), b"server closed");
        }
        std::thread::sleep(Duration::from_millis(20));
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Accept any server certificate (the dev-trust model — see the module docs).
#[derive(Debug)]
struct AcceptAnyCert(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// **Whose certificate a client checks.**
///
/// The open relay and a direct host present a self-signed certificate minted
/// at startup — the dev-trust model, where the lobby code
/// is the secret and the transport is not. A managed relay is reached by a
/// name under `fopull.com`, and a name is something a certificate can be
/// issued for: that connection is verified against the public roots, with
/// the name as SNI, so a game key and every packet of a managed session go
/// to the relay and not to whatever answers at that address on this network.
///
/// **Behind a fallback, for now.** Until the managed relay presents a chain
/// that verifies, a failed verification falls back to the dev-trust model
/// with a warning the host and the joiner both surface — so a managed session
/// keeps working the day this ships, and refusing is one line to flip once
/// the certificate is live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientTrust {
    /// Verify against the public roots, presenting `server_name`.
    Verify { server_name: String },
    /// Accept whatever answers: the open relay, a direct host, an address.
    AcceptAny,
}

/// The trust an address gets: a DNS name under `fopull.com` is verified,
/// everything else — an IP, a self-hosted relay's name — is the dev-trust
/// model. Decided from the string the developer or the region list wrote,
/// before anything is resolved.
pub fn client_trust_for(addr: &str) -> ClientTrust {
    let host = addr.rsplit_once(':').map_or(addr, |(h, _)| h);
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.');
    if host.parse::<std::net::IpAddr>().is_ok() {
        return ClientTrust::AcceptAny;
    }
    let lower = host.to_ascii_lowercase();
    if lower == "fopull.com" || lower.ends_with(".fopull.com") {
        ClientTrust::Verify {
            server_name: host.to_string(),
        }
    } else {
        ClientTrust::AcceptAny
    }
}

/// A client endpoint connecting to a [`QuicServer`]. [`QuicClient::connect`]
/// returns immediately; the handshake completes in the background (reliable
/// sends queue meanwhile — the session's `Hello` is the first thing through).
pub struct QuicClient {
    runtime: Option<tokio::runtime::Runtime>,
    events: mpsc::Receiver<Incoming>,
    conn: Arc<Mutex<Option<quinn::Connection>>>,
    reliable: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    seq: AtomicU64,
    /// What the handshake had to say — today, that a managed relay's
    /// certificate did not verify and the dev-trust fallback was taken.
    warnings: Arc<Mutex<Vec<String>>>,
    /// See [`Events`]: Fontelle's own.
    wake: Arc<Mutex<Option<crate::transport::Wake>>>,
}

/// A TLS client config under one trust model.
fn tls_config(trust: &ClientTrust) -> Result<quinn::ClientConfig, String> {
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .ok_or("no crypto provider")?;
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("tls: {e}"))?;
    let tls = match trust {
        ClientTrust::Verify { .. } => {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            builder.with_root_certificates(roots).with_no_client_auth()
        }
        ClientTrust::AcceptAny => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
            .with_no_client_auth(),
    };
    let mut client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls)
            .map_err(|e| format!("quic tls: {e}"))?,
    ));
    client_config.transport_config(Arc::new(transport_config()));
    Ok(client_config)
}

impl QuicClient {
    /// Connect to `host:port` (an IP or a resolvable name), under the trust
    /// [`client_trust_for`] assigns the address.
    pub fn connect(addr: &str) -> Result<Self, String> {
        Self::connect_with_trust(addr, client_trust_for(addr))
    }

    /// The warnings the handshake raised, once each.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut *self.warnings.lock().unwrap())
    }

    /// [`Self::connect`] under an explicit trust model.
    pub fn connect_with_trust(addr: &str, trust: ClientTrust) -> Result<Self, String> {
        Self::connect_inner(addr, trust, true)
    }

    /// Verify the server's chain for `server_name` against the public roots
    /// and take no fallback: a chain that does not verify is a
    /// [`Incoming::Disconnected`] carrying the reason, never a connection.
    /// What `floptle-relay-bench --verify` runs, and what every managed
    /// connection becomes once the fallback in [`Self::connect_with_trust`]
    /// is removed.
    pub fn connect_verified(addr: &str, server_name: &str) -> Result<Self, String> {
        Self::connect_inner(
            addr,
            ClientTrust::Verify {
                server_name: server_name.into(),
            },
            false,
        )
    }

    /// The leaf certificate the server presented, DER — `None` until the
    /// handshake completes. For telling which certificate answered: the
    /// self-signed dev one, the one on disk, or the one before a renewal.
    pub fn peer_certificate(&self) -> Option<Vec<u8>> {
        let conn = self.conn.lock().unwrap().clone()?;
        let identity = conn.peer_identity()?;
        let chain = identity
            .downcast::<Vec<rustls::pki_types::CertificateDer<'static>>>()
            .ok()?;
        chain.first().map(|c| c.as_ref().to_vec())
    }

    fn connect_inner(addr: &str, trust: ClientTrust, allow_fallback: bool) -> Result<Self, String> {
        install_crypto_provider();
        let remote: SocketAddr = addr
            .to_socket_addrs()
            .map_err(|e| format!("resolve {addr}: {e}"))?
            .next()
            .ok_or_else(|| format!("resolve {addr}: no address"))?;

        let client_config = tls_config(&trust)?;
        let fallback = match &trust {
            ClientTrust::Verify { .. } if allow_fallback => {
                Some(tls_config(&ClientTrust::AcceptAny)?)
            }
            ClientTrust::Verify { .. } | ClientTrust::AcceptAny => None,
        };
        let server_name = match &trust {
            ClientTrust::Verify { server_name } => server_name.clone(),
            ClientTrust::AcceptAny => "floptle-dev".to_string(),
        };
        let addr_shown = addr.to_string();
        let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| format!("net runtime: {e}"))?;
        let bind: SocketAddr = if remote.is_ipv6() {
            "[::]:0".parse().expect("valid")
        } else {
            "0.0.0.0:0".parse().expect("valid")
        };
        let mut endpoint = runtime
            .block_on(async { quinn::Endpoint::client(bind) })
            .map_err(|e| format!("bind: {e}"))?;
        endpoint.set_default_client_config(client_config);

        let (events_tx, events_rx) = mpsc::channel();
        let events_tx = Events::new(events_tx);
        let (reliable_tx, reliable_rx) = tokio::sync::mpsc::unbounded_channel();
        let wake = events_tx.wake.clone();
        let conn_slot: Arc<Mutex<Option<quinn::Connection>>> = Arc::new(Mutex::new(None));
        {
            let conn_slot = conn_slot.clone();
            let warnings = warnings.clone();
            runtime.spawn(async move {
                let connecting = match endpoint.connect(remote, &server_name) {
                    Ok(c) => c,
                    Err(_) => {
                        let _ = events_tx.send(Incoming::dropped(SERVER));
                        return;
                    }
                };
                let conn = match connecting.await {
                    Ok(c) => c,
                    // **The verified handshake failed; fall back, and say so.**
                    // The relay at a managed name did not present a chain this
                    // build can verify. Until it does, the session goes ahead
                    // on the dev-trust model — with a warning that reaches the
                    // Console — rather than every managed game failing today.
                    Err(e) => {
                        let Some(any) = fallback else {
                            // No fallback to take: the reason travels with the
                            // disconnect, the way a relay's refusal does, so
                            // whoever asked for a verified connection is told
                            // why there is not one.
                            let ev = match &trust {
                                ClientTrust::Verify { server_name } => Incoming::refused(
                                    SERVER,
                                    format!(
                                        "the relay at {addr_shown} did not present a certificate \
                                         for {server_name} that this build can verify ({e})"
                                    ),
                                ),
                                ClientTrust::AcceptAny => Incoming::dropped(SERVER),
                            };
                            let _ = events_tx.send(ev);
                            return;
                        };
                        warnings.lock().unwrap().push(format!(
                            "the relay at {addr_shown} did not present a certificate this build \
                             can verify ({e}); connecting anyway on the older trust model. A \
                             future release will refuse this."
                        ));
                        let connecting = match endpoint.connect_with(any, remote, &server_name) {
                            Ok(c) => c,
                            Err(_) => {
                                let _ = events_tx.send(Incoming::dropped(SERVER));
                                return;
                            }
                        };
                        match connecting.await {
                            Ok(c) => c,
                            Err(_) => {
                                let _ = events_tx.send(Incoming::dropped(SERVER));
                                return;
                            }
                        }
                    }
                };
                *conn_slot.lock().unwrap() = Some(conn.clone());
                let _ = events_tx.send(Incoming::Connected(SERVER));
                let c = conn.clone();
                tokio::spawn(async move {
                    let Ok(tx) = c.open_uni().await else { return };
                    write_frames(tx, reliable_rx).await;
                });
                let c = conn.clone();
                let ev = events_tx.clone();
                tokio::spawn(async move {
                    let Ok(rx) = c.accept_uni().await else { return };
                    read_frames(rx, SERVER, ev).await;
                });
                tokio::spawn(read_datagrams(conn.clone(), SERVER, events_tx.clone()));
                conn.closed().await;
                *conn_slot.lock().unwrap() = None;
                let _ = events_tx.send(Incoming::dropped(SERVER));
            });
        }
        Ok(Self {
            runtime: Some(runtime),
            events: events_rx,
            conn: conn_slot,
            reliable: reliable_tx,
            seq: AtomicU64::new(0),
            warnings,
            wake,
        })
    }
}

impl Transport for QuicClient {
    fn take_notices(&mut self) -> Vec<String> {
        self.take_warnings()
    }

    fn set_wake(&mut self, wake: crate::transport::Wake) {
        *self.wake.lock().unwrap() = Some(wake);
    }

    fn send(&mut self, _peer: PeerId, channel: Channel, bytes: &[u8]) {
        match channel {
            Channel::Reliable => {
                // Queues even before the handshake completes — the writer task
                // drains once the stream opens (Hello arrives first, always).
                let _ = self.reliable.send(bytes.to_vec());
            }
            Channel::Unreliable | Channel::UnreliableSequenced => {
                let conn = self.conn.lock().unwrap().clone();
                let Some(conn) = conn else { return }; // not connected: droppable by contract
                let tag = if channel == Channel::UnreliableSequenced {
                    DGRAM_SEQUENCED
                } else {
                    DGRAM_UNRELIABLE
                };
                let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
                let mut b = Vec::with_capacity(9 + bytes.len());
                b.push(tag);
                b.extend_from_slice(&seq.to_le_bytes());
                b.extend_from_slice(bytes);
                let fits = conn.max_datagram_size().is_some_and(|max| b.len() <= max);
                if !fits || conn.send_datagram(b.into()).is_err() {
                    let _ = self.reliable.send(bytes.to_vec());
                }
            }
        }
    }

    fn poll(&mut self) -> Vec<Incoming> {
        self.events.try_iter().collect()
    }

    fn stats(&self, _peer: PeerId) -> LinkStats {
        let rtt = self
            .conn
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| c.rtt().as_secs_f32() * 1000.0)
            .unwrap_or(0.0);
        LinkStats {
            rtt_ms: rtt,
            loss: 0.0,
        }
    }
}

impl Drop for QuicClient {
    fn drop(&mut self) {
        // Graceful goodbye so the server learns now, not at the idle timeout.
        if let Some(c) = self.conn.lock().unwrap().take() {
            c.close(0u32.into(), b"left");
        }
        std::thread::sleep(Duration::from_millis(20));
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠ **The buffer a relay gets is the one the kernel reports, not the one
    /// it asked for**.
    ///
    /// Two asks against the real kernel: a small one every box grants, and
    /// one no box grants (a gibibyte, above any `rmem_max`). The small one
    /// must read back as applied — on Linux exactly double the ask, which is
    /// how the kernel books it — and the two must differ, or the setsockopt
    /// never happened and the read-back is the default wearing a new name.
    /// The huge one must say `clamped` and name the sysctl, because the
    /// silent clamp is the whole finding: a relay that asked for 8 MiB on a
    /// stock box ran at 425,984 B and nothing said so.
    #[test]
    fn a_sized_bind_reads_back_what_the_kernel_granted_and_says_when_it_was_clamped() {
        let cert = ServerCertificate::self_signed().unwrap();
        assert_eq!(
            QuicServer::bind(0).unwrap().socket_buffers(),
            None,
            "nothing asked"
        );

        let small_ask = 64 << 10;
        let small = QuicServer::bind_sized(0, &cert, Some(small_ask))
            .unwrap()
            .socket_buffers()
            .expect("asked, so answered");
        assert_eq!(small.asked, small_ask);
        assert!(
            !small.clamped(),
            "{small:?} — 64 KiB is under every default rmem_max"
        );
        assert_eq!(small.advice(), None);
        if cfg!(target_os = "linux") {
            assert_eq!(
                small.recv,
                2 * small_ask,
                "Linux reports double what it applied"
            );
            assert_eq!(small.send, 2 * small_ask);
        } else {
            assert!(
                small.recv >= small_ask && small.send >= small_ask,
                "{small:?}"
            );
        }

        let huge = QuicServer::bind_sized(0, &cert, Some(1 << 30))
            .unwrap()
            .socket_buffers()
            .unwrap();
        assert_ne!(
            huge.recv, small.recv,
            "two asks, one answer: the ask is not applied"
        );
        if cfg!(target_os = "linux") {
            assert!(huge.clamped(), "{huge:?} — no box grants a gibibyte");
            let advice = huge
                .advice()
                .expect("a clamp comes with the sysctl to raise");
            assert!(advice.contains("net.core.rmem_max"), "{advice}");
            assert!(advice.contains("net.core.wmem_max"), "{advice}");
            assert!(advice.contains("restart"), "{advice}");
        }
        // The report carries all three numbers, so a journal line is enough
        // to compare against `ss -ulnm`.
        let r = huge.report();
        assert!(
            r.contains(&format!("asked {} B", 1 << 30)) && r.contains("reports rx"),
            "{r}"
        );
    }

    /// ⚠ **A clamp to exactly half the ask read as "granted".** The first cut
    /// compared the reported figure against the ask; on a box with
    /// `rmem_max = 4 MiB` an 8 MiB ask reports `8388608` — Linux's doubled
    /// booking of the 4 MiB it applied — and the comparison passed. That is
    /// the relay running at half its inbox and saying nothing, which is the
    /// exact silence the card was opened for. Pure data, so it is asserted
    /// directly rather than hoping a box with that sysctl runs the suite.
    #[test]
    fn a_grant_of_exactly_half_the_ask_is_a_clamp_on_linux() {
        let asked = 8 << 20;
        let half = SocketBuffers {
            asked,
            recv: asked,
            send: asked,
        };
        let full = SocketBuffers {
            asked,
            recv: 2 * asked,
            send: 2 * asked,
        };
        if cfg!(target_os = "linux") {
            assert!(
                half.clamped(),
                "{half:?} is 4 MiB usable against an 8 MiB ask"
            );
            let advice = half.advice().unwrap();
            assert!(advice.contains("4194304 B usable"), "{advice}");
            assert!(
                half.report().contains("usable rx 4194304 B"),
                "{}",
                half.report()
            );
            assert!(!full.clamped());
            assert_eq!(full.advice(), None);
        } else {
            assert!(!half.clamped());
        }
        // One side short is still a clamp, and the advice names only that side.
        let rx_only = SocketBuffers {
            asked,
            recv: asked,
            send: 2 * asked,
        };
        if cfg!(target_os = "linux") {
            let advice = rx_only.advice().unwrap();
            assert!(
                advice.contains("rmem_max") && !advice.contains("wmem_max"),
                "{advice}"
            );
        }
    }

    /// Accumulates every polled event so a wait for one kind never discards
    /// another that arrived in the same batch.
    struct Polled<'a> {
        t: &'a mut dyn Transport,
        seen: Vec<Incoming>,
    }

    impl<'a> Polled<'a> {
        fn new(t: &'a mut dyn Transport) -> Self {
            Self {
                t,
                seen: Vec::new(),
            }
        }

        /// Wait (~2 s) until `want` accumulated events match `pred`.
        fn wait_for(
            &mut self,
            mut pred: impl FnMut(&Incoming) -> bool,
            want: usize,
        ) -> Vec<Incoming> {
            for _ in 0..400 {
                self.seen.extend(self.t.poll());
                let got: Vec<Incoming> = self.seen.iter().filter(|i| pred(i)).cloned().collect();
                if got.len() >= want {
                    return got;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            self.seen.iter().filter(|i| pred(i)).cloned().collect()
        }
    }

    #[test]
    fn quic_end_to_end_all_channels() {
        let mut server = QuicServer::bind(0).expect("bind");
        let port = server.local_port();
        let mut client = QuicClient::connect(&format!("127.0.0.1:{port}")).expect("connect");

        // Reliable queued before the handshake completes must still arrive first.
        client.send(SERVER, Channel::Reliable, b"hello");

        let mut on_server = Polled::new(&mut server);
        assert_eq!(
            on_server
                .wait_for(|i| matches!(i, Incoming::Connected(1)), 1)
                .len(),
            1,
            "server must see the client connect"
        );
        assert_eq!(
            on_server
                .wait_for(
                    |i| matches!(i, Incoming::Message(1, Channel::Reliable, b) if b == b"hello"),
                    1,
                )
                .len(),
            1,
            "pre-handshake reliable send must arrive"
        );
        drop(on_server);

        // Client sees Connected once the handshake lands.
        let mut on_client = Polled::new(&mut client);
        assert_eq!(
            on_client
                .wait_for(|i| matches!(i, Incoming::Connected(SERVER)), 1)
                .len(),
            1
        );
        drop(on_client);

        // A big reliable frame survives framing (larger than any datagram).
        let big = vec![0xAB; 64 * 1024];
        server.send(1, Channel::Reliable, &big);
        let mut on_client = Polled::new(&mut client);
        assert_eq!(
            on_client
                .wait_for(
                    |i| matches!(i, Incoming::Message(SERVER, Channel::Reliable, b) if b.len() == big.len()),
                    1,
                )
                .len(),
            1,
            "64 KiB reliable frame must arrive intact"
        );
        drop(on_client);

        // Datagrams both ways (localhost: no loss in practice).
        client.send(SERVER, Channel::Unreliable, b"dgram");
        let mut on_server = Polled::new(&mut server);
        assert_eq!(
            on_server
                .wait_for(
                    |i| matches!(i, Incoming::Message(1, Channel::Unreliable, b) if b == b"dgram"),
                    1,
                )
                .len(),
            1,
            "unreliable datagram must arrive on localhost"
        );
        drop(on_server);

        server.send(1, Channel::UnreliableSequenced, b"snap1");
        server.send(1, Channel::UnreliableSequenced, b"snap2");
        let mut on_client = Polled::new(&mut client);
        let got = on_client.wait_for(
            |i| {
                matches!(
                    i,
                    Incoming::Message(SERVER, Channel::UnreliableSequenced, _)
                )
            },
            2,
        );
        assert!(!got.is_empty(), "sequenced datagrams must arrive");
        // In-order arrivals all deliver; the newest must be among them.
        assert!(
            got.iter()
                .any(|i| matches!(i, Incoming::Message(_, _, b) if b == b"snap2"))
        );
        drop(on_client);

        // Dropping the client surfaces as a disconnect on the server.
        drop(client);
        let mut on_server = Polled::new(&mut server);
        assert_eq!(
            on_server
                .wait_for(|i| matches!(i, Incoming::Disconnected(1, _)), 1)
                .len(),
            1,
            "server must notice the client vanish"
        );
    }

    // `a_full_session_replicates_over_quic` stays in the engine: it drives the engine's own game session
    // (`NetSession`, `floptle_core::World`), which does not come over.
    // Fontelle's end-to-end test of this path is `tests/relay.rs`.

    /// **A managed relay's name is verified; an address or a self-hosted
    /// relay's name is not.** Decided from the string, before resolving, so a
    /// name that merely resolves to the managed relay's address gets no
    /// special trust and a name under `fopull.com` gets no less.
    #[test]
    fn a_fopull_name_is_verified_and_everything_else_is_the_dev_trust_model() {
        assert_eq!(
            client_trust_for("us-east.relay.fopull.com:7788"),
            ClientTrust::Verify {
                server_name: "us-east.relay.fopull.com".into()
            }
        );
        assert_eq!(
            client_trust_for("FOPULL.COM:7788"),
            ClientTrust::Verify {
                server_name: "FOPULL.COM".into()
            }
        );
        for any in [
            "192.168.1.5:7788",
            "[::1]:7788",
            "127.0.0.1:7788",
            "relay.example.org:7788",
            "fopull.com.evil.example:7788",
            "notfopull.com:7788",
        ] {
            assert_eq!(client_trust_for(any), ClientTrust::AcceptAny, "{any}");
        }
    }

    /// **The fallback works and says so.** A client told to verify a server
    /// that presents the dev self-signed certificate cannot verify it — and
    /// connects anyway, once, with a warning the transport hands up. Until
    /// the managed relay's certificate is live this is what every managed
    /// session does; when it is, the fallback is the line to remove.
    #[test]
    fn a_certificate_that_does_not_verify_falls_back_with_a_warning() {
        let server = QuicServer::bind(0).unwrap();
        let addr = format!("127.0.0.1:{}", server.local_port());
        let mut client = QuicClient::connect_with_trust(
            &addr,
            ClientTrust::Verify {
                server_name: "us-east.relay.fopull.com".into(),
            },
        )
        .unwrap();
        let mut connected = false;
        for _ in 0..200 {
            if client
                .poll()
                .iter()
                .any(|i| matches!(i, Incoming::Connected(SERVER)))
            {
                connected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(connected, "the fallback never connected");
        let warnings = client.take_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("did not present a certificate this build can verify"),
            "{}",
            warnings[0]
        );
        assert!(warnings[0].contains(&addr), "{}", warnings[0]);
        assert!(
            client.take_warnings().is_empty(),
            "the warning was not once"
        );
        // The ordinary trust model raises nothing.
        let mut plain = QuicClient::connect(&addr).unwrap();
        for _ in 0..200 {
            if plain
                .poll()
                .iter()
                .any(|i| matches!(i, Incoming::Connected(SERVER)))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(plain.take_warnings().is_empty());
    }

    /// A certificate for `name`, PEM, as certbot would leave it on disk.
    fn pem_for(name: &str) -> (Vec<u8>, Vec<u8>) {
        let cert = rcgen::generate_simple_self_signed(vec![name.into()]).unwrap();
        (
            cert.cert.pem().into_bytes(),
            cert.key_pair.serialize_pem().into_bytes(),
        )
    }

    /// Connect under the dev-trust model and return the leaf the server
    /// presented, once the handshake is up.
    fn presented_by(addr: &str) -> (QuicClient, Vec<u8>) {
        let mut client = QuicClient::connect(addr).unwrap();
        for _ in 0..200 {
            if client
                .poll()
                .iter()
                .any(|i| matches!(i, Incoming::Connected(SERVER)))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let leaf = client.peer_certificate().expect("the handshake completed");
        (client, leaf)
    }

    /// **A server presents the certificate it was given, not one it minted**.
    /// The managed relay has to answer with the chain a CA
    /// issued for its region name; this is the seam that lets it, checked by
    /// reading the leaf back off a live connection.
    #[test]
    fn a_server_presents_the_certificate_it_was_handed() {
        let (cert_pem, key_pem) = pem_for("us-east.relay.fopull.com");
        let cert = ServerCertificate::from_pem(&cert_pem, &key_pem).unwrap();
        let server = QuicServer::bind_with_certificate(0, &cert).unwrap();
        let addr = format!("127.0.0.1:{}", server.local_port());
        let (_client, leaf) = presented_by(&addr);
        assert_eq!(
            fingerprint_of(&leaf),
            cert.fingerprint(),
            "a different certificate answered"
        );
        // …and the fingerprint is openssl's spelling, so an operator can
        // compare it against the file on the box.
        assert_eq!(
            cert.fingerprint().len(),
            32 * 3 - 1,
            "{}",
            cert.fingerprint()
        );
        assert!(
            cert.fingerprint()
                .chars()
                .all(|c| c == ':' || c.is_ascii_hexdigit())
        );
    }

    /// **A renewal reaches new handshakes and ends no live connection.**
    /// certbot renews on its own schedule; a relay that could only read its
    /// certificate at startup would turn every renewal into every lobby in
    /// the region ending at once. So: swap, then check that a client from
    /// before the swap is still up and one from after sees the new leaf.
    #[test]
    fn a_renewed_certificate_is_presented_to_new_connections_and_old_ones_stay_up() {
        let (c1, k1) = pem_for("us-east.relay.fopull.com");
        let (c2, k2) = pem_for("us-east.relay.fopull.com");
        let before = ServerCertificate::from_pem(&c1, &k1).unwrap();
        let after = ServerCertificate::from_pem(&c2, &k2).unwrap();
        assert_ne!(
            before.fingerprint(),
            after.fingerprint(),
            "two mints, two certificates"
        );

        let mut server = QuicServer::bind_with_certificate(0, &before).unwrap();
        let addr = format!("127.0.0.1:{}", server.local_port());
        let (mut old, leaf_old) = presented_by(&addr);
        assert_eq!(fingerprint_of(&leaf_old), before.fingerprint());
        // The server has seen the old client arrive.
        let mut seen = Polled::new(&mut server);
        assert_eq!(
            seen.wait_for(|i| matches!(i, Incoming::Connected(1)), 1)
                .len(),
            1,
            "old client never arrived"
        );

        server.set_certificate(&after).unwrap();

        let (_new, leaf_new) = presented_by(&addr);
        assert_eq!(
            fingerprint_of(&leaf_new),
            after.fingerprint(),
            "the renewal was not presented"
        );
        // The old connection still carries traffic on the certificate it
        // agreed: a frame sent now arrives.
        old.send(SERVER, Channel::Reliable, b"still here");
        let mut seen = Polled::new(&mut server);
        assert_eq!(
            seen.wait_for(
                |i| matches!(i, Incoming::Message(1, _, b) if b == b"still here"),
                1
            )
            .len(),
            1,
            "the pre-renewal connection went quiet: {:?}",
            seen.seen
        );
        assert!(
            !seen
                .seen
                .iter()
                .any(|i| matches!(i, Incoming::Disconnected(1, _))),
            "the renewal dropped the old connection: {:?}",
            seen.seen
        );
        assert!(
            old.peer_certificate()
                .is_some_and(|l| fingerprint_of(&l) == before.fingerprint())
        );
    }

    /// **A chain that cannot be loaded is refused at load, never presented as
    /// nothing.** A key that belongs to another certificate, an empty chain, a
    /// file that is not PEM: each is an error with the reason in it, so a
    /// renewal that wrote a torn file leaves the relay on the certificate it
    /// had (that is the watcher's half, in `floptle-relay`).
    #[test]
    fn a_certificate_that_does_not_match_its_key_is_refused_at_load() {
        let (c1, _k1) = pem_for("us-east.relay.fopull.com");
        let (_c2, k2) = pem_for("us-east.relay.fopull.com");
        let e = ServerCertificate::from_pem(&c1, &k2).expect_err("a stranger's key");
        assert!(e.contains("server tls"), "{e}");
        let e = ServerCertificate::from_pem(b"", &k2).expect_err("no certificate at all");
        assert!(e.contains("no certificate"), "{e}");
        let e = ServerCertificate::from_pem(
            &c1,
            b"-----BEGIN PRIVATE KEY-----\nnope\n-----END PRIVATE KEY-----\n",
        )
        .expect_err("not a key");
        assert!(e.contains("private key"), "{e}");
        let e = ServerCertificate::from_pem(&c1, b"not pem at all").expect_err("not PEM");
        assert!(e.contains("key PEM"), "{e}");
    }

    /// **A verified-only connection refuses, with the reason, and never falls
    /// back.** This is what `floptle-relay-bench --verify` runs against a
    /// managed relay, and what every managed connection becomes once the
    /// fallback goes: the self-signed dev certificate does not verify for the
    /// region name, so the outcome is a disconnect that says so.
    #[test]
    fn a_verified_only_connection_refuses_an_unverifiable_chain_with_the_reason() {
        let server = QuicServer::bind(0).unwrap();
        let addr = format!("127.0.0.1:{}", server.local_port());
        let mut client = QuicClient::connect_verified(&addr, "us-east.relay.fopull.com").unwrap();
        let mut outcome = None;
        for _ in 0..300 {
            for ev in client.poll() {
                match ev {
                    Incoming::Connected(SERVER) => outcome = Some(Err("connected".to_string())),
                    Incoming::Disconnected(SERVER, why) => outcome = Some(Ok(why)),
                    _ => {}
                }
            }
            if outcome.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let why = outcome
            .expect("the handshake never resolved")
            .expect("it connected anyway");
        let why = why.expect("refused with no reason");
        assert!(why.contains("us-east.relay.fopull.com"), "{why}");
        assert!(why.contains("did not present a certificate"), "{why}");
        assert!(
            client.take_warnings().is_empty(),
            "a refusal is not a warning"
        );
        assert!(client.peer_certificate().is_none());
    }
}
