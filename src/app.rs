use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use hidapi::HidApi;

use crate::battery;
use crate::device_scanner;

const LUNACY_VENDOR_ID: u16 = 0x0C45;
const LUNACY_PRODUCT_ID: u16 = 0x8009;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    #[default]
    Auto,
    Headset,
    Speaker,
    Keyboard,
    Mouse,
    Gamepad,
    Other,
}

impl DeviceType {
    pub const ALL: [Self; 7] = [
        Self::Auto,
        Self::Headset,
        Self::Speaker,
        Self::Keyboard,
        Self::Mouse,
        Self::Gamepad,
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Авто",
            Self::Headset => "Гарнитура",
            Self::Speaker => "Колонка",
            Self::Keyboard => "Клавиатура",
            Self::Mouse => "Мышь",
            Self::Gamepad => "Геймпад",
            Self::Other => "Другое",
        }
    }
}

/// Устройство с актуальным уровнем заряда (снимок для GUI и трея).
#[derive(Clone, Debug)]
pub struct DeviceInfoView {
    pub name: String,
    pub path: String,
    /// Заряд 0–100 %, если протокол устройства удалось прочитать.
    pub level: Option<u8>,
}

/// Состояние приложения: общий для окна egui и системного трея.
pub struct AppState {
    pub devices: Vec<DeviceInfoView>,
    pub last_scan: Option<Instant>,
    pub scan_error: Option<String>,
    /// Запрос на завершение работы (из меню окна или трея).
    pub quit_requested: bool,
    pub dark_mode: bool,
    pub theme_initialized: bool,
    pub expanded_devices: HashSet<String>,
    pub device_types: HashMap<String, DeviceType>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            last_scan: None,
            scan_error: None,
            quit_requested: false,
            dark_mode: false,
            theme_initialized: false,
            expanded_devices: HashSet::new(),
            device_types: HashMap::new(),
        }
    }
}

impl AppState {
    /// Минимальный интервал между сканированиями HID-шины.
    const SCAN_INTERVAL: Duration = Duration::from_secs(5);

    pub fn needs_rescan(&self, now: Instant) -> bool {
        match self.last_scan {
            Some(at) => now.duration_since(at) >= Self::SCAN_INTERVAL,
            None => true,
        }
    }

    /// Перечисляет устройства и опрашивает заряд каждого.
    /// Возвращает false, если не удалось инициализировать HID-подсистему.
    pub fn rescan(&mut self) -> bool {
        let api = match hidapi::HidApi::new() {
            Ok(api) => api,
            Err(error) => {
                self.scan_error = Some(format!("Ошибка инициализации HID: {error}"));
                self.last_scan = Some(Instant::now());
                return false;
            }
        };

        self.scan_error = None;
        self.devices = collect_devices(&api);
        self.last_scan = Some(Instant::now());
        true
    }

    /// Иконка в трее: минимальный заряд среди устройств с известным уровнем.
    pub fn tray_level(&self) -> Option<u8> {
        self.devices
            .iter()
            .filter_map(|device| device.level)
            .min()
    }
}

fn collect_devices(api: &HidApi) -> Vec<DeviceInfoView> {
    device_scanner::get_24ghz_devices(api)
        .iter()
        .filter_map(|device| {
            let name = device.product_string().unwrap_or("Unknown").to_string();
            let path = device.path().to_string_lossy().to_string();
            let show_without_charge = device.vendor_id() == LUNACY_VENDOR_ID
                && device.product_id() == LUNACY_PRODUCT_ID;

            match battery::get_device_battery_level(api, device) {
                Ok(Some(level)) => Some(DeviceInfoView {
                    name,
                    path,
                    level: Some(level),
                }),
                Ok(None) if show_without_charge => Some(DeviceInfoView {
                    name,
                    path,
                    level: None,
                }),
                Ok(None) => None,
                Err(error) => {
                    eprintln!("Ошибка чтения заряда устройства {name}: {error}");
                    show_without_charge.then_some(DeviceInfoView {
                        name,
                        path,
                        level: None,
                    })
                }
            }
        })
        // Одно физическое устройство может раскрываться несколькими
        // HID-интерфейсами — оставляем запись с известным зарядом.
        .fold(HashMap::<String, DeviceInfoView>::new(), |mut acc, item| {
            match acc.get(&item.name) {
                Some(existing) if existing.level.is_some() && item.level.is_none() => {}
                _ => {
                    acc.insert(item.name.clone(), item);
                }
            }
            acc
        })
        .into_values()
        .collect()
}

