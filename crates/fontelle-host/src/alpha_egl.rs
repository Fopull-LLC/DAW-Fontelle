//! Plugin editors that cannot draw with this machine's EGL, refused before
//! they are opened rather than after they have taken the studio down.
//!
//! Reported (2026-10-05): opening Vital's editor, CLAP or VST 3, showed a
//! black window and then aborted the studio from Vital's render thread —
//! *"BGFX FATAL ... Failed to create surface"*. Vital draws with bgfx over
//! EGL and asks for a config with 8 bits of alpha. NVIDIA's EGL (open driver
//! 615, `egl-x11`) offers such configs only with a depth-32 visual; Vital
//! makes its surface on a depth-24 window, `eglCreateWindowSurface` answers
//! `EGL_BAD_CONFIG`, and bgfx calls `abort()`. Vital's own standalone app
//! dies the same way, so no host can make the window work — but a host can
//! decline to open it, and leave the plugin's parameters in its own panel.
//!
//! Whether this machine is one of those is asked of a **child process**
//! ([`EGL_PROBE_FLAG`], answered by the studio's own binary as the bundle
//! scan is): it does what bgfx does — the first 8-bit RGBA window config,
//! and a surface on an unmapped window of the root's visual — and says
//! whether the surface was made. A driver that crashes there crashes the
//! child. The answer is asked once per session ([`EditorGate`]).
//!
//! Mesa's EGL makes the surface (llvmpipe, on the CPU), which is what the
//! **Compatible plugin graphics** setting turns on at the next start —
//! `gui::egl_vendor_for`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fontelle_types::PluginKey;

/// The argument that makes a Fontelle binary try an EGL window surface the
/// way bgfx makes one, print the answer and exit: `fontelle
/// --fontelle-egl-probe`.
pub const EGL_PROBE_FLAG: &str = "--fontelle-egl-probe";

/// How long the probe may take. Making a display and a surface is
/// milliseconds; this is for a driver that never answers.
pub const EGL_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// A plugin whose editor needs an EGL window config with 8 bits of alpha on
/// the window it is given, by every name it is known by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedsAlphaEgl {
    /// The name it shows, which a message says and a scan reports.
    pub name: &'static str,
    /// Its keys, as `PluginKey` writes them (`clap:…`, `vst3:…`).
    pub keys: &'static [&'static str],
}

/// The plugins known to need it, and why each is here. Extend it with a
/// plugin that aborts in `eglCreateWindowSurface` (or with bgfx's *"Failed
/// to create surface"*) on a machine whose probe says
/// [`AlphaEgl::Fails`].
pub const NEEDS_ALPHA_EGL: &[NeedsAlphaEgl] = &[
    // Vital 1.5 (Matt Tytel): bgfx over EGL, an RGBA8 config, its surface on
    // a depth-24 window. Every official build: the CLAP, the VST 3, the VST 2
    // (through a bridge) and the standalone app. Vitalium and Vial are JUCE
    // OpenGL over GLX and are not here — which is why the name is matched
    // whole.
    NeedsAlphaEgl {
        name: "Vital",
        keys: &[
            "clap:audio.vital.synth",
            "vst3:56535456697461766974616C00000000",
            "vst2:56697461",
        ],
    },
];

/// Whether the plugin `key`, called `name`, is in `table`: by key, or by
/// its whole name.
pub fn needs_alpha_egl(table: &[NeedsAlphaEgl], key: &PluginKey, name: &str) -> bool {
    let key = key.to_string();
    table
        .iter()
        .any(|entry| entry.name == name || entry.keys.contains(&key.as_str()))
}

/// What the probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlphaEgl {
    /// The surface was made: an editor like Vital's draws here.
    Works { vendor: String },
    /// It was not: such an editor aborts here. `why` is EGL's error.
    Fails { vendor: String, why: String },
    /// The probe itself died — a driver that crashes trying is one that
    /// would take the studio with it.
    Crashed(String),
    /// Nothing to go on: no X display, no `libEGL`, a probe that did not
    /// answer in time or could not be started. Not a reason to refuse.
    Unknown(String),
}

