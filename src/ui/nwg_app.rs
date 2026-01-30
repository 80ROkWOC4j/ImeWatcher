use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use native_windows_gui as nwg;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::hid::{HidManager, LightingSnapshot};
use crate::ime::{LangId, LanguageTracker};
use crate::system;
use crate::utils::{PROGRAM_NAME, PROGRAM_WINDOW};

const RAW_HANDLER_ID: usize = 0x10000;

const LIGHTING_ANIM_TIMER_INTERVAL: Duration = Duration::from_millis(30);
const LIGHTING_ANIM_PULSE_1: Duration = Duration::from_millis(45);
const LIGHTING_ANIM_GAP: Duration = Duration::from_millis(70);
const LIGHTING_ANIM_PULSE_2: Duration = Duration::from_millis(45);

const UI_PADDING: i32 = 16;
const UI_COL_GAP: i32 = 16;
const UI_ROW_GAP: i32 = 12;
const UI_COL_W: i32 = 236;
const UI_ROW_H: i32 = 32;
const UI_CHECKBOX_H: i32 = 24;
const UI_SECTION_TITLE_H: i32 = 18;
const UI_SECTION_TITLE_GAP: i32 = 2;
const UI_HEADER_H: i32 = 20;
const UI_SECTION_GAP: i32 = 22;
const UI_TABLE_ROWS_GAP_TOP: i32 = 12;
const UI_WINDOW_H: i32 = 420;
const UI_LABEL_H: i32 = 22;
const UI_COMBO_Y_OFFSET: i32 = -4;

#[derive(Clone, Copy)]
struct Layout {
    window_w: i32,
    content_w: i32,
    col1_x: i32,
    col2_x: i32,
    device_label_y: i32,
    device_combo_y: i32,
    sync_checkbox_y: i32,
    header_y: i32,
    rows_start_y: i32,
}

fn layout() -> Layout {
    let content_w = (UI_COL_W * 2) + UI_COL_GAP;
    let window_w = (UI_PADDING * 2) + content_w;

    let col1_x = UI_PADDING;
    let col2_x = UI_PADDING + UI_COL_W + UI_COL_GAP;

    let device_label_y = UI_PADDING;
    let device_combo_y = UI_PADDING + UI_SECTION_TITLE_H + UI_SECTION_TITLE_GAP;
    let sync_checkbox_y = device_combo_y + UI_ROW_H + UI_ROW_GAP;
    let header_y = sync_checkbox_y + UI_CHECKBOX_H + UI_SECTION_GAP;
    let rows_start_y = header_y + UI_HEADER_H + UI_TABLE_ROWS_GAP_TOP;

    Layout {
        window_w,
        content_w,
        col1_x,
        col2_x,
        device_label_y,
        device_combo_y,
        sync_checkbox_y,
        header_y,
        rows_start_y,
    }
}

#[derive(Clone)]
pub struct AppUi {
    inner: Rc<AppInner>,
}

struct LangRow {
    lang_id: LangId,
    #[allow(dead_code)]
    label: nwg::Label,
    combo: nwg::ComboBox<String>,
    populated: bool,
}

enum LightingAnimTick {
    NoChange,
    SetBrightness(u8),
    Done,
}

struct LightingAnim {
    steps: Vec<(u8, Duration)>,
    index: usize,
    next_at: Instant,

    snapshot: LightingSnapshot,
}

impl LightingAnim {
    fn new(snapshot: LightingSnapshot) -> Self {
        let target_brightness = snapshot.brightness;
        let steps = vec![
            (0, LIGHTING_ANIM_PULSE_1),
            (target_brightness, LIGHTING_ANIM_GAP),
            (0, LIGHTING_ANIM_PULSE_2),
            (target_brightness, Duration::from_millis(0)),
        ];

        let now = Instant::now();
        let next_at = now + steps[0].1;
        Self {
            steps,
            index: 0,
            next_at,
            snapshot,
        }
    }

    fn current_brightness(&self) -> u8 {
        self.steps.get(self.index).map(|(b, _)| *b).unwrap_or(0)
    }

