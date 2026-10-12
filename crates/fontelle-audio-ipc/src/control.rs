//! The helper's command line and the text lines on its standard streams.
//!
//! One line per message, fields separated by tabs, UTF-8. A side ignores a
//! line it does not know, so either can grow without breaking the other;
//! changing what a known line means is a new protocol version.
//!
//! The helper's first line is always `fontelle-asio\t<version>`.
//!
//! Helper → Fontelle (standard output):
//!
//! | line | when |
//! |---|---|
//! | `fontelle-asio\t1` | first, always |
//! | `driver\t<name>` | `list`: one per installed driver |
//! | `ready\tkey=value…` | `run`: the driver is running ([`ReadyInfo`]) |
//! | `event\treset` | the driver asked for a reset and the helper did it |
//! | `event\trate\t<hz>` | the driver's rate moved |
//! | `event\tbuffer\t<frames>` | the driver's buffer size moved |
//! | `event\tlatencies\t<in>\t<out>` | the driver's latencies moved (frames) |
//! | `event\toverload` | the driver reported an overload |
//! | `error\t<words>` | something a person should read |
//! | `bye\t<words>` | the helper is exiting, and why |
//!
//! Fontelle → helper (standard input): `panel` (open the driver's control
//! panel) and `stop`. Standard input closing is `stop` too — so a Fontelle
//! that crashes takes its helper with it.

/// The helper program's name, without `.exe`.
pub const HELPER_NAME: &str = "fontelle-asio";

/// Something the helper says.
#[derive(Debug, Clone, PartialEq)]
pub enum HelperLine {
    Hello { version: u32 },
    Driver { name: String },
    Ready(ReadyInfo),
    Event(HelperEvent),
    Error(String),
    Bye(String),
}

/// Something that happened to a running driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperEvent {
    Reset,
    RateChanged(u32),
    BufferChanged(u32),
    LatenciesChanged { input: u32, output: u32 },
    Overload,
}

/// What the driver opened as.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadyInfo {
    pub driver: String,
    pub frames: u32,
    pub rate: u32,
    pub inputs: u32,
    pub outputs: u32,
    /// Frames, as the driver says them.
    pub input_latency: u32,
    pub output_latency: u32,
    /// The driver's own buffer range: smallest, largest, preferred, and the
    /// step (`-1` is powers of two, `0` is one size only).
    pub min_frames: u32,
    pub max_frames: u32,
    pub preferred_frames: u32,
    pub granularity: i32,
    /// The driver's sample format, by ASIO's name for it ("Int32LSB").
    pub format: String,
}

/// Something Fontelle says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientLine {
    Panel,
    Stop,
}

