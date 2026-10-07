//! An ABI 2 singer engine's **own window**, hosted.
//!
//! The engine draws into a window Fontelle makes and owns — a
//! [`PluginWindow`](crate::PluginWindow), the same one a plugin's editor is
//! embedded into — and is driven like one: [`SingerEditor::tick`] once a
//! frame, from the loop that already runs. It reads the part it edits
//! through `HostCallbacks.get_part` and asks for edits through
//! `apply_edits`; this side **queues** them rather than applying them,
//! because the callbacks run inside `editor_tick`, while the app is in the
//! middle of calling it. The app applies each queued batch after the tick
//! as one command (one undo step), and hands the part back with
//! [`set_part`](SingerEditor::set_part), which tells the engine it changed.
//!
//! An edit batch is checked against the part the engine was last shown —
//! every note it names must be in it, every number a number — and refused
//! whole (`apply_edits` answers false) when it is not, as the ABI says.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Arc;

use fontelle_sing::{PartEdit, PartView, Transport};
use fontelle_singer_abi::{EngineInstance, HostCallbacks, PhraseResult};

use crate::singer::SingerEngine;

/// A take the window may read (`HostCallbacks.take_at`): one sung phrase,
/// placed in the song.
#[derive(Debug, Clone)]
pub struct EditorTake {
    /// Song seconds of the phrase's first note and last note's end.
    pub start: f64,
    pub end: f64,
    pub sample_rate: u32,
    /// Seconds from the phrase's first note to the first sample.
    pub take_start: f64,
    pub samples: Arc<[f32]>,
    /// Hz every `pitch_step` from `take_start`.
    pub pitch: Arc<[f32]>,
    pub pitch_step: f64,
    /// What each note was sung as (indices into the phrase's notes).
    pub phonemes: Vec<fontelle_sing::SungNote>,
}

/// A take's phonemes in the ABI's report form, `offsets` included.
fn phonemes_json(notes: &[fontelle_sing::SungNote]) -> CString {
    let report: Vec<serde_json::Value> = notes
        .iter()
        .map(|n| {
            serde_json::json!({
                "note": n.note,
                "phonemes": n.phonemes.iter().map(|p| p.symbol.as_str()).collect::<Vec<_>>(),
                "offsets": n.phonemes.iter().map(|p| p.offset).collect::<Vec<_>>(),
                "guessed": n.guessed,
            })
        })
        .collect();
    CString::new(serde_json::Value::Array(report).to_string()).unwrap_or_default()
}

/// What the window asked for during one tick.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SingerEditorRequests {
    /// Each batch is one undo step, in the order asked.
    pub edits: Vec<Vec<PartEdit>>,
    pub transport: Vec<Transport>,
}

/// The callbacks' side: what the engine is shown and what it asked.
#[derive(Default)]
struct Link {
    part: RefCell<Option<CString>>,
    ids: RefCell<HashSet<u64>>,
    asked: RefCell<SingerEditorRequests>,
    takes: RefCell<Vec<(EditorTake, CString)>>,
}

