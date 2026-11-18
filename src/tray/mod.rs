use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::app::{AppState, DeviceInfoView};
use crate::tray::icon::battery_icon;

/// События из трея (меню/клики) в поток eframe.
pub enum TrayCommand {
    ShowWindow,
    Quit,
}

/// Владелец системного трея: иконка с уровнем заряда + контекстное меню.
pub struct TrayHandle {
    tray: TrayIcon,
    rx: Receiver<TrayCommand>,
    last_tooltip: String,
    last_level: Option<u8>,
}

impl TrayHandle {
    pub fn new(state: &AppState) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<TrayCommand>();

        // Контекстное меню трея.
        let menu = Menu::new().map_err(|e| e.to_string())?;
        let show_item =
            MenuItem::with_id("show", "Показать окно", true, None).map_err(|e| e.to_string())?;
        let refresh_item =
            MenuItem::with_id("refresh", "Обновить", true, None).map_err(|e| e.to_string())?;
        let quit_item =
            MenuItem::with_id("quit", "Выход", true, None).map_err(|e| e.to_string())?;
        menu.append(&show_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&refresh_item).map_err(|e| e.to_string())?;
        menu.append(&quit_item).map_err(|e| e.to_string())?;

        let level = state.tray_level();
        let tooltip = tray_tooltip(state);
        let icon = build_tray_icon(level)?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(&tooltip)
            .with_icon(icon)
            .build()
            .map_err(|e| e.to_string())?;

        // Обработчик событий меню: пересылает команды в цикл eframe.
        let tx_menu = tx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id().as_ref() {
            "show" | "refresh" => {
                let _ = tx_menu.send(TrayCommand::ShowWindow);
            }
            "quit" => {
                let _ = tx_menu.send(TrayCommand::Quit);
            }
            _ => {}
        }));

        // Двойной клик по иконке — показать окно.
        let tx_click = tx;
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                let _ = tx_click.send(TrayCommand::ShowWindow);
            }
        }));

        Ok(Self {
            tray,
            rx,
            last_tooltip: tooltip,
            last_level: level,
        })
    }

    /// Обновляет иконку и тултип, если значения изменились.
    pub fn sync(&mut self, state: &AppState) {
        let level = state.tray_level();
        let tooltip = tray_tooltip(state);

        if level != self.last_level {
            self.last_level = level;
            if let Ok(icon) = build_tray_icon(level) {
                let _ = self.tray.set_icon(Some(icon));
            }
        }

        if tooltip != self.last_tooltip {
            self.last_tooltip = tooltip;
            let _ = self.tray.set_tooltip(Some(self.last_tooltip.as_str()));
        }
    }

    /// Неблокирующий опрос команд от трея.
    pub fn poll(&self) -> Vec<TrayCommand> {
        let mut commands = Vec::new();
        while let Ok(command) = self.rx.try_recv() {
            commands.push(command);
        }
        commands
    }
}

fn build_tray_icon(level: Option<u8>) -> Result<Icon, String> {
    let size = 32;
    let img = battery_icon(size, level);
    Icon::from_rgba(img.into_raw(), size, size).map_err(|e| e.to_string())
}

fn tray_tooltip(state: &AppState) -> String {
    if state.devices.is_empty() {
        return "Wireless Monitor: устройства не найдены".to_string();
    }

    let lines: Vec<String> = state
        .devices
        .iter()
        .take(6)
        .map(|d: &DeviceInfoView| match d.level {
            Some(level) => format!("{}: {}%", d.name, level),
            None => format!("{}: —", d.name),
        })
        .collect();

    format!("Wireless Monitor\n{}", lines.join("\n"))
}

/// Периодическое пробуждение цикла egui, чтобы трей обновлялся,
/// даже когда окно скрыто и событий ввода нет.
pub fn spawn_wakeup(ctx: egui::Context) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(1000));
        ctx.request_repaint();
    });
}
