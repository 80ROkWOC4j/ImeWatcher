use log::{debug, info, warn};

use crate::config::{ViaAudioConfig, ViaLangConfig, ViaLightingConfig};
use crate::hid::{HidManager, ViaLightingChannel};

fn supports_speed(channel: ViaLightingChannel) -> bool {
    matches!(
        channel,
        ViaLightingChannel::RgbLight
            | ViaLightingChannel::RgbMatrix
            | ViaLightingChannel::LedMatrix
    )
}

fn supports_color(channel: ViaLightingChannel) -> bool {
    matches!(
        channel,
        ViaLightingChannel::RgbLight | ViaLightingChannel::RgbMatrix
    )
}

fn detect_lighting_channel(hm: &HidManager) -> Result<ViaLightingChannel, String> {
    let api = hm.open_keyboard_api()?;

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

        if brightness.is_ok() {
            return Ok(channel);
        }
    }

    Err("No supported VIA lighting channel found".to_string())
}

fn apply_lighting(hm: &HidManager, cfg: &ViaLightingConfig) -> Result<(), String> {
    let api = hm.open_keyboard_api()?;
    let channel = detect_lighting_channel(hm)?;
    debug!("via_lighting_channel={:?}", channel);

    // Apply effect first (may enable/disable the lighting engine).
    if let Some(effect) = cfg.effect {
        let r = match channel {
            ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_effect(effect),
            ViaLightingChannel::RgbLight => api.set_rgblight_effect(effect),
            ViaLightingChannel::Backlight => api.set_backlight_effect(effect),
            ViaLightingChannel::LedMatrix => api.set_led_matrix_effect(effect),
        };
        if let Err(e) = r {
            warn!(
                "via_lighting_set_effect_failed channel={:?} error={}",
                channel, e
            );
        }
    }

    if supports_speed(channel) {
        if let Some(speed) = cfg.speed {
            let r = match channel {
                ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_effect_speed(speed),
                ViaLightingChannel::RgbLight => api.set_rgblight_effect_speed(speed),
                ViaLightingChannel::LedMatrix => api.set_led_matrix_effect_speed(speed),
                ViaLightingChannel::Backlight => Ok(()),
            };
            if let Err(e) = r {
                warn!(
                    "via_lighting_set_speed_failed channel={:?} error={}",
                    channel, e
                );
            }
        }
    } else if cfg.speed.is_some() {
        debug!("via_lighting_skip_speed channel={:?}", channel);
    }

    if supports_color(channel) {
        if let (Some(h), Some(s)) = (cfg.color_h, cfg.color_s) {
            let r = match channel {
                ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_color(h, s),
                ViaLightingChannel::RgbLight => api.set_rgblight_color(h, s),
                ViaLightingChannel::Backlight | ViaLightingChannel::LedMatrix => Ok(()),
            };
            if let Err(e) = r {
                warn!(
                    "via_lighting_set_color_failed channel={:?} error={}",
                    channel, e
                );
            }
        }
    } else if cfg.color_h.is_some() || cfg.color_s.is_some() {
        debug!("via_lighting_skip_color channel={:?}", channel);
    }

    if let Some(brightness) = cfg.brightness {
        let r = match channel {
            ViaLightingChannel::RgbMatrix => api.set_rgb_matrix_brightness(brightness),
            ViaLightingChannel::RgbLight => api.set_rgblight_brightness(brightness),
            ViaLightingChannel::Backlight => api.set_backlight_brightness(brightness),
            ViaLightingChannel::LedMatrix => api.set_led_matrix_brightness(brightness),
        };
        if let Err(e) = r {
            warn!(
                "via_lighting_set_brightness_failed channel={:?} error={}",
                channel, e
            );
        }
    }

    Ok(())
}

fn apply_audio(hm: &HidManager, cfg: &ViaAudioConfig) -> Result<(), String> {
    let api = hm.open_keyboard_api()?;

    if let Some(enabled) = cfg.enabled {
        if let Err(e) = api.set_audio_enabled(enabled) {
            warn!("via_audio_set_enabled_failed error={}", e);
        }
    }

    if let Some(clicky) = cfg.clicky {
        if let Err(e) = api.set_audio_clicky_enabled(clicky) {
            warn!("via_audio_set_clicky_failed error={}", e);
        }
    }

    Ok(())
}

pub fn apply_via_lang_config(
    hm: &HidManager,
    keyboard_id: &str,
    lang_key: &str,
    cfg: &ViaLangConfig,
) {
    let has_lighting = cfg.lighting.is_some();
    let has_audio = cfg.audio.is_some();

    if !has_lighting && !has_audio {
        debug!(
            "via_apply_skipped keyboard={} lang={} reason=empty",
            keyboard_id, lang_key
        );
        return;
    }

    info!(
        "via_apply_start keyboard={} lang={} lighting={} audio={}",
        keyboard_id, lang_key, has_lighting, has_audio
    );

    if let Some(ref lighting) = cfg.lighting {
        if let Err(e) = apply_lighting(hm, lighting) {
            warn!(
                "via_apply_lighting_failed keyboard={} lang={} error={}",
                keyboard_id, lang_key, e
            );
        } else {
            debug!(
                "via_apply_lighting_ok keyboard={} lang={}",
                keyboard_id, lang_key
            );
        }
    }

    if let Some(ref audio) = cfg.audio {
        if let Err(e) = apply_audio(hm, audio) {
            warn!(
                "via_apply_audio_failed keyboard={} lang={} error={}",
                keyboard_id, lang_key, e
            );
        } else {
            debug!(
                "via_apply_audio_ok keyboard={} lang={}",
                keyboard_id, lang_key
            );
        }
    }
}
