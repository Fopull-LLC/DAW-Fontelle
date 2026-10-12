//! The operating system's half: the shared block and the doorbells.
//!
//! Fontelle makes everything ([`Region::create`], [`Bells::create`]) under a
//! name from [`channel_name`] and hands the name to the helper on its
//! command line; the helper opens the same names. On Windows the block is a
//! named file mapping backed by the page file and each doorbell a named
//! auto-reset event (`<name>-request`, `<name>-reply`), with a
//! high-resolution waitable timer for timeouts shorter than the scheduler's
//! tick; on Linux the block is a POSIX shared-memory object and each
//! doorbell a futex on the shared word itself. Other platforms get the
//! [`PollBell`] and no named block, enough for tests on the heap.

use std::ptr::NonNull;
use std::sync::atomic::AtomicU32;
use std::time::{Duration, Instant};

use crate::handshake::Doorbell;
use crate::layout::{Layout, LayoutError, Shared};

/// The base name of one exchange: unique per process and per `n`.
pub fn channel_name(pid: u32, n: u32) -> String {
    if cfg!(windows) {
        format!("Local\\fontelle-audio-{pid}-{n}")
    } else {
        format!("/fontelle-audio-{pid}-{n}")
    }
}

/// A mapping of a shared block, unmapped (and, by its maker, removed) on
/// drop.
pub struct Region {
    shared: Shared,
    inner: Inner,
}

// SAFETY: a `Region` is its mapping and a `Shared` view of it; the view is
// `Sync` (see `layout`), and the mapping is only unmapped on drop.
unsafe impl Send for Region {}
unsafe impl Sync for Region {}

enum Inner {
    Heap {
        ptr: NonNull<u8>,
        layout: std::alloc::Layout,
    },
    #[cfg(target_os = "linux")]
    Posix {
        ptr: NonNull<u8>,
        len: usize,
        /// The maker removes the name.
        unlink: Option<std::ffi::CString>,
    },
    #[cfg(windows)]
    Mapping {
        view: NonNull<u8>,
        handle: windows_sys::Win32::Foundation::HANDLE,
    },
}

impl Region {
    /// Makes the block `name` for `layout`. Fontelle's side.
    pub fn create(name: &str, layout: Layout) -> std::io::Result<Self> {
        imp::create(name, layout)
    }

    /// Opens the block `name` somebody else made. The helper's side.
    pub fn open(name: &str) -> std::io::Result<Self> {
        imp::open(name)
    }

    /// A block on this process's heap, for tests and for a helper that is a
    /// thread.
    pub fn heap(layout: Layout) -> Self {
        let alloc = std::alloc::Layout::from_size_align(layout.bytes(), 64)
            .expect("a layout's size is bounded");
        // SAFETY: a non-zero size (the header alone is 512 bytes).
        let ptr = unsafe { std::alloc::alloc_zeroed(alloc) };
        let ptr = NonNull::new(ptr).unwrap_or_else(|| std::alloc::handle_alloc_error(alloc));
        // SAFETY: freshly allocated, 64-aligned, `layout.bytes()` long, and
        // only freed when the region goes.
        let shared = unsafe { Shared::init(ptr, layout.bytes(), layout) }
            .expect("the allocation is the layout's size");
        Self {
            shared,
            inner: Inner::Heap { ptr, layout: alloc },
        }
    }

