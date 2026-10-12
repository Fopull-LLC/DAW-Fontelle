// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC. GPL-3.0-only; see `LICENSE` beside
// `Cargo.toml`.

//! The program's commands, and the loop `run` sits in while the driver
//! plays.

use std::io::{BufRead, Write};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use fontelle_audio_ipc::control::HELPER_NAME;
use fontelle_audio_ipc::{
    Bells, ClientLine, HelperEvent, HelperLine, PROTOCOL_VERSION, ReadyInfo, Region, RunArgs,
    Shared, State,
};

use crate::device::{ClockDevice, Device, Notice, TEST_CLOCK, test_clock_enabled};
use crate::pump::Pump;

/// How long the driver may go without calling before the helper decides it
/// has gone (unplugged, or taken by another program): this, or twenty
/// buffers, whichever is longer.
const SILENT_DRIVER: Duration = Duration::from_secs(1);

/// One line to Fontelle. A Fontelle that has gone away cannot read it, and
/// that is not worth dying over.
fn say(line: &HelperLine) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", line.format());
    let _ = out.flush();
}

/// Runs the command in `args` (after the program's name); the exit code.
pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("--version" | "version") => {
            say(&HelperLine::Hello {
                version: PROTOCOL_VERSION,
            });
            0
        }
        Some("list") => {
            say(&HelperLine::Hello {
                version: PROTOCOL_VERSION,
            });
            for name in driver_names() {
                say(&HelperLine::Driver { name });
            }
            0
        }
        Some("panel") => panel(&args[1..]),
        Some("run") => match RunArgs::parse(args) {
            Ok(run_args) => run(&run_args),
            Err(why) => {
                say(&HelperLine::Error(why));
                2
            }
        },
        _ => {
            eprintln!(
                "{HELPER_NAME} {}: hosts an ASIO\u{ae} driver for Fontelle.\n\
                 usage: {HELPER_NAME} --version | list | panel --driver NAME | run …\n\
                 This program is started by Fontelle; see docs/asio.md in Fontelle's source.\n\
                 GPL-3.0-only. ASIO is a registered trademark of Steinberg Media Technologies \
                 GmbH.\n\
                 The ASIO Compatible Logo, which a terminal cannot show, is in Fontelle's \
                 licenses/trademarks/asio/ and its Audio output settings.",
                env!("CARGO_PKG_VERSION")
            );
            2
        }
    }
}

/// Every driver installed, by the name `run --driver` takes.
pub fn driver_names() -> Vec<String> {
    #[allow(unused_mut)]
    let mut names = Vec::new();
    #[cfg(windows)]
    names.extend(crate::asio::driver_names());
    if test_clock_enabled() {
        names.push(TEST_CLOCK.to_string());
    }
    names
}

fn open_device(name: &str) -> Result<Box<dyn Device>, String> {
    if name == TEST_CLOCK && test_clock_enabled() {
        return Ok(Box::new(ClockDevice::from_env()));
    }
    #[cfg(windows)]
    {
        crate::asio::AsioDevice::load(name).map(|d| Box::new(d) as Box<dyn Device>)
    }
    #[cfg(not(windows))]
    {
        Err(format!(
            "{name}: ASIO drivers are a Windows thing, and this is not Windows"
        ))
    }
}

/// `panel --driver NAME`: the driver's own settings, while nothing is
/// playing through it. Stays until its window closes.
fn panel(args: &[String]) -> i32 {
    let name = match args {
        [flag, name, ..] if flag == "--driver" => name.clone(),
        _ => {
            say(&HelperLine::Error("panel wants --driver NAME".to_string()));
            return 2;
        }
    };
    say(&HelperLine::Hello {
        version: PROTOCOL_VERSION,
    });
    let mut device = match open_device(&name) {
        Ok(device) => device,
        Err(why) => {
            say(&HelperLine::Error(why));
            return 1;
        }
    };
    if let Err(why) = device.control_panel() {
        say(&HelperLine::Error(why));
        return 1;
    }
    // A panel that is its own modal loop has closed by now; one that is a
    // window of ours is waited out, a second of no window at a time so a
    // panel that takes a moment to appear is not missed.
    let mut quiet_since = Instant::now();
    loop {
        device.idle(Duration::from_millis(100));
        if device.windows_open() {
            quiet_since = Instant::now();
        } else if quiet_since.elapsed() > Duration::from_secs(1) {
            break;
        }
    }
    say(&HelperLine::Bye("the control panel closed".to_string()));
    0
}

