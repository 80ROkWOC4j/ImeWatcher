use hidapi::{DeviceInfo, HidApi, HidDevice};
use qmk_via_api::api::KeyboardApi;

// QMK RAW HID constants (VIA/QMK Raw HID interface)
const QMK_RAW_USAGE_PAGE: u16 = 0xFF60;
const QMK_RAW_USAGE: u16 = 0x61;

// Custom Command ID for Layer Switch (requires QMK firmware support)
const CUSTOM_CMD_LAYER_SWITCH: u8 = 0x21;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViaLightingChannel {
    Backlight,
    RgbLight,
    RgbMatrix,
    LedMatrix,
}

impl ViaLightingChannel {
    fn supports_speed(self) -> bool {
        matches!(self, Self::RgbLight | Self::RgbMatrix | Self::LedMatrix)
    }

    fn supports_color(self) -> bool {
        matches!(self, Self::RgbLight | Self::RgbMatrix)
    }
}

#[derive(Clone, Debug)]
pub struct LightingSnapshot {
    pub channel: ViaLightingChannel,
    pub brightness: u8,
    pub effect: u8,
    pub effect_speed: Option<u8>,
    pub color_hs: Option<(u8, u8)>,
}

#[derive(Clone, Debug)]
struct SelectedDevice {
    path: String,
    vendor_id: u16,
    product_id: u16,
    usage_page: u16,
}

pub struct HidManager {
    api: HidApi,
    selected_device: Option<SelectedDevice>,
}

