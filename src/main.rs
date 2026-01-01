mod ime;
mod system;
mod ui;
mod utils;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetMessageW, MSG,
    RegisterClassW, SetWindowsHookExW, ShowWindow, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use windows::core::PCWSTR;

use crate::utils::{PROGRAM_NAME, PROGRAM_WINDOW, to_wstring};

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
            Some(instance.into()), // Fix: Wrap in Some() and convert HMODULE to HINSTANCE
            None,
        );

        let hwnd = hwnd?;

        if hwnd.0.is_null() {
            return Ok(());
        }

        WINDOW_HANDLE = hwnd;

        // Fix: Wrap instance in Some() and convert
        system::KEYBOARD_HOOK = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(system::keyboard_proc),
            Some(instance.into()),
            0,
        )?;

        let win_event_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(system::win_event_proc_callback),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        // Initial update removed (handled in WM_CREATE)

        let _ = ShowWindow(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWDEFAULT,
        );

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if !system::KEYBOARD_HOOK.0.is_null() {
            let _ = UnhookWindowsHookEx(system::KEYBOARD_HOOK);
        }
        if !win_event_hook.0.is_null() {
            let _ = UnhookWinEvent(win_event_hook);
        }
    }

    Ok(())
}
