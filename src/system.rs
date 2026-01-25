
use std::collections::{HashMap, HashSet};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::{AllocConsole, FreeConsole};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_AUTOCHECKBOX, CallNextHookEx, CreateWindowExW, DefWindowProcW, GWLP_USERDATA,
    GetForegroundWindow, GetWindowLongPtrW, HHOOK, HMENU, KillTimer, PostMessageW,
    PostQuitMessage, SC_CLOSE, SC_MINIMIZE, SendMessageW, SetTimer, SetWindowLongPtrW,
    WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_SYSCOMMAND, WM_TIMER, WS_CHILD, WS_VISIBLE,
};

use crate::hid::HidManager;
use crate::ime::{LangId, LanguageTracker};
use crate::ui::tray::TrayIcon;
use crate::utils::{to_wstring, WM_TRAY_CALLBACK};
use windows::core::PCWSTR;

// Global hook handle
pub static mut KEYBOARD_HOOK: HHOOK = HHOOK(std::ptr::null_mut());

// Store the main window handle globally for hooks to send messages to
static mut APP_HWND: HWND = HWND(std::ptr::null_mut());

const WM_APP_IME_CHANGE: u32 = WM_APP + 1;
const IDC_COMBOBOX: u32 = 101;
const IDC_ENABLE_SYNC_CHECKBOX: u32 = 102;
const IDC_DYN_COMBO_BASE: u32 = 200;
const IDT_LAYER_POLL_TIMER: usize = 1;

// Combobox Constants
const CBS_DROPDOWNLIST: u32 = 0x0003;
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETCURSEL: u32 = 0x0147;
const CBN_SELCHANGE: u32 = 1;

struct LangUiControls {
    lang_id: LangId,
    #[allow(dead_code)]
    label_hwnd: HWND,
    combo_hwnd: HWND,
    populated: bool,
}

struct AppState {
    tray: TrayIcon,
    ime_tracker: LanguageTracker,
    hid_manager: Option<HidManager>,
    devices: Vec<hidapi::DeviceInfo>,
    combo_hwnd: HWND,
    layer_count: u8,
    // IME-Layer Sync UI
    sync_enabled: bool,
    layer_config: HashMap<LangId, Option<u8>>,
    lang_ui_controls: Vec<LangUiControls>,
}

const CB_RESETCONTENT: u32 = 0x014B;