/// Reads the probe's answer: the last line of its standard output that is
/// JSON, and how it ended — `crashed` names the signal when it died of one.
pub fn read_egl_answer(stdout: &str, crashed: Option<&str>) -> AlphaEgl {
    let answer = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .and_then(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok());
    let text = |value: &serde_json::Value, field: &str| {
        value
            .get(field)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    if let Some(value) = &answer {
        if let Some(surface) = value.get("surface").and_then(|v| v.as_bool()) {
            let vendor = text(value, "vendor");
            return if surface {
                AlphaEgl::Works { vendor }
            } else {
                AlphaEgl::Fails {
                    vendor,
                    why: text(value, "why"),
                }
            };
        }
        if let Some(why) = value.get("unknown").and_then(|v| v.as_str()) {
            return AlphaEgl::Unknown(why.to_string());
        }
    }
    match crashed {
        Some(how) => AlphaEgl::Crashed(how.to_string()),
        None => AlphaEgl::Unknown("the probe's answer could not be read".to_string()),
    }
}

/// The driver as a person knows it: NVIDIA's EGL calls itself "NVIDIA".
fn driver(vendor: &str) -> &str {
    if vendor.to_ascii_lowercase().contains("nvidia") {
        "NVIDIA"
    } else if vendor.trim().is_empty() {
        "EGL"
    } else {
        vendor.trim()
    }
}

/// Whether to refuse to open the editor of `name`, and the sentence that
/// says why. `needs` is [`needs_alpha_egl`]'s answer; `compatible` whether
/// Compatible plugin graphics is in force in this process; `probe` asks
/// the machine, and is not called for a plugin that does not need it.
///
/// Refused when the probe says the surface cannot be made, or died trying.
/// An unknown answer opens the editor, as before this check existed.
pub fn editor_refusal(
    name: &str,
    needs: bool,
    compatible: bool,
    probe: impl FnOnce() -> AlphaEgl,
) -> Option<String> {
    if !needs {
        return None;
    }
    let vendor = match probe() {
        AlphaEgl::Works { .. } | AlphaEgl::Unknown(_) => return None,
        AlphaEgl::Fails { vendor, .. } => vendor,
        AlphaEgl::Crashed(_) => String::new(),
    };
    let driver = driver(&vendor);
    Some(if compatible {
        format!(
            "{name}'s window can't open with this graphics driver ({driver} on X11), \
             even with Compatible plugin graphics on. Its knobs are in Fontelle's panel."
        )
    } else {
        format!(
            "{name}'s window can't open with this graphics driver ({driver} on X11). \
             Its knobs are in Fontelle's panel. Turn on Settings \u{2192} Compatible \
             plugin graphics and restart to use its window."
        )
    })
}

type Probe = Box<dyn Fn() -> AlphaEgl + Send + Sync>;

/// What a rack asks before it opens a plugin's own editor: the table, the
/// probe, and its answer, kept for the session once asked.
pub struct EditorGate {
    table: Vec<NeedsAlphaEgl>,
    compatible: bool,
    probe: Option<Probe>,
    answer: Mutex<Option<AlphaEgl>>,
    probes: AtomicUsize,
}

impl Default for EditorGate {
    fn default() -> Self {
        Self::off()
    }
}

impl std::fmt::Debug for EditorGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorGate")
            .field("table", &self.table)
            .field("compatible", &self.compatible)
            .field("probing", &self.probe.is_some())
            .finish()
    }
}

impl EditorGate {
    /// Refuses nothing: a rack with no studio (a bounce, a test) and every
    /// platform but Linux.
    pub fn off() -> Self {
        Self {
            table: Vec::new(),
            compatible: false,
            probe: None,
            answer: Mutex::new(None),
            probes: AtomicUsize::new(0),
        }
    }

    /// The studio's: [`NEEDS_ALPHA_EGL`], asked of `helper` run with
    /// [`EGL_PROBE_FLAG`]. Off except on Linux, where plugin windows are X11.
    pub fn probing(helper: PathBuf, compatible: bool) -> Self {
        if !cfg!(target_os = "linux") {
            return Self::off();
        }
        Self::with(NEEDS_ALPHA_EGL.to_vec(), compatible, move || {
            run_egl_probe(egl_probe_command(&helper), EGL_PROBE_TIMEOUT)
        })
    }

    /// A gate with its own table and probe — a test's.
    pub fn with(
        table: Vec<NeedsAlphaEgl>,
        compatible: bool,
        probe: impl Fn() -> AlphaEgl + Send + Sync + 'static,
    ) -> Self {
        Self {
            table,
            compatible,
            probe: Some(Box::new(probe)),
            answer: Mutex::new(None),
            probes: AtomicUsize::new(0),
        }
    }

    /// [`editor_refusal`] for this plugin, the probe asked at most once.
    pub fn refusal(&self, key: &PluginKey, name: &str) -> Option<String> {
        let probe = self.probe.as_ref()?;
        let needs = needs_alpha_egl(&self.table, key, name);
        editor_refusal(name, needs, self.compatible, || {
            let mut answer = self.answer.lock().unwrap_or_else(|e| e.into_inner());
            answer
                .get_or_insert_with(|| {
                    self.probes.fetch_add(1, Ordering::Relaxed);
                    let found = probe();
                    eprintln!("Fontelle: plugin windows that need EGL alpha: {found:?}");
                    found
                })
                .clone()
        })
    }

