mod icon;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, w};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW,
};
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
    device_trays: HashMap<String, DeviceTrayIcon>,
    rx: Receiver<TrayCommand>,
    last_tooltip: String,
    last_level: Option<u8>,
    last_device_scan: Option<Instant>,
    dark_taskbar: bool,
    main_icon_dark_taskbar: bool,
}

struct DeviceTrayIcon {
    tray: TrayIcon,
    level: Option<u8>,
    tooltip: String,
    dark_taskbar: bool,
}

impl TrayHandle {
    pub fn new(state: &AppState) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<TrayCommand>();

        // Контекстное меню трея.
        let menu = Menu::new();
        let show_item = MenuItem::with_id("show", "Показать окно", true, None);
        let refresh_item = MenuItem::with_id("refresh", "Обновить", true, None);
        let quit_item = MenuItem::with_id("quit", "Выход", true, None);
        menu.append(&show_item).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        menu.append(&refresh_item).map_err(|e| e.to_string())?;
        menu.append(&quit_item).map_err(|e| e.to_string())?;

        let level = state.tray_level();
        let tooltip = tray_tooltip(state);
        let dark_taskbar = match read_system_dark_theme() {
            Ok(dark) => dark,
            Err(error) => {
                eprintln!("Не удалось определить тему панели Windows: {error}");
                false
            }
        };
        let icon = build_tray_icon(level, dark_taskbar)?;

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

        let mut handle = Self {
            tray,
            device_trays: HashMap::new(),
            rx,
            last_tooltip: tooltip,
            last_level: level,
            last_device_scan: None,
            dark_taskbar,
            main_icon_dark_taskbar: dark_taskbar,
        };
        handle.sync_device_trays(state, dark_taskbar);
        handle.last_device_scan = state.last_scan;
        Ok(handle)
    }

    /// Обновляет иконку и тултип, если значения изменились.
    pub fn sync(&mut self, state: &AppState) {
        let level = state.tray_level();
        let scan_changed = self.last_device_scan != state.last_scan;
        if scan_changed {
            match read_system_dark_theme() {
                Ok(dark) => self.dark_taskbar = dark,
                Err(error) => eprintln!("Не удалось определить тему панели Windows: {error}"),
            }
        }

        if level != self.last_level
            || (scan_changed && self.main_icon_dark_taskbar != self.dark_taskbar)
        {
            match build_tray_icon(level, self.dark_taskbar).and_then(|icon| {
                self.tray.set_icon(Some(icon)).map_err(|error| error.to_string())
            }) {
                Ok(()) => {
                    self.last_level = level;
                    self.main_icon_dark_taskbar = self.dark_taskbar;
                }
                Err(error) => eprintln!("Не удалось обновить значок в трее: {error}"),
            }
        }

        let tooltip = tray_tooltip(state);
        if tooltip != self.last_tooltip {
            match self.tray.set_tooltip(Some(tooltip.as_str())) {
                Ok(()) => self.last_tooltip = tooltip,
                Err(error) => eprintln!("Не удалось обновить подсказку трея: {error}"),
            }
        }

        if scan_changed {
            self.last_device_scan = state.last_scan;
        }
        self.sync_device_trays(state, self.dark_taskbar);
    }

    fn sync_device_trays(&mut self, state: &AppState, dark_taskbar: bool) {
        let devices = &state.devices;
        let active_paths: HashSet<&str> =
            devices.iter().map(|device| device.path.as_str()).collect();
        self.device_trays
            .retain(|path, _| active_paths.contains(path.as_str()));

        for device in devices {
            let display_name = state.display_name(device);
            let tooltip = device
                .level
                .map_or_else(|| format!("{}: —", display_name), |level| {
                    format!("{}: {}%", display_name, level)
                });
            if let Some(icon) = self.device_trays.get_mut(&device.path) {
                if icon.level != device.level || icon.dark_taskbar != dark_taskbar {
                    match build_tray_icon(device.level, dark_taskbar).and_then(|image| {
                        icon.tray.set_icon(Some(image)).map_err(|error| error.to_string())
                    }) {
                        Ok(()) => {
                            icon.level = device.level;
                            icon.dark_taskbar = dark_taskbar;
                        }
                        Err(error) => eprintln!(
                            "Не удалось обновить индикатор заряда {}: {error}",
                            device.name
                        ),
                    }
                }

                if icon.tooltip != tooltip {
                    match icon.tray.set_tooltip(Some(tooltip.as_str())) {
                        Ok(()) => icon.tooltip = tooltip,
                        Err(error) => eprintln!(
                            "Не удалось обновить подсказку заряда {}: {error}",
                            device.name
                        ),
                    }
                }
                continue;
            }

            match build_device_tray(device, tooltip, dark_taskbar) {
                Ok(icon) => {
                    self.device_trays.insert(device.path.clone(), icon);
                }
                Err(error) => {
                    eprintln!(
                        "Не удалось создать индикатор заряда {}: {error}",
                        device.name
                    );
                }
            }
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

fn build_tray_icon(level: Option<u8>, dark_taskbar: bool) -> Result<Icon, String> {
    let size = 32;
    let img = battery_icon(size, level, dark_taskbar);
    Icon::from_rgba(img.into_raw(), size, size).map_err(|e| e.to_string())
}

fn build_device_tray(
    device: &DeviceInfoView,
    tooltip: String,
    dark_taskbar: bool,
) -> Result<DeviceTrayIcon, String> {
    let icon = build_tray_icon(device.level, dark_taskbar)?;
    let tray = TrayIconBuilder::new()
        .with_tooltip(&tooltip)
        .with_icon(icon)
        .build()
        .map_err(|error| error.to_string())?;

    Ok(DeviceTrayIcon {
        tray,
        level: device.level,
        tooltip,
        dark_taskbar,
    })
}

fn read_system_dark_theme() -> Result<bool, String> {
    match read_light_theme_value(w!("SystemUsesLightTheme")) {
        Ok(is_light) => Ok(!is_light),
        Err(system_error) => read_light_theme_value(w!("AppsUseLightTheme"))
            .map(|is_light| !is_light)
            .map_err(|app_error| {
                format!(
                    "не удалось прочитать параметры SystemUsesLightTheme ({system_error}) \
                     и AppsUseLightTheme ({app_error})"
                )
            }),
    }
}

fn read_light_theme_value(value_name: PCWSTR) -> Result<bool, String> {
    let mut value = 0u32;
    let mut value_size = std::mem::size_of_val(&value) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            value_name,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut value_size),
        )
    };
    status
        .ok()
        .map(|()| value != 0)
        .map_err(|error| error.to_string())
}

fn tray_tooltip(state: &AppState) -> String {
    if state.devices.is_empty() {
        return "Wireless Monitor: устройства с зарядом не найдены".to_string();
    }

    let lines: Vec<String> = state
        .devices
        .iter()
        .take(6)
        .map(|d: &DeviceInfoView| {
            let display_name = state.display_name(d);
            d.level.map_or_else(
                || format!("{}: —", display_name),
                |level| format!("{}: {}%", display_name, level),
            )
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
