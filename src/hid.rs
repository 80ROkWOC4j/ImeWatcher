use hidapi::{DeviceInfo, HidApi, HidDevice};

// QMK RAW HID Constants
const QMK_RAW_USAGE_PAGE: u16 = 0xFF60;
const QMK_RAW_USAGE: u16 = 0x61;

// Custom Command ID for Layer Switch (User must implement this in QMK firmware)
const CUSTOM_CMD_LAYER_SWITCH: u8 = 0x21;

// VIA custom lighting commands (VIA protocol v12)
const VIA_CUSTOM_SET_VALUE: u8 = 0x07;
const VIA_CUSTOM_GET_VALUE: u8 = 0x08;

// VIA lighting channels
const VIA_CHANNEL_BACKLIGHT: u8 = 1;
const VIA_CHANNEL_RGBLIGHT: u8 = 2;
const VIA_CHANNEL_RGB_MATRIX: u8 = 3;
const VIA_CHANNEL_LED_MATRIX: u8 = 5;

// VIA value ids
const VIA_VALUE_BRIGHTNESS: u8 = 1;
const VIA_VALUE_EFFECT: u8 = 2;
const VIA_VALUE_EFFECT_SPEED: u8 = 3;
const VIA_VALUE_COLOR: u8 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViaLightingChannel {
    Backlight,
    RgbLight,
    RgbMatrix,
    LedMatrix,
}

impl ViaLightingChannel {
    fn id(self) -> u8 {
        match self {
            Self::Backlight => VIA_CHANNEL_BACKLIGHT,
            Self::RgbLight => VIA_CHANNEL_RGBLIGHT,
            Self::RgbMatrix => VIA_CHANNEL_RGB_MATRIX,
            Self::LedMatrix => VIA_CHANNEL_LED_MATRIX,
        }
    }

