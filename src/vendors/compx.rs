use std::error::Error;

use hidapi::{DeviceInfo, HidApi};

use super::{BatteryDevice, BatteryProvider};

/// VID/PID USB-приёмника Compx.
const COMPX_VENDOR_ID: u16 = 0x3554;
const COMPX_PRODUCT_ID: u16 = 0xF53C;
/// Vendor-defined HID-интерфейс приёмника, через который идёт обмен с микроконтроллером.
const COMPX_USAGE_PAGE: u16 = 0xFF02;

pub struct CompxProvider;

impl BatteryProvider for CompxProvider {
    fn supports(&self, vendor_id: u16, product_id: u16) -> bool {
        vendor_id == COMPX_VENDOR_ID && product_id == COMPX_PRODUCT_ID
    }

    fn get_battery_levels(
        &self,
        api: &HidApi,
        device: &DeviceInfo,
    ) -> Result<Vec<BatteryDevice>, Box<dyn Error>> {
        // Пропускаем другие интерфейсы того же устройства (например, стандартный Consumer Control)
        if device.usage_page() != COMPX_USAGE_PAGE {
            return Ok(Vec::new());
        }

        let handle = api.open_path(device.path())?;

        // Report ID + 16 байт данных
        let mut request = [0u8; 17];
        request[0] = 0x08;
        request[1] = 0x04;
        request[15] = 0xEF;

        let sum: u16 = request[..16].iter().map(|&b| b as u16).sum();
        request[16] = (0x55u16.wrapping_sub(sum) & 0xFF) as u8;

        handle.write(&request)?;

        let mut response = [0u8; 17];
        let length = handle.read_timeout(&mut response, 1500)?;
        if length != 17 {
            return Ok(Vec::new());
        }

        // Проверка Report ID и команды
        if response[0] != 0x08 || response[1] != 0x04 {
            return Ok(Vec::new());
        }

        // Проверка контрольной суммы
        let sum: u16 = response[..16].iter().map(|&b| b as u16).sum();
        let checksum = (0x55u16.wrapping_sub(sum) & 0xFF) as u8;
        if checksum != response[16] || response[6] > 100 {
            return Ok(Vec::new());
        }

        Ok(vec![BatteryDevice {
            name: device.product_string().unwrap_or("Unknown").to_string(),
            path: device.path().to_string_lossy().to_string(),
            level: response[6],
        }])
    }
}
