//! The song's written form is pinned to the session protocol.
//!
//! Two studios in a session compare a fingerprint of the song as it is
//! written (`Project::sync_hash`). A build that writes the song differently
//! — a field added, one no longer written — can never agree with one that
//! does not, and the join hung on it: Ty, joining a friend from the phase 1
//! preview (2026-10-18), *"its getting stuck on this"*. Solo had stopped
//! being written, and both builds still called themselves 0.28.0.
//!
//! So the names the song writes are listed in `song_form.txt` under the
//! `PROTOCOL` they belong to. Change what a song writes and this fails until
//! `PROTOCOL` moves with it, so two such builds are refused at the door with
//! a sentence rather than left waiting. `FONTELLE_BLESS=1` rewrites the list
//! once `PROTOCOL` has moved.

use std::collections::BTreeSet;

use fontelle_model::wire::PROTOCOL;

const LIST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/song_form.txt");

/// Every key the song writes, except those that are ids of things in it.
fn names(json: &serde_json::Value, into: &mut BTreeSet<String>) {
    match json {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let id = key.chars().all(|c| c.is_ascii_hexdigit() || c == '-') && key.len() >= 4
                    || key.chars().all(|c| c.is_ascii_digit());
                if !id {
                    into.insert(key.clone());
                }
                names(value, into);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| names(v, into)),
        _ => {}
    }
}

#[test]
fn what_a_song_writes_moves_only_with_the_protocol() {
    let song = fontelle_app::tour::tour_project(48_000);
    let mut written = BTreeSet::new();
    names(&serde_json::to_value(&song).unwrap(), &mut written);
    let wrote = format!(
        "protocol {PROTOCOL}\n{}\n",
        written.iter().cloned().collect::<Vec<_>>().join("\n")
    );
    if std::env::var_os("FONTELLE_BLESS").is_some() {
        std::fs::write(LIST, &wrote).unwrap();
        return;
    }
    let listed = std::fs::read_to_string(LIST).unwrap_or_default();
    let mut lines = listed.lines();
    let protocol = lines.next().unwrap_or_default().to_string();
    let was: BTreeSet<String> = lines.map(str::to_string).collect();
    let added: Vec<_> = written.difference(&was).collect();
    let gone: Vec<_> = was.difference(&written).collect();
    if added.is_empty() && gone.is_empty() {
        assert_eq!(
            protocol,
            format!("protocol {PROTOCOL}"),
            "PROTOCOL moved: rewrite the list with FONTELLE_BLESS=1"
        );
        return;
    }
    assert_ne!(
        protocol,
        format!("protocol {PROTOCOL}"),
        "the song writes differently (new: {added:?}, gone: {gone:?}) but PROTOCOL is still \
         {PROTOCOL}: move PROTOCOL in fontelle-model's wire.rs, then rewrite the list with \
         FONTELLE_BLESS=1"
    );
    panic!(
        "PROTOCOL moved and the song writes differently (new: {added:?}, gone: {gone:?}): \
         rewrite the list with FONTELLE_BLESS=1"
    );
}
