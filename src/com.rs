//! The COM side: the thumbnail provider, its class factory, the DLL exports and registration.
//!
//! The provider takes its input through IInitializeWithStream, so Explorer runs it in an
//! isolated surrogate process: a damaged file can never take Explorer down.

use std::cell::RefCell;
use std::ffi::c_void;
use std::io::{self, Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, E_UNEXPECTED, ERROR_ALREADY_INITIALIZED, ERROR_SUCCESS, HMODULE, S_FALSE,
    S_OK,
};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, IStream, STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET};
use windows::Win32::System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW, GetModuleHandleExW};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegGetValueW,
    RegSetValueExW,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithStream, IInitializeWithStream_Impl};
use windows::Win32::UI::Shell::{IThumbnailProvider, IThumbnailProvider_Impl, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, WTS_ALPHATYPE, WTSAT_RGB};
use windows_core::{BOOL, GUID, HRESULT, HSTRING, IUnknown, Interface, PCWSTR, Ref, implement};

pub const CLSID: GUID = GUID::from_u128(0x9f2321af_6f5f_43b4_8a26_f7a29d74b5ba);
const CLSID_TEXT: &str = "{9F2321AF-6F5F-43B4-8A26-F7A29D74B5BA}";
const THUMBNAIL_HANDLER: &str = "{e357fccd-a995-4576-b01f-234630154e96}";

/// File types the provider is registered for.
pub const EXTENSIONS: &[&str] = &[".zip", ".cbz", ".rar", ".cbr", ".7z", ".cb7", ".tar", ".cbt", ".epub", ".azw3", ".azw", ".mobi"];

/// Live objects and server locks, for DllCanUnloadNow.
static LIVE: AtomicIsize = AtomicIsize::new(0);

#[implement(IInitializeWithStream, IThumbnailProvider)]
pub struct Provider {
    stream: RefCell<Option<IStream>>,
}

impl Provider {
    pub fn new() -> Self {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Provider { stream: RefCell::new(None) }
    }
}

impl Default for Provider {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IInitializeWithStream_Impl for Provider_Impl {
    fn Initialize(&self, stream: Ref<IStream>, _mode: u32) -> windows_core::Result<()> {
        let mut slot = self.stream.borrow_mut();
        if slot.is_some() {
            return Err(HRESULT::from_win32(ERROR_ALREADY_INITIALIZED.0).into());
        }
        *slot = Some(stream.ok()?.clone());
        Ok(())
    }
}

impl IThumbnailProvider_Impl for Provider_Impl {
    fn GetThumbnail(&self, cx: u32, phbmp: *mut HBITMAP, pdwalpha: *mut WTS_ALPHATYPE) -> windows_core::Result<()> {
        if phbmp.is_null() || pdwalpha.is_null() {
            return Err(E_POINTER.into());
        }
        let stream = self.stream.borrow().clone().ok_or(windows_core::Error::from(E_UNEXPECTED))?;
        match catch_unwind(AssertUnwindSafe(|| thumbnail(stream, cx))) {
            Ok(Ok(bitmap)) => {
                unsafe {
                    *phbmp = bitmap;
                    *pdwalpha = WTSAT_RGB;
                }
                Ok(())
            }
            _ => Err(E_FAIL.into()),
        }
    }
}

fn thumbnail(stream: IStream, cx: u32) -> crate::Result<HBITMAP> {
    let cover = crate::cover::find(ComStream(stream))?;
    let thumb = crate::image::render(&cover, cx)?;
    crate::image::to_hbitmap(&thumb)
}

/// An IStream as `Read + Seek`.
pub struct ComStream(pub IStream);

// The stream is only used on the thread that received it: every reader runs synchronously
// on the calling thread (unrar-rs is set to serial decoding). `Send` is needed only because
// the RAR reader's signature asks for it.
unsafe impl Send for ComStream {}

impl Read for ComStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut n = 0u32;
        let len = buf.len().min(u32::MAX as usize) as u32;
        unsafe { self.0.Read(buf.as_mut_ptr().cast(), len, Some(&mut n)) }.ok().map_err(io::Error::other)?;
        Ok(n as usize)
    }
}