pub unsafe extern "system" fn keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        match w_param.0 as u32 {
            WM_KEYDOWN | WM_KEYUP => {
                unsafe {
                    if !APP_HWND.0.is_null() {
                        let _ = PostMessageW(Some(APP_HWND), WM_APP_IME_CHANGE, WPARAM(0), LPARAM(0));
                    }
                }
            }
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
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
    let maybe_state = if state_ptr.is_null() {
        None
    } else {
        Some(&mut *state_ptr)
    };

    match message {
        WM_CREATE => {
            APP_HWND = hwnd;
            #[cfg(debug_assertions)]
            {
                let _ = AllocConsole();
            }

            // HID Init
            let mut hid_manager = HidManager::new().ok();
            let mut devices = Vec::new();
            if let Some(ref mut hm) = hid_manager {
                devices = hm.list_devices();
                hm.auto_select_first();

                // Check Protocol Version
                match hm.get_protocol_version() {
                    Ok(version) => println!("VIA Protocol Version: {:04x}", version),
                    Err(e) => println!("Failed to get protocol version: {}", e),
                }
            }

            // Create Device Combobox
            let combo_style = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | CBS_DROPDOWNLIST);
            let combo_hwnd = CreateWindowExW(
                Default::default(), PCWSTR(to_wstring("COMBOBOX").as_ptr()), PCWSTR(std::ptr::null()),
                combo_style, 10, 10, 360, 200, Some(hwnd), Some(HMENU(IDC_COMBOBOX as _)), None, None)
                .unwrap_or_default();

            for dev in &devices {
                let name = format!(
                    "{} {} ({:04x}:{:04x})",
                    dev.manufacturer_string().unwrap_or("Unknown"),
                    dev.product_string().unwrap_or("Device"),
                    dev.vendor_id(),
                    dev.product_id());
                SendMessageW(combo_hwnd, CB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(to_wstring(&name).as_ptr() as _)));
            }
            if !devices.is_empty() {
                SendMessageW(combo_hwnd, CB_SETCURSEL, Some(WPARAM(0)), Some(LPARAM(0)));
            }

            // Create IME-Sync UI
            let static_style = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0);
            let checkbox_style = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | (BS_AUTOCHECKBOX as u32));

            let enable_sync_hwnd = CreateWindowExW(
                Default::default(), PCWSTR(to_wstring("BUTTON").as_ptr()), PCWSTR(to_wstring("Enable IME-Layer Sync").as_ptr()),
                checkbox_style, 10, 45, 200, 20, Some(hwnd), Some(HMENU(IDC_ENABLE_SYNC_CHECKBOX as _)), None, None)
                .unwrap_or_default();
            
            let _ = CreateWindowExW(
                Default::default(), PCWSTR(to_wstring("STATIC").as_ptr()), PCWSTR(to_wstring("Language Detected").as_ptr()),
                static_style, 10, 75, 150, 20, Some(hwnd), None, None, None);
            let _ = CreateWindowExW(
                Default::default(), PCWSTR(to_wstring("STATIC").as_ptr()), PCWSTR(to_wstring("Switch to Layer").as_ptr()),
                static_style, 170, 75, 150, 20, Some(hwnd), None, None, None);


            // Initialize state
            let app_state = Box::new(AppState {
                tray: TrayIcon::new(hwnd),
                ime_tracker: LanguageTracker::new(),
                hid_manager,
                devices,
                combo_hwnd,
                layer_count: 0,
                sync_enabled: false,
                layer_config: HashMap::new(),
                lang_ui_controls: Vec::new(),
            });

            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app_state) as isize);
            let _ = PostMessageW(Some(hwnd), WM_APP_IME_CHANGE, WPARAM(0), LPARAM(0));
            SetTimer(Some(hwnd), IDT_LAYER_POLL_TIMER, 1000, None);

            LRESULT(0)
        }
        WM_TIMER => {
            if w_param.0 == IDT_LAYER_POLL_TIMER {
                if let Some(state) = maybe_state {
                    if let Some(ref hm) = state.hid_manager {
                        if let Ok(count) = hm.get_layer_count() {
                            if state.layer_count != count {
                                println!("Layer count changed to: {}", count);
                                state.layer_count = count;
                                // Repopulate all dropdowns
                                for control in &mut state.lang_ui_controls {
                                    control.populated = false;
                                }
                            }
                        }
                    }
                    
                    // Populate any unpopulated dropdowns
                    if state.layer_count > 0 {
                        for control in &mut state.lang_ui_controls {
                            if !control.populated {
                                // Clear
                                SendMessageW(control.combo_hwnd, CB_RESETCONTENT, Some(WPARAM(0)), Some(LPARAM(0)));
                                // Add "Do not change"
                                SendMessageW(control.combo_hwnd, CB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(to_wstring("Do not change").as_ptr() as _)));
                                // Add layers
                                for i in 0..state.layer_count {
                                    let layer_name = to_wstring(&format!("Layer {}", i));
                                    SendMessageW(control.combo_hwnd, CB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(layer_name.as_ptr() as _)));
                                }
                                
                                // Restore selection
                                let selection = state.layer_config.get(&control.lang_id).cloned().flatten().map_or(0, |v| (v + 1) as usize);
                                SendMessageW(control.combo_hwnd, CB_SETCURSEL, Some(WPARAM(selection)), Some(LPARAM(0)));

                                control.populated = true;
                                println!("Populated dropdown for lang {}", control.lang_id);
                            }
                        }
                    }
                }
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
            if let Some(state) = maybe_state {
                let changed = state.ime_tracker.check_and_update();

                // Dynamic UI creation
                let current_ui_langs: HashSet<LangId> = state.lang_ui_controls.iter().map(|c| c.lang_id).collect();
                for &lang_id in &state.ime_tracker.detected_langs {
                    if !current_ui_langs.contains(&lang_id) {
                        println!("Creating UI for new lang: {}", lang_id);
                        let y_pos = 100 + (state.lang_ui_controls.len() as i32 * 25);
                        
                        let label_hwnd = CreateWindowExW(
                            Default::default(), PCWSTR(to_wstring("STATIC").as_ptr()), PCWSTR(to_wstring(&lang_id.to_string()).as_ptr()),
                            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0), 10, y_pos, 150, 20, Some(hwnd), None, None, None)
                            .unwrap_or_default();
                        
                        let combo_id = IDC_DYN_COMBO_BASE + state.lang_ui_controls.len() as u32;
                        let combo_hwnd = CreateWindowExW(
                            Default::default(), PCWSTR(to_wstring("COMBOBOX").as_ptr()), PCWSTR(std::ptr::null()),
                            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | CBS_DROPDOWNLIST), 170, y_pos, 150, 200, Some(hwnd), Some(HMENU(combo_id as _)), None, None)
                            .unwrap_or_default();
                        
                        state.lang_ui_controls.push(LangUiControls {
                            lang_id,
                            label_hwnd,
                            combo_hwnd,
                            populated: false,
                        });
                    }
                }

                // Layer switching logic
                if changed && state.sync_enabled {
                    println!("IME changed and sync is enabled.");
                    if let Some(ref hm) = state.hid_manager {
                         let current_lang = state.ime_tracker.current();
                         println!("Current lang: {}. Checking config.", current_lang);
                         match state.layer_config.get(&current_lang) {
                            Some(Some(target_layer)) => {
                                println!("Found config: switch to layer {}. Sending custom command.", target_layer);
                                match hm.set_layer_state(*target_layer) {
                                    Ok(_) => println!("Custom command sent successfully."),
                                    Err(e) => println!("Error setting layer state: {}", e),
                                }
                            }
                            Some(None) => println!("Found config: 'Do not change'."),
                            None => println!("No config found for this language."),
                         }
                    }
                } else if changed {
                    println!("IME changed but sync is DISABLED.");
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
            _ => DefWindowProcW(hwnd, message, w_param, l_param),
        },
        WM_COMMAND => {
            let low_word = (w_param.0 & 0xFFFF) as u32;
            let hi_word = ((w_param.0 >> 16) & 0xFFFF) as u32;

            if let Some(state) = maybe_state {
                // Device selection
                if low_word == IDC_COMBOBOX && hi_word == CBN_SELCHANGE {
                     let idx = SendMessageW(state.combo_hwnd, CB_GETCURSEL, None, None).0 as usize;
                    if let Some(device) = state.devices.get(idx) {
                        if let Some(ref mut hm) = state.hid_manager {
                            hm.select_device(device.path().to_string_lossy().to_string());
                            let _ = hm.update_lighting(state.ime_tracker.current());

                            // Check Protocol Version on device change
                            match hm.get_protocol_version() {
                                Ok(version) => println!("VIA Protocol Version: {:04x}", version),
                                Err(e) => println!("Failed to get protocol version: {}", e),
                            }
                        }
                    }
                } 
                // Enable checkbox
                else if low_word == IDC_ENABLE_SYNC_CHECKBOX {
                    state.sync_enabled = !state.sync_enabled;
                    println!("Sync enabled: {}", state.sync_enabled);
                }
                // Dynamic layer selection
                else if low_word >= IDC_DYN_COMBO_BASE && hi_word == CBN_SELCHANGE {
                    let control_index = (low_word - IDC_DYN_COMBO_BASE) as usize;
                    if let Some(control) = state.lang_ui_controls.get(control_index) {
                        let selection = SendMessageW(control.combo_hwnd, CB_GETCURSEL, None, None).0 as u8;
                        let target_layer = if selection == 0 { None } else { Some(selection - 1) };
                        state.layer_config.insert(control.lang_id, target_layer);
                        println!("Set layer for {} to {:?}", control.lang_id, target_layer);
                    }
                } else {
                    state.tray.handle_command(low_word);
                }
            }
            LRESULT(0)
        }
        msg if msg == WM_TRAY_CALLBACK => {
            if let Some(state) = maybe_state {
                state.tray.handle_message(msg, l_param);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), IDT_LAYER_POLL_TIMER);
            if !state_ptr.is_null() {
                let _ = Box::from_raw(state_ptr);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            let _ = FreeConsole();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, w_param, l_param),
    }
}