    fn tick(&mut self) -> LightingAnimTick {
        let now = Instant::now();
        if now < self.next_at {
            return LightingAnimTick::NoChange;
        }

        self.index += 1;
        if self.index >= self.steps.len() {
            return LightingAnimTick::Done;
        }
        let (b, dur) = self.steps[self.index];
        self.next_at = now + dur;
        LightingAnimTick::SetBrightness(b)
    }
}

struct AppModel {
    ime_tracker: LanguageTracker,
    hid_manager: Option<HidManager>,
    devices: Vec<hidapi::DeviceInfo>,

    layer_count: u8,
    sync_enabled: bool,
    layer_config: HashMap<LangId, Option<u8>>,
    lang_rows: Vec<LangRow>,

    lighting_anim: Option<LightingAnim>,
}

struct AppInner {
    // Resources
    #[allow(dead_code)]
    icon: nwg::Icon,

    #[allow(dead_code)]
    font_ui: Option<nwg::Font>,

    #[allow(dead_code)]
    font_header: Option<nwg::Font>,

    #[allow(dead_code)]
    font_ui_bold: Option<nwg::Font>,

    // Tray
    #[allow(dead_code)]
    tray_window: nwg::MessageWindow,
    tray: nwg::TrayNotification,
    tray_menu: nwg::Menu,
    tray_settings: nwg::MenuItem,
    tray_exit: nwg::MenuItem,

    // Main window + controls
    window: nwg::Window,

    #[allow(dead_code)]
    device_label: nwg::Label,
    device_combo: nwg::ComboBox<String>,
    sync_checkbox: nwg::CheckBox,
    #[allow(dead_code)]
    header_lang: nwg::Label,
    #[allow(dead_code)]
    header_layer: nwg::Label,

    poll_timer: nwg::AnimationTimer,
    lighting_anim_timer: nwg::AnimationTimer,

    // State
    model: RefCell<AppModel>,

    // Re-entrancy guard: NWG control calls can re-enter the event loop.
    in_model_update: Cell<bool>,

    // Event handler handles (must be kept alive)
    handlers: RefCell<Vec<nwg::EventHandler>>,
    raw_handler: RefCell<Option<nwg::RawEventHandler>>,
}

