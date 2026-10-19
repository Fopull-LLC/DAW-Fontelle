//! Discord status: the two settings rows, the wire format of Discord's local
//! socket, what the status says, when the worker sends, and the transport end
//! to end against a fake Discord.
//!
//! `docs/next-update-plan.md` §6. Off by default — a song's title is something
//! people may not want broadcast — and inert until the build carries the
//! Discord application's ID.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use fontelle_app::discord::{
    DISCORD_CLIENT_ID, Link, OP_CLOSE, OP_FRAME, OP_HANDSHAKE, Shown, Step, activity, decode,
    encode, next_step, unix_socket_paths, windows_pipe_paths,
};
use fontelle_app::settings::{SETTING_ROWS, SettingControlKind, SettingRow, Settings};
use serde_json::json;

// ------------------------------------------------------------ settings ---

#[test]
fn both_switches_are_off_until_somebody_turns_them_on() {
    let s = Settings::default();
    assert!(!s.discord_status);
    assert!(!s.discord_song_name);
    // And a settings file from before they existed reads them as off too.
    let mut old = serde_json::from_str::<serde_json::Value>(&s.to_json()).unwrap();
    let map = old.as_object_mut().unwrap();
    map.remove("discord_status");
    map.remove("discord_song_name");
    let read = Settings::from_json(&old.to_string()).expect("an older file loads");
    assert!(!read.discord_status);
    assert!(!read.discord_song_name);
}

#[test]
fn the_switches_survive_a_save() {
    let s = Settings {
        discord_status: true,
        discord_song_name: true,
        ..Settings::default()
    };
    let read = Settings::from_json(&s.to_json()).unwrap();
    assert!(read.discord_status);
    assert!(read.discord_song_name);
}

#[test]
fn the_rows_sit_under_their_own_heading_just_before_updates() {
    let at = |row| SETTING_ROWS.iter().position(|r| *r == row).unwrap();
    let heading = at(SettingRow::Heading("Discord"));
    assert_eq!(SETTING_ROWS[heading + 1], SettingRow::DiscordStatus);
    assert_eq!(SETTING_ROWS[heading + 2], SettingRow::DiscordSongName);
    assert_eq!(SETTING_ROWS[heading + 3], SettingRow::Heading("Updates"));
}

#[test]
fn the_rows_are_switches_that_say_on_or_off() {
    let off = Settings::default();
    let on = Settings {
        discord_status: true,
        discord_song_name: true,
        ..Settings::default()
    };
    for row in [SettingRow::DiscordStatus, SettingRow::DiscordSongName] {
        assert_eq!(row.control_kind(), SettingControlKind::Switch);
        assert_eq!(row.value(&off), "Off");
        assert_eq!(row.value(&on), "On");
        assert!(!row.help().is_empty(), "{row:?} has no help line");
    }
    assert_eq!(
        SettingRow::DiscordStatus.label(&off),
        "Show what I'm doing in Discord"
    );
    assert_eq!(
        SettingRow::DiscordSongName.label(&off),
        "Include the song's name"
    );
}

#[test]
fn a_build_without_the_app_id_says_so_under_the_switch() {
    // A switch that does nothing, saying nothing, is the worst of both.
    if DISCORD_CLIENT_ID.is_none() {
        assert!(
            SettingRow::DiscordStatus
                .help()
                .to_lowercase()
                .contains("not in this build"),
            "{:?}",
            SettingRow::DiscordStatus.help()
        );
    }
}

// ------------------------------------------------------------ the wire ---

#[test]
fn a_message_is_opcode_length_then_json_all_little_endian() {
    let body = json!({"v": 1, "client_id": "123"});
    let bytes = encode(OP_HANDSHAKE, &body);
    let text = body.to_string();
    assert_eq!(&bytes[0..4], &0u32.to_le_bytes());
    assert_eq!(&bytes[4..8], &(text.len() as u32).to_le_bytes());
    assert_eq!(&bytes[8..], text.as_bytes());
    assert_eq!((OP_HANDSHAKE, OP_FRAME, OP_CLOSE), (0, 1, 2));
}

