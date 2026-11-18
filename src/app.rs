use std::collections::HashMap;
use std::time::{Duration, Instant};

use hidapi::HidApi;

use crate::battery;
use crate::device_scanner;

/// Устройство с актуальным уровнем заряда (снимок для GUI и трея).
#[derive(Clone, Debug)]
pub struct DeviceInfoView {
    pub name: String,
    pub path: String,
    /// Заряд 0–100 % или None, если данные недоступны.
    pub level: Option<u8>,
    pub error: Option<String>,
}

/// Состояние приложения: общий для окна egui и системного трея.
pub struct AppState {
    pub devices: Vec<DeviceInfoView>,
    pub last_scan: Option<Instant>,
    pub scan_error: Option<String>,
    /// Запрос на завершение работы (из меню окна или трея).
    pub quit_requested: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            last_scan: None,
            scan_error: None,
            quit_requested: false,
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
        self.devices.iter().filter_map(|d| d.level).min()
    }
}

fn collect_devices(api: &HidApi) -> Vec<DeviceInfoView> {
    device_scanner::get_24ghz_devices(api)
        .iter()
        .map(|device| {
            let name = device.product_string().unwrap_or("Unknown").to_string();
            let path = device.path().to_string_lossy().to_string();

            match battery::get_device_battery_level(api, device) {
                Ok(Some(level)) => DeviceInfoView {
                    name,
                    path,
                    level: Some(level),
                    error: None,
                },
                Ok(None) => DeviceInfoView {
                    name,
                    path,
                    level: None,
                    error: None,
                },
                Err(error) => DeviceInfoView {
                    name,
                    path,
                    level: None,
                    error: Some(error.to_string()),
                },
            }
        })
        // Одно физическое устройство может раскрываться несколькими
        // HID-интерфейсами — оставляем запись с известным зарядом.
        .fold(HashMap::<String, DeviceInfoView>::new(), |mut acc, item| {
            match acc.get_mut(&item.name) {
                Some(existing) if existing.level.is_some() && item.level.is_none() => {}
                Some(existing) => {
                    *existing = item;
                }
                None => {
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

    /// Плоттер интерфейса: перечисление устройств + запрос на выход.
    pub fn ui(state: &mut AppState, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Wireless Monitor");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Выход").clicked() {
                        state.quit_requested = true;
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                match state.last_scan {
                    Some(at) => ui.label(format!("Обновлено: {} с назад", at.elapsed().as_secs())),
                    None => ui.label("Сканирование…"),
                };
                ui.separator();
                if let Some(error) = &state.scan_error {
                    ui.colored_label(egui::Color32::RED, error);
                } else {
                    ui.label(format!("Устройств: {}", state.devices.len()));
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if state.devices.is_empty() && state.scan_error.is_none() {
                ui.label("HID-устройства не найдены.");
                return;
            }

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for device in &state.devices {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&device.name).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| match device.level {
                                    Some(level) => {
                                        let color = battery_color(level);
                                        ui.colored_label(color, format!("{level}%"));
                                        ui.add(
                                            egui::ProgressBar::new(level as f32 / 100.0)
                                                .desired_width(140.0)
                                                .fill(color),
                                        );
                                    }
                                    None => {
                                        let text = device
                                            .error
                                            .as_deref()
                                            .map(|e| format!("ошибка: {e}"))
                                            .unwrap_or_else(|| "заряд недоступен".to_string());
                                        ui.weak(text);
                                    }
                                },
                            );
                        });
                        ui.small(device.path.as_str());
                    });
                }
            });
        });
    }

    pub fn battery_color(level: u8) -> egui::Color32 {
        match level {
            0..=20 => egui::Color32::RED,
            21..=50 => egui::Color32::from_rgb(255, 170, 0),
            _ => egui::Color32::from_rgb(0, 200, 80),
        }
    }
    pub use battery_color;
}

#[cfg(all(feature = "gui", windows))]
pub use gui::ui;