/// A field with the separators taken out: a driver's name is the driver's to
/// choose.
pub fn clean(text: &str) -> String {
    text.split(['\t', '\n', '\r'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

impl HelperLine {
    pub fn format(&self) -> String {
        match self {
            Self::Hello { version } => format!("{HELPER_NAME}\t{version}"),
            Self::Driver { name } => format!("driver\t{}", clean(name)),
            Self::Ready(r) => format!(
                "ready\tdriver={}\tframes={}\trate={}\tinputs={}\toutputs={}\t\
                 input_latency={}\toutput_latency={}\tmin_frames={}\tmax_frames={}\t\
                 preferred_frames={}\tgranularity={}\tformat={}",
                clean(&r.driver),
                r.frames,
                r.rate,
                r.inputs,
                r.outputs,
                r.input_latency,
                r.output_latency,
                r.min_frames,
                r.max_frames,
                r.preferred_frames,
                r.granularity,
                clean(&r.format),
            ),
            Self::Event(HelperEvent::Reset) => "event\treset".to_string(),
            Self::Event(HelperEvent::RateChanged(hz)) => format!("event\trate\t{hz}"),
            Self::Event(HelperEvent::BufferChanged(n)) => format!("event\tbuffer\t{n}"),
            Self::Event(HelperEvent::LatenciesChanged { input, output }) => {
                format!("event\tlatencies\t{input}\t{output}")
            }
            Self::Event(HelperEvent::Overload) => "event\toverload".to_string(),
            Self::Error(words) => format!("error\t{}", clean(words)),
            Self::Bye(words) => format!("bye\t{}", clean(words)),
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        let mut fields = line.split('\t');
        let first = fields.next()?;
        let rest: Vec<&str> = fields.collect();
        let number = |at: usize| rest.get(at)?.parse::<u32>().ok();
        Some(match (first, rest.as_slice()) {
            (HELPER_NAME, [version, ..]) => Self::Hello {
                version: version.parse().ok()?,
            },
            ("driver", [name, ..]) => Self::Driver {
                name: name.to_string(),
            },
            ("ready", fields) => Self::Ready(parse_ready(fields)?),
            ("event", ["reset", ..]) => Self::Event(HelperEvent::Reset),
            ("event", ["overload", ..]) => Self::Event(HelperEvent::Overload),
            ("event", ["rate", ..]) => Self::Event(HelperEvent::RateChanged(number(1)?)),
            ("event", ["buffer", ..]) => Self::Event(HelperEvent::BufferChanged(number(1)?)),
            ("event", ["latencies", ..]) => Self::Event(HelperEvent::LatenciesChanged {
                input: number(1)?,
                output: number(2)?,
            }),
            ("error", [words, ..]) => Self::Error(words.to_string()),
            ("bye", [words, ..]) => Self::Bye(words.to_string()),
            _ => return None,
        })
    }
}

/// A ready line's `key=value` fields. Every field this side knows must be
/// there and read; a field it does not know is a newer helper's, skipped.
fn parse_ready(fields: &[&str]) -> Option<ReadyInfo> {
    let mut r = ReadyInfo::default();
    let mut found = 0u32;
    for field in fields {
        let (key, value) = field.split_once('=')?;
        let n = || value.parse::<u32>().ok();
        let bit = match key {
            "driver" => {
                r.driver = value.to_string();
                1
            }
            "frames" => {
                r.frames = n()?;
                2
            }
            "rate" => {
                r.rate = n()?;
                4
            }
            "inputs" => {
                r.inputs = n()?;
                8
            }
            "outputs" => {
                r.outputs = n()?;
                16
            }
            "input_latency" => {
                r.input_latency = n()?;
                32
            }
            "output_latency" => {
                r.output_latency = n()?;
                64
            }
            "min_frames" => {
                r.min_frames = n()?;
                128
            }
            "max_frames" => {
                r.max_frames = n()?;
                256
            }
            "preferred_frames" => {
                r.preferred_frames = n()?;
                512
            }
            "granularity" => {
                r.granularity = value.parse().ok()?;
                1024
            }
            "format" => {
                r.format = value.to_string();
                2048
            }
            _ => 0,
        };
        found |= bit;
    }
    (found == 4095).then_some(r)
}

impl ClientLine {
    pub fn format(&self) -> &'static str {
        match self {
            Self::Panel => "panel",
            Self::Stop => "stop",
        }
    }

    pub fn parse(line: &str) -> Option<Self> {
        match line.trim_end_matches(['\r', '\n']).split('\t').next()? {
            "panel" => Some(Self::Panel),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }
}

/// What `fontelle-asio run` is told on its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunArgs {
    /// The driver, by the name `list` gave.
    pub driver: String,
    /// The shared block's base name ([`crate::channel_name`]).
    pub channel: String,
    /// The studio's rate; the helper sets the driver to it or refuses.
    pub rate: u32,
    /// Frames per buffer; `None` is the driver's own preferred size.
    pub frames: Option<u32>,
    pub inputs: u32,
    pub outputs: u32,
    /// Fontelle's process: the helper exits when it does.
    pub parent: u32,
}

impl RunArgs {
    /// The arguments after the program's name, `run` first.
    pub fn to_args(&self) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "--driver".to_string(),
            self.driver.clone(),
            "--channel".to_string(),
            self.channel.clone(),
            "--rate".to_string(),
            self.rate.to_string(),
            "--inputs".to_string(),
            self.inputs.to_string(),
            "--outputs".to_string(),
            self.outputs.to_string(),
            "--parent".to_string(),
            self.parent.to_string(),
        ];
        if let Some(frames) = self.frames {
            args.extend(["--frames".to_string(), frames.to_string()]);
        }
        args
    }

    /// Reads what [`to_args`](Self::to_args) wrote, `run` included.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let (first, rest) = args.split_first().ok_or("no command")?;
        if first != "run" {
            return Err(format!("{first} is not run"));
        }
        let mut get = std::collections::HashMap::new();
        let mut it = rest.iter();
        while let Some(flag) = it.next() {
            let value = it.next().ok_or_else(|| format!("{flag} wants a value"))?;
            get.insert(flag.as_str(), value.clone());
        }
        let text = |flag: &str| {
            get.get(flag)
                .cloned()
                .ok_or_else(|| format!("{flag} is missing"))
        };
        let number = |flag: &str| {
            text(flag)?
                .parse::<u32>()
                .map_err(|e| format!("{flag}: {e}"))
        };
        Ok(Self {
            driver: text("--driver")?,
            channel: text("--channel")?,
            rate: number("--rate")?,
            frames: match get.get("--frames") {
                Some(_) => Some(number("--frames")?),
                None => None,
            },
            inputs: number("--inputs")?,
            outputs: number("--outputs")?,
            parent: number("--parent")?,
        })
    }
}
