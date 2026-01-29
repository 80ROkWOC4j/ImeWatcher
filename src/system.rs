use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, HHOOK, PostMessageW, WM_APP, WM_KEYDOWN, WM_KEYUP,
};

pub const WM_APP_IME_CHANGE: u32 = WM_APP + 1;

// Store HWND as integer to satisfy Send/Sync requirements.
static APP_HWND: OnceLock<isize> = OnceLock::new();

// Global hook handle (used by CallNextHookEx)
pub static mut KEYBOARD_HOOK: HHOOK = HHOOK(std::ptr::null_mut());

pub fn set_app_hwnd(hwnd: HWND) {
    let _ = APP_HWND.set(hwnd.0 as isize);
}

fn post_ime_change() {
    if let Some(hwnd) = APP_HWND.get() {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(*hwnd as _)),
                WM_APP_IME_CHANGE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub unsafe extern "system" fn keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        match w_param.0 as u32 {
            WM_KEYDOWN | WM_KEYUP => post_ime_change(),
            _ => {}
        }
    }
    unsafe { CallNextHookEx(Some(KEYBOARD_HOOK), n_code, w_param, l_param) }
}

pub unsafe extern "system" fn win_event_proc_callback(
    _h_win_event_hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _dw_event_thread: u32,
    _dwms_event_time: u32,
) {
    let foreground_window = unsafe { GetForegroundWindow() };
    if !foreground_window.0.is_null() {
        post_ime_change();
    }
}
