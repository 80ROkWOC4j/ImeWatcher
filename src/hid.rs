use hidapi::{DeviceInfo, HidApi, HidDevice};
use log::{debug, warn};
use qmk_via_api::api::KeyboardApi;

// QMK RAW HID constants (VIA/QMK Raw HID interface)
const QMK_RAW_USAGE_PAGE: u16 = 0xFF60;
const QMK_RAW_USAGE: u16 = 0x61;

// ImeWatcher custom Raw HID protocol (requires QMK firmware support)
const IMEWATCHER_CMD: u8 = 0x21;
const IMEWATCHER_SIG: [u8; 4] = *b"IMEW";
const IMEWATCHER_OP_SET_DEFAULT_LAYER: u8 = 0x01;

const IMEWATCHER_STATUS_OK: u8 = 0x00;
const IMEWATCHER_STATUS_BAD_LAYER: u8 = 0x01;
const IMEWATCHER_STATUS_BAD_PAYLOAD: u8 = 0x02;

/// Device metadata including stable keyboard ID
#[derive(Clone, Debug)]
pub struct DeviceMetadata {
    pub keyboard_id: String,
    pub label: String,
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
}

/// Extract metadata from a HID device, generating a stable keyboard ID
pub fn extract_device_metadata(device: &DeviceInfo) -> DeviceMetadata {
    let vid = device.vendor_id();
    let pid = device.product_id();
    let usage_page = device.usage_page();

    // Build human-readable label
    let manufacturer = device.manufacturer_string().unwrap_or("Unknown");
    let product = device.product_string().unwrap_or("Device");
    let label = format!("{} {} ({:04x}:{:04x})", manufacturer, product, vid, pid);

    // Generate keyboard ID
    let keyboard_id = if let Some(serial) = device.serial_number() {
        if !serial.is_empty() {
            format!("{:04x}:{:04x}:{:04x}:sn:{}", vid, pid, usage_page, serial)
        } else {
            let path_hash = fnv1a_64_hash(device.path().to_bytes());
            format!(
                "{:04x}:{:04x}:{:04x}:path:{:016x}",
                vid, pid, usage_page, path_hash
            )
        }
    } else {
        let path_hash = fnv1a_64_hash(device.path().to_bytes());
        format!(
            "{:04x}:{:04x}:{:04x}:path:{:016x}",
            vid, pid, usage_page, path_hash
        )
    };

    DeviceMetadata {
        keyboard_id,
        label,
        vid,
        pid,
        usage_page,
    }
}

/// FNV-1a 64-bit hash function for stable path-based IDs
fn fnv1a_64_hash(data: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

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

    pub(crate) fn open_keyboard_api(&self) -> Result<KeyboardApi, String> {
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
    /// Requires QMK firmware modification to handle ImeWatcher Raw HID protocol.
    pub fn set_layer_state(&self, layer_index: u8) -> Result<(), String> {
        let device = self.open_selected_device()?;

        let mut data = [0u8; 33];
        data[0] = 0x00; // Report ID
                        // QMK-side sees 32 bytes starting from data[1]
        data[1] = IMEWATCHER_CMD;
        data[2..6].copy_from_slice(&IMEWATCHER_SIG);
        data[6] = IMEWATCHER_OP_SET_DEFAULT_LAYER;
        data[7] = layer_index;

        debug!(
            "rawhid_tx cmd=0x{:02x} sig={:02x?} op=0x{:02x} layer={} (33b host report)",
            data[1],
            &data[2..6],
            data[6],
            data[7]
        );

        device.write(&data).map_err(|e| e.to_string())?;

        // Best-effort: read response so we can log firmware status.
        let mut buf = [0u8; 33];
        match device.read_timeout(&mut buf, 120) {
            Ok(0) => {
                debug!(
                    "rawhid_rx timeout cmd=0x{:02x} op=0x{:02x} layer={} (no response)",
                    data[1], data[6], data[7]
                );
                warn!("rawhid_no_response (missing firmware handler or wrong interface/usage)");
                return Err(
                    "No response from device (firmware might not include ImeWatcher Raw HID handler)"
                        .to_string(),
                );
            }
            Ok(_n) => {
                // buf[0] is report id; firmware payload starts at buf[1]
                let rx_cmd = buf[1];
                let rx_sig = [buf[2], buf[3], buf[4], buf[5]];
                let rx_op = buf[6];
                let rx_layer = buf[7];
                let rx_status = buf[8];

                debug!(
                    "rawhid_rx cmd=0x{:02x} sig={:02x?} op=0x{:02x} layer={} status=0x{:02x}",
                    rx_cmd, rx_sig, rx_op, rx_layer, rx_status
                );

                if rx_cmd != IMEWATCHER_CMD {
                    return Err(format!(
                        "Unexpected response cmd=0x{rx_cmd:02x} (expected 0x{IMEWATCHER_CMD:02x})"
                    ));
                }

                if rx_sig != IMEWATCHER_SIG {
                    return Err(format!(
                        "Unexpected response signature={rx_sig:02x?} (expected {:02x?})",
                        IMEWATCHER_SIG
                    ));
                }

                if rx_op != IMEWATCHER_OP_SET_DEFAULT_LAYER {
                    return Err(format!(
                        "Unexpected response opcode=0x{rx_op:02x} (expected 0x{IMEWATCHER_OP_SET_DEFAULT_LAYER:02x})"
                    ));
                }

                if rx_layer != layer_index {
                    return Err(format!(
                        "Unexpected response layer={rx_layer} (expected {layer_index})"
                    ));
                }

                match rx_status {
                    IMEWATCHER_STATUS_OK => Ok(()),
                    IMEWATCHER_STATUS_BAD_LAYER => {
                        warn!("rawhid_status BAD_LAYER layer={}", rx_layer);
                        Err(format!("Device rejected layer={rx_layer} (BAD_LAYER)"))
                    }
                    IMEWATCHER_STATUS_BAD_PAYLOAD => {
                        warn!("rawhid_status BAD_PAYLOAD (likely firmware/protocol mismatch)");
                        Err(
                            "Device reported BAD_PAYLOAD (protocol mismatch or handler not active)"
                                .to_string(),
                        )
                    }
                    other => {
                        warn!("rawhid_status UNKNOWN=0x{:02x}", other);
                        Err(format!("Device reported unknown status=0x{other:02x}"))
                    }
                }
            }
            Err(e) => {
                debug!("rawhid_rx error={}", e);
                warn!("rawhid_rx_failed error={}", e);
                Err(format!("Failed to read response: {e}"))
            }
        }
    }
}