/// What the run loop hears from outside the driver.
enum Outside {
    Line(ClientLine),
    /// Standard input closed: Fontelle exited, or crashed.
    Closed,
    /// Fontelle's process ended (Windows watches the process itself too).
    #[cfg_attr(not(windows), allow(dead_code))]
    ParentGone,
}

fn listen(parent: u32) -> Receiver<Outside> {
    let (tx, rx) = channel();
    let lines = tx.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if let Some(line) = ClientLine::parse(&line)
                && lines.send(Outside::Line(line)).is_err()
            {
                return;
            }
        }
        let _ = lines.send(Outside::Closed);
    });
    #[cfg(windows)]
    crate::asio::watch_parent(parent, move || {
        let _ = tx.send(Outside::ParentGone);
    });
    #[cfg(not(windows))]
    let _ = (parent, tx);
    rx
}

/// Writes what the driver opened as into the block's header.
fn publish(shared: &Shared, info: &ReadyInfo) {
    let h = shared.header();
    h.buffer_frames.store(info.frames, Ordering::Relaxed);
    h.sample_rate.store(info.rate, Ordering::Relaxed);
    h.inputs.store(info.inputs, Ordering::Relaxed);
    h.outputs.store(info.outputs, Ordering::Relaxed);
    h.input_latency.store(info.input_latency, Ordering::Relaxed);
    h.output_latency
        .store(info.output_latency, Ordering::Relaxed);
}

/// Tells Fontelle the helper has stopped, unless Fontelle closed first.
fn stopped(shared: &Shared, bells: &Bells) {
    if shared.state() != Some(State::ClientClosed) {
        shared.set_state(State::HelperStopped);
    }
    bells.request.ring(&shared.header().request_seq);
}

