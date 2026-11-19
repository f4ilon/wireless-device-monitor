mod app;
mod battery;
mod device_scanner;
mod vendors;

#[cfg(all(feature = "gui", windows))]
mod tray;

use std::time::Instant;

use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

use crate::app::AppState;
use crate::tray::{TrayCommand, TrayHandle};

const DEVICE_TYPES_STORAGE_KEY: &str = "device_types";
const DEVICE_NAMES_STORAGE_KEY: &str = "device_names";

/// Приложение: окно egui + системный трей с иконкой заряда.
struct WirelessMonitorApp {
    state: AppState,
    /// Флаг скрытия окна: при закрытии крестиком приложение остаётся в трее.
    window_visible: bool,
    window_positioned: bool,
    tray: Option<TrayHandle>,
}

impl WirelessMonitorApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = AppState::default();
        if let Some(storage) = cc.storage {
            if let Some(device_types) = eframe::get_value(storage, DEVICE_TYPES_STORAGE_KEY) {
                state.device_types = device_types;
            }
            if let Some(device_names) = eframe::get_value(storage, DEVICE_NAMES_STORAGE_KEY) {
                state.device_names = device_names;
            }
        }
        state.rescan();

        let tray = match TrayHandle::new(&state) {
            Ok(handle) => Some(handle),
            Err(error) => {
                eprintln!("Не удалось создать иконку в трее: {error}");
                None
            }
        };

        // Пробуждение цикла перерисовки раз в секунду — нужно, когда окно скрыто.
        crate::tray::spawn_wakeup(cc.egui_ctx.clone());

        Self {
            state,
            window_visible: true,
            window_positioned: false,
            tray,
        }
    }
}

impl eframe::App for WirelessMonitorApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.window_positioned {
            match bottom_right_position(ctx) {
                Ok(Some(position)) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
                    self.window_positioned = true;
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("Не удалось расположить окно над панелью задач: {error}");
                    self.window_positioned = true;
                }
            }
        }

        let now = Instant::now();
        if self.state.needs_rescan(now) {
            self.state.rescan();
        }

        // Команды из трея (показать окно / выйти).
        let commands = self.tray.as_ref().map(TrayHandle::poll).unwrap_or_default();
        for command in commands {
            match command {
                TrayCommand::ShowWindow => {
                    self.window_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                }
                TrayCommand::Quit => self.state.quit_requested = true,
            }
        }

        // Синхронизация иконки/тултипа трея с текущим состоянием.
        if let Some(tray) = self.tray.as_mut() {
            tray.sync(&self.state);
        }

        // «Выход» из окна или меню трея завершает процесс целиком.
        if self.state.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Закрытие крестиком перехватывается: окно скрывается, приложение живёт в трее.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_visible = false;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.window_visible {
            crate::app::ui(&mut self.state, ui);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, DEVICE_TYPES_STORAGE_KEY, &self.state.device_types);
        eframe::set_value(storage, DEVICE_NAMES_STORAGE_KEY, &self.state.device_names);
    }
}

fn bottom_right_position(
    ctx: &egui::Context,
) -> Result<Option<egui::Pos2>, windows::core::Error> {
    let (outer_rect, pixels_per_point) = ctx.input(|input| {
        let viewport = input.viewport();
        (viewport.outer_rect, viewport.native_pixels_per_point)
    });

    let (Some(outer_rect), Some(pixels_per_point)) = (outer_rect, pixels_per_point) else {
        return Ok(None);
    };
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return Ok(None);
    }

    let mut work_area = RECT::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&mut work_area as *mut RECT).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
    }

    Ok(Some(egui::pos2(
        work_area.right as f32 / pixels_per_point - outer_rect.width(),
        work_area.bottom as f32 / pixels_per_point - outer_rect.height(),
    )))
}

pub fn main() -> Result<(), eframe::Error> {
    let fixed_size = [420.0, 360.0];
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(fixed_size)
            .with_min_inner_size(fixed_size)
            .with_max_inner_size(fixed_size)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_title("Wireless Monitor"),
        ..Default::default()
    };

    eframe::run_native(
        "Wireless Monitor",
        options,
        Box::new(|cc| Ok(Box::new(WirelessMonitorApp::new(cc)))),
    )
}
