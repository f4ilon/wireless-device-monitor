mod device_scanner;
mod hid_utils;
mod vendors;

fn main() {
    let devices = device_scanner::get_24ghz_devices();

    match devices {
        Ok(devices) => {
            for device in devices.iter() {
                let result = vendors::get_device_battery_level(
                    device.vendor_id(),
                    device.product_id(),
                );

                match result {
                    Ok(Some(level)) => {
                        println!(
                            "{} — заряд: {}%",
                            device.product_string()
                                .unwrap_or("Unknown"),
                            level
                        );
                    }

                    Ok(None) => {}

                    Err(error) => {
                        println!("Ошибка: {}", error);
                    }
                }
            }
        }

        Err(error) => {
            println!("Ошибка сканирования: {}", error);
        }
    }
}