//! Which plugin this thread is inside, for a crash report to name.
//!
//! > *"sometimes they'll just revert back to the init preset"* — and the
//! > sweep that went looking found five plugins that crash in their own
//! > code: Calf Wavetable, JuceOPL, Odin2, padthv1, sfizz.
//!
//! Plugins run in the studio's own process, so a plugin that crashes takes
//! the studio with it, and the report the crash handler writes
//! (`fontelle_app::crashlog`) says which *library* the faulting instruction
//! was in. That is not always the plugin: padthv1 calls a pure virtual
//! function and `libstdc++` aborts, so the fault is in `libc`. What does know
//! is the thread: the host marks each call it makes into a plugin, on the
//! thread that makes it, and the handler, running on the faulting thread,
//! reads the mark.
//!
//! A plugin's **own** threads are never marked — the host does not run
//! them. padthv1's scheduler thread crashed while the main thread waited in
//! padthv1's destructor; for that, calls made on the main thread
//! ([`calling_main`]) also leave a process-wide mark ([`main`]), which the
//! handler falls back on when the faulting thread has none of its own.
//!
//! **Async-signal-safe to read.** [`current`] and [`main`] read a pointer to
//! a [`Label`]'s bytes, which are made once per plugin and never freed. No
//! lock, no allocation, nothing a fault could be holding.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

use fontelle_types::PluginKey;

/// What a mark points at. Leaked: a handler may read it at any moment.
struct Bytes(Box<[u8]>);

thread_local! {
    /// The plugin this thread is calling into, or null.
    static CALLING: Cell<*const Bytes> = const { Cell::new(std::ptr::null()) };
}

/// The plugin the main thread is calling into, or null — see the module
/// note.
static MAIN: AtomicPtr<Bytes> = AtomicPtr::new(std::ptr::null_mut());

/// Every label made, by key, so a plugin opened a hundred times leaks one.
static MADE: Mutex<Option<HashMap<PluginKey, Label>>> = Mutex::new(None);

/// A plugin's name and key, as a crash report says them: `name\tkey`, the
/// key as `PluginKey`'s `Display` writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label(&'static Bytes);

impl std::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.0))
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Bytes {}

impl Label {
    /// The label for the plugin with `key`, made the first time it is asked
    /// for and called `name` then. Control characters in the name are
    /// flattened, so the label stays one field of one line.
    pub fn new(name: &str, key: &PluginKey) -> Self {
        let mut made = MADE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *made
            .get_or_insert_with(HashMap::new)
            .entry(key.clone())
            .or_insert_with(|| {
                let flat: String = name
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                let bytes = format!("{flat}\t{key}").into_bytes().into_boxed_slice();
                Label(Box::leak(Box::new(Bytes(bytes))))
            })
    }

    pub fn bytes(&self) -> &'static [u8] {
        &self.0.0
    }

    fn ptr(self) -> *const Bytes {
        self.0
    }
}

/// Marks the calls that follow, on this thread, as made into a plugin, until
/// it is dropped. Nested marks put the outer one back.
#[must_use = "the mark lasts as long as the guard"]
pub struct Calling {
    before: *const Bytes,
    before_main: Option<*mut Bytes>,
}

/// **RT.** Marks this thread's calls as made into `label` — the audio
/// thread's way.
pub fn calling(label: Label) -> Calling {
    let before = CALLING.with(|cell| cell.replace(label.ptr()));
    Calling {
        before,
        before_main: None,
    }
}

/// [`calling`], and the process-wide mark as well — for calls made on the
/// main thread, where a plugin's own threads wait on what the main thread is
/// doing to it.
pub fn calling_main(label: Label) -> Calling {
    let mut calling = calling(label);
    calling.before_main = Some(MAIN.swap(label.ptr().cast_mut(), Ordering::AcqRel));
    calling
}

impl Drop for Calling {
    fn drop(&mut self) {
        CALLING.with(|cell| cell.set(self.before));
        if let Some(before) = self.before_main {
            MAIN.store(before, Ordering::Release);
        }
    }
}

/// The plugin this thread is inside, as `name\tkey` bytes.
///
/// **Async-signal-safe**: a crash handler calls this on the faulting thread.
pub fn current() -> Option<&'static [u8]> {
    let ptr = CALLING.with(Cell::get);
    // SAFETY: a mark is only ever a `Label`'s bytes, leaked and so valid for
    // the rest of the process.
    unsafe { ptr.as_ref() }.map(|bytes| &*bytes.0)
}

/// The plugin the main thread is inside, as `name\tkey` bytes —
/// async-signal-safe, like [`current`].
pub fn main() -> Option<&'static [u8]> {
    // SAFETY: see `current`.
    unsafe { MAIN.load(Ordering::Acquire).as_ref() }.map(|bytes| &*bytes.0)
}

/// The **first** field of a struct that owns a plugin: dropped before the
/// fields after it, it marks the thread as inside the plugin while they go —
/// a plugin's teardown is a call into it like any other (padthv1's crashed
/// there). [`DropUnmark`], the struct's **last** field, ends the mark.
pub struct DropMark(pub Option<Label>);

impl Drop for DropMark {
    fn drop(&mut self) {
        if let Some(label) = self.0 {
            // Ended by `DropUnmark`, not by a guard's own drop.
            std::mem::forget(calling_main(label));
        }
    }
}

/// The last field of a struct whose first is a [`DropMark`]: ends its mark.
pub struct DropUnmark(pub bool);

impl Drop for DropUnmark {
    fn drop(&mut self) {
        if self.0 {
            CALLING.with(|cell| cell.set(std::ptr::null()));
            MAIN.store(std::ptr::null_mut(), Ordering::Release);
        }
    }
}