#[test]
fn decode_reads_back_what_encode_wrote_and_says_how_much() {
    let a = json!({"cmd": "SET_ACTIVITY", "nonce": "1"});
    let b = json!({"evt": "READY"});
    let mut stream = encode(OP_FRAME, &a);
    let first = stream.len();
    stream.extend(encode(OP_CLOSE, &b));
    let (op, value, used) = decode(&stream).expect("a whole message");
    assert_eq!((op, &value, used), (OP_FRAME, &a, first));
    let (op, value, used) = decode(&stream[first..]).expect("and the next");
    assert_eq!((op, &value, used), (OP_CLOSE, &b, stream.len() - first));
}

#[test]
fn decode_waits_for_a_whole_message() {
    let bytes = encode(OP_FRAME, &json!({"evt": "READY"}));
    for cut in 0..bytes.len() {
        assert!(decode(&bytes[..cut]).is_none(), "decoded from {cut} bytes");
    }
}

#[test]
fn decode_refuses_a_body_that_is_not_json() {
    let mut bytes = 1u32.to_le_bytes().to_vec();
    bytes.extend(3u32.to_le_bytes());
    bytes.extend(b"{x}");
    assert!(decode(&bytes).is_none());
}

// ---------------------------------------------------------- the status ---

#[test]
fn without_the_song_it_says_only_making_music() {
    let a = activity(None, false, 1_700_000_000);
    assert_eq!(a["details"], "Making music in Fontelle");
    assert!(a.get("state").is_none(), "{a}");
    assert_eq!(a["timestamps"]["start"], 1_700_000_000u64);
    assert_eq!(a["assets"]["large_image"], "fontelle");
    assert_eq!(a["assets"]["large_text"], "Fontelle");
}

#[test]
fn with_the_song_it_names_it_and_says_when_it_plays() {
    let a = activity(Some("Night Drive"), true, 5);
    assert_eq!(a["details"], "Working on Night Drive");
    assert_eq!(a["state"], "Playing");
}

#[test]
fn a_blank_song_name_reads_as_no_name() {
    let a = activity(Some("   "), false, 5);
    assert_eq!(a["details"], "Making music in Fontelle");
}

#[test]
fn a_long_song_name_is_cut_to_what_discord_accepts() {
    // Discord refuses a whole activity whose details pass 128 characters.
    let long = "ä".repeat(400);
    let a = activity(Some(&long), false, 5);
    let details = a["details"].as_str().unwrap();
    assert!(
        details.chars().count() <= 128,
        "{}",
        details.chars().count()
    );
    assert!(details.starts_with("Working on ä"));
}

// ------------------------------------------------------- the decisions ---

fn shown(song: Option<&str>, playing: bool) -> Shown {
    Shown {
        song: song.map(str::to_string),
        playing,
    }
}

#[test]
fn nothing_happens_while_it_is_off_and_never_connected() {
    let now = Instant::now();
    assert_eq!(next_step(&Link::default(), None, now), Step::Wait);
}

#[test]
fn turning_it_on_connects_at_once() {
    let now = Instant::now();
    let want = shown(None, false);
    assert_eq!(next_step(&Link::default(), Some(&want), now), Step::Connect);
}

#[test]
fn a_discord_that_is_not_running_is_asked_again_only_every_fifteen_seconds() {
    let t0 = Instant::now();
    let link = Link {
        last_attempt: Some(t0),
        ..Link::default()
    };
    let want = shown(None, false);
    assert_eq!(
        next_step(&link, Some(&want), t0 + Duration::from_secs(14)),
        Step::Wait
    );
    assert_eq!(
        next_step(&link, Some(&want), t0 + Duration::from_secs(15)),
        Step::Connect
    );
}

#[test]
fn once_connected_it_sends_what_is_wanted() {
    let now = Instant::now();
    let link = Link {
        connected: true,
        last_attempt: Some(now),
        ..Link::default()
    };
    let want = shown(Some("A"), false);
    assert_eq!(next_step(&link, Some(&want), now), Step::Send(want));
}

