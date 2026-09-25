//! The folder, open and save dialogs on Windows: the system's own
//! `IFileDialog`, in this process, owned by the studio's window.
//!
//! > *"anything that says click doesn't work"*
//!
//! They used to be PowerShell putting up a WinForms dialog. That froze the
//! studio for the seconds PowerShell takes to load .NET, and the dialog that
//! came up belonged to another process — which Windows' focus rules put
//! **behind** the window that asked for it. The studio, blocked until it was
//! answered, was then marked *Not Responding*. A dialog owned by the window
//! that asked is in front of it and modal to it, which is the whole fix.
//!
//! `windows-sys` declares functions, not COM interfaces, so the two vtables
//! used are declared here: `IFileDialog` (shared by the open and the save
//! dialog, which only add to it) and `IShellItem`. Slots this never calls are
//! kept as pointers, there only to put the ones it does call at the right
//! offsets — the order is the SDK's `shobjidl_core.h`.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows_sys::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows_sys::Win32::UI::Shell::SHCreateItemFromParsingName;
use windows_sys::core::{GUID, HRESULT, PCWSTR, PWSTR};

/// What is being asked for.
pub(crate) enum Ask<'a> {
    Folder,
    Open { filter: &'a str },
    Save { name: &'a str },
}

const CLSID_FILE_OPEN_DIALOG: GUID = GUID::from_u128(0xdc1c5a9c_e88a_4dde_a5a1_60f82a20aef7);
const CLSID_FILE_SAVE_DIALOG: GUID = GUID::from_u128(0xc0b4e2f3_ba21_4773_8dba_335ec946eb8b);
const IID_IFILE_DIALOG: GUID = GUID::from_u128(0x42f85136_db7e_439c_85f1_e4075d135fc8);
const IID_ISHELL_ITEM: GUID = GUID::from_u128(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);

const FOS_OVERWRITEPROMPT: u32 = 0x2;
const FOS_PICKFOLDERS: u32 = 0x20;
const FOS_FORCEFILESYSTEM: u32 = 0x40;
const FOS_PATHMUSTEXIST: u32 = 0x800;
const FOS_FILEMUSTEXIST: u32 = 0x1000;
/// `SIGDN_FILESYSPATH`: the item as a path on disk.
const SIGDN_FILESYSPATH: i32 = 0x8005_8000_u32 as i32;
/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: the person closed the dialog.
const CANCELLED: HRESULT = 0x8007_04C7_u32 as HRESULT;

type Slot = *const c_void;

#[repr(C)]
struct IUnknownVtbl {
    query_interface: Slot,
    add_ref: Slot,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct IFileDialogVtbl {
    unknown: IUnknownVtbl,
    // IModalWindow
    show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    // IFileDialog
    set_file_types:
        unsafe extern "system" fn(*mut c_void, u32, *const COMDLG_FILTERSPEC) -> HRESULT,
    set_file_type_index: Slot,
    get_file_type_index: Slot,
    advise: Slot,
    unadvise: Slot,
    set_options: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    set_default_folder: Slot,
    set_folder: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    get_folder: Slot,
    get_current_selection: Slot,
    set_file_name: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_file_name: Slot,
    set_title: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    set_ok_button_label: Slot,
    set_file_name_label: Slot,
    get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    add_place: Slot,
    set_default_extension: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
}

#[repr(C)]
struct IShellItemVtbl {
    unknown: IUnknownVtbl,
    bind_to_handler: Slot,
    get_parent: Slot,
    get_display_name: unsafe extern "system" fn(*mut c_void, i32, *mut PWSTR) -> HRESULT,
}

/// A COM object held, and released when dropped.
struct Held(*mut c_void);

impl Held {
    /// The object's vtable, read as `T`.
    ///
    /// # Safety
    /// `T` must be a prefix of the object's real vtable.
    unsafe fn vtable<T>(&self) -> &T {
        unsafe { &**(self.0 as *mut *const T) }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: every `Held` is a live COM object this module was given
            // one reference to.
            unsafe { (self.vtable::<IUnknownVtbl>().release)(self.0) };
        }
    }
}

/// `s` as a NUL-terminated UTF-16 string.
fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// Asks, blocking until the dialog is answered. `Ok(None)` is a cancel.
pub(crate) fn ask(
    title: &str,
    start: Option<&Path>,
    what: Ask<'_>,
) -> Result<Option<PathBuf>, String> {
    // SAFETY: COM is initialised for this call if it was not already, and
    // uninitialised only if this call is what initialised it.
    unsafe {
        let init = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let answer = show(title, start, what);
        if init >= 0 {
            CoUninitialize();
        }
        answer
    }
}

unsafe fn show(
    title: &str,
    start: Option<&Path>,
    what: Ask<'_>,
) -> Result<Option<PathBuf>, String> {
    let class = match what {
        Ask::Save { .. } => &CLSID_FILE_SAVE_DIALOG,
        _ => &CLSID_FILE_OPEN_DIALOG,
    };
    let mut raw = std::ptr::null_mut();
    let made = unsafe {
        CoCreateInstance(
            class,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IFILE_DIALOG,
            &mut raw,
        )
    };
    if made < 0 || raw.is_null() {
        return Err(format!(
            "the system's file dialog would not open (0x{:08X})",
            made as u32
        ));
    }
    let dialog = Held(raw);
    // SAFETY: `IFileDialogVtbl` is the head of both dialogs' vtables.
    let vt = unsafe { dialog.vtable::<IFileDialogVtbl>() };

    let mut options = 0;
    unsafe { (vt.get_options)(dialog.0, &mut options) };
    options |= FOS_FORCEFILESYSTEM
        | FOS_PATHMUSTEXIST
        | match what {
            Ask::Folder => FOS_PICKFOLDERS,
            Ask::Open { .. } => FOS_FILEMUSTEXIST,
            Ask::Save { .. } => FOS_OVERWRITEPROMPT,
        };
    unsafe { (vt.set_options)(dialog.0, options) };
    let title = wide(title);
    unsafe { (vt.set_title)(dialog.0, title.as_ptr()) };

    if let Some(start) = start.filter(|dir| dir.is_dir()) {
        let path = wide(start);
        let mut item = std::ptr::null_mut();
        let found = unsafe {
            SHCreateItemFromParsingName(
                path.as_ptr(),
                std::ptr::null_mut(),
                &IID_ISHELL_ITEM,
                &mut item,
            )
        };
        if found >= 0 && !item.is_null() {
            let item = Held(item);
            unsafe { (vt.set_folder)(dialog.0, item.0) };
        }
    }

    // The filter's strings have to outlive `show`, so they are made here.
    let pattern = match &what {
        Ask::Open { filter } => Some(super::windows_filter(filter)),
        Ask::Save { name } => Path::new(name)
            .extension()
            .map(|ext| super::windows_filter(&format!("*.{}", ext.to_string_lossy()))),
        Ask::Folder => None,
    };
    let pattern = pattern.map(|(name, spec)| (wide(name), wide(spec)));
    if let Some((name, spec)) = &pattern {
        let filter = COMDLG_FILTERSPEC {
            pszName: name.as_ptr(),
            pszSpec: spec.as_ptr(),
        };
        unsafe { (vt.set_file_types)(dialog.0, 1, &filter) };
    }
    if let Ask::Save { name } = &what {
        let file = wide(name);
        unsafe { (vt.set_file_name)(dialog.0, file.as_ptr()) };
        // So a name typed without its extension gets one.
        if let Some(ext) = Path::new(name).extension() {
            let ext = wide(ext);
            unsafe { (vt.set_default_extension)(dialog.0, ext.as_ptr()) };
        }
    }

    // Owned by the window that asked — the one the press was in — so it is
    // in front of it and that window waits for it.
    let owner = unsafe { GetActiveWindow() };
    let shown = unsafe { (vt.show)(dialog.0, owner) };
    if shown == CANCELLED {
        return Ok(None);
    }
    if shown < 0 {
        return Err(format!(
            "the system's file dialog failed (0x{:08X})",
            shown as u32
        ));
    }
    let mut item = std::ptr::null_mut();
    if unsafe { (vt.get_result)(dialog.0, &mut item) } < 0 || item.is_null() {
        return Ok(None);
    }
    let item = Held(item);
    // SAFETY: `get_result` hands back an `IShellItem`.
    let ivt = unsafe { item.vtable::<IShellItemVtbl>() };
    let mut name: PWSTR = std::ptr::null_mut();
    if unsafe { (ivt.get_display_name)(item.0, SIGDN_FILESYSPATH, &mut name) } < 0 || name.is_null()
    {
        return Err("the chosen item is not a place on disk".to_string());
    }
    // SAFETY: a NUL-terminated string the shell allocated with the COM
    // allocator, freed with it once copied.
    let path = unsafe {
        let mut len = 0;
        while *name.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(name, len));
        CoTaskMemFree(name as *const c_void);
        text
    };
    Ok(Some(PathBuf::from(path)))
}
