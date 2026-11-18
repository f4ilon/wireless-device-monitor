mod battery;
mod device_scanner;
mod vendors;

fn main() {
    // HidApi создаём один раз и переиспользуем для сканирования и обмена
    let api = match hidapi::HidApi::new() {
        Ok(api) => api,
        Err(error) => {
            eprintln!("Ошибка инициализации HID: {}", error);
            std::process::exit(1);
        }
    };

    let devices = device_scanner::get_24ghz_devices(&api);

    if devices.is_empty() {
        println!("HID-устройства не найдены.");
        return;
    }

    for device in &devices {
        let name = device
            .product_string()
            .unwrap_or("Unknown")
            .to_string();

        match battery::get_device_battery_level(&api, device) {
            Ok(Some(level)) => {
                println!("{} — заряд: {}%", name, level);
            }
            Ok(None) => {
                // Для этого устройства нет поставщика данных об аккумуляторе
            }
            Err(error) => {
                println!("{} — ошибка: {}", name, error);
            }
        }
    }
}