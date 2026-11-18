use hidapi::{DeviceInfo, HidApi};

/// Полное перечисление всех HID-устройств в консоль (отладочная утилита).
#[allow(dead_code)] // вызывается по необходимости при отладке
pub fn scan_devices() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;

    println!("=== HID DEVICES ===");

    for device in api.device_list() {
        println!("\n------------------------");

        println!("Manufacturer: {:?}", device.manufacturer_string());
        println!("Product: {:?}", device.product_string());
        println!("VID: {:04X}", device.vendor_id());
        println!("PID: {:04X}", device.product_id());
        println!("Interface: {}", device.interface_number());
        println!("Usage Page: {:04X}", device.usage_page());
        println!("Usage: {:04X}", device.usage());
        println!("Path: {}", device.path().to_string_lossy());
    }

    Ok(())
}

/// Возвращает список всех HID-устройств с ненулевыми VID/PID.
pub fn get_24ghz_devices(api: &HidApi) -> Vec<DeviceInfo> {
    api.device_list()
        .filter(|device| {
            // Исключаем системные устройства и устройства без идентификаторов
            device.vendor_id() != 0 && device.product_id() != 0
        })
        .cloned()
        .collect()
}
