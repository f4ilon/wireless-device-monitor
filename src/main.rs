mod app;
mod battery;
mod device_scanner;
mod vendors;

#[cfg(all(feature = "gui", windows))]
mod tray;

// Под Windows с включённой фичей gui точка входа — в gui_main.rs
// (окно egui + системный трей). Во всех остальных случаях — консольный режим.
#[cfg(all(feature = "gui", windows))]
#[path = "gui_main.rs"]
mod gui_main;

#[cfg(not(all(feature = "gui", windows)))]
fn main() {
    console_main();
}

/// Консольный режим: однократный опрос устройств и печать заряда.
#[allow(dead_code)]
fn console_main() {
    let api = match hidapi::HidApi::new() {
        Ok(api) => api,
        Err(error) => {
            eprintln!("Ошибка инициализации HID: {error}");
            std::process::exit(1);
        }
    };

    let devices = device_scanner::get_24ghz_devices(&api);
    if devices.is_empty() {
        println!("HID-устройства не найдены.");
        return;
    }

    for device in &devices {
        let name = device.product_string().unwrap_or("Unknown");
        match battery::get_device_battery_level(&api, device) {
            Ok(Some(level)) => println!("{name} — заряд: {level}%"),
            Ok(None) => {}
            Err(error) => println!("{name} — ошибка: {error}"),
        }
    }
}
