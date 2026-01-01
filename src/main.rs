mod ime;
mod system;
mod ui;
mod utils;

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, GetMessageW, RegisterClassW, ShowWindow,
    TranslateMessage, CW_USEDEFAULT, EVENT_SYSTEM_FOREGROUND, MSG,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
};

use crate::utils::{to_wstring, PROGRAM_NAME, PROGRAM_WINDOW};

static mut WINDOW_HANDLE: HWND = HWND(std::ptr::null_mut());

fn main() -> windows::core::Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class_name = to_wstring(PROGRAM_NAME);
        let window_name = to_wstring(PROGRAM_WINDOW);

        let wc = WNDCLASSW {
            lpfnWndProc: Some(system::wnd_proc),
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

        system::KEYBOARD_HOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(system::keyboard_proc), instance, 0)?;

        let win_event_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(system::win_event_proc_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        // Initial update
        ime::update_ime_lang();

        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWDEFAULT);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if system::KEYBOARD_HOOK.0 != std::ptr::null_mut() {
            let _ = UnhookWindowsHookEx(system::KEYBOARD_HOOK);
        }
        if win_event_hook.0 != std::ptr::null_mut() {
            let _ = UnhookWinEvent(win_event_hook);
        }
    }

    Ok(())
}