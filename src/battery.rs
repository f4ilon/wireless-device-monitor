use std::error::Error;

use hidapi::{DeviceInfo, HidApi};

use crate::vendors::{self, BatteryDevice};

/// Возвращает уровни заряда устройства и подключённых к нему приёмников,
/// если для его производителя известен протокол.
pub fn get_device_battery_levels(
    api: &HidApi,
    device: &DeviceInfo,
) -> Result<Vec<BatteryDevice>, Box<dyn Error>> {
    let vendor_id = device.vendor_id();
    let product_id = device.product_id();

    for provider in vendors::providers() {
        if provider.supports(vendor_id, product_id) {
            // Если провайдер не смог прочитать этот интерфейс,
            // переходим к следующему провайдеру.
            match provider.get_battery_levels(api, device) {
                Ok(devices) if devices.is_empty() => continue,
                result => return result,
            }
        }
    }

    Ok(Vec::new())
}