#[cfg(all(feature = "gui", windows))]
mod gui {
    use super::*;

    struct Palette {
        background: egui::Color32,
        card: egui::Color32,
        card_hover: egui::Color32,
        border: egui::Color32,
        text: egui::Color32,
        muted: egui::Color32,
        accent: egui::Color32,
    }

    fn palette(dark_mode: bool) -> Palette {
        if dark_mode {
            Palette {
                background: egui::Color32::from_rgb(32, 32, 32),
                card: egui::Color32::from_rgb(43, 43, 43),
                card_hover: egui::Color32::from_rgb(54, 54, 54),
                border: egui::Color32::from_rgb(58, 58, 58),
                text: egui::Color32::from_rgb(245, 245, 245),
                muted: egui::Color32::from_rgb(174, 174, 174),
                accent: egui::Color32::from_rgb(76, 174, 255),
            }
        } else {
            Palette {
                background: egui::Color32::from_rgb(244, 245, 247),
                card: egui::Color32::WHITE,
                card_hover: egui::Color32::from_rgb(235, 237, 240),
                border: egui::Color32::from_rgb(222, 224, 228),
                text: egui::Color32::from_rgb(28, 29, 31),
                muted: egui::Color32::from_rgb(104, 107, 112),
                accent: egui::Color32::from_rgb(0, 120, 212),
            }
        }
    }

    fn apply_theme(ctx: &egui::Context, dark_mode: bool, palette: &Palette) {
        let mut visuals = if dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = palette.background;
        visuals.window_fill = palette.background;
        visuals.extreme_bg_color = palette.card;
        visuals.faint_bg_color = palette.card;
        visuals.override_text_color = Some(palette.text);
        visuals.selection.bg_fill = palette.accent;
        ctx.set_visuals(visuals);
    }

