use crate::ime::LangId;
use hidapi::{DeviceInfo, HidApi, HidDevice};

// QMK RAW HID Constants
const QMK_RAW_USAGE_PAGE: u16 = 0xFF60;
const QMK_RAW_USAGE: u16 = 0x61;

// Custom Command ID for Layer Switch (User must implement this in QMK firmware)
const CUSTOM_CMD_LAYER_SWITCH: u8 = 0x21;

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
        let device = self.open_selected_device()?;
        self.send_lighting_command(&device, lang_id)
    }

    fn open_selected_device(&self) -> Result<HidDevice, String> {
        let path_str = self
            .selected_device_path
            .as_ref()
            .ok_or("No device selected")?;
        let path = std::ffi::CString::new(path_str.as_str()).map_err(|_| "Invalid path")?;
        self.api
            .open_path(&path)
            .map_err(|e| format!("Failed to open device: {}", e))
    }

    pub fn get_protocol_version(&self) -> Result<u16, String> {
        let device = self.open_selected_device()?;

        let mut data = [0u8; 33];
        data[0] = 0x00; // Report ID
        data[1] = 0x01; // id_get_protocol_version

        device.write(&data).map_err(|e| e.to_string())?;

        let mut buf = [0u8; 33];
        let res = device.read_timeout(&mut buf, 100).map_err(|e| e.to_string())?;

        let offset = if res > 0 && buf[0] == 0x00 { 1 } else { 0 };

        if res > offset + 2 {
            if buf[offset] == 0x01 {
                let version = u16::from_be_bytes([buf[offset+1], buf[offset+2]]);
                Ok(version)
            } else {
                Err(format!("Unexpected response for protocol version: {:?}", &buf[..res]))
            }
        } else {
            Err("No response from device for protocol version".to_string())
        }
    }

    pub fn get_layer_count(&self) -> Result<u8, String> {
        let device = self.open_selected_device()?;
        
        let mut data = [0u8; 33];
        data[0] = 0x00; // Report ID
        data[1] = 0x11; // id_dynamic_keymap_get_layer_count

        device.write(&data).map_err(|e| e.to_string())?;

        let mut buf = [0u8; 33];
        let res = device.read_timeout(&mut buf, 100).map_err(|e| e.to_string())?;

        let offset = if res > 0 && buf[0] == 0x00 { 1 } else { 0 };

        if res > offset + 1 {
            if buf[offset] == 0x11 {
                Ok(buf[offset + 1])
            } else {
                Ok(buf[offset])
            }
        } else {
            Err("No response from device for get_layer_count".to_string())
        }
    }

    /// Sends a custom command to switch layers.
    /// Requires QMK firmware modification to handle command ID 0x21.
    pub fn set_layer_state(&self, layer_index: u8) -> Result<(), String> {
        let device = self.open_selected_device()?;

        let mut data = [0u8; 33];
        data[0] = 0x00; // Report ID
        data[1] = CUSTOM_CMD_LAYER_SWITCH; // Custom Command
        data[2] = layer_index;

        device.write(&data).map_err(|e| e.to_string())?;

        // We don't expect a standard VIA response for a custom command unless programmed.
        // Just checking if write succeeded is often enough, or we can read garbage.
        let mut buf = [0u8; 33];
        let _ = device.read_timeout(&mut buf, 20);

        Ok(())
    }

    fn send_lighting_command(&self, device: &HidDevice, lang_id: LangId) -> Result<(), String> {
        let is_ime_active = lang_id != LangId::english();
        let brightness = if is_ime_active { 255 } else { 0 };

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

            let mut buf = [0u8; 33];
            let _ = device.read_timeout(&mut buf, 20);
        }

        Ok(())
    }
}