impl AppUi {
    pub fn build() -> Result<Self, Box<dyn std::error::Error>> {
        let l = layout();

        let mut icon = nwg::Icon::default();
        nwg::Icon::builder()
            .source_system(Some(nwg::OemIcon::WinLogo))
            .build(&mut icon)?;

        // Font fallback:
        // - Prefer Segoe UI when available.
        // - Otherwise, let NWG/Win32 use the global default font.
        let mut font_ui = nwg::Font::default();
        let font_ui = if nwg::Font::builder()
            .family("Segoe UI")
            .size_absolute(14)
            .build(&mut font_ui)
            .is_ok()
        {
            Some(font_ui)
        } else {
            None
        };

        let mut font_header = nwg::Font::default();
        let font_header = if nwg::Font::builder()
            .family("Segoe UI")
            .size_absolute(14)
            .weight(600)
            .build(&mut font_header)
            .is_ok()
        {
            Some(font_header)
        } else {
            None
        };

        let mut font_ui_bold = nwg::Font::default();
        let font_ui_bold = if nwg::Font::builder()
            .family("Segoe UI")
            .size_absolute(14)
            .weight(600)
            .build(&mut font_ui_bold)
            .is_ok()
        {
            Some(font_ui_bold)
        } else {
            None
        };

        let mut tray_window = nwg::MessageWindow::default();
        nwg::MessageWindow::builder().build(&mut tray_window)?;

        let mut tray = nwg::TrayNotification::default();
        nwg::TrayNotification::builder()
            .parent(&tray_window)
            .icon(Some(&icon))
            .tip(Some(PROGRAM_NAME))
            .build(&mut tray)?;

        let mut tray_menu = nwg::Menu::default();
        nwg::Menu::builder()
            .popup(true)
            .parent(&tray_window)
            .build(&mut tray_menu)?;

        let mut tray_settings = nwg::MenuItem::default();
        nwg::MenuItem::builder()
            .text("Settings")
            .parent(&tray_menu)
            .build(&mut tray_settings)?;

        let mut tray_exit = nwg::MenuItem::default();
        nwg::MenuItem::builder()
            .text("Exit")
            .parent(&tray_menu)
            .build(&mut tray_exit)?;

        let mut window = nwg::Window::default();
        nwg::Window::builder()
            .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
            .size((l.window_w, UI_WINDOW_H))
            .position((300, 300))
            .title(PROGRAM_WINDOW)
            .build(&mut window)?;

        let mut device_label = nwg::Label::default();
        nwg::Label::builder()
            .parent(&window)
            .position((l.col1_x, l.device_label_y))
            .size((l.content_w, UI_SECTION_TITLE_H))
            .text("Keyboard")
            .font(font_header.as_ref())
            .build(&mut device_label)?;

        let mut device_combo = nwg::ComboBox::<String>::default();
        nwg::ComboBox::builder()
            .parent(&window)
            .position((l.col1_x, l.device_combo_y))
            .size((l.content_w, UI_ROW_H))
            .font(font_ui.as_ref())
            .build(&mut device_combo)?;

        let mut sync_checkbox = nwg::CheckBox::default();
        nwg::CheckBox::builder()
            .parent(&window)
            .position((l.col1_x, l.sync_checkbox_y))
            .size((l.content_w, UI_CHECKBOX_H))
            .text("Sync keyboard layer with IME")
            .font(font_ui.as_ref())
            .build(&mut sync_checkbox)?;

        let mut header_lang = nwg::Label::default();
        nwg::Label::builder()
            .parent(&window)
            .position((l.col1_x, l.header_y))
            .size((UI_COL_W, UI_HEADER_H))
            .text("Language")
            .font(font_header.as_ref())
            .build(&mut header_lang)?;

        let mut header_layer = nwg::Label::default();
        nwg::Label::builder()
            .parent(&window)
            .position((l.col2_x, l.header_y))
            .size((UI_COL_W, UI_HEADER_H))
            .text("Layer")
            .font(font_header.as_ref())
            .build(&mut header_layer)?;

        let mut poll_timer = nwg::AnimationTimer::default();
        nwg::AnimationTimer::builder()
            .parent(&window)
            .interval(Duration::from_millis(1000))
            .active(true)
            .build(&mut poll_timer)?;

        let mut lighting_anim_timer = nwg::AnimationTimer::default();
        nwg::AnimationTimer::builder()
            .parent(&window)
            .interval(LIGHTING_ANIM_TIMER_INTERVAL)
            .active(true)
            .build(&mut lighting_anim_timer)?;

        // Initialize model
        let ime_tracker = LanguageTracker::new();

        let mut hid_manager = HidManager::new().ok();
        let mut devices = Vec::new();
        if let Some(ref mut hm) = hid_manager {
            devices = hm.list_devices();
            hm.auto_select_first();
        }

        let device_items = devices
            .iter()
            .map(|dev| {
                format!(
                    "{} {} ({:04x}:{:04x})",
                    dev.manufacturer_string().unwrap_or("Unknown"),
                    dev.product_string().unwrap_or("Device"),
                    dev.vendor_id(),
                    dev.product_id()
                )
            })
            .collect::<Vec<_>>();
        device_combo.set_collection(device_items);
        if !devices.is_empty() {
            device_combo.set_selection(Some(0));
        }

        // Update global hwnd for hook callbacks
        if let Some(hwnd) = window.handle.hwnd() {
            system::set_app_hwnd(HWND(hwnd as _));
        }

        let inner = Rc::new(AppInner {
            icon,
            font_ui,
            font_header,
            font_ui_bold,
            tray_window,
            tray,
            tray_menu,
            tray_settings,
            tray_exit,
            window,
            device_label,
            device_combo,
            sync_checkbox,
            header_lang,
            header_layer,
            poll_timer,
            lighting_anim_timer,
            model: RefCell::new(AppModel {
                ime_tracker,
                hid_manager,
                devices,
                layer_count: 0,
                sync_enabled: false,
                layer_config: HashMap::new(),
                lang_rows: Vec::new(),
                lighting_anim: None,
            }),
            in_model_update: Cell::new(false),
            handlers: RefCell::new(Vec::new()),
            raw_handler: RefCell::new(None),
        });

        let ui = Self { inner };
        ui.bind_events();
        ui.sync_language_ui();
        Ok(ui)
    }