#[test]
fn what_discord_already_shows_is_not_sent_again() {
    let t0 = Instant::now();
    let want = shown(Some("A"), true);
    let link = Link {
        connected: true,
        shown: Some(want.clone()),
        last_send: Some(t0),
        last_attempt: Some(t0),
    };
    assert_eq!(
        next_step(&link, Some(&want), t0 + Duration::from_secs(60)),
        Step::Wait
    );
}

#[test]
fn a_change_waits_out_four_seconds_since_the_last_send() {
    // Discord's own limit is five a twenty seconds; play/stop pressed quickly
    // must not run into it.
    let t0 = Instant::now();
    let link = Link {
        connected: true,
        shown: Some(shown(Some("A"), false)),
        last_send: Some(t0),
        last_attempt: Some(t0),
    };
    let want = shown(Some("A"), true);
    assert_eq!(
        next_step(&link, Some(&want), t0 + Duration::from_millis(3_999)),
        Step::Wait
    );
    assert_eq!(
        next_step(&link, Some(&want), t0 + Duration::from_secs(4)),
        Step::Send(want)
    );
}

#[test]
fn turning_it_off_clears_the_status_at_once_then_closes() {
    let t0 = Instant::now();
    let showing = Link {
        connected: true,
        shown: Some(shown(Some("A"), false)),
        last_send: Some(t0),
        last_attempt: Some(t0),
    };
    // Not held to the four seconds: the person asked for it to go.
    assert_eq!(next_step(&showing, None, t0), Step::Clear);
    let cleared = Link {
        shown: None,
        ..showing
    };
    assert_eq!(next_step(&cleared, None, t0), Step::Close);
}

// ------------------------------------------------------ the socket paths ---

#[test]
fn the_unix_paths_try_every_folder_discord_is_known_to_use() {
    let env = |name: &str| match name {
        "XDG_RUNTIME_DIR" => Some("/run/user/1000".to_string()),
        "TMPDIR" => Some("/var/tmp".to_string()),
        _ => None,
    };
    let paths = unix_socket_paths(env);
    let p = |s: &str| PathBuf::from(s);
    // The plain runtime folder first, socket 0 first.
    assert_eq!(paths[0], p("/run/user/1000/discord-ipc-0"));
    for want in [
        "/run/user/1000/discord-ipc-9",
        "/run/user/1000/app/com.discordapp.Discord/discord-ipc-0",
        "/run/user/1000/snap.discord/discord-ipc-3",
        "/run/user/1000/.flatpak/com.discordapp.Discord/xdg-run/discord-ipc-0",
        "/var/tmp/discord-ipc-0",
        "/tmp/discord-ipc-0",
        "/tmp/snap.discord/discord-ipc-9",
    ] {
        assert!(paths.contains(&p(want)), "{want} is not tried");
    }
    // The runtime folder before TMPDIR before /tmp.
    let at = |s: &str| paths.iter().position(|x| *x == p(s)).unwrap();
    assert!(at("/run/user/1000/discord-ipc-0") < at("/var/tmp/discord-ipc-0"));
    assert!(at("/var/tmp/discord-ipc-0") < at("/tmp/discord-ipc-0"));
    // Three folders, four places in each, ten sockets in each place.
    assert_eq!(paths.len(), 3 * 4 * 10);
}

#[test]
fn a_folder_named_twice_is_tried_once() {
    let env = |name: &str| match name {
        "TMPDIR" | "TMP" => Some("/tmp".to_string()),
        "TEMP" => Some("/tmp/".to_string()),
        _ => None,
    };
    assert_eq!(unix_socket_paths(env).len(), 4 * 10);
}

#[test]
fn the_windows_pipes_are_numbered_zero_to_nine() {
    let pipes = windows_pipe_paths();
    assert_eq!(pipes.len(), 10);
    assert_eq!(pipes[0], PathBuf::from(r"\\.\pipe\discord-ipc-0"));
    assert_eq!(pipes[9], PathBuf::from(r"\\.\pipe\discord-ipc-9"));
}

// ------------------------------------------------- end to end, on unix ---

#[cfg(unix)]
mod fake_discord {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::time::Duration;

