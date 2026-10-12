//! The text half of the protocol, `docs/asio.md`'s table line by line.

use fontelle_audio_ipc::control::{HELPER_NAME, clean};
use fontelle_audio_ipc::{ClientLine, HelperEvent, HelperLine, ReadyInfo, RunArgs};

fn ready() -> ReadyInfo {
    ReadyInfo {
        driver: "Focusrite USB ASIO".to_string(),
        frames: 256,
        rate: 48_000,
        inputs: 0,
        outputs: 2,
        input_latency: 300,
        output_latency: 340,
        min_frames: 16,
        max_frames: 2048,
        preferred_frames: 256,
        granularity: -1,
        format: "Int32LSB".to_string(),
    }
}

#[test]
fn every_helper_line_reads_back_as_itself() {
    let lines = [
        HelperLine::Hello { version: 1 },
        HelperLine::Driver {
            name: "ASIO4ALL v2".to_string(),
        },
        HelperLine::Ready(ready()),
        HelperLine::Event(HelperEvent::Reset),
        HelperLine::Event(HelperEvent::RateChanged(44_100)),
        HelperLine::Event(HelperEvent::BufferChanged(512)),
        HelperLine::Event(HelperEvent::LatenciesChanged {
            input: 10,
            output: 20,
        }),
        HelperLine::Event(HelperEvent::Overload),
        HelperLine::Error("the driver would not start".to_string()),
        HelperLine::Bye("Fontelle closed".to_string()),
    ];
    for line in lines {
        let text = line.format();
        assert!(!text.contains('\n'), "{text:?}");
        assert_eq!(HelperLine::parse(&text), Some(line.clone()), "{text:?}");
        // As read off a pipe on Windows.
        assert_eq!(
            HelperLine::parse(&format!("{text}\r\n")),
            Some(line),
            "{text:?}"
        );
    }
}

#[test]
fn the_words_on_the_wire_are_the_documented_ones() {
    assert_eq!(HELPER_NAME, "fontelle-asio");
    assert_eq!(
        HelperLine::Hello { version: 1 }.format(),
        "fontelle-asio\t1"
    );
    assert_eq!(
        HelperLine::Driver {
            name: "FlexASIO".into()
        }
        .format(),
        "driver\tFlexASIO"
    );
    assert_eq!(
        HelperLine::Event(HelperEvent::Reset).format(),
        "event\treset"
    );
    assert_eq!(
        HelperLine::Event(HelperEvent::LatenciesChanged {
            input: 1,
            output: 2
        })
        .format(),
        "event\tlatencies\t1\t2"
    );
    let r = HelperLine::Ready(ready()).format();
    assert!(r.starts_with("ready\t"), "{r}");
    for field in [
        "driver=Focusrite USB ASIO",
        "frames=256",
        "rate=48000",
        "outputs=2",
        "output_latency=340",
        "granularity=-1",
        "format=Int32LSB",
    ] {
        assert!(r.split('\t').any(|f| f == field), "{field} in {r}");
    }
    assert_eq!(ClientLine::Panel.format(), "panel");
    assert_eq!(ClientLine::Stop.format(), "stop");
    assert_eq!(ClientLine::parse("panel\r"), Some(ClientLine::Panel));
    assert_eq!(ClientLine::parse("stop"), Some(ClientLine::Stop));
}

#[test]
fn a_line_from_a_newer_side_is_ignored_and_a_ready_line_may_grow() {
    assert_eq!(HelperLine::parse("telemetry\tsomething"), None);
    assert_eq!(HelperLine::parse(""), None);
    assert_eq!(HelperLine::parse("event\tnew-thing"), None);
    assert_eq!(ClientLine::parse("dance"), None);
    // A field this side does not know is skipped; one it does is read.
    let grown = format!("{}\tclock=Internal", HelperLine::Ready(ready()).format());
    assert_eq!(HelperLine::parse(&grown), Some(HelperLine::Ready(ready())));
    // A ready line missing a number is not a ready line.
    assert_eq!(HelperLine::parse("ready\tdriver=x\tframes=abc"), None);
}

#[test]
fn a_name_with_tabs_or_newlines_cannot_break_a_line() {
    assert_eq!(clean("Bad\tDriver\nName\r"), "Bad Driver Name");
    let line = HelperLine::Driver {
        name: clean("Bad\tName"),
    };
    assert_eq!(HelperLine::parse(&line.format()), Some(line));
    let line = HelperLine::Error(clean("two\nlines"));
    assert_eq!(line.format(), "error\ttwo lines");
}

#[test]
fn the_run_command_line_reads_back() {
    let args = RunArgs {
        driver: "Yamaha Steinberg USB ASIO".to_string(),
        channel: "Local\\fontelle-audio-1234-1".to_string(),
        rate: 48_000,
        frames: Some(128),
        inputs: 0,
        outputs: 2,
        parent: 1234,
    };
    let v = args.to_args();
    assert_eq!(v[0], "run");
    assert_eq!(RunArgs::parse(&v), Ok(args.clone()));
    let auto = RunArgs {
        frames: None,
        ..args
    };
    assert_eq!(RunArgs::parse(&auto.to_args()), Ok(auto));
    assert!(RunArgs::parse(&["run".to_string()]).is_err());
    assert!(RunArgs::parse(&["list".to_string()]).is_err());
}