impl Seek for ComStream {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let (offset, origin) = match to {
            SeekFrom::Start(n) => (n as i64, STREAM_SEEK_SET),
            SeekFrom::Current(n) => (n, STREAM_SEEK_CUR),
            SeekFrom::End(n) => (n, STREAM_SEEK_END),
        };
        let mut pos = 0u64;
        unsafe { self.0.Seek(offset, origin, Some(&mut pos)) }.map_err(io::Error::other)?;
        Ok(pos)
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, riid: *const GUID, ppv: *mut *mut c_void) -> windows_core::Result<()> {
        if ppv.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = std::ptr::null_mut() };
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let provider: IUnknown = Provider::new().into();
        unsafe { provider.query(riid, ppv) }.ok()
    }

    fn LockServer(&self, lock: BOOL) -> windows_core::Result<()> {
        if lock.as_bool() {
            LIVE.fetch_add(1, Ordering::SeqCst);
        } else {
            LIVE.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// # Safety
/// Called by COM with valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if clsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = std::ptr::null_mut();
        if *clsid != CLSID {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory.into();
        factory.query(riid, ppv)
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LIVE.load(Ordering::SeqCst) > 0 { S_FALSE } else { S_OK }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    match register() {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    unregister();
    S_OK
}

/// Registers for the current user only; no administrator rights needed.
fn register() -> windows_core::Result<()> {
    let path = module_path()?;
    let class = format!(r"Software\Classes\CLSID\{CLSID_TEXT}");
    set_value(&class, None, "mierucover")?;
    set_value(&format!(r"{class}\InprocServer32"), None, &path)?;
    set_value(&format!(r"{class}\InprocServer32"), Some("ThreadingModel"), "Apartment")?;
    for ext in EXTENSIONS {
        set_value(&format!(r"Software\Classes\{ext}\ShellEx\{THUMBNAIL_HANDLER}"), None, CLSID_TEXT)?;
    }
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    Ok(())
}

/// Removes our keys, leaving any other provider registered for the same types alone.
fn unregister() {
    for ext in EXTENSIONS {
        let key = format!(r"Software\Classes\{ext}\ShellEx\{THUMBNAIL_HANDLER}");
        if get_value(&key).is_some_and(|v| v.eq_ignore_ascii_case(CLSID_TEXT)) {
            let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
        }
    }
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(format!(r"Software\Classes\CLSID\{CLSID_TEXT}")));
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

fn module_path() -> windows_core::Result<String> {
    let mut module = HMODULE::default();
    let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    unsafe { GetModuleHandleExW(flags, PCWSTR(module_path as *const () as *const u16), &mut module)? };
    let mut buf = vec![0u16; 32768];
    let n = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    if n == 0 {
        return Err(windows_core::Error::from_thread());
    }
    Ok(String::from_utf16_lossy(&buf[..n]))
}

fn set_value(path: &str, name: Option<&str>, value: &str) -> windows_core::Result<()> {
    let mut key = HKEY::default();
    unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None) }.ok()?;
    let data: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2) };
    let name = name.map(HSTRING::from);
    let name = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    let result = unsafe { RegSetValueExW(key, name, None, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    result.ok()
}

fn get_value(path: &str) -> Option<String> {
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let result = unsafe { RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(path), PCWSTR::null(), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
    if result != ERROR_SUCCESS {
        return None;
    }
    let n = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// A provider object for tests and the dev tool, without going through the registry.
pub fn create_provider() -> IUnknown {
    Provider::new().into()
}

/// The provider interface of an object made by `create_provider`.
pub fn as_thumbnail_provider(unknown: &IUnknown) -> windows_core::Result<(IInitializeWithStream, IThumbnailProvider)> {
    Ok((unknown.cast()?, unknown.cast()?))
}
