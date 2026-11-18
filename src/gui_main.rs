use std::time::Instant;

use crate::app::AppState;
use crate::tray::{TrayCommand, TrayHandle};

/// Приложение: окно egui + системный трей с иконкой заряда.
struct WirelessMonitorApp {
    state: AppState,
    /// Флаг скрытия окна: при закрытии крестиком приложение остаётся в трее.
    window_visible: bool,
    tray: Option<TrayHandle>,
}

impl WirelessMonitorApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = AppState::default();
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
            tray,
        }
    }
}

impl eframe::App for WirelessMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        if self.state.needs_rescan(now) {
            self.state.rescan();
        }

        // Команды из трея (показать окно / выйти).
        let commands = self.tray.as_ref().map(TrayHandle::poll).unwrap_or_default();
        for command in commands {
            match command {
                TrayCommand::ShowWindow => self.window_visible = true,
                TrayCommand::Quit => self.state.quit_requested = true,
            }
        }

        // Синхронизация иконки/тултипа трея с текущим состоянием.
        if let Some(tray) = self.tray.as_mut() {
            tray.sync(&self.state);
        }

        // «Выход» из окна или меню трея завершает процесс целиком.
        if self.state.quit_requested {
            std::process::exit(0);
        }

        // Закрытие крестиком перехватывается: окно скрывается, приложение живёт в трее.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CloseCancel(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_visible = false;
        }

        if self.window_visible {
            crate::app::ui(&mut self.state, ctx);
        }
    }
}

pub fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([420.0, 360.0])
            .with_min_inner_size([320.0, 240.0])
            .with_title("Wireless Monitor"),
        ..Default::default()
    };

    eframe::run_native(
        "Wireless Monitor",
        options,
        Box::new(|cc| Ok(Box::new(WirelessMonitorApp::new(cc)))),
    )
}