    /// How many times the probe has been asked.
    pub fn probes(&self) -> usize {
        self.probes.load(Ordering::Relaxed)
    }
}

/// The command that runs `helper` as the probe.
pub fn egl_probe_command(helper: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(helper);
    command
        .arg(EGL_PROBE_FLAG)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        // A driver's own complaints are the driver's; the answer is stdout.
        .stderr(std::process::Stdio::null());
    command
}

/// Runs the probe `command` ([`egl_probe_command`]) and reads its answer. A
/// probe still running at `timeout` is asked to stop (SIGTERM) and given a
/// moment before it is killed: it holds a GPU driver's display, and a driver
/// killed in the middle of something can be left wedged.
pub fn run_egl_probe(mut command: std::process::Command, timeout: Duration) -> AlphaEgl {
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return AlphaEgl::Unknown(format!("the probe could not be started: {e}")),
    };
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(out) = stdout.as_mut() {
            use std::io::Read;
            let _ = out.read_to_string(&mut text);
        }
        text
    });
    let started = std::time::Instant::now();
    let mut asked_to_stop = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(e) => return AlphaEgl::Unknown(format!("the probe was lost: {e}")),
        }
        match asked_to_stop {
            None if started.elapsed() >= timeout => {
                terminate(&child);
                asked_to_stop = Some(std::time::Instant::now());
            }
            Some(at) if at.elapsed() >= Duration::from_secs(3) => {
                // It would not go when asked: the last resort.
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let text = reader.join().unwrap_or_default();
    if asked_to_stop.is_some() {
        return AlphaEgl::Unknown(format!(
            "the probe did not answer within {} seconds",
            timeout.as_secs()
        ));
    }
    let crashed = status.as_ref().and_then(crate::probe::crashed_by);
    read_egl_answer(&text, crashed.as_deref())
}

/// Asks `child` to stop: SIGTERM, which a driver's exit path can tidy up
/// after, where SIGKILL leaves whatever it was doing half done.
fn terminate(child: &std::process::Child) {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        const SIGTERM: i32 = 15;
        // SAFETY: a plain syscall on our own child's pid, still unreaped.
        unsafe { kill(child.id() as i32, SIGTERM) };
    }
    #[cfg(not(unix))]
    let _ = child;
}

/// The child's half: with [`EGL_PROBE_FLAG`] in `args` (everything after
/// the program's name), tries the surface, prints one line of JSON and
/// exits; `None` when `args` are not a probe's.
pub fn egl_probe_main(args: &[String]) -> Option<i32> {
    if !args.iter().any(|arg| arg == EGL_PROBE_FLAG) {
        return None;
    }
    let answer = probe_here();
    let line = match &answer {
        AlphaEgl::Works { vendor } => serde_json::json!({ "surface": true, "vendor": vendor }),
        AlphaEgl::Fails { vendor, why } => {
            serde_json::json!({ "surface": false, "vendor": vendor, "why": why })
        }
        AlphaEgl::Crashed(why) | AlphaEgl::Unknown(why) => serde_json::json!({ "unknown": why }),
    };
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
    drop(out);
    // Out at once, as the scan's child does: a driver's exit handlers are
    // not ours to sit through.
    crate::exit_now(0)
}

#[cfg(not(target_os = "linux"))]
fn probe_here() -> AlphaEgl {
    AlphaEgl::Unknown("plugin windows are not X11 here".to_string())
}

#[cfg(target_os = "linux")]
fn probe_here() -> AlphaEgl {
    if std::env::var_os("DISPLAY").is_none() {
        return AlphaEgl::Unknown("no X display".to_string());
    }
    // SAFETY: the libraries are the system's, loaded by their sonames, and
    // every call below is made as their headers declare it.
    match unsafe { x11::surface_as_bgfx_makes_it() } {
        Ok(answer) | Err(answer) => answer,
    }
}

/// bgfx's surface, made the way bgfx makes it, through `libX11` and `libEGL`
/// loaded at run time — nothing the studio links.
#[cfg(target_os = "linux")]
mod x11 {
    use std::ffi::{CStr, c_char, c_int, c_uint, c_ulong, c_void};

    use super::AlphaEgl;

    type Display = c_void;
    type EglDisplay = *mut c_void;
    type EglConfig = *mut c_void;
    type EglSurface = *mut c_void;
    type EglInt = i32;
    type EglBoolean = c_uint;

