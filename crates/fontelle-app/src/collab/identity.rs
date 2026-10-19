//! Who a studio is, provably, and what a session's password is good for.
//!
//! Ty, 2026-10-19: *"we should try our best to implement security so that
//! users can be blocked from sessions and stuff like if there are bad actors
//! and stuff, maybe making password only sessions"*. Until then a studio was
//! the install id its hello named — a number anybody could type into a
//! modified build — so a removal kept out only the honest.
//!
//! **An install is an Ed25519 key pair**, made the first time it shares or
//! joins and kept beside the settings file, readable by its owner alone. A
//! joiner proves it holds the key by signing the host's fresh nonce together
//! with the session's code ([`join_message`]): the signature is good for
//! that one connection to that one session, so it can be neither replayed
//! later nor carried into somebody else's session. What the host blocks is
//! that public key.
//!
//! **A password is never sent.** Both sides stretch it with PBKDF2 under the
//! session's code ([`PasswordKeys::derive`]) and split what comes out, by
//! label, into a key for the proof (an HMAC of the nonce and the joiner's
//! key) and a key the connection is sealed with ([`PasswordKeys::link`],
//! one key a direction, mixed with the connection's nonce). Somebody who
//! watched a handshake can still guess passwords offline at PBKDF2's price;
//! nothing short of a PAKE stops that, and a long password is the defence.

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use ring::{hmac, pbkdf2};

/// What a joiner signs ahead of the nonce and the code: the purpose, so a
/// signature made here is no signature of anything else.
const JOIN_CONTEXT: &[u8] = b"fontelle join v1\0";

/// PBKDF2's rounds. A tenth of a second or so on a laptop, paid once by the
/// host for each password it is given and once by a joiner for each it types
/// — and by anyone guessing, for every guess.
const PASSWORD_ROUNDS: u32 = 200_000;

/// This studio's key pair. Cloned cheaply; the secret half never leaves it.
#[derive(Clone)]
pub struct Identity {
    pair: Arc<Ed25519KeyPair>,
    pkcs8: Arc<Vec<u8>>,
}

impl std::fmt::Debug for Identity {
    // The public half only: a debug print is the last place a secret key
    // should turn up.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Identity({})", hex(&self.public_key()))
    }
}

impl Identity {
    /// A new key pair, from the system's randomness.
    pub fn generate() -> Identity {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("the system has randomness");
        Identity::from_pkcs8(pkcs8.as_ref()).expect("a key just made reads")
    }

    /// The key pair written in `bytes` (PKCS#8 v2, as [`pkcs8`] gives it).
    ///
    /// [`pkcs8`]: Identity::pkcs8
    pub fn from_pkcs8(bytes: &[u8]) -> Result<Identity, String> {
        let pair = Ed25519KeyPair::from_pkcs8(bytes).map_err(|e| format!("not a key: {e}"))?;
        Ok(Identity {
            pair: Arc::new(pair),
            pkcs8: Arc::new(bytes.to_vec()),
        })
    }

