#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod hid;
mod ime;
mod logging;
mod system;
mod ui;
mod utils;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
        WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };

    use native_windows_gui as nwg;

    logging::init();

    nwg::init()?;
    nwg::enable_visual_styles();

    let ui = ui::nwg_app::AppUi::build()?;

    unsafe {
        let instance = GetModuleHandleW(None)?;

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

        ui.request_ime_update();
        nwg::dispatch_thread_events();

        if !system::KEYBOARD_HOOK.0.is_null() {
            let _ = UnhookWindowsHookEx(system::KEYBOARD_HOOK);
        }
        if !win_event_hook.0.is_null() {
            let _ = UnhookWinEvent(win_event_hook);
        }
    }

    Ok(())
}
