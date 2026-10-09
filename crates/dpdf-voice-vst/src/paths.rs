//! Windows helpers: the folder this DLL was loaded from, the calling thread's
//! free stack, and logging.
//!
//! Plain kernel32 declarations keep the plugin free of extra dependencies.

use std::ffi::{c_void, OsString};
use std::io::Write;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x2;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, filename: *mut u16, size: u32) -> u32;
    fn OutputDebugStringW(text: *const u16);
    fn GetCurrentThreadStackLimits(low_limit: *mut usize, high_limit: *mut usize);
}

/// Folder containing this plugin DLL (not the host executable).
pub fn plugin_dir() -> Option<PathBuf> {
    let mut module = std::ptr::null_mut();
    let anchor = plugin_dir as *const () as *const u16;
    let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
    if unsafe { GetModuleHandleExW(flags, anchor, &mut module) } == 0 {
        return None;
    }
    let mut buf = vec![0u16; 32768];
    let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if len == 0 || len >= buf.len() {
        return None;
    }
    let path = PathBuf::from(OsString::from_wide(&buf[..len]));
    path.parent().map(PathBuf::from)
}

/// Bytes of stack still available below the caller's frame. Windows commits
/// stack pages on demand up to the reserved size, so this is the reserve
/// that is left, not just the committed part.
#[inline(never)]
pub fn stack_remaining() -> usize {
    let (mut low, mut high) = (0usize, 0usize);
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    let marker = 0u8;
    let here = std::ptr::addr_of!(marker) as usize;
    here.saturating_sub(low)
}

/// Log file in the host's temp folder. For Equalizer APO that is the audio
/// service's profile: C:\Windows\ServiceProfiles\LocalService\AppData\Local\Temp.
fn log_path() -> PathBuf {
    std::env::temp_dir().join("dpdf-voice.log")
}

/// Writes to the Windows debug output and appends to the log file. Only used
/// for rare events (load, errors), never per audio block.
pub fn log(msg: &str) {
    let wide: Vec<u16> = OsString::from(format!("dpdf-voice: {msg}"))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { OutputDebugStringW(wide.as_ptr()) };

    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path()) {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let _ = writeln!(f, "{secs} {exe}[{}] {msg}", std::process::id());
    }
}
