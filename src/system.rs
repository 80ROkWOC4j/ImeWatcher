use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::{AllocConsole, FreeConsole};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DefWindowProcW, GetForegroundWindow,
    PostQuitMessage, SC_CLOSE, SC_MINIMIZE, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_DESTROY, WM_INPUTLANGCHANGE, WM_SYSCOMMAND, HHOOK, WM_KEYDOWN, WM_KEYUP,
};

use crate::ime;
use crate::ui::tray;

// Global hook handle needs to be stored somewhere accessible
// Win32 hooks are inevitably global/static, so static mut is often used or AtomicPtr.
// For HHOOK which is a pointer-like handle, AtomicPtr or specific Atomic types could be used,
// but HHOOK is a struct wrapping a pointer.
// We can use a simpler approach or ignore the warning for this specific legacy-style hook storage.
pub static mut KEYBOARD_HOOK: HHOOK = HHOOK(std::ptr::null_mut());

pub unsafe extern "system" fn keyboard_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 {
        if w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_KEYUP {
            ime::update_ime_lang();
        }
    }
    // Note: CallNextHookEx expects Option<HHOOK> in recent windows versions
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
        println!("창변경 감지");
        ime::update_ime_lang();
    }
}

pub unsafe extern "system" fn wnd_proc(hwnd: HWND, message: u32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    // Delegate tray icon message handling
    tray::handle_message(message, l_param);

    match message {
        WM_CREATE => {
            #[cfg(debug_assertions)]
            {
                unsafe { let _ = AllocConsole(); }
            }
            tray::init(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            tray::minimize();
            LRESULT(0)
        }
        WM_INPUTLANGCHANGE => {
            println!("키보드 레이아웃 변경 감지");
            ime::update_ime_lang();
            LRESULT(0)
        }
        WM_SYSCOMMAND => {
            match w_param.0 as u32 & 0xFFF0 { 
                SC_MINIMIZE | SC_CLOSE => {
                    tray::minimize();
                    LRESULT(0)
                }
                _ => unsafe { DefWindowProcW(hwnd, message, w_param, l_param) },
            }
        }
        WM_COMMAND => {
            let low_word = (w_param.0 & 0xFFFF) as u32;
            tray::handle_command(low_word);
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                let _ = FreeConsole();
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w_param, l_param) },
    }
}