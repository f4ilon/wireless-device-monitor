use std::error::Error;

use hidapi::{DeviceInfo, HidApi};

use crate::vendors;

/// Возвращает уровень заряда аккумулятора устройства, если для его
/// производителя известен протокол. Иначе — Ok(None).
pub fn get_device_battery_level(
    api: &HidApi,
    device: &DeviceInfo,
) -> Result<Option<u8>, Box<dyn Error>> {
    let vendor_id = device.vendor_id();
    let product_id = device.product_id();

    for provider in vendors::providers() {
        if provider.supports(vendor_id, product_id) {
            // Если провайдер не смог прочитать этот интерфейс,
            // переходим к следующему провайдеру.
            match provider.get_battery_level(api, device) {
                Ok(None) => continue,
                result => return result,
            }
        }
    }

    Ok(None)
}
