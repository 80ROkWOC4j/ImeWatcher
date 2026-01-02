use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::{AllocConsole, FreeConsole};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetForegroundWindow,
    GetWindowLongPtrW, HHOOK, HMENU, PostMessageW, PostQuitMessage, SC_CLOSE, SC_MINIMIZE,
    SendMessageW, SetWindowLongPtrW, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_DESTROY, WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_SYSCOMMAND, WS_CHILD, WS_VISIBLE,
};

use crate::hid::HidManager;
use crate::ime::LanguageTracker;
use crate::ui::tray::TrayIcon;
use crate::utils::{WM_TRAY_CALLBACK, to_wstring};
use windows::core::PCWSTR;

// Global hook handle
pub static mut KEYBOARD_HOOK: HHOOK = HHOOK(std::ptr::null_mut());

// Store the main window handle globally for hooks to send messages to
static mut APP_HWND: HWND = HWND(std::ptr::null_mut());

const WM_APP_IME_CHANGE: u32 = WM_APP + 1;
const IDC_COMBOBOX: u32 = 101;

// Combobox Constants
const CBS_DROPDOWNLIST: u32 = 0x0003;
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETCURSEL: u32 = 0x0147;
const CBN_SELCHANGE: u32 = 1;

struct AppState {
    tray: TrayIcon,
    ime_tracker: LanguageTracker,
    hid_manager: Option<HidManager>,
    devices: Vec<hidapi::DeviceInfo>,
    combo_hwnd: HWND,
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
    let maybe_state = if state_ptr.is_null() {
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

            // HID Init
            let mut hid_manager = HidManager::new().ok();
            let mut devices = Vec::new();
            if let Some(ref mut hm) = hid_manager {
                devices = hm.list_devices();
                hm.auto_select_first();
            }

            // Create Combobox
            let style = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | CBS_DROPDOWNLIST);

            let combo_hwnd = unsafe {
                CreateWindowExW(
                    Default::default(),
                    PCWSTR(to_wstring("COMBOBOX").as_ptr()),
                    PCWSTR(std::ptr::null()),
                    style,
                    10,
                    10,
                    360,
                    200,
                    Some(hwnd),
                    Some(HMENU(IDC_COMBOBOX as _)),
                    None,
                    None,
                )
                .unwrap_or_default()
            };

            // Populate
            for dev in &devices {
                let name = format!(
                    "{} {} ({:04x}:{:04x})",
                    dev.manufacturer_string().unwrap_or("Unknown"),
                    dev.product_string().unwrap_or("Device"),
                    dev.vendor_id(),
                    dev.product_id()
                );
                let wname = to_wstring(&name);
                unsafe {
                    SendMessageW(
                        combo_hwnd,
                        CB_ADDSTRING,
                        Some(WPARAM(0)),
                        Some(LPARAM(wname.as_ptr() as _)),
                    );
                }
            }

            if !devices.is_empty() {
                unsafe {
                    SendMessageW(combo_hwnd, CB_SETCURSEL, Some(WPARAM(0)), Some(LPARAM(0)));
                }
            }

            // Initialize state
            let app_state = Box::new(AppState {
                tray: TrayIcon::new(hwnd),
                ime_tracker: LanguageTracker::new(),
                hid_manager,
                devices,
                combo_hwnd,
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
            if let Some(state) = maybe_state {
                state.tray.minimize();
            }
            LRESULT(0)
        }
        WM_INPUTLANGCHANGE | WM_APP_IME_CHANGE => {
            if let Some(state) = maybe_state
                && state.ime_tracker.check_and_update()
            {
                let lang = state.ime_tracker.current();
                if let Some(ref mut hm) = state.hid_manager {
                    let _ = hm.update_lighting(lang);
                }
            }
            LRESULT(0)
        }
        WM_SYSCOMMAND => match w_param.0 as u32 & 0xFFF0 {
            SC_MINIMIZE | SC_CLOSE => {
                if let Some(state) = maybe_state {
                    state.tray.minimize();
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, message, w_param, l_param) },
        },
        WM_COMMAND => {
            let low_word = (w_param.0 & 0xFFFF) as u32;
            let hi_word = ((w_param.0 >> 16) & 0xFFFF) as u32;

            if let Some(state) = maybe_state {
                if low_word == IDC_COMBOBOX && hi_word == CBN_SELCHANGE {
                    let idx = unsafe {
                        SendMessageW(state.combo_hwnd, CB_GETCURSEL, None, None).0 as usize
                    };
                    if let Some(device) = state.devices.get(idx) {
                        let path = device.path().to_string_lossy().to_string();
                        if let Some(ref mut hm) = state.hid_manager {
                            hm.select_device(path);
                            let lang = state.ime_tracker.current();
                            let _ = hm.update_lighting(lang);
                        }
                    }
                } else {
                    state.tray.handle_command(low_word);
                }
            }
            LRESULT(0)
        }
        // Handle tray icon messages
        msg if msg == WM_TRAY_CALLBACK => {
            if let Some(state) = maybe_state {
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
