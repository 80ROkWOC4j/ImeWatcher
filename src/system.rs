use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::{AllocConsole, FreeConsole};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DefWindowProcW, GWLP_USERDATA, GetForegroundWindow, GetWindowLongPtrW, HHOOK,
    PostMessageW, PostQuitMessage, SC_CLOSE, SC_MINIMIZE, SetWindowLongPtrW, WM_APP, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WM_DESTROY, WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_SYSCOMMAND,
};

use crate::ime::LanguageTracker;
use crate::ui::tray::TrayIcon;

// Global hook handle
pub static mut KEYBOARD_HOOK: HHOOK = HHOOK(std::ptr::null_mut());

// Store the main window handle globally for hooks to send messages to
static mut APP_HWND: HWND = HWND(std::ptr::null_mut());

const WM_APP_IME_CHANGE: u32 = WM_APP + 1;

struct AppState {
    tray: TrayIcon,
    ime_tracker: LanguageTracker,
}

pub unsafe extern "system" fn keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        match w_param.0 as u32 {
            WM_KEYDOWN | WM_KEYUP => unsafe {
                if !APP_HWND.0.is_null() {
                    let _ = PostMessageW(Some(APP_HWND), WM_APP_IME_CHANGE, WPARAM(0), LPARAM(0));
                }
            },
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
        println!("창변경 감지");
        unsafe {
            if !APP_HWND.0.is_null() {
                let _ = PostMessageW(Some(APP_HWND), WM_APP_IME_CHANGE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

pub unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    // Retrieve app state from window user data
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState };
    let state = if state_ptr.is_null() {
        None
    } else {
        unsafe { Some(&mut *state_ptr) }
    };

    match message {
        WM_CREATE => {
            unsafe {
                APP_HWND = hwnd;
                #[cfg(debug_assertions)]
                {
                    let _ = AllocConsole();
                }
            }

            // Initialize state
            let app_state = Box::new(AppState {
                tray: TrayIcon::new(hwnd),
                ime_tracker: LanguageTracker::new(),
            });

            unsafe {
                // Store state pointer in window
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app_state) as isize);

                // Initial IME check
                let _ = PostMessageW(Some(hwnd), WM_APP_IME_CHANGE, WPARAM(0), LPARAM(0));
            }

            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(state) = state {
                state.tray.minimize();
            }
            LRESULT(0)
        }
        WM_INPUTLANGCHANGE => {
            println!("키보드 레이아웃 변경 감지");
            if let Some(state) = state {
                state.ime_tracker.check_and_update();
            }
            LRESULT(0)
        }
        WM_APP_IME_CHANGE => {
            if let Some(state) = state {
                state.ime_tracker.check_and_update();
            }
            LRESULT(0)
        }
        WM_SYSCOMMAND => match w_param.0 as u32 & 0xFFF0 {
            SC_MINIMIZE | SC_CLOSE => {
                if let Some(state) = state {
                    state.tray.minimize();
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, message, w_param, l_param) },
        },
        WM_COMMAND => {
            let low_word = (w_param.0 & 0xFFFF) as u32;
            if let Some(state) = state {
                state.tray.handle_command(low_word);
            }
            LRESULT(0)
        }
        // Handle tray icon messages
        msg if msg == crate::utils::WM_TRAY_CALLBACK => {
            if let Some(state) = state {
                state.tray.handle_message(msg, l_param);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                let _ = FreeConsole();
                PostQuitMessage(0);

                // Clean up state
                if !state_ptr.is_null() {
                    let _ = Box::from_raw(state_ptr);
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w_param, l_param) },
    }
}
