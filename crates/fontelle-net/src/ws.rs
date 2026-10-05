//! **Fontelle's own, not the engine's**: where the engine's `ws.rs` (the
//! relay's WebSocket leg, which is how a browser game reaches it) would be.
//!
//! The lifted `relay.rs` names the leg — `RelayServer::listen_websocket`, and
//! the `Legs` it forwards through — so this file gives it the names and
//! nothing behind them. A studio is never a browser, and the only relay
//! Fontelle runs is the in-process one its tests start, which never listens
//! for a page. Copying the real leg would bring a WebSocket stack
//! (`tokio-tungstenite`, `tokio-rustls`) into the tree for no one, so
//! `relay.rs` stays a verbatim copy and this stays empty: a [`WsServer`]
//! cannot be made, so every branch that reaches one is unreachable.

use std::net::SocketAddr;
use std::sync::Arc;

use crate::transport::{Channel, Incoming, LinkStats, PeerId, Transport};

/// Ids a WebSocket peer would get, far above anything a QUIC server counts
/// to — the engine's number, so `relay.rs` reads the same.
pub const WS_PEER_BASE: PeerId = 1 << 48;

/// A WebSocket listener that cannot exist here.
pub enum WsServer {}

impl WsServer {
    pub fn bind(_addr: &str, _tls: Option<Arc<rustls::ServerConfig>>) -> Result<Self, String> {
        Err("Fontelle's relay has no WebSocket leg: a studio is never a browser".to_string())
    }

    pub fn local_addr(&self) -> SocketAddr {
        match *self {}
    }

    pub fn remote_addr(&self, _peer: PeerId) -> Option<SocketAddr> {
        match *self {}
    }

    pub fn set_tls(&self, _tls: Option<Arc<rustls::ServerConfig>>) {
        match *self {}
    }
}

impl Transport for WsServer {
    fn send(&mut self, _peer: PeerId, _channel: Channel, _bytes: &[u8]) {
        match *self {}
    }

    fn poll(&mut self) -> Vec<Incoming> {
        match *self {}
    }

    fn stats(&self, _peer: PeerId) -> LinkStats {
        match *self {}
    }

    fn disconnect(&mut self, _peer: PeerId) {
        match *self {}
    }
}