/// `run`: the driver plays until Fontelle says stop, goes away, or the
/// driver does.
fn run(args: &RunArgs) -> i32 {
    say(&HelperLine::Hello {
        version: PROTOCOL_VERSION,
    });
    // Leaked: they live as long as the process, and the pump on the
    // driver's thread borrows them for that long.
    let region: &'static Region = match Region::open(&args.channel) {
        Ok(region) => Box::leak(Box::new(region)),
        Err(e) => {
            say(&HelperLine::Error(format!(
                "could not open Fontelle's audio block {}: {e}",
                args.channel
            )));
            return 1;
        }
    };
    let bells: &'static Bells = match Bells::open(&args.channel) {
        Ok(bells) => Box::leak(Box::new(bells)),
        Err(e) => {
            say(&HelperLine::Error(format!(
                "could not open Fontelle's doorbells {}: {e}",
                args.channel
            )));
            return 1;
        }
    };
    let shared = region.shared();
    shared
        .header()
        .helper_pid
        .store(std::process::id(), Ordering::Relaxed);
    let layout = shared.layout();
    let fail = |why: String| {
        say(&HelperLine::Error(why.clone()));
        stopped(shared, bells);
        say(&HelperLine::Bye(why));
        1
    };

    let mut device = match open_device(&args.driver) {
        Ok(device) => device,
        Err(why) => return fail(why),
    };
    let open = |device: &mut Box<dyn Device>, frames: Option<u32>| {
        let info = device.open(args.rate, frames, layout.max_outputs, layout.max_inputs)?;
        if info.frames > layout.max_frames {
            device.close();
            return Err(format!(
                "{} runs {} frames to a buffer, more than Fontelle's {}; choose a smaller \
                 buffer in its control panel",
                args.driver, info.frames, layout.max_frames
            ));
        }
        Ok(info)
    };
    let mut info = match open(&mut device, args.frames) {
        Ok(info) => info,
        Err(why) => return fail(why),
    };
    publish(shared, &info);
    let mut pump = Pump::new(shared, &*bells.request, &*bells.reply);
    pump.set_buffer(info.frames, info.rate);
    shared.set_state(State::Running);
    if let Err((why, _)) = device.start(pump) {
        device.close();
        return fail(why);
    }
    say(&HelperLine::Ready(info.clone()));

    let outside = listen(args.parent);
    let mut requested = args.frames;
    let mut last_cycles = shared.counters().cycles;
    let mut progress = Instant::now();
    let reason: String = 'run: loop {
        device.idle(Duration::from_millis(20));
        loop {
            match outside.try_recv() {
                Ok(Outside::Line(ClientLine::Panel)) => {
                    if let Err(why) = device.control_panel() {
                        say(&HelperLine::Error(why));
                    }
                }
                Ok(Outside::Line(ClientLine::Stop)) => break 'run "Fontelle asked".to_string(),
                Ok(Outside::Closed) | Err(TryRecvError::Disconnected) => {
                    break 'run "Fontelle went away".to_string();
                }
                Ok(Outside::ParentGone) => break 'run "Fontelle exited".to_string(),
                Err(TryRecvError::Empty) => break,
            }
        }
        if shared.state() == Some(State::ClientClosed) {
            break 'run "Fontelle closed its end".to_string();
        }

        for notice in device.notices() {
            match notice {
                Notice::Lost(n) => {
                    say(&HelperLine::Event(HelperEvent::Overload));
                    shared
                        .header()
                        .device_xruns
                        .fetch_add(u64::from(n), Ordering::Relaxed);
                }
                Notice::LatenciesChanged => {
                    if let Some((input, output)) = device.latencies() {
                        info.input_latency = input;
                        info.output_latency = output;
                        publish(shared, &info);
                        say(&HelperLine::Event(HelperEvent::LatenciesChanged {
                            input,
                            output,
                        }));
                    }
                }
                Notice::Reset | Notice::RateChanged => {
                    // Stopped, closed, opened again as the driver now is.
                    // A rate that moved is put back to the studio's — or
                    // refused, which ends the run with the reason.
                    if notice == Notice::RateChanged
                        && let Some(rate) = device.rate()
                    {
                        if rate == args.rate {
                            continue;
                        }
                        say(&HelperLine::Event(HelperEvent::RateChanged(rate)));
                    }
                    let Some(mut pump) = device.stop() else {
                        break 'run "the driver's thread was lost".to_string();
                    };
                    device.close();
                    if let Err(why) = device.reload() {
                        say(&HelperLine::Error(why.clone()));
                        pump.stop();
                        break 'run why;
                    }
                    // A preferred size that changed is the person's choice in
                    // the driver's panel; otherwise Fontelle's stands.
                    let before = info.preferred_frames;
                    let reopened = open(&mut device, requested).and_then(|again| {
                        if again.preferred_frames != before {
                            requested = None;
                            device.close();
                            open(&mut device, None)
                        } else {
                            Ok(again)
                        }
                    });
                    let again = match reopened {
                        Ok(again) => again,
                        Err(why) => {
                            say(&HelperLine::Error(why.clone()));
                            pump.stop();
                            break 'run why;
                        }
                    };
                    shared.header().resets.fetch_add(1, Ordering::Relaxed);
                    say(&HelperLine::Event(HelperEvent::Reset));
                    if again.frames != info.frames {
                        say(&HelperLine::Event(HelperEvent::BufferChanged(again.frames)));
                    }
                    if (again.input_latency, again.output_latency)
                        != (info.input_latency, info.output_latency)
                    {
                        say(&HelperLine::Event(HelperEvent::LatenciesChanged {
                            input: again.input_latency,
                            output: again.output_latency,
                        }));
                    }
                    info = again;
                    publish(shared, &info);
                    pump.set_buffer(info.frames, info.rate);
                    pump.pause();
                    if let Err((why, pump)) = device.start(pump) {
                        say(&HelperLine::Error(why.clone()));
                        pump.stop();
                        break 'run why;
                    }
                    progress = Instant::now();
                }
            }
        }

        // A driver that stops calling has gone: unplugged, or taken.
        let cycles = shared.counters().cycles;
        if cycles != last_cycles {
            last_cycles = cycles;
            progress = Instant::now();
        } else {
            let buffer = Duration::from_secs_f64(f64::from(info.frames) / f64::from(info.rate));
            if progress.elapsed() > SILENT_DRIVER.max(buffer * 20) {
                let why = format!(
                    "{} stopped asking for audio \u{2014} the device may have been unplugged \
                     or taken by another program",
                    args.driver
                );
                say(&HelperLine::Error(why.clone()));
                break 'run why;
            }
        }
    };

    let _ = device.stop();
    device.close();
    stopped(shared, bells);
    say(&HelperLine::Bye(reason));
    0
}