impl HidManager {
    pub fn new() -> Result<Self, String> {
        let api = HidApi::new().map_err(|e| e.to_string())?;
        Ok(Self {
            api,
            selected_device: None,
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

    pub fn select_device(&mut self, device: &DeviceInfo) {
        self.selected_device = Some(SelectedDevice {
            path: device.path().to_string_lossy().to_string(),
            vendor_id: device.vendor_id(),
            product_id: device.product_id(),
            usage_page: device.usage_page(),
        });
    }

    fn selected(&self) -> Result<&SelectedDevice, String> {
        self.selected_device
            .as_ref()
            .ok_or("No device selected".to_string())
    }

    fn open_selected_device(&self) -> Result<HidDevice, String> {
        let path_str = &self.selected()?.path;
        let path = std::ffi::CString::new(path_str.as_str()).map_err(|_| "Invalid path")?;
        self.api
            .open_path(&path)
            .map_err(|e| format!("Failed to open device: {e}"))
    }

    fn open_keyboard_api(&self) -> Result<KeyboardApi, String> {
        let sel = self.selected()?;
        KeyboardApi::new(sel.vendor_id, sel.product_id, sel.usage_page).map_err(|e| e.to_string())
    }

    pub fn capture_lighting_snapshot(&self) -> Result<LightingSnapshot, String> {
        let api = self.open_keyboard_api()?;

        // Prefer RGB matrix > RGB light > backlight > LED matrix.
        let candidates = [
            ViaLightingChannel::RgbMatrix,
            ViaLightingChannel::RgbLight,
            ViaLightingChannel::Backlight,
            ViaLightingChannel::LedMatrix,
        ];

        for channel in candidates {
            let brightness = match channel {
                ViaLightingChannel::RgbMatrix => api.get_rgb_matrix_brightness(),
                ViaLightingChannel::RgbLight => api.get_rgblight_brightness(),
                ViaLightingChannel::Backlight => api.get_backlight_brightness(),
                ViaLightingChannel::LedMatrix => api.get_led_matrix_brightness(),
            };

            let brightness = match brightness {
                Ok(v) => v,
                Err(_) => continue,
            };

            let effect = match channel {
                ViaLightingChannel::RgbMatrix => api.get_rgb_matrix_effect().unwrap_or(0),
                ViaLightingChannel::RgbLight => api.get_rgblight_effect().unwrap_or(0),
                ViaLightingChannel::Backlight => api.get_backlight_effect().unwrap_or(0),
                ViaLightingChannel::LedMatrix => api.get_led_matrix_effect().unwrap_or(0),
            };

            let effect_speed = if channel.supports_speed() {
                match channel {
                    ViaLightingChannel::RgbMatrix => api.get_rgb_matrix_effect_speed().ok(),
                    ViaLightingChannel::RgbLight => api.get_rgblight_effect_speed().ok(),
                    ViaLightingChannel::LedMatrix => api.get_led_matrix_effect_speed().ok(),
                    ViaLightingChannel::Backlight => None,
                }
            } else {
                None
            };

            let color_hs = if channel.supports_color() {
                match channel {
                    ViaLightingChannel::RgbMatrix => api.get_rgb_matrix_color().ok(),
                    ViaLightingChannel::RgbLight => api.get_rgblight_color().ok(),
                    ViaLightingChannel::Backlight | ViaLightingChannel::LedMatrix => None,
                }
            } else {
                None
            };

            return Ok(LightingSnapshot {
                channel,
                brightness,
                effect,
                effect_speed,
                color_hs,
            });
        }

        Err("No supported VIA lighting channel found".to_string())
    }

    pub fn set_snapshot_brightness(
        &self,
        snapshot: &LightingSnapshot,
        brightness: u8,
    ) -> Result<(), String> {
        let api = self.open_keyboard_api()?;

        match snapshot.channel {
            ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_brightness(brightness),
            ViaLightingChannel::RgbLight => api.set_rgblight_brightness(brightness),
            ViaLightingChannel::Backlight => api.set_backlight_brightness(brightness),
            ViaLightingChannel::LedMatrix => api.set_led_matrix_brightness(brightness),
        }
        .map_err(|e| e.to_string())
    }

    pub fn restore_lighting_snapshot(&self, snapshot: &LightingSnapshot) -> Result<(), String> {
        let api = self.open_keyboard_api()?;

        // Restore effect first (may enable/disable the lighting engine).
        let _ = match snapshot.channel {
            ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_effect(snapshot.effect),
            ViaLightingChannel::RgbLight => api.set_rgblight_effect(snapshot.effect),
            ViaLightingChannel::Backlight => api.set_backlight_effect(snapshot.effect),
            ViaLightingChannel::LedMatrix => api.set_led_matrix_effect(snapshot.effect),
        };

        if let Some(speed) = snapshot.effect_speed {
            let _ = match snapshot.channel {
                ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_effect_speed(speed),
                ViaLightingChannel::RgbLight => api.set_rgblight_effect_speed(speed),
                ViaLightingChannel::LedMatrix => api.set_led_matrix_effect_speed(speed),
                ViaLightingChannel::Backlight => Ok(()),
            };
        }

        if let Some((h, s)) = snapshot.color_hs {
            let _ = match snapshot.channel {
                ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_color(h, s),
                ViaLightingChannel::RgbLight => api.set_rgblight_color(h, s),
                ViaLightingChannel::Backlight | ViaLightingChannel::LedMatrix => Ok(()),
            };
        }

        match snapshot.channel {
            ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_brightness(snapshot.brightness),
            ViaLightingChannel::RgbLight => api.set_rgblight_brightness(snapshot.brightness),
            ViaLightingChannel::Backlight => api.set_backlight_brightness(snapshot.brightness),
            ViaLightingChannel::LedMatrix => api.set_led_matrix_brightness(snapshot.brightness),
        }
        .map_err(|e| e.to_string())
    }

    pub fn get_protocol_version(&self) -> Result<u16, String> {
        self.open_keyboard_api()?
            .get_protocol_version()
            .map_err(|e| e.to_string())
    }

    pub fn get_layer_count(&self) -> Result<u8, String> {
        self.open_keyboard_api()?
            .get_layer_count()
            .map_err(|e| e.to_string())
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

        // Ignore response (custom command may not return anything useful).
        let mut buf = [0u8; 33];
        let _ = device.read_timeout(&mut buf, 20);

        Ok(())
    }
}
