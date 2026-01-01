use hidapi::HidApi;
use std::fmt::{Display, Formatter};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Globalization::{GetLocaleInfoW, LOCALE_SLANGUAGE};
use windows::Win32::UI::Input::Ime::{
    IME_CMODE_ALPHANUMERIC, IME_CMODE_NATIVE, IME_CONVERSION_MODE, ImmGetDefaultIMEWnd,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SendMessageW, WM_IME_CONTROL,
};

// https://learn.microsoft.com/ko-kr/windows/win32/intl/locale-name-constants
const LOCALE_NAME_MAX_LENGTH: usize = 85;
const IMC_GET_OPEN_STATUS: WPARAM = WPARAM(0x0005);

// QMK RAW HID Constants
const QMK_RAW_USAGE_PAGE: u16 = 0xFF60;
const QMK_RAW_USAGE: u16 = 0x61;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LangId(pub u16);

impl LangId {
    const fn english() -> Self {
        Self(0x0409)
    }
}

impl Display for LangId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let lcid = self.0 as u32;

        let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
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
}

impl LanguageTracker {
    pub fn new() -> Self {
        Self {
            current: None,
            previous: None,
        }
    }

    fn update(&mut self, new_lang: LangId) {
        self.previous = self.current;
        self.current = Some(new_lang);
    }

    fn is_changed(&self) -> bool {
        match (self.previous, self.current) {
            (Some(prev), Some(curr)) => prev != curr,
            _ => false,
        }
    }

    fn current(&self) -> LangId {
        self.current.unwrap_or(LangId::english())
    }

    pub fn check_and_update(&mut self) {
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

        if self.is_changed() {
            let current_lang = self.current();
            println!("{}", get_lang_string_from(current_lang));
            send_ime_changed_event_to_keyboard(current_lang);
        }
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

fn get_lang_string_from(lang_id: LangId) -> String {
    let lcid = lang_id.0 as u32;

    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    let len = unsafe { GetLocaleInfoW(lcid, LOCALE_SLANGUAGE, Some(&mut buffer)) };

    if len > 0 {
        String::from_utf16_lossy(&buffer[..(len as usize - 1)])
    } else {
        "Unknown Language".to_string()
    }
}

fn send_ime_changed_event_to_keyboard(lang_id: LangId) {
    let api = match HidApi::new() {
        Ok(api) => api,
        Err(e) => {
            eprintln!("Error initializing HID API: {}", e);
            return;
        }
    };

    // Automatically find device with QMK Raw HID Usage Page/Usage
    let device_info = api
        .device_list()
        .find(|d| d.usage_page() == QMK_RAW_USAGE_PAGE && d.usage() == QMK_RAW_USAGE);

    if let Some(info) = device_info {
        match info.open_device(&api) {
            Ok(device) => {
                let manufacturer = info.manufacturer_string().unwrap_or("Unknown");
                let product = info.product_string().unwrap_or("Unknown Device");

                // 1. Check Connection & Protocol Version
                // Command: id_get_protocol_version (0x01)
                let mut version_cmd = [0u8; 33];
                version_cmd[0] = 0x00;
                version_cmd[1] = 0x01; // id_get_protocol_version
                version_cmd[2] = 0x01; // Version 1 (asking for version)
                
                if let Err(e) = device.write(&version_cmd) {
                     eprintln!("Failed to write version command: {}", e);
                } else {
                    let mut buf = [0u8; 33];
                    match device.read_timeout(&mut buf, 100) {
                        Ok(len) if len > 0 => {
                            // println!("VIA Protocol Response: {:02x?}", &buf[..len.min(10)]);
                        },
                        _ => println!("No response to Protocol Version Check"),
                    }
                }

                let is_ime_active = lang_id != LangId::english();
                let brightness = if is_ime_active { 255 } else { 0 };

                // VIA Protocol v3: id_set_keyboard_value (0x03)
                // This is the standard way to change settings in newer VIA/QMK.
                let commands = [
                    (0x01, "Backlight Brightness"),
                    (0x03, "RGB Matrix Brightness"),
                    (0x05, "RGBLight Brightness"),
                ];

                for (id, name) in commands {
                    // Packet: [ReportID, Command, SettingID, Value]
                    let mut data = [0u8; 33];
                    data[0] = 0x00; 
                    data[1] = 0x03; // id_set_keyboard_value
                    data[2] = id;
                    data[3] = brightness;

                    if let Err(e) = device.write(&data) {
                        eprintln!("Failed to write {} command: {}", name, e);
                    } else {
                        let mut buf = [0u8; 33];
                        match device.read_timeout(&mut buf, 50) { 
                            Ok(len) if len > 0 => {
                                // Successful response to 0x03 usually echoes the command
                                // If it returns 0xFF, that ID is not supported.
                                println!("Cmd: {} (0x{:02x}) -> Response: {:02x?}", name, id, &buf[..len.min(5)]);
                            },
                            _ => {}, 
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                
                println!(
                    "Keyboard: {} {} (VID={:04x} PID={:04x})", 
                    manufacturer, product, info.vendor_id(), info.product_id()
                );
            },
            Err(e) => eprintln!("Failed to open keyboard device: {}", e),
        }
    } else {
        // Only print if we expected to find one but didn't (optional: reduce noise)
        // eprintln!("No QMK/VIA compatible keyboard found (UsagePage={:04x} Usage={:02x})", QMK_RAW_USAGE_PAGE, QMK_RAW_USAGE);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn lang_id_print_test() {
        println!("English: {}", LangId::english()); // 영어(미국)
    }
}