    /// Плоттер интерфейса: перечисление устройств + запрос на выход.
    pub fn ui(state: &mut AppState, ui: &mut egui::Ui) {
        let colors = palette(state.dark_mode);
        if !state.theme_initialized {
            apply_theme(ui.ctx(), state.dark_mode, &colors);
            state.theme_initialized = true;
        }

        let mut toggled_device = None;
        let mut changed_types = Vec::new();
        egui::Panel::bottom("device_footer")
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ui, |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    let status = state
                        .last_scan
                        .map(|at| format!("Обновлено {} с назад", at.elapsed().as_secs()))
                        .unwrap_or_else(|| "Сканирование…".to_string());
                    ui.label(
                        egui::RichText::new(status)
                            .size(11.0)
                            .color(colors.muted),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let exit = ui
                                .add_sized([34.0, 30.0], egui::Button::new("").frame(false))
                                .on_hover_text("Выход");
                            draw_close_icon(ui.painter(), exit.rect, colors.text);
                            if exit.clicked() {
                                state.quit_requested = true;
                            }

                            let theme = ui
                                .add_sized([34.0, 30.0], egui::Button::new("").frame(false))
                                .on_hover_text(if state.dark_mode {
                                    "Светлая тема"
                                } else {
                                    "Тёмная тема"
                                });
                            draw_gear_icon(ui.painter(), theme.rect, colors.text);
                            if theme.clicked() {
                                state.dark_mode = !state.dark_mode;
                                let updated_colors = palette(state.dark_mode);
                                apply_theme(ui.ctx(), state.dark_mode, &updated_colors);
                            }
                        },
                    );
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(colors.background)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Устройства")
                            .size(20.0)
                            .strong()
                            .color(colors.text),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(
                                egui::RichText::new(state.devices.len().to_string())
                                    .size(12.0)
                                    .color(colors.muted),
                            );
                        },
                    );
                });
                ui.add_space(8.0);

                if let Some(error) = &state.scan_error {
                    ui.label(
                        egui::RichText::new(error)
                            .size(12.0)
                            .color(egui::Color32::from_rgb(220, 70, 70)),
                    );
                    ui.add_space(6.0);
                }

                egui::ScrollArea::vertical()
                    .id_salt("device_list")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if state.devices.is_empty() && state.scan_error.is_none() {
                            ui.add_space(44.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new(
                                        "Устройства с данными о заряде не найдены",
                                    )
                                    .color(colors.muted),
                                );
                            });
                        }

                        for device in &state.devices {
                            let open = state.expanded_devices.contains(&device.path);
                            let device_type = state
                                .device_types
                                .get(&device.path)
                                .copied()
                                .unwrap_or(DeviceType::Auto);
                            if draw_device_row(
                                ui,
                                device,
                                device.level,
                                device_type,
                                open,
                                &colors,
                            ) {
                                toggled_device = Some((device.path.clone(), open));
                            }
                            if open {
                                egui::Frame::new()
                                    .fill(colors.card)
                                    .inner_margin(egui::Margin::symmetric(14, 8))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new("Путь устройства")
                                                .size(11.0)
                                                .color(colors.muted),
                                        );
                                        ui.label(
                                            egui::RichText::new(&device.path)
                                                .size(11.0)
                                                .color(colors.text),
                                        );
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new("Тип устройства")
                                                    .size(12.0)
                                                    .color(colors.muted),
                                            );
                                            let mut selected_type = device_type;
                                            let response = egui::ComboBox::from_id_salt((
                                                "device_type",
                                                device.path.as_str(),
                                            ))
                                            .selected_text(selected_type.label())
                                            .show_ui(ui, |ui| {
                                                let mut changed = false;
                                                for kind in DeviceType::ALL {
                                                    changed |= ui
                                                        .selectable_value(
                                                            &mut selected_type,
                                                            kind,
                                                            kind.label(),
                                                        )
                                                        .changed();
                                                }
                                                changed
                                            });
                                            if response.inner.unwrap_or(false) {
                                                changed_types
                                                    .push((device.path.clone(), selected_type));
                                            }
                                        });
                                    });
                            }
                            ui.add_space(5.0);
                        }
                    });
            });

        if let Some((path, was_open)) = toggled_device {
            if was_open {
                state.expanded_devices.remove(&path);
            } else {
                state.expanded_devices.insert(path);
            }
        }

        for (path, device_type) in changed_types {
            if device_type == DeviceType::Auto {
                state.device_types.remove(&path);
            } else {
                state.device_types.insert(path, device_type);
            }
        }
    }

    pub fn battery_color(level: u8) -> egui::Color32 {
        match level {
            0..=20 => egui::Color32::RED,
            21..=50 => egui::Color32::from_rgb(255, 170, 0),
            _ => egui::Color32::from_rgb(0, 200, 80),
        }
    }

    fn draw_device_row(
        ui: &mut egui::Ui,
        device: &DeviceInfoView,
        level: Option<u8>,
        device_type: DeviceType,
        open: bool,
        colors: &Palette,
    ) -> bool {
        let width = ui.available_width();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 52.0), egui::Sense::click());
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(7),
            if response.hovered() {
                colors.card_hover
            } else {
                colors.card
            },
        );
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(7),
            egui::Stroke::new(1.0, colors.border),
            egui::StrokeKind::Inside,
        );

        let center = rect.center();
        draw_device_icon(
            painter,
            device_type,
            &device.name,
            egui::pos2(rect.left() + 22.0, center.y),
            colors.text,
        );
        let text_right = rect.right() - 112.0;
        painter
            .with_clip_rect(egui::Rect::from_min_max(
                egui::pos2(rect.left() + 42.0, rect.top()),
                egui::pos2(text_right, rect.bottom()),
            ))
            .text(
                egui::pos2(rect.left() + 42.0, center.y),
                egui::Align2::LEFT_CENTER,
                &device.name,
                egui::FontId::proportional(14.0),
                colors.text,
            );

        painter.text(
            egui::pos2(rect.right() - 78.0, center.y),
            egui::Align2::RIGHT_CENTER,
            level.map_or_else(|| "—".to_string(), |level| format!("{level}%")),
            egui::FontId::proportional(13.0),
            colors.text,
        );
        draw_battery_icon(
            painter,
            egui::pos2(rect.right() - 53.0, center.y),
            level,
            level.map_or(colors.muted, battery_color),
            colors.text,
        );

        let chevron_x = rect.right() - 18.0;
        let chevron_y = center.y;
        let points = if open {
            vec![
                egui::pos2(chevron_x - 4.0, chevron_y + 2.0),
                egui::pos2(chevron_x, chevron_y - 2.0),
                egui::pos2(chevron_x + 4.0, chevron_y + 2.0),
            ]
        } else {
            vec![
                egui::pos2(chevron_x - 4.0, chevron_y - 2.0),
                egui::pos2(chevron_x, chevron_y + 2.0),
                egui::pos2(chevron_x + 4.0, chevron_y - 2.0),
            ]
        };
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(1.5, colors.text),
        ));
        response.clicked()
    }

    fn draw_battery_icon(
        painter: &egui::Painter,
        center: egui::Pos2,
        level: Option<u8>,
        fill: egui::Color32,
        outline: egui::Color32,
    ) {
        let body = egui::Rect::from_center_size(center, egui::vec2(18.0, 10.0));
        painter.rect_stroke(
            body,
            egui::CornerRadius::same(2),
            egui::Stroke::new(1.2, outline),
            egui::StrokeKind::Inside,
        );
        if let Some(level) = level {
            let fill_rect = egui::Rect::from_min_size(
                egui::pos2(body.left() + 2.0, body.top() + 2.0),
                egui::vec2(14.0 * level as f32 / 100.0, 6.0),
            );
            painter.rect_filled(fill_rect, egui::CornerRadius::same(1), fill);
        }
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(body.right(), center.y - 2.0),
                egui::vec2(2.0, 4.0),
            ),
            egui::CornerRadius::same(1),
            outline,
        );
    }

    fn draw_device_icon(
        painter: &egui::Painter,
        device_type: DeviceType,
        name: &str,
        center: egui::Pos2,
        color: egui::Color32,
    ) {
        let device_type = match device_type {
            DeviceType::Auto => infer_device_type(name),
            device_type => device_type,
        };
        let stroke = egui::Stroke::new(1.5, color);

        if device_type == DeviceType::Keyboard {
            let rect = egui::Rect::from_center_size(center, egui::vec2(22.0, 16.0));
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Inside,
            );
            for row in 0..2 {
                for column in 0..4 {
                    painter.circle_filled(
                        egui::pos2(
                            rect.left() + 4.0 + column as f32 * 4.0,
                            rect.top() + 5.0 + row as f32 * 4.0,
                        ),
                        0.8,
                        color,
                    );
                }
            }
        } else if device_type == DeviceType::Mouse {
            let rect = egui::Rect::from_center_size(center, egui::vec2(16.0, 22.0));
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(8),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x, rect.top() + 2.0),
                    egui::pos2(center.x, center.y - 2.0),
                ],
                stroke,
            );
        } else if device_type == DeviceType::Headset {
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(center.x - 8.0, center.y + 2.0),
                    egui::pos2(center.x - 8.0, center.y - 2.0),
                    egui::pos2(center.x - 5.0, center.y - 7.0),
                    egui::pos2(center.x, center.y - 9.0),
                    egui::pos2(center.x + 5.0, center.y - 7.0),
                    egui::pos2(center.x + 8.0, center.y - 2.0),
                    egui::pos2(center.x + 8.0, center.y + 2.0),
                ],
                stroke,
            ));
            painter.rect_stroke(
                egui::Rect::from_min_size(
                    egui::pos2(center.x - 10.0, center.y),
                    egui::vec2(4.0, 8.0),
                ),
                egui::CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_min_size(
                    egui::pos2(center.x + 6.0, center.y),
                    egui::vec2(4.0, 8.0),
                ),
                egui::CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
        } else if device_type == DeviceType::Speaker {
            let rect = egui::Rect::from_center_size(center, egui::vec2(18.0, 22.0));
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(4),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.circle_stroke(
                egui::pos2(center.x, center.y - 3.0),
                3.0,
                stroke,
            );
            painter.circle_stroke(
                egui::pos2(center.x, center.y + 5.0),
                2.0,
                stroke,
            );
        } else if device_type == DeviceType::Gamepad {
            let rect = egui::Rect::from_center_size(center, egui::vec2(24.0, 15.0));
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(7),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 7.0, center.y - 2.0),
                    egui::pos2(center.x - 7.0, center.y + 3.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 9.5, center.y + 0.5),
                    egui::pos2(center.x - 4.5, center.y + 0.5),
                ],
                stroke,
            );
            painter.circle_filled(
                egui::pos2(center.x + 5.0, center.y - 1.5),
                1.2,
                color,
            );
            painter.circle_filled(
                egui::pos2(center.x + 8.0, center.y + 2.5),
                1.2,
                color,
            );
        } else {
            let rect = egui::Rect::from_center_size(center, egui::vec2(18.0, 22.0));
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(4),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.circle_stroke(center, 3.0, stroke);
        }
    }

    fn infer_device_type(name: &str) -> DeviceType {
        let lower_name = name.to_lowercase();
        if lower_name.contains("keyboard") {
            DeviceType::Keyboard
        } else if lower_name.contains("mouse") {
            DeviceType::Mouse
        } else if lower_name.contains("head")
            || lower_name.contains("jabra")
            || lower_name.contains("headset")
        {
            DeviceType::Headset
        } else if lower_name.contains("speaker")
            || lower_name.contains("jbl")
            || lower_name.contains("music")
        {
            DeviceType::Speaker
        } else if lower_name.contains("gamepad")
            || lower_name.contains("controller")
            || lower_name.contains("xbox")
        {
            DeviceType::Gamepad
        } else {
            DeviceType::Other
        }
    }

    fn draw_gear_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
        let center = rect.center();
        let stroke = egui::Stroke::new(1.4, color);
        for index in 0..8 {
            let angle = index as f32 * std::f32::consts::TAU / 8.0;
            let direction = egui::vec2(angle.cos(), angle.sin());
            painter.line_segment(
                [center + direction * 5.0, center + direction * 8.0],
                stroke,
            );
        }
        painter.circle_stroke(center, 5.5, stroke);
        painter.circle_stroke(center, 2.2, stroke);
    }

    fn draw_close_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
        let center = rect.center();
        let stroke = egui::Stroke::new(1.8, color);
        painter.line_segment(
            [
                egui::pos2(center.x - 6.0, center.y - 6.0),
                egui::pos2(center.x + 6.0, center.y + 6.0),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(center.x + 6.0, center.y - 6.0),
                egui::pos2(center.x - 6.0, center.y + 6.0),
            ],
            stroke,
        );
    }
}

#[cfg(all(feature = "gui", windows))]
pub use gui::ui;
