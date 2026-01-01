use std::collections::VecDeque;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Globalization::{GetLocaleInfoW, LOCALE_SLANGUAGE};
use windows::Win32::System::Console::{AllocConsole, FreeConsole};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Input::Ime::{
    ImmGetDefaultIMEWnd, IME_CMODE_ALPHANUMERIC, IME_CMODE_NATIVE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NIF_ICON, NIF_MESSAGE, NIF_TIP,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, LoadIconW,
    PostQuitMessage, RegisterClassW, SendMessageW,
    SetWindowsHookExW, ShowWindow, TrackPopupMenu, TranslateMessage,
    UnhookWindowsHookEx, CW_USEDEFAULT, EVENT_SYSTEM_FOREGROUND, HHOOK, HMENU,
    IDI_APPLICATION, MF_STRING, MSG, SW_HIDE,
    SW_RESTORE, TPM_BOTTOMALIGN, TPM_RIGHTALIGN, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WH_KEYBOARD_LL, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_IME_CONTROL, WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
    WM_RBUTTONDOWN, WM_SYSCOMMAND, WM_USER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    SC_CLOSE, SC_MINIMIZE,
};

// --- Constants & Global State ---

const PROGRAM_NAME: &str = "ImeWatcher";
const PROGRAM_WINDOW: &str = "ImeWatcherWindow";
const WM_TRAY_CALLBACK: u32 = WM_USER + 1;
const LOCALE_NAME_MAX_LENGTH: usize = 85;
const IMC_GETOPENSTATUS: WPARAM = WPARAM(0x0005);

static mut KEYBOARD_HOOK: HHOOK = HHOOK(std::ptr::null_mut());
static mut WINDOW_HANDLE: HWND = HWND(std::ptr::null_mut());

// Helper to convert Rust string to wide string (UTF-16) with null terminator
fn to_wstring(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

// --- Logic Classes ---

struct LanguageTracker {
    queue: VecDeque<u16>,
}

impl LanguageTracker {
    fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    fn update(&mut self, new_lang: u16) {
        if self.queue.len() >= 2 {
            self.queue.pop_front();
        }
        self.queue.push_back(new_lang);
    }

    fn is_changed(&self) -> bool {
        if self.queue.is_empty() {
            return false;
        }
        self.queue.front() != self.queue.back()
    }

    fn current(&self) -> u16 {
        *self.queue.back().unwrap_or(&0x0409) // Default to English (0x0409) if empty
    }
}

static mut TRACKER: Option<LanguageTracker> = None;

fn get_tracker() -> &'static mut LanguageTracker {
    unsafe {
        if TRACKER.is_none() {
            TRACKER = Some(LanguageTracker::new());
        }
        TRACKER.as_mut().unwrap()
    }
}

// --- Tray Icon ---

struct TrayIcon {
    notify_icon_data: NOTIFYICONDATAW,
    tray_menu: HMENU,
    is_minimized: bool,
    hwnd: HWND,
}

impl TrayIcon {
    unsafe fn new(hwnd: HWND) -> Self {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY_CALLBACK;
        
        let icon_handle = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        nid.hIcon = icon_handle;

        let tip_wide = to_wstring(PROGRAM_NAME);
        let len = std::cmp::min(tip_wide.len(), nid.szTip.len() - 1);
        nid.szTip[..len].copy_from_slice(&tip_wide[..len]);
        nid.szTip[len] = 0;

        let _ = Shell_NotifyIconW(NIM_ADD, &nid);

        let menu = CreatePopupMenu().unwrap_or_default();
        let menu_settings = to_wstring("설정");
        let menu_exit = to_wstring("종료");
        
        let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR(menu_settings.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, 2, PCWSTR(menu_exit.as_ptr()));

        Self {
            notify_icon_data: nid,
            tray_menu: menu,
            is_minimized: false,
            hwnd,
        }
    }

    unsafe fn minimize(&mut self) {
        if !self.is_minimized {
            let _ = Shell_NotifyIconW(NIM_ADD, &self.notify_icon_data);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            self.is_minimized = true;
        }
    }

    unsafe fn restore(&mut self) {
        if self.is_minimized {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.notify_icon_data);
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            self.is_minimized = false;
        }
    }

    unsafe fn popup_menu(&self) {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = TrackPopupMenu(self.tray_menu, TPM_RIGHTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, 0, self.hwnd, None);
    }

    unsafe fn handle_message(&mut self, message: u32, lparam: LPARAM) {
        if message == self.notify_icon_data.uCallbackMessage {
            let low_word = (lparam.0 & 0xFFFF) as u32;
            match low_word {
                WM_RBUTTONDOWN => self.popup_menu(),
                WM_LBUTTONDBLCLK => self.restore(),
                _ => {}
            }
        }
    }
}

