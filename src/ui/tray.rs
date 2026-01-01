use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NIF_ICON, NIF_MESSAGE, NIF_TIP,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyWindow, GetCursorPos, LoadIconW,
    ShowWindow, TrackPopupMenu, IDI_APPLICATION, MF_STRING,
    SW_HIDE, SW_RESTORE, TPM_BOTTOMALIGN, TPM_RIGHTALIGN, WM_LBUTTONDBLCLK,
    WM_RBUTTONDOWN,
};

use crate::utils::{to_wstring, PROGRAM_NAME, WM_TRAY_CALLBACK};

pub struct TrayIcon {
    notify_icon_data: NOTIFYICONDATAW,
    tray_menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    is_minimized: bool,
    hwnd: HWND,
}

impl TrayIcon {
    pub unsafe fn new(hwnd: HWND) -> Self {
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

    pub unsafe fn minimize(&mut self) {
        if !self.is_minimized {
            let _ = Shell_NotifyIconW(NIM_ADD, &self.notify_icon_data);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            self.is_minimized = true;
        }
    }

    pub unsafe fn restore(&mut self) {
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

    pub unsafe fn handle_message(&mut self, message: u32, lparam: LPARAM) {
        if message == self.notify_icon_data.uCallbackMessage {
            let low_word = (lparam.0 & 0xFFFF) as u32;
            match low_word {
                WM_RBUTTONDOWN => self.popup_menu(),
                WM_LBUTTONDBLCLK => self.restore(),
                _ => {}
            }
        }
    }

    pub unsafe fn handle_command(&mut self, command_id: u32) {
         match command_id {
            1 => { // Settings
                self.restore();
            }
            2 => { // Exit
                let _ = DestroyWindow(self.hwnd);
            }
            _ => {}
        }
    }
}

// Global instance management
static mut TRAY_ICON: Option<TrayIcon> = None;

pub unsafe fn init(hwnd: HWND) {
    TRAY_ICON = Some(TrayIcon::new(hwnd));
}

pub unsafe fn handle_message(message: u32, lparam: LPARAM) {
    if let Some(tray) = TRAY_ICON.as_mut() {
        tray.handle_message(message, lparam);
    }
}

pub unsafe fn handle_command(command_id: u32) {
    if let Some(tray) = TRAY_ICON.as_mut() {
        tray.handle_command(command_id);
    }
}

pub unsafe fn minimize() {
    if let Some(tray) = TRAY_ICON.as_mut() {
        tray.minimize();
    }
}