/// Whether every note `edits` names is one of `ids` and every number in
/// them is a number — what `apply_edits` checks before taking a batch.
pub fn part_edits_valid(edits: &[PartEdit], ids: &HashSet<u64>) -> bool {
    let known = |id: &u64| ids.contains(id);
    let finite_curve = |c: &Option<fontelle_sing::Curve>| {
        c.as_ref().is_none_or(|c| {
            c.start.is_finite()
                && c.step.is_finite()
                && c.step > 0.0
                && c.values.iter().all(|v| v.is_finite())
        })
    };
    edits.iter().all(|edit| match edit {
        PartEdit::Settings(s) => s.values().all(|v| v.is_finite()),
        PartEdit::State(_) => true,
        PartEdit::Curve { curve, .. } | PartEdit::ExtraCurve { curve, .. } => finite_curve(curve),
        PartEdit::Lyrics(l) => l.iter().all(|(id, _)| known(id)),
        PartEdit::Phonemes { note, phonemes } => {
            known(note)
                && phonemes
                    .iter()
                    .flatten()
                    .all(|p| p.offset.is_none_or(f64::is_finite))
        }
        PartEdit::Velocity { notes, velocity } => notes.iter().all(known) && velocity.is_finite(),
        PartEdit::Vibrato { notes, vibrato } => {
            notes.iter().all(known)
                && vibrato.is_none_or(|v| {
                    [v.depth, v.rate, v.delay, v.fade_in, v.fade_out]
                        .iter()
                        .all(|x| x.is_finite())
                })
        }
        PartEdit::Portamento { notes, portamento } => {
            notes.iter().all(known)
                && portamento.is_none_or(|p| p.length.is_finite() && p.position.is_finite())
        }
        PartEdit::NoteOptions { notes, .. } => notes.iter().all(known),
        PartEdit::Select(notes) => notes.iter().all(known),
    })
}

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    // Nothing may unwind into the engine's frames.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

fn link<'a>(context: *mut c_void) -> &'a Link {
    // SAFETY: `context` is the `Link` boxed in the `SingerEditor`, alive from
    // `editor_open` to `editor_close`, the only span the engine may call in.
    unsafe { &*(context as *const Link) }
}

unsafe extern "C" fn get_part(context: *mut c_void) -> *mut c_char {
    guard(std::ptr::null_mut(), || {
        link(context)
            .part
            .borrow()
            .clone()
            .map_or(std::ptr::null_mut(), CString::into_raw)
    })
}

unsafe extern "C" fn free_string(_context: *mut c_void, string: *mut c_char) {
    if !string.is_null() {
        // SAFETY: every string handed out is a `CString::into_raw`.
        drop(unsafe { CString::from_raw(string) });
    }
}

unsafe extern "C" fn apply_edits(context: *mut c_void, json: *const c_char) -> bool {
    guard(false, || {
        if json.is_null() {
            return false;
        }
        let text = unsafe { CStr::from_ptr(json) }.to_string_lossy();
        let Ok(edits) = serde_json::from_str::<Vec<PartEdit>>(&text) else {
            return false;
        };
        let link = link(context);
        if edits.is_empty() || !part_edits_valid(&edits, &link.ids.borrow()) {
            return false;
        }
        link.asked.borrow_mut().edits.push(edits);
        true
    })
}

unsafe extern "C" fn transport(context: *mut c_void, json: *const c_char) {
    guard((), || {
        if json.is_null() {
            return;
        }
        let text = unsafe { CStr::from_ptr(json) }.to_string_lossy();
        if let Ok(t) = serde_json::from_str::<Transport>(&text) {
            link(context).asked.borrow_mut().transport.push(t);
        }
    })
}

unsafe extern "C" fn take_at(context: *mut c_void, seconds: f64, take: *mut PhraseResult) -> bool {
    guard(false, || {
        if take.is_null() {
            return false;
        }
        let takes = link(context).takes.borrow();
        let Some((found, phonemes)) = takes
            .iter()
            .find(|(t, _)| seconds >= t.start && seconds < t.end)
        else {
            return false;
        };
        // Pointers into buffers the take keeps alive until the next
        // `set_takes`, which is never inside a tick; the engine must not
        // free them (the ABI says so).
        unsafe {
            *take = PhraseResult {
                sample_rate: found.sample_rate,
                samples: found.samples.as_ptr() as *mut f32,
                frame_count: found.samples.len() as u64,
                start: found.take_start,
                pitch: found.pitch.as_ptr() as *mut f32,
                pitch_count: found.pitch.len() as u64,
                pitch_step: found.pitch_step,
                phonemes: phonemes.as_ptr() as *mut c_char,
            };
        }
        true
    })
}

/// An engine's window, open. Main thread only.
pub struct SingerEditor {
    engine: Arc<SingerEngine>,
    instance: *mut EngineInstance,
    link: Box<Link>,
    callbacks: Box<HostCallbacks>,
    size: (u32, u32),
}