    pub fn request_ime_update(&self) {
        if let Some(hwnd) = self.inner.window.handle.hwnd() {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as _)),
                    system::WM_APP_IME_CHANGE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }

    fn sync_language_ui(&self) {
        self.inner.sync_language_ui();
    }

    fn bind_events(&self) {
        let weak: Weak<AppInner> = Rc::downgrade(&self.inner);
        let handler_main = nwg::full_bind_event_handler(
            &self.inner.window.handle,
            move |evt, evt_data, handle| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                AppInner::handle_event(&inner, evt, evt_data, handle);
            },
        );

        let weak: Weak<AppInner> = Rc::downgrade(&self.inner);
        let handler_tray = nwg::full_bind_event_handler(
            &self.inner.tray_window.handle,
            move |evt, evt_data, handle| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                AppInner::handle_event(&inner, evt, evt_data, handle);
            },
        );

        let mut handlers = self.inner.handlers.borrow_mut();
        handlers.push(handler_main);
        handlers.push(handler_tray);

        let weak: Weak<AppInner> = Rc::downgrade(&self.inner);
        let raw = nwg::bind_raw_event_handler(
            &self.inner.window.handle,
            RAW_HANDLER_ID,
            move |_hwnd, msg, _w, _l| {
                if msg == system::WM_APP_IME_CHANGE
                    && let Some(inner) = weak.upgrade()
                {
                    inner.on_ime_change();
                }
                None
            },
        )
        .expect("bind_raw_event_handler");

        *self.inner.raw_handler.borrow_mut() = Some(raw);
    }
}