    pub fn shared(&self) -> &Shared {
        &self.shared
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        match &self.inner {
            // SAFETY: allocated in `heap` with this layout.
            Inner::Heap { ptr, layout } => unsafe { std::alloc::dealloc(ptr.as_ptr(), *layout) },
            #[cfg(target_os = "linux")]
            Inner::Posix { ptr, len, unlink } => imp::unmap(*ptr, *len, unlink.as_deref()),
            #[cfg(windows)]
            Inner::Mapping { view, handle } => imp::unmap(*view, *handle),
        }
    }
}

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
fn layout_error(e: LayoutError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

/// The two doorbells of one exchange: `request` is the one Fontelle sleeps
/// on (the helper rings it), `reply` the one the helper sleeps on.
pub struct Bells {
    pub request: Box<dyn Doorbell>,
    pub reply: Box<dyn Doorbell>,
}

impl Bells {
    /// Makes the bells of `channel`. Fontelle's side.
    pub fn create(channel: &str) -> std::io::Result<Self> {
        imp::bells(channel, true)
    }

    /// Opens the bells of `channel`. The helper's side.
    pub fn open(channel: &str) -> std::io::Result<Self> {
        imp::bells(channel, false)
    }
}

/// A doorbell that never sleeps in the kernel: it spins a little, then
/// naps. For platforms without a real one, and for tests.
#[derive(Debug, Default)]
pub struct PollBell;

impl Doorbell for PollBell {
    fn ring(&self, _word: &AtomicU32) {}

    fn sleep(&self, word: &AtomicU32, seen: u32, timeout: Duration) {
        use std::sync::atomic::Ordering;
        let until = Instant::now() + timeout;
        for _ in 0..256 {
            if word.load(Ordering::Acquire) != seen {
                return;
            }
            std::hint::spin_loop();
        }
        let now = Instant::now();
        if now < until {
            std::thread::sleep((until - now).min(Duration::from_micros(50)));
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::{CStr, CString};
    use std::io;
    use std::ptr::NonNull;
    use std::sync::atomic::AtomicU32;
    use std::time::Duration;

    use super::{Bells, Inner, Region, layout_error};
    use crate::handshake::Doorbell;
    use crate::layout::{Layout, Shared};

    fn c_name(name: &str) -> io::Result<CString> {
        CString::new(name).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
    }

    /// Maps `len` bytes of `fd`, closing it either way.
    fn map(fd: libc::c_int, len: usize) -> io::Result<NonNull<u8>> {
        // SAFETY: a fresh mapping of an open descriptor; the descriptor is
        // not needed once mapped.
        unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            let err = io::Error::last_os_error();
            libc::close(fd);
            if ptr == libc::MAP_FAILED {
                return Err(err);
            }
            Ok(NonNull::new_unchecked(ptr.cast()))
        }
    }

    pub fn unmap(ptr: NonNull<u8>, len: usize, unlink: Option<&CStr>) {
        // SAFETY: a mapping made here of this length, and its name.
        unsafe {
            libc::munmap(ptr.as_ptr().cast(), len);
            if let Some(name) = unlink {
                libc::shm_unlink(name.as_ptr());
            }
        }
    }

    pub fn create(name: &str, layout: Layout) -> io::Result<Region> {
        let c = c_name(name)?;
        let len = layout.bytes();
        // SAFETY: plain calls on a NUL-terminated name.
        let fd = unsafe {
            libc::shm_open(
                c.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: as above.
        if unsafe { libc::ftruncate(fd, len as libc::off_t) } != 0 {
            let err = io::Error::last_os_error();
            // SAFETY: as above.
            unsafe {
                libc::close(fd);
                libc::shm_unlink(c.as_ptr());
            }
            return Err(err);
        }
        let ptr = match map(fd, len) {
            Ok(ptr) => ptr,
            Err(e) => {
                // SAFETY: as above.
                unsafe { libc::shm_unlink(c.as_ptr()) };
                return Err(e);
            }
        };
        // SAFETY: a fresh page-aligned mapping of `len` bytes, ours.
        match unsafe { Shared::init(ptr, len, layout) } {
            Ok(shared) => Ok(Region {
                shared,
                inner: Inner::Posix {
                    ptr,
                    len,
                    unlink: Some(c),
                },
            }),
            Err(e) => {
                unmap(ptr, len, Some(&c));
                Err(layout_error(e))
            }
        }
    }

    pub fn open(name: &str) -> io::Result<Region> {
        let c = c_name(name)?;
        // SAFETY: plain calls on a NUL-terminated name and an owned fd.
        let (fd, len) = unsafe {
            let fd = libc::shm_open(c.as_ptr(), libc::O_RDWR, 0);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut stat: libc::stat = std::mem::zeroed();
            if libc::fstat(fd, &mut stat) != 0 {
                let err = io::Error::last_os_error();
                libc::close(fd);
                return Err(err);
            }
            (fd, stat.st_size as usize)
        };
        let ptr = map(fd, len)?;
        // SAFETY: a page-aligned shared mapping of the whole object.
        match unsafe { Shared::open(ptr, len) } {
            Ok(shared) => Ok(Region {
                shared,
                inner: Inner::Posix {
                    ptr,
                    len,
                    unlink: None,
                },
            }),
            Err(e) => {
                unmap(ptr, len, None);
                Err(layout_error(e))
            }
        }
    }

    /// A futex on the shared word itself. Not `FUTEX_PRIVATE_FLAG`: the
    /// word is in a mapping two processes share, which is exactly what the
    /// shared futex is keyed on.
    struct FutexBell;

    impl Doorbell for FutexBell {
        fn ring(&self, word: &AtomicU32) {
            // SAFETY: a futex wake on a live, aligned u32.
            unsafe {
                libc::syscall(
                    libc::SYS_futex,
                    word.as_ptr(),
                    libc::FUTEX_WAKE,
                    i32::MAX,
                    std::ptr::null::<libc::timespec>(),
                    std::ptr::null::<u32>(),
                    0,
                );
            }
        }

        fn sleep(&self, word: &AtomicU32, seen: u32, timeout: Duration) {
            let ts = libc::timespec {
                tv_sec: timeout.as_secs() as libc::time_t,
                tv_nsec: timeout.subsec_nanos() as libc::c_long,
            };
            // SAFETY: a futex wait on a live, aligned u32; the kernel
            // returns at once if it no longer holds `seen`.
            unsafe {
                libc::syscall(
                    libc::SYS_futex,
                    word.as_ptr(),
                    libc::FUTEX_WAIT,
                    seen,
                    &ts as *const libc::timespec,
                    std::ptr::null::<u32>(),
                    0,
                );
            }
        }
    }

    pub fn bells(_channel: &str, _create: bool) -> io::Result<Bells> {
        Ok(Bells {
            request: Box::new(FutexBell),
            reply: Box::new(FutexBell),
        })
    }
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_BASIC_INFORMATION,
        MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, PAGE_READWRITE,
        UnmapViewOfFile, VirtualQuery,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateEventW, CreateWaitableTimerExW,
        EVENT_MODIFY_STATE, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE, SetEvent, SetWaitableTimer,
        TIMER_ALL_ACCESS, WaitForMultipleObjects, WaitForSingleObject,
    };

    use super::{Bells, Inner, Region, layout_error};
    use crate::handshake::Doorbell;
    use crate::layout::{Layout, Shared};

    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn view(handle: HANDLE, len: usize) -> io::Result<NonNull<u8>> {
        // SAFETY: a view of a mapping handle we hold.
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, len) };
        match NonNull::new(view.Value.cast::<u8>()) {
            Some(ptr) => Ok(ptr),
            None => {
                let err = io::Error::last_os_error();
                // SAFETY: ours, and no view of it was made.
                unsafe { CloseHandle(handle) };
                Err(err)
            }
        }
    }

    pub fn unmap(ptr: NonNull<u8>, handle: HANDLE) {
        // SAFETY: a view and a handle made here and not yet released.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: ptr.as_ptr().cast(),
            });
            CloseHandle(handle);
        }
    }

    pub fn create(name: &str, layout: Layout) -> io::Result<Region> {
        let len = layout.bytes();
        let w = wide(name);
        // SAFETY: a page-file-backed mapping under a NUL-terminated name.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                (len as u64 >> 32) as u32,
                len as u32,
                w.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: straight after the call that set it.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: ours.
            unsafe { CloseHandle(handle) };
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{name} is already in use"),
            ));
        }
        let ptr = view(handle, len)?;
        // SAFETY: a fresh page-aligned view of `len` bytes, ours.
        match unsafe { Shared::init(ptr, len, layout) } {
            Ok(shared) => Ok(Region {
                shared,
                inner: Inner::Mapping { view: ptr, handle },
            }),
            Err(e) => {
                unmap(ptr, handle);
                Err(layout_error(e))
            }
        }
    }

    pub fn open(name: &str) -> io::Result<Region> {
        let w = wide(name);
        // SAFETY: a NUL-terminated name.
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, w.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let ptr = view(handle, 0)?;
        // The view's size: the mapping's, rounded up to a page.
        // SAFETY: a query of a view we hold into a zeroed struct.
        let len = unsafe {
            let mut info: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
            VirtualQuery(
                ptr.as_ptr().cast(),
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            );
            info.RegionSize
        };
        // SAFETY: a page-aligned view of `len` bytes.
        match unsafe { Shared::open(ptr, len) } {
            Ok(shared) => Ok(Region {
                shared,
                inner: Inner::Mapping { view: ptr, handle },
            }),
            Err(e) => {
                unmap(ptr, handle);
                Err(layout_error(e))
            }
        }
    }

    /// A named auto-reset event, and a high-resolution timer of this
    /// process's own for waits shorter than the scheduler's tick: a 64-frame
    /// buffer's deadline is about a millisecond, and `WaitForSingleObject`'s
    /// timeout is in whole milliseconds rounded to the tick.
    struct EventBell {
        event: HANDLE,
        timer: HANDLE,
    }

    // SAFETY: kernel handles, usable from any thread.
    unsafe impl Send for EventBell {}
    unsafe impl Sync for EventBell {}

    impl Drop for EventBell {
        fn drop(&mut self) {
            // SAFETY: ours.
            unsafe {
                CloseHandle(self.event);
                if !self.timer.is_null() {
                    CloseHandle(self.timer);
                }
            }
        }
    }

    impl EventBell {
        fn new(name: &str, create: bool) -> io::Result<Self> {
            let w = wide(name);
            // SAFETY: NUL-terminated names; auto-reset, not signalled.
            let event = unsafe {
                if create {
                    CreateEventW(std::ptr::null(), 0, 0, w.as_ptr())
                } else {
                    OpenEventW(
                        EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                        0,
                        w.as_ptr(),
                    )
                }
            };
            if event.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: an unnamed timer; null when the system is older than
            // high-resolution timers, and the waits fall back to milliseconds.
            let timer = unsafe {
                CreateWaitableTimerExW(
                    std::ptr::null(),
                    std::ptr::null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS,
                )
            };
            Ok(Self { event, timer })
        }
    }

    impl Doorbell for EventBell {
        fn ring(&self, _word: &AtomicU32) {
            // SAFETY: our handle.
            unsafe { SetEvent(self.event) };
        }

        fn sleep(&self, word: &AtomicU32, seen: u32, timeout: Duration) {
            if word.load(Ordering::Acquire) != seen {
                return;
            }
            let ms = timeout.as_micros().div_ceil(1000).min(u32::MAX as u128 - 4) as u32;
            // SAFETY: our handles; `due` outlives the call that reads it.
            unsafe {
                if !self.timer.is_null() && timeout < Duration::from_millis(20) {
                    // Relative, in 100 ns units.
                    let due = -((timeout.as_nanos() / 100).max(1) as i64);
                    if SetWaitableTimer(self.timer, &due, 0, None, std::ptr::null(), 0) != 0 {
                        let handles = [self.event, self.timer];
                        WaitForMultipleObjects(2, handles.as_ptr(), 0, ms + 2);
                        return;
                    }
                }
                WaitForSingleObject(self.event, ms.max(1));
            }
        }
    }

    pub fn bells(channel: &str, create: bool) -> io::Result<Bells> {
        Ok(Bells {
            request: Box::new(EventBell::new(&format!("{channel}-request"), create)?),
            reply: Box::new(EventBell::new(&format!("{channel}-reply"), create)?),
        })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use std::io;

    use super::{Bells, PollBell, Region};
    use crate::layout::Layout;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "no shared audio block on this platform",
        )
    }

    pub fn create(_name: &str, _layout: Layout) -> io::Result<Region> {
        Err(unsupported())
    }

    pub fn open(_name: &str) -> io::Result<Region> {
        Err(unsupported())
    }

    pub fn bells(_channel: &str, _create: bool) -> io::Result<Bells> {
        Ok(Bells {
            request: Box::new(PollBell),
            reply: Box::new(PollBell),
        })
    }
}