    use fontelle_app::discord::{
        Connection, DiscordPresence, OP_CLOSE, OP_FRAME, OP_HANDSHAKE, decode, encode,
    };
    use serde_json::{Value, json};

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fontelle-discord-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One message off the socket, as Discord would read it.
    fn read_message(stream: &mut UnixStream, pending: &mut Vec<u8>) -> (u32, Value) {
        loop {
            if let Some((op, value, used)) = decode(pending) {
                pending.drain(..used);
                return (op, value);
            }
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).expect("the client is still there");
            assert!(n > 0, "the client hung up mid-message");
            pending.extend_from_slice(&chunk[..n]);
        }
    }

    /// Answers as Discord does: READY to the handshake, an echo with the
    /// nonce to a command.
    fn answer(stream: &mut UnixStream, op: u32, value: &Value) {
        let reply = match op {
            OP_HANDSHAKE => json!({"cmd": "DISPATCH", "evt": "READY", "data": {"v": 1}}),
            OP_FRAME => json!({"cmd": value["cmd"], "nonce": value["nonce"], "data": null}),
            _ => return,
        };
        stream.write_all(&encode(OP_FRAME, &reply)).unwrap();
    }

    #[test]
    fn the_handshake_then_the_activity_arrive_framed() {
        let dir = scratch("connection");
        let path = dir.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut pending = Vec::new();
            let mut seen = Vec::new();
            for _ in 0..2 {
                let (op, value) = read_message(&mut stream, &mut pending);
                answer(&mut stream, op, &value);
                seen.push((op, value));
            }
            seen
        });
        // A socket that is not there first: the list is tried in order.
        let missing = dir.join("discord-ipc-9");
        let mut connection =
            Connection::open(&[missing, path], "4242").expect("connects to the fake");
        let a = json!({"details": "Making music in Fontelle"});
        connection.set_activity(Some(&a)).expect("sends");
        let seen = server.join().unwrap();
        assert_eq!(seen[0].0, OP_HANDSHAKE);
        assert_eq!(seen[0].1, json!({"v": 1, "client_id": "4242"}));
        assert_eq!(seen[1].0, OP_FRAME);
        assert_eq!(seen[1].1["cmd"], "SET_ACTIVITY");
        assert_eq!(seen[1].1["args"]["pid"], std::process::id());
        assert_eq!(seen[1].1["args"]["activity"], a);
        assert!(seen[1].1["nonce"].as_str().is_some_and(|n| !n.is_empty()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_discord_is_an_error_not_a_hang() {
        let dir = scratch("absent");
        assert!(Connection::open(&[dir.join("discord-ipc-0")], "1").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_presence_shows_the_song_then_clears_and_closes_when_turned_off() {
        let dir = scratch("presence");
        let path = dir.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut pending = Vec::new();
            let mut seen = Vec::new();
            loop {
                let (op, value) = read_message(&mut stream, &mut pending);
                answer(&mut stream, op, &value);
                let done = op == OP_CLOSE;
                seen.push((op, value));
                if done {
                    return seen;
                }
            }
        });
        let mut presence = DiscordPresence::with_paths("4242", vec![path]);
        presence.update(true, Some("Night Drive"), false);
        // The UI thread never waits on the socket; give the worker a moment.
        std::thread::sleep(Duration::from_millis(600));
        presence.update(false, Some("Night Drive"), false);
        let seen = server.join().unwrap();
        let ops: Vec<u32> = seen.iter().map(|(op, _)| *op).collect();
        assert_eq!(ops, [OP_HANDSHAKE, OP_FRAME, OP_FRAME, OP_CLOSE]);
        assert_eq!(
            seen[1].1["args"]["activity"]["details"],
            "Working on Night Drive"
        );
        assert!(seen[2].1["args"]["activity"].is_null(), "{}", seen[2].1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_returns_at_once_when_nobody_is_listening() {
        let dir = scratch("nobody");
        let mut presence = DiscordPresence::with_paths("4242", vec![dir.join("discord-ipc-0")]);
        let start = std::time::Instant::now();
        for _ in 0..100 {
            presence.update(true, Some("A"), true);
        }
        assert!(start.elapsed() < Duration::from_millis(100));
        drop(presence);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