impl AppInner {
    fn begin_model_update(&self) -> Option<ModelUpdateGuard<'_>> {
        if self.in_model_update.replace(true) {
            return None;
        }
        Some(ModelUpdateGuard {
            flag: &self.in_model_update,
        })
    }

    fn sync_language_ui(&self) {
        let Some(_guard) = self.begin_model_update() else {
            return;
        };

        let mut model = self.model.borrow_mut();
        let lang = model.ime_tracker.current();
        self.update_language_rows_accent(&mut model, lang);
    }

    fn update_language_rows_accent(&self, model: &mut AppModel, lang: LangId) {
        for row in &model.lang_rows {
            let base = row.lang_id.to_string();
            row.label.set_text(&base);
            if row.lang_id == lang {
                if let Some(font) = self.font_ui_bold.as_ref() {
                    row.label.set_font(Some(font));
                }
            } else if let Some(font) = self.font_ui.as_ref() {
                row.label.set_font(Some(font));
            }
        }
    }
    fn handle_event(&self, evt: nwg::Event, evt_data: nwg::EventData, handle: nwg::ControlHandle) {
        use nwg::Event as E;

        match evt {
            E::OnContextMenu if handle == self.tray => {
                let (x, y) = nwg::GlobalCursor::position();
                self.tray_menu.popup(x, y);
            }
            E::OnMenuItemSelected if handle == self.tray_settings => {
                self.window.set_visible(true);
            }
            E::OnMenuItemSelected if handle == self.tray_exit => {
                self.exit();
            }
            E::OnWindowClose if handle == self.window => {
                if let nwg::EventData::OnWindowClose(close_data) = evt_data {
                    close_data.close(false);
                }
                // Background app: closing the window hides it.
                self.window.set_visible(false);
            }
            E::OnButtonClick if handle == self.sync_checkbox => {
                self.toggle_sync();
            }
            E::OnComboxBoxSelection if handle == self.device_combo => {
                self.on_device_selection();
            }
            E::OnComboxBoxSelection => {
                self.on_dynamic_combo_selection(&handle);
            }
            E::OnTimerTick if handle == self.poll_timer => {
                self.on_timer_tick();
            }
            E::OnTimerTick if handle == self.lighting_anim_timer => {
                self.on_lighting_anim_tick();
            }
            _ => {}
        }
    }

    fn exit(&self) {
        if let Some(raw) = self.raw_handler.borrow_mut().take() {
            let _ = nwg::unbind_raw_event_handler(&raw);
        }

        let handlers = self.handlers.borrow();
        for h in handlers.iter() {
            nwg::unbind_event_handler(h);
        }

        nwg::stop_thread_dispatch();
    }

    fn toggle_sync(&self) {
        let mut model = self.model.borrow_mut();
        model.sync_enabled = !model.sync_enabled;
        let state = if model.sync_enabled {
            nwg::CheckBoxState::Checked
        } else {
            nwg::CheckBoxState::Unchecked
        };
        self.sync_checkbox.set_check_state(state);
        info!("sync_enabled={}", model.sync_enabled);
    }

    fn on_device_selection(&self) {
        let idx = self.device_combo.selection().unwrap_or(0);
        let mut model = self.model.borrow_mut();
        let device_path = model
            .devices
            .get(idx)
            .map(|d| d.path().to_string_lossy().to_string());
        let current_lang = model.ime_tracker.current();

        if let (Some(path), Some(ref mut hm)) = (device_path, model.hid_manager.as_mut()) {
            hm.select_device(path);
            match hm.get_protocol_version() {
                Ok(version) => debug!("via_protocol_version=0x{:04x}", version),
                Err(e) => warn!("via_protocol_version_error={}", e),
            }
        }

        let _ = current_lang;
    }

    fn on_dynamic_combo_selection(&self, handle: &nwg::ControlHandle) {
        let mut model = self.model.borrow_mut();

        let found = model
            .lang_rows
            .iter()
            .find(|row| handle == &row.combo)
            .map(|row| {
                let selection = row.combo.selection().unwrap_or(0) as u8;
                let target_layer = if selection == 0 {
                    None
                } else {
                    Some(selection - 1)
                };
                (row.lang_id, target_layer)
            });

        if let Some((lang_id, target_layer)) = found {
            model.layer_config.insert(lang_id, target_layer);
            info!(
                "set_layer_config lang={} target_layer={:?}",
                lang_id, target_layer
            );
        }
    }

    fn on_timer_tick(&self) {
        let Some(_guard) = self.begin_model_update() else {
            return;
        };
        let mut model = self.model.borrow_mut();

        if let Some(count) = model
            .hid_manager
            .as_ref()
            .and_then(|hm| hm.get_layer_count().ok())
            && model.layer_count != count
        {
            debug!("layer_count_changed={}", count);
            model.layer_count = count;
            for row in &mut model.lang_rows {
                row.populated = false;
            }
        }

        let layer_count = model.layer_count;
        if layer_count == 0 {
            return;
        }

        // `model` is a RefMut; clone to avoid borrow splitting issues while mutating `lang_rows`.
        let layer_config_snapshot = model.layer_config.clone();

        for row in &mut model.lang_rows {
            if row.populated {
                continue;
            }

            let mut items = Vec::with_capacity((layer_count as usize) + 1);
            items.push("Do not change".to_string());
            for i in 0..layer_count {
                items.push(format!("Layer {}", i));
            }
            row.combo.set_collection(items);

            let selection = layer_config_snapshot
                .get(&row.lang_id)
                .cloned()
                .flatten()
                .map_or(0, |v| (v + 1) as usize);
            row.combo.set_selection(Some(selection));
            row.populated = true;
        }
    }

    fn on_lighting_anim_tick(&self) {
        let Some(_guard) = self.begin_model_update() else {
            return;
        };
        let mut model = self.model.borrow_mut();

        let (tick, snapshot) = if let Some(ref mut anim) = model.lighting_anim {
            (anim.tick(), anim.snapshot.clone())
        } else {
            return;
        };

        match tick {
            LightingAnimTick::NoChange => {}
            LightingAnimTick::SetBrightness(b) => {
                if let Some(ref hm) = model.hid_manager {
                    let _ = hm.set_snapshot_brightness(&snapshot, b);
                } else {
                    model.lighting_anim = None;
                }
            }
            LightingAnimTick::Done => {
                if let Some(ref hm) = model.hid_manager {
                    let _ = hm.restore_lighting_snapshot(&snapshot);
                }
                model.lighting_anim = None;
            }
        }
    }

    fn on_ime_change(&self) {
        let Some(_guard) = self.begin_model_update() else {
            return;
        };

        let l = layout();

        // Phase 1: decide what to do without calling NWG/HID while holding the borrow.
        struct NewRowPlan {
            lang_id: LangId,
            y_pos: i32,
        }

        let (changed, active_lang, new_rows, do_layer_switch) = {
            let mut model = self.model.borrow_mut();
            let changed = model.ime_tracker.check_and_update();
            let active_lang = model.ime_tracker.current();

            let current_ui_langs: HashSet<LangId> =
                model.lang_rows.iter().map(|r| r.lang_id).collect();
            let detected_langs: Vec<LangId> =
                model.ime_tracker.detected_langs.iter().copied().collect();

            let base_index = model.lang_rows.len() as i32;
            let mut new_rows = Vec::new();
            let mut added = 0i32;
            for lang_id in detected_langs {
                if current_ui_langs.contains(&lang_id) {
                    continue;
                }
                let row_index = base_index + added;
                let y_pos = l.rows_start_y + (row_index * UI_ROW_H);
                new_rows.push(NewRowPlan { lang_id, y_pos });
                added += 1;
            }

            let do_layer_switch = changed && model.sync_enabled;

            (changed, active_lang, new_rows, do_layer_switch)
        };

        // Phase 2: create any missing UI rows (no model borrow).
        let mut created_rows = Vec::new();
        for plan in new_rows {
            let mut label = nwg::Label::default();
            nwg::Label::builder()
                .parent(&self.window)
                .position((l.col1_x, plan.y_pos))
                .size((UI_COL_W, UI_LABEL_H))
                .text(&plan.lang_id.to_string())
                .font(self.font_ui.as_ref())
                .build(&mut label)
                .expect("label build");

            let mut combo = nwg::ComboBox::<String>::default();
            nwg::ComboBox::builder()
                .parent(&self.window)
                .position((l.col2_x, plan.y_pos + UI_COMBO_Y_OFFSET))
                .size((UI_COL_W, UI_ROW_H))
                .font(self.font_ui.as_ref())
                .build(&mut combo)
                .expect("combo build");

            created_rows.push(LangRow {
                lang_id: plan.lang_id,
                label,
                combo,
                populated: false,
            });
        }

        if !created_rows.is_empty() {
            let mut model = self.model.borrow_mut();
            model.lang_rows.extend(created_rows);
        }

        // Phase 3: apply accent updates.
        {
            let mut model = self.model.borrow_mut();
            self.update_language_rows_accent(&mut model, active_lang);
        }

        // Phase 4: lighting animation (snapshot -> pulse brightness -> restore)
        if changed {
            // If an animation is already running, restore first so the next snapshot is clean.
            let prior_snapshot = {
                let model = self.model.borrow();
                model.lighting_anim.as_ref().map(|a| a.snapshot.clone())
            };
            if let Some(snapshot) = prior_snapshot {
                {
                    let model = self.model.borrow();
                    if let Some(hm) = model.hid_manager.as_ref() {
                        let _ = hm.restore_lighting_snapshot(&snapshot);
                    }
                }
                self.model.borrow_mut().lighting_anim = None;
            }

            let snapshot_res = {
                let model = self.model.borrow();
                model
                    .hid_manager
                    .as_ref()
                    .map(|hm| hm.capture_lighting_snapshot())
            };

            match snapshot_res {
                Some(Ok(snapshot)) => {
                    let anim = LightingAnim::new(snapshot);
                    let initial = anim.current_brightness();

                    // Start at first step.
                    {
                        let model = self.model.borrow();
                        if let Some(hm) = model.hid_manager.as_ref() {
                            let _ = hm.set_snapshot_brightness(&anim.snapshot, initial);
                        }
                    }

                    {
                        let mut model = self.model.borrow_mut();
                        model.lighting_anim = Some(anim);
                    }
                }
                Some(Err(e)) => warn!("lighting_snapshot_error={}", e),
                None => {}
            }
        }

        if do_layer_switch {
            let model = self.model.borrow();
            if let Some(ref hm) = model.hid_manager {
                match model.layer_config.get(&active_lang) {
                    Some(Some(target_layer)) => match hm.set_layer_state(*target_layer) {
                        Ok(_) => info!("switched_layer={}", target_layer),
                        Err(e) => warn!("set_layer_state_error={}", e),
                    },
                    Some(None) => {}
                    None => {}
                }
            }
        }
    }
}

struct ModelUpdateGuard<'a> {
    flag: &'a Cell<bool>,
}

impl Drop for ModelUpdateGuard<'_> {
    fn drop(&mut self) {
        self.flag.set(false);
    }
}