    /// The key kept at `path`, made and written there the first time —
    /// with only its owner able to read it, where the system can say so.
    pub fn load_or_create(path: &Path) -> Result<Identity, String> {
        if let Ok(bytes) = std::fs::read(path) {
            return Identity::from_pkcs8(&bytes).map_err(|e| format!("{}: {e}", path.display()));
        }
        let made = Identity::generate();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        // Written beside and renamed, so a half-written key is never read
        // back as this studio's — which would make it somebody new.
        let part = path.with_extension("part");
        write_private(&part, made.pkcs8()).map_err(|e| format!("{}: {e}", part.display()))?;
        std::fs::rename(&part, path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(made)
    }

    /// The pair as PKCS#8, for writing down.
    pub fn pkcs8(&self) -> &[u8] {
        &self.pkcs8
    }

    /// The public half, 32 bytes: what the host sees, and blocks.
    pub fn public_key(&self) -> Vec<u8> {
        self.pair.public_key().as_ref().to_vec()
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.pair.sign(message).as_ref().to_vec()
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    // Windows: the file sits in the user's own profile, whose folder is
    // already theirs alone.
    std::fs::write(path, bytes)
}

/// Whether `signature` is `key`'s over `message`.
pub fn verify(key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    key.len() == 32
        && UnparsedPublicKey::new(&ED25519, key)
            .verify(message, signature)
            .is_ok()
}

/// What a joiner signs to come into the session `code` on the connection
/// the host challenged with `nonce`.
pub fn join_message(nonce: &[u8], code: &str) -> Vec<u8> {
    let mut message = JOIN_CONTEXT.to_vec();
    message.extend_from_slice(nonce);
    message.extend_from_slice(code.as_bytes());
    message
}

/// A session's code however it was typed — spaces, a dash, lower case — as
/// both sides sign and salt with it.
pub fn normal_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase()
}

/// A fresh nonce for a challenge.
pub fn nonce() -> Vec<u8> {
    let mut nonce = vec![0; 32];
    SystemRandom::new()
        .fill(&mut nonce)
        .expect("the system has randomness");
    nonce
}

/// What a session's password is stretched into: one key to prove it with,
/// one to seal connections under — never the same, by label.
#[derive(Clone)]
pub struct PasswordKeys {
    proof: [u8; 32],
    seal: [u8; 32],
}

impl PasswordKeys {
    /// `password`, stretched under the session `code` (as [`normal_code`]
    /// writes it). The code is the salt: the same password in another
    /// session is other keys.
    pub fn derive(password: &str, code: &str) -> PasswordKeys {
        let mut salt = b"fontelle session v1:".to_vec();
        salt.extend_from_slice(code.as_bytes());
        let mut stretched = [0u8; 32];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            NonZeroU32::new(PASSWORD_ROUNDS).expect("not zero"),
            &salt,
            password.as_bytes(),
            &mut stretched,
        );
        let key = hmac::Key::new(hmac::HMAC_SHA256, &stretched);
        PasswordKeys {
            proof: tag32(&key, &[b"fontelle proof v1"]),
            seal: tag32(&key, &[b"fontelle seal v1"]),
        }
    }

    /// The proof that the joiner holding `key` knows the password, for the
    /// connection challenged with `nonce`.
    pub fn proof(&self, nonce: &[u8], key: &[u8]) -> Vec<u8> {
        let mac = hmac::Key::new(hmac::HMAC_SHA256, &self.proof);
        tag32(&mac, &[b"fontelle join proof v1", nonce, key]).to_vec()
    }

    /// Whether `proof` is [`proof`](Self::proof)'s — compared in constant
    /// time, so its timing says nothing about how near a guess came.
    pub fn check(&self, nonce: &[u8], key: &[u8], proof: &[u8]) -> bool {
        let mac = hmac::Key::new(hmac::HMAC_SHA256, &self.proof);
        let mut message = b"fontelle join proof v1".to_vec();
        message.extend_from_slice(nonce);
        message.extend_from_slice(key);
        hmac::verify(&mac, &message, proof).is_ok()
    }

    /// The keys the connection challenged with `nonce` is sealed under, one
    /// each way. Mixed with the nonce, so every connection has its own and a
    /// frame from one is nothing on another.
    pub fn link(&self, nonce: &[u8]) -> LinkKeys {
        let mac = hmac::Key::new(hmac::HMAC_SHA256, &self.seal);
        LinkKeys {
            host_to_joiner: tag32(&mac, &[b"host to joiner", nonce]),
            joiner_to_host: tag32(&mac, &[b"joiner to host", nonce]),
        }
    }
}

/// One connection's two keys.
#[derive(Clone)]
pub struct LinkKeys {
    pub host_to_joiner: [u8; 32],
    pub joiner_to_host: [u8; 32],
}

fn tag32(key: &hmac::Key, parts: &[&[u8]]) -> [u8; 32] {
    let mut context = hmac::Context::with_key(key);
    for part in parts {
        context.update(part);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(context.sign().as_ref());
    out
}

/// A key as the settings file writes it.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// [`hex`] read back; `None` for anything that is not.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}
