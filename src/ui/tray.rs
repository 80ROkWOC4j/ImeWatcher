use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyWindow, GetCursorPos, IDI_APPLICATION, LoadIconW,
    MF_STRING, SW_HIDE, SW_RESTORE, ShowWindow, TPM_BOTTOMALIGN, TPM_RIGHTALIGN, TrackPopupMenu,
    WM_LBUTTONDBLCLK, WM_RBUTTONDOWN,
};
use windows::core::PCWSTR;

use crate::utils::{PROGRAM_NAME, WM_TRAY_CALLBACK, to_wstring};

pub struct TrayIcon {
    notify_icon_data: NOTIFYICONDATAW,
    tray_menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    is_minimized: bool,
    hwnd: HWND,
}

impl TrayIcon {
    pub fn new(hwnd: HWND) -> Self {
        unsafe {
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
    }

    pub fn minimize(&mut self) {
        if !self.is_minimized {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_ADD, &self.notify_icon_data);
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.is_minimized = true;
        }
    }

    pub fn restore(&mut self) {
        if self.is_minimized {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.notify_icon_data);
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            }
            self.is_minimized = false;
        }
    }

    fn popup_menu(&self) {
        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // Fix: Pass Some(0) for nreserved
            let _ = TrackPopupMenu(
                self.tray_menu,
                TPM_RIGHTALIGN | TPM_BOTTOMALIGN,
                pt.x,
                pt.y,
                Some(0),
                self.hwnd,
                None,
            );
        }
    }

    pub fn handle_message(&mut self, message: u32, lparam: LPARAM) {
        if message == self.notify_icon_data.uCallbackMessage {
            let low_word = (lparam.0 & 0xFFFF) as u32;
            match low_word {
                WM_RBUTTONDOWN => self.popup_menu(),
                WM_LBUTTONDBLCLK => self.restore(),
                _ => {}
            }
        }
    }

    pub fn handle_command(&mut self, command_id: u32) {
        match command_id {
            1 => {
                // Settings
                self.restore();
            }
            2 => {
                // Exit
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            _ => {}
        }
    }
}
