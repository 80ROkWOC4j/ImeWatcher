use crate::ime::LangId;
use hidapi::{DeviceInfo, HidApi, HidDevice};

// QMK RAW HID Constants
const QMK_RAW_USAGE_PAGE: u16 = 0xFF60;
const QMK_RAW_USAGE: u16 = 0x61;

pub struct HidManager {
    api: HidApi,
    selected_device_path: Option<String>,
}

impl HidManager {
    pub fn new() -> Result<Self, String> {
        let api = HidApi::new().map_err(|e| e.to_string())?;
        Ok(Self {
            api,
            selected_device_path: None,
        })
    }

    pub fn list_devices(&mut self) -> Vec<DeviceInfo> {
        // Refresh devices? HidApi::new() refreshes.
        // If we want to refresh, we might need to recreate HidApi or use refresh_devices() if available.
        // rust-hidapi's HidApi::refresh_devices() scans again.
        let _ = self.api.refresh_devices();

        self.api
            .device_list()
            .filter(|d| d.usage_page() == QMK_RAW_USAGE_PAGE && d.usage() == QMK_RAW_USAGE)
            .cloned()
            .collect()
    }

    pub fn select_device(&mut self, path: String) {
        self.selected_device_path = Some(path);
    }

    pub fn auto_select_first(&mut self) -> bool {
        if let Some(device) = self.list_devices().first() {
            self.selected_device_path = Some(device.path().to_string_lossy().to_string());
            return true;
        }
        false
    }

    pub fn update_lighting(&self, lang_id: LangId) -> Result<(), String> {
        let path_str = match &self.selected_device_path {
            Some(p) => p,
            None => return Err("No device selected".to_string()),
        };

        // Find the device info again to ensure it's still there (or just try open)
        // rust-hidapi open needs a CString path, usually we use open_path
        let device = self
            .api
            .open_path(
                std::ffi::CString::new(path_str.as_str())
                    .map_err(|_| "Invalid path")?
                    .as_c_str(),
            )
            .map_err(|e| format!("Failed to open device: {}", e))?;

        self.send_lighting_command(&device, lang_id)
    }

    fn send_lighting_command(&self, device: &HidDevice, lang_id: LangId) -> Result<(), String> {
        // 1. Check Connection & Protocol Version (Optional but good practice)
        // Leaving out for brevity/performance unless necessary, or we can do it once on connect.

        let is_ime_active = lang_id != LangId::english();
        let brightness = if is_ime_active { 255 } else { 0 };

        // VIA Protocol v3: id_set_keyboard_value (0x03)
        let commands = [
            (0x01, "Backlight Brightness"),
            (0x03, "RGB Matrix Brightness"),
            (0x05, "RGBLight Brightness"),
        ];

        for (id, _name) in commands {
            let mut data = [0u8; 33];
            data[0] = 0x00;
            data[1] = 0x03; // id_set_keyboard_value
            data[2] = id;
            data[3] = brightness;

            device.write(&data).map_err(|e| e.to_string())?;

            // Read response (optional but recommended to clear buffer)
            let mut buf = [0u8; 33];
            let _ = device.read_timeout(&mut buf, 20); // Short timeout
        }

        Ok(())
    }
}