impl std::fmt::Debug for SingerEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingerEditor")
            .field("engine", &self.engine.name())
            .field("size", &self.size)
            .finish()
    }
}

impl SingerEditor {
    /// Opens `engine`'s window inside `parent` (an X11 window id on Linux,
    /// an `HWND` on Windows, an `NSView*` on macOS; 0 for none, which an
    /// engine must accept and simply not draw). An instance of its own, as
    /// the ABI says: never one a render worker uses.
    pub fn open(engine: &Arc<SingerEngine>, parent: u64) -> Result<Self, String> {
        let Some(v2) = engine.editor_table() else {
            return Err(format!("{} has no window of its own", engine.name()));
        };
        let instance = engine.raw_instance()?;
        let link = Box::new(Link::default());
        let callbacks = Box::new(HostCallbacks {
            context: &*link as *const Link as *mut c_void,
            get_part,
            apply_edits,
            transport,
            take_at,
            free_string,
        });
        let (mut width, mut height) = (0u32, 0u32);
        // SAFETY: the instance is fresh; the callbacks and their context are
        // boxed and outlive `editor_close`.
        let opened =
            unsafe { (v2.editor_open)(instance, parent, &*callbacks, &mut width, &mut height) };
        if !opened {
            engine.destroy_raw(instance);
            return Err(format!("{} could not open its window", engine.name()));
        }
        let size = (
            width.clamp(200, 4096).max(1),
            height.clamp(150, 4096).max(1),
        );
        Ok(Self {
            engine: Arc::clone(engine),
            instance,
            link,
            callbacks,
            size,
        })
    }

    pub fn engine(&self) -> &Arc<SingerEngine> {
        &self.engine
    }

    /// The size the window asked for.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Shows the window `view`; when it differs from what it was shown last,
    /// tells it the part changed (an edit, a take, the selection, the
    /// playhead).
    pub fn set_part(&mut self, view: &PartView) {
        let Ok(json) = serde_json::to_string(view) else {
            return;
        };
        let json = CString::new(json).unwrap_or_default();
        if self.link.part.borrow().as_ref() == Some(&json) {
            return;
        }
        *self.link.part.borrow_mut() = Some(json);
        *self.link.ids.borrow_mut() = view.notes.iter().map(|n| n.id).collect();
        if let Some(v2) = self.engine.editor_table() {
            // SAFETY: open until `Drop`.
            unsafe { (v2.editor_part_changed)(self.instance) };
        }
    }

    /// The takes `take_at` answers from.
    pub fn set_takes(&mut self, takes: Vec<EditorTake>) {
        *self.link.takes.borrow_mut() = takes
            .into_iter()
            .map(|t| {
                let json = phonemes_json(&t.phonemes);
                (t, json)
            })
            .collect();
    }

    /// One frame of the window: it draws and handles its events, and what it
    /// asked for meanwhile comes back.
    pub fn tick(&mut self) -> SingerEditorRequests {
        if let Some(v2) = self.engine.editor_table() {
            // SAFETY: open until `Drop`; main thread.
            unsafe { (v2.editor_tick)(self.instance) };
        }
        std::mem::take(&mut *self.link.asked.borrow_mut())
    }

    /// The host's window was resized.
    pub fn resized(&mut self, width: u32, height: u32) {
        if let Some(v2) = self.engine.editor_table() {
            unsafe { (v2.editor_resized)(self.instance, width, height) };
        }
    }
}

impl Drop for SingerEditor {
    fn drop(&mut self) {
        if let Some(v2) = self.engine.editor_table() {
            // SAFETY: opened in `open`, closed once.
            unsafe { (v2.editor_close)(self.instance) };
        }
        self.engine.destroy_raw(self.instance);
        // The callbacks go after the engine has let go of them.
        let _ = &self.callbacks;
    }
}
