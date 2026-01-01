use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub const PROGRAM_NAME: &str = "ImeWatcher";
pub const PROGRAM_WINDOW: &str = "ImeWatcherWindow";
pub const WM_TRAY_CALLBACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1;

// Helper to convert Rust string to wide string (UTF-16) with null terminator
pub fn to_wstring(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}