    const EGL_ALPHA_SIZE: EglInt = 0x3021;
    const EGL_BLUE_SIZE: EglInt = 0x3022;
    const EGL_GREEN_SIZE: EglInt = 0x3023;
    const EGL_RED_SIZE: EglInt = 0x3024;
    const EGL_DEPTH_SIZE: EglInt = 0x3025;
    const EGL_STENCIL_SIZE: EglInt = 0x3026;
    const EGL_SURFACE_TYPE: EglInt = 0x3033;
    const EGL_NONE: EglInt = 0x3038;
    const EGL_RENDERABLE_TYPE: EglInt = 0x3040;
    const EGL_VENDOR: EglInt = 0x3053;
    const EGL_WINDOW_BIT: EglInt = 0x0004;
    const EGL_OPENGL_ES2_BIT: EglInt = 0x0004;
    const EGL_OPENGL_BIT: EglInt = 0x0008;

    fn error_name(code: EglInt) -> String {
        match code {
            0x3000 => "EGL_SUCCESS",
            0x3001 => "EGL_NOT_INITIALIZED",
            0x3002 => "EGL_BAD_ACCESS",
            0x3003 => "EGL_BAD_ALLOC",
            0x3004 => "EGL_BAD_ATTRIBUTE",
            0x3005 => "EGL_BAD_CONFIG",
            0x3006 => "EGL_BAD_CONTEXT",
            0x3007 => "EGL_BAD_CURRENT_SURFACE",
            0x3008 => "EGL_BAD_DISPLAY",
            0x3009 => "EGL_BAD_MATCH",
            0x300A => "EGL_BAD_NATIVE_PIXMAP",
            0x300B => "EGL_BAD_NATIVE_WINDOW",
            0x300C => "EGL_BAD_PARAMETER",
            0x300D => "EGL_BAD_SURFACE",
            0x300E => "EGL_CONTEXT_LOST",
            other => return format!("EGL error 0x{other:04X}"),
        }
        .to_string()
    }

    /// An X error is noted and passed over: Xlib's own handler would end the
    /// probe with no answer, and a refused surface may well come with one.
    unsafe extern "C" fn ignore_x_error(_display: *mut Display, _event: *mut c_void) -> c_int {
        0
    }

    macro_rules! symbol {
        ($library:expr, $name:literal, $ty:ty) => {
            *$library
                .get::<$ty>(concat!($name, "\0").as_bytes())
                .map_err(|_| AlphaEgl::Unknown(format!("{} is missing", $name)))?
        };
    }