    fn supports_speed_and_color(self) -> bool {
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

    pub fn capture_lighting_snapshot(&self) -> Result<LightingSnapshot, String> {
        let device = self.open_selected_device()?;

        // Prefer RGB matrix > RGB light > backlight > LED matrix.
        let candidates = [
            ViaLightingChannel::RgbMatrix,
            ViaLightingChannel::RgbLight,
            ViaLightingChannel::Backlight,
            ViaLightingChannel::LedMatrix,
        ];

        for channel in candidates {
            let brightness =
                match self.via_custom_get_u8(&device, channel.id(), VIA_VALUE_BRIGHTNESS) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

            let effect = self
                .via_custom_get_u8(&device, channel.id(), VIA_VALUE_EFFECT)
                .unwrap_or(0);

            let (effect_speed, color_hs) = if channel.supports_speed_and_color() {
                let speed = self
                    .via_custom_get_u8(&device, channel.id(), VIA_VALUE_EFFECT_SPEED)
                    .ok();
                let color = self.via_custom_get_color_hs(&device, channel.id()).ok();
                (speed, color)
            } else {
                (None, None)
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
        let device = self.open_selected_device()?;
        self.via_custom_set_u8(
            &device,
            snapshot.channel.id(),
            VIA_VALUE_BRIGHTNESS,
            brightness,
        )
    }

    pub fn restore_lighting_snapshot(&self, snapshot: &LightingSnapshot) -> Result<(), String> {
        let device = self.open_selected_device()?;
        let channel = snapshot.channel.id();

        // Restore effect first (may enable/disable the lighting engine).
        let _ = self.via_custom_set_u8(&device, channel, VIA_VALUE_EFFECT, snapshot.effect);

        if let Some(speed) = snapshot.effect_speed {
            let _ = self.via_custom_set_u8(&device, channel, VIA_VALUE_EFFECT_SPEED, speed);
        }
        if let Some((h, s)) = snapshot.color_hs {
            let _ = self.via_custom_set_color_hs(&device, channel, h, s);
        }

        self.via_custom_set_u8(&device, channel, VIA_VALUE_BRIGHTNESS, snapshot.brightness)
    }

    pub fn get_protocol_version(&self) -> Result<u16, String> {
        let device = self.open_selected_device()?;

        let mut data = [0u8; 33];
        data[0] = 0x00; // Report ID
        data[1] = 0x01; // id_get_protocol_version

        device.write(&data).map_err(|e| e.to_string())?;

        let mut buf = [0u8; 33];
        let res = device
            .read_timeout(&mut buf, 100)
            .map_err(|e| e.to_string())?;

        let offset = if res > 0 && buf[0] == 0x00 { 1 } else { 0 };

        if res > offset + 2 {
            if buf[offset] == 0x01 {
                let version = u16::from_be_bytes([buf[offset + 1], buf[offset + 2]]);
                Ok(version)
            } else {
                Err(format!(
                    "Unexpected response for protocol version: {:?}",
                    &buf[..res]
                ))
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
        let res = device
            .read_timeout(&mut buf, 100)
            .map_err(|e| e.to_string())?;

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

    fn via_custom_get_u8(
        &self,
        device: &HidDevice,
        channel_id: u8,
        value_id: u8,
    ) -> Result<u8, String> {
        let mut data = [0u8; 33];
        data[0] = 0x00;
        data[1] = VIA_CUSTOM_GET_VALUE;
        data[2] = channel_id;
        data[3] = value_id;

        device.write(&data).map_err(|e| e.to_string())?;

        let mut buf = [0u8; 33];
        let res = device
            .read_timeout(&mut buf, 100)
            .map_err(|e| e.to_string())?;
        let offset = if res > 0 && buf[0] == 0x00 { 1 } else { 0 };

        if res <= offset {
            return Err("No response from device".to_string());
        }
        let cmd = buf[offset];
        if cmd == 0xFF {
            return Err("VIA custom get value unhandled".to_string());
        }
        if cmd != VIA_CUSTOM_GET_VALUE {
            return Err(format!("Unexpected VIA response: cmd=0x{cmd:02x}"));
        }
        if res <= offset + 3 {
            return Err("Short VIA response".to_string());
        }

        Ok(buf[offset + 3])
    }

    fn via_custom_get_color_hs(
        &self,
        device: &HidDevice,
        channel_id: u8,
    ) -> Result<(u8, u8), String> {
        let mut data = [0u8; 33];
        data[0] = 0x00;
        data[1] = VIA_CUSTOM_GET_VALUE;
        data[2] = channel_id;
        data[3] = VIA_VALUE_COLOR;

        device.write(&data).map_err(|e| e.to_string())?;

        let mut buf = [0u8; 33];
        let res = device
            .read_timeout(&mut buf, 100)
            .map_err(|e| e.to_string())?;
        let offset = if res > 0 && buf[0] == 0x00 { 1 } else { 0 };

        if res <= offset {
            return Err("No response from device".to_string());
        }
        let cmd = buf[offset];
        if cmd == 0xFF {
            return Err("VIA custom get value unhandled".to_string());
        }
        if cmd != VIA_CUSTOM_GET_VALUE {
            return Err(format!("Unexpected VIA response: cmd=0x{cmd:02x}"));
        }
        if res <= offset + 4 {
            return Err("Short VIA response".to_string());
        }

        Ok((buf[offset + 3], buf[offset + 4]))
    }

    fn via_custom_set_u8(
        &self,
        device: &HidDevice,
        channel_id: u8,
        value_id: u8,
        value: u8,
    ) -> Result<(), String> {
        let mut data = [0u8; 33];
        data[0] = 0x00;
        data[1] = VIA_CUSTOM_SET_VALUE;
        data[2] = channel_id;
        data[3] = value_id;
        data[4] = value;

        device.write(&data).map_err(|e| e.to_string())?;
        let mut buf = [0u8; 33];
        let _ = device.read_timeout(&mut buf, 20);
        Ok(())
    }

    fn via_custom_set_color_hs(
        &self,
        device: &HidDevice,
        channel_id: u8,
        hue: u8,
        sat: u8,
    ) -> Result<(), String> {
        let mut data = [0u8; 33];
        data[0] = 0x00;
        data[1] = VIA_CUSTOM_SET_VALUE;
        data[2] = channel_id;
        data[3] = VIA_VALUE_COLOR;
        data[4] = hue;
        data[5] = sat;

        device.write(&data).map_err(|e| e.to_string())?;
        let mut buf = [0u8; 33];
        let _ = device.read_timeout(&mut buf, 20);
        Ok(())
    }
}