static mut TRAY_ICON: Option<TrayIcon> = None;

// --- Core Logic ---

unsafe fn get_keyboard_layout() -> u16 {
    let thread_id = GetWindowThreadProcessId(GetForegroundWindow(), None);
    let hkl = GetKeyboardLayout(thread_id);
    let lang_id = (hkl.0 as usize & 0xFFFF) as u16;
    lang_id
}

unsafe fn get_lang_string_from(lang_id: u16) -> String {
    let lcid = (lang_id as u32) | ((0 as u32) << 16); 

    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    let len = GetLocaleInfoW(lcid, LOCALE_SLANGUAGE, Some(&mut buffer));

    if len > 0 {
        String::from_utf16_lossy(&buffer[..(len as usize - 1)]) 
    } else {
        "Unknown Language".to_string()
    }
}

unsafe fn send_ime_changed_event_to_keyboard() {
    // TODO: Implement actual keyboard communication
}

unsafe fn update_ime_lang() {
    let hwnd = GetForegroundWindow();
    let h_ime = ImmGetDefaultIMEWnd(hwnd);
    
    let status = SendMessageW(h_ime, WM_IME_CONTROL, IMC_GETOPENSTATUS, LPARAM(0));
    let lang = get_keyboard_layout();
    
    let tracker = get_tracker();

    let status_val = status.0 as u32;

    if status_val == IME_CMODE_NATIVE.0 {
        tracker.update(lang);
    } else if status_val == IME_CMODE_ALPHANUMERIC.0 {
        tracker.update(0x0409); // English
    } else {
        tracker.update(0x0409); // Default
    }

    if tracker.is_changed() {
        let current_lang = tracker.current();
        println!("{}", get_lang_string_from(current_lang));
        send_ime_changed_event_to_keyboard();
    }
}

// --- Callbacks ---

unsafe extern "system" fn keyboard_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 {
        if w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_KEYUP {
            update_ime_lang();
        }
    }
    CallNextHookEx(KEYBOARD_HOOK, n_code, w_param, l_param)
}

unsafe extern "system" fn win_event_proc_callback(
    _h_win_event_hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _dw_event_thread: u32,
    _dwms_event_time: u32,
) {
    let foreground_window = GetForegroundWindow();
    if foreground_window.0 != std::ptr::null_mut() {
        println!("창변경 감지");
        update_ime_lang();
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, message: u32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if let Some(tray) = TRAY_ICON.as_mut() {
        tray.handle_message(message, l_param);
    }

    match message {
        WM_CREATE => {
            #[cfg(debug_assertions)]
            {
                let _ = AllocConsole();
            }
            TRAY_ICON = Some(TrayIcon::new(hwnd));
            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(tray) = TRAY_ICON.as_mut() {
                tray.minimize();
            }
            LRESULT(0)
        }
        WM_INPUTLANGCHANGE => {
            println!("키보드 레이아웃 변경 감지");
            update_ime_lang();
            LRESULT(0)
        }
        WM_SYSCOMMAND => {
            match w_param.0 as u32 & 0xFFF0 { 
                SC_MINIMIZE | SC_CLOSE => {
                    if let Some(tray) = TRAY_ICON.as_mut() {
                        tray.minimize();
                    }
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, message, w_param, l_param),
            }
        }
        WM_COMMAND => {
            let low_word = (w_param.0 & 0xFFFF) as u32;
            match low_word {
                1 => { // Settings
                    if let Some(tray) = TRAY_ICON.as_mut() {
                        tray.restore();
                    }
                }
                2 => { // Exit
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = FreeConsole();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, w_param, l_param),
    }
}

// --- Main ---

fn main() -> windows::core::Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class_name = to_wstring(PROGRAM_NAME);
        let window_name = to_wstring(PROGRAM_WINDOW);

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            Default::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(window_name.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            400,
            300,
            None,
            None,
            instance,
            None,
        );

        let hwnd = hwnd?;

        if hwnd.0 == std::ptr::null_mut() {
            return Ok(());
        }

        WINDOW_HANDLE = hwnd;

        KEYBOARD_HOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), instance, 0)?;

        let win_event_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(win_event_proc_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        update_ime_lang();

        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWDEFAULT);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if KEYBOARD_HOOK.0 != std::ptr::null_mut() {
            let _ = UnhookWindowsHookEx(KEYBOARD_HOOK);
        }
        if win_event_hook.0 != std::ptr::null_mut() {
            let _ = UnhookWinEvent(win_event_hook);
        }
    }

    Ok(())
}