    /// `Ok` and `Err` both carry the answer; `Err` is for leaving early.
    pub(super) unsafe fn surface_as_bgfx_makes_it() -> Result<AlphaEgl, AlphaEgl> {
        unsafe {
            let xlib = libloading::Library::new("libX11.so.6")
                .map_err(|_| AlphaEgl::Unknown("no libX11".to_string()))?;
            let egl = libloading::Library::new("libEGL.so.1")
                .map_err(|_| AlphaEgl::Unknown("no libEGL".to_string()))?;

            let x_open_display = symbol!(
                xlib,
                "XOpenDisplay",
                unsafe extern "C" fn(*const c_char) -> *mut Display
            );
            let x_set_error_handler = symbol!(
                xlib,
                "XSetErrorHandler",
                unsafe extern "C" fn(
                    Option<unsafe extern "C" fn(*mut Display, *mut c_void) -> c_int>,
                ) -> *mut c_void
            );
            let x_default_root_window = symbol!(
                xlib,
                "XDefaultRootWindow",
                unsafe extern "C" fn(*mut Display) -> c_ulong
            );
            let x_create_simple_window = symbol!(
                xlib,
                "XCreateSimpleWindow",
                unsafe extern "C" fn(
                    *mut Display,
                    c_ulong,
                    c_int,
                    c_int,
                    c_uint,
                    c_uint,
                    c_uint,
                    c_ulong,
                    c_ulong,
                ) -> c_ulong
            );
            let x_destroy_window = symbol!(
                xlib,
                "XDestroyWindow",
                unsafe extern "C" fn(*mut Display, c_ulong) -> c_int
            );
            let x_sync = symbol!(
                xlib,
                "XSync",
                unsafe extern "C" fn(*mut Display, c_int) -> c_int
            );
            let x_close_display = symbol!(
                xlib,
                "XCloseDisplay",
                unsafe extern "C" fn(*mut Display) -> c_int
            );
            let egl_get_display = symbol!(
                egl,
                "eglGetDisplay",
                unsafe extern "C" fn(*mut c_void) -> EglDisplay
            );
            let egl_initialize = symbol!(
                egl,
                "eglInitialize",
                unsafe extern "C" fn(EglDisplay, *mut EglInt, *mut EglInt) -> EglBoolean
            );
            let egl_query_string = symbol!(
                egl,
                "eglQueryString",
                unsafe extern "C" fn(EglDisplay, EglInt) -> *const c_char
            );
            let egl_choose_config = symbol!(
                egl,
                "eglChooseConfig",
                unsafe extern "C" fn(
                    EglDisplay,
                    *const EglInt,
                    *mut EglConfig,
                    EglInt,
                    *mut EglInt,
                ) -> EglBoolean
            );
            let egl_create_window_surface = symbol!(
                egl,
                "eglCreateWindowSurface",
                unsafe extern "C" fn(EglDisplay, EglConfig, c_ulong, *const EglInt) -> EglSurface
            );
            let egl_destroy_surface = symbol!(
                egl,
                "eglDestroySurface",
                unsafe extern "C" fn(EglDisplay, EglSurface) -> EglBoolean
            );
            let egl_get_error = symbol!(egl, "eglGetError", unsafe extern "C" fn() -> EglInt);
            let egl_terminate = symbol!(
                egl,
                "eglTerminate",
                unsafe extern "C" fn(EglDisplay) -> EglBoolean
            );

            let display = x_open_display(std::ptr::null());
            if display.is_null() {
                return Err(AlphaEgl::Unknown(
                    "the X display would not open".to_string(),
                ));
            }
            x_set_error_handler(Some(ignore_x_error));
            // A window of the root's own visual and depth, never mapped:
            // what Vital draws into, without putting anything on the screen.
            let root = x_default_root_window(display);
            let window = x_create_simple_window(display, root, 0, 0, 16, 16, 0, 0, 0);
            x_sync(display, 0);

            let answer = (|| {
                let egl_display = egl_get_display(display);
                if egl_display.is_null() {
                    return AlphaEgl::Fails {
                        vendor: String::new(),
                        why: "no EGL display for X11".to_string(),
                    };
                }
                let (mut major, mut minor) = (0, 0);
                if egl_initialize(egl_display, &mut major, &mut minor) == 0 {
                    return AlphaEgl::Fails {
                        vendor: String::new(),
                        why: format!("eglInitialize: {}", error_name(egl_get_error())),
                    };
                }
                let vendor = {
                    let text = egl_query_string(egl_display, EGL_VENDOR);
                    if text.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(text).to_string_lossy().into_owned()
                    }
                };
                // bgfx's attributes: desktop GL when it is built for it, ES 2
                // otherwise — the first that has any config at all — and the
                // first config it is given.
                let mut config: EglConfig = std::ptr::null_mut();
                let mut found = 0;
                for renderable in [EGL_OPENGL_BIT, EGL_OPENGL_ES2_BIT] {
                    let attributes = [
                        EGL_RENDERABLE_TYPE,
                        renderable,
                        EGL_SURFACE_TYPE,
                        EGL_WINDOW_BIT,
                        EGL_BLUE_SIZE,
                        8,
                        EGL_GREEN_SIZE,
                        8,
                        EGL_RED_SIZE,
                        8,
                        EGL_ALPHA_SIZE,
                        8,
                        EGL_DEPTH_SIZE,
                        24,
                        EGL_STENCIL_SIZE,
                        8,
                        EGL_NONE,
                    ];
                    if egl_choose_config(
                        egl_display,
                        attributes.as_ptr(),
                        &mut config,
                        1,
                        &mut found,
                    ) != 0
                        && found > 0
                    {
                        break;
                    }
                    found = 0;
                }
                let answer = if found == 0 {
                    AlphaEgl::Fails {
                        vendor,
                        why: "no 8-bit RGBA window config".to_string(),
                    }
                } else {
                    let surface =
                        egl_create_window_surface(egl_display, config, window, std::ptr::null());
                    if surface.is_null() {
                        AlphaEgl::Fails {
                            vendor,
                            why: error_name(egl_get_error()),
                        }
                    } else {
                        egl_destroy_surface(egl_display, surface);
                        AlphaEgl::Works { vendor }
                    }
                };
                egl_terminate(egl_display);
                answer
            })();

            x_destroy_window(display, window);
            x_sync(display, 0);
            x_close_display(display);
            // The libraries stay loaded: the process ends at once anyway, and
            // a driver unloaded under its own threads is a crash on the way.
            std::mem::forget(egl);
            std::mem::forget(xlib);
            Ok(answer)
        }
    }
}
