use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::{GetLocaleInfoW, LOCALE_SLANGUAGE};
use windows::Win32::System::SystemServices::LOCALE_NAME_MAX_LENGTH;
use windows::Win32::UI::Input::Ime::{
    IME_CMODE_ALPHANUMERIC, IME_CMODE_NATIVE, IME_CONVERSION_MODE, ImmGetDefaultIMEWnd,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SendMessageW, WM_IME_CONTROL,
};

// IMC_GETOPENSTATUS, 혹은 IMC_GETCONVERSIONMODE 사용해야 함
// 현재 windows crate에서 해당 값 찾을 수 없어 여기서 정의
// https://learn.microsoft.com/en-us/previous-versions/windows/embedded/ms920838(v=msdn.10)
// https://learn.microsoft.com/en-us/previous-versions/windows/embedded/ms905959(v=msdn.10)
const IMC_GET_OPEN_STATUS: WPARAM = WPARAM(0x0005);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LangId(pub u16);

// https://learn.microsoft.com/ko-kr/windows/win32/intl/language-identifiers 참고
impl LangId {
    pub const fn english() -> Self {
        Self(0x0409)
    }
}

impl Display for LangId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let lcid = self.0 as u32;

        let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH as usize];
        let len = unsafe { GetLocaleInfoW(lcid, LOCALE_SLANGUAGE, Some(&mut buffer)) };

        // os 언어에 따라 다른 결과가 나옴
        if len > 0 {
            f.write_str(&String::from_utf16_lossy(&buffer[..(len as usize - 1)]))
        } else {
            f.write_str("Unknown Language")
        }
    }
}

pub struct LanguageTracker {
    current: Option<LangId>,
    previous: Option<LangId>,
    pub detected_langs: HashSet<LangId>,
}

impl LanguageTracker {
    pub fn new() -> Self {
        let mut detected_langs = HashSet::new();
        detected_langs.insert(LangId::english()); // Start with English

        Self {
            current: None,
            previous: None,
            detected_langs,
        }
    }

    fn update(&mut self, new_lang: LangId) {
        self.previous = self.current;
        self.current = Some(new_lang);
        self.detected_langs.insert(new_lang);
        if self.is_changed() {
            println!("current lang: {new_lang}");
        }
    }

    fn is_changed(&self) -> bool {
        match (self.previous, self.current) {
            (Some(prev), Some(curr)) => prev != curr,
            _ => false,
        }
    }

    pub fn current(&self) -> LangId {
        self.current.unwrap_or(LangId::english())
    }

    pub fn check_and_update(&mut self) -> bool {
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

        match IME_CONVERSION_MODE(status.0 as u32) {
            IME_CMODE_NATIVE => {
                self.update(get_keyboard_layout());
            }
            IME_CMODE_ALPHANUMERIC => {
                self.update(LangId::english()); // English
            }
            _ => {
                self.update(LangId::english()); // Default
            }
        }

        self.is_changed()
    }
}

fn get_keyboard_layout() -> LangId {
    let hkl = unsafe {
        let thread_id = GetWindowThreadProcessId(GetForegroundWindow(), None);
        GetKeyboardLayout(thread_id)
    };
    let lang_id = (hkl.0 as usize & 0xFFFF) as u16;
    LangId(lang_id)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn lang_id_print_test() {
        println!("English: {}", LangId::english()); // 영어(미국)
    }
}
