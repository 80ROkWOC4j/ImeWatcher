use std::cell::RefCell;
use std::collections::VecDeque;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::{GetLocaleInfoW, LOCALE_SLANGUAGE};
use windows::Win32::UI::Input::Ime::{
    IME_CMODE_ALPHANUMERIC, IME_CMODE_NATIVE, ImmGetDefaultIMEWnd,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SendMessageW, WM_IME_CONTROL,
};

// https://learn.microsoft.com/ko-kr/windows/win32/intl/locale-name-constants
const LOCALE_NAME_MAX_LENGTH: usize = 85;
const IMC_GET_OPEN_STATUS: WPARAM = WPARAM(0x0005);

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

thread_local! {
    static TRACKER: RefCell<LanguageTracker> = RefCell::new(LanguageTracker::new());
}

unsafe fn get_keyboard_layout() -> u16 {
    let hkl = unsafe {
        let thread_id = GetWindowThreadProcessId(GetForegroundWindow(), None);
        GetKeyboardLayout(thread_id)
    };
    let lang_id = (hkl.0 as usize & 0xFFFF) as u16;
    lang_id
}

fn get_lang_string_from(lang_id: u16) -> String {
    let lcid = (lang_id as u32) | ((0 as u32) << 16);

    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    let len = unsafe { GetLocaleInfoW(lcid, LOCALE_SLANGUAGE, Some(&mut buffer)) };

    if len > 0 {
        String::from_utf16_lossy(&buffer[..(len as usize - 1)])
    } else {
        "Unknown Language".to_string()
    }
}

fn send_ime_changed_event_to_keyboard() {
    // TODO: Implement actual keyboard communication
}

pub unsafe fn update_ime_lang() {
    // Fix: Wrap WPARAM/LPARAM in Some() as required by windows crate 0.62+
    let status = unsafe {
        let hwnd = GetForegroundWindow();
        let h_ime = ImmGetDefaultIMEWnd(hwnd);
        SendMessageW(
            h_ime,
            WM_IME_CONTROL,
            Some(IMC_GET_OPEN_STATUS),
            Some(LPARAM(0)),
        )
    };
    let lang = unsafe { get_keyboard_layout() };

    let status_val = status.0 as u32;

    TRACKER.with(|tracker_cell| {
        let mut tracker = tracker_cell.borrow_mut();

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
    });
}
