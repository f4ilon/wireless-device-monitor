use std::error::Error;

use hidapi::{DeviceInfo, HidApi};

use super::BatteryProvider;

/// VID/PID USB-приёмника Compx.
const COMPX_VENDOR_ID: u16 = 0x3554;
const COMPX_PRODUCT_ID: u16 = 0xF53C;
/// Vendor-defined HID-интерфейс приёмника, через который идёт обмен с микроконтроллером.
const COMPX_USAGE_PAGE: u16 = 0xFF02;

pub struct CompxProvider;

impl BatteryProvider for CompxProvider {

    fn supports(
        &self,
        vendor_id: u16,
        product_id: u16,
    ) -> bool {
        vendor_id == COMPX_VENDOR_ID && product_id == COMPX_PRODUCT_ID
    }

    fn get_battery_level(
        &self,
        api: &HidApi,
        device: &DeviceInfo,
    ) -> Result<Option<u8>, Box<dyn Error>> {

        // Пропускаем другие интерфейсы того же устройства (например, стандартный Consumer Control)
        if device.usage_page() != COMPX_USAGE_PAGE {
            return Ok(None);
        }

        let device = api.open_path(device.path())?;

        // Report ID + 16 байт данных
        let mut request = [0u8; 17];

        request[0] = 0x08;
        request[1] = 0x04;
        request[15] = 0xEF;

        let sum: u16 = request[..16]
            .iter()
            .map(|&b| b as u16)
            .sum();

        request[16] =
            (0x55u16.wrapping_sub(sum) & 0xFF) as u8;

        device.write(&request)?;

        let mut response = [0u8; 17];

        let length = device.read_timeout(
            &mut response,
            1500,
        )?;

        if length != 17 {
            return Ok(None);
        }

        // Проверка Report ID и команды
        if response[0] != 0x08 || response[1] != 0x04 {
            return Ok(None);
        }

        // Проверка контрольной суммы
        let sum: u16 = response[..16]
            .iter()
            .map(|&b| b as u16)
            .sum();

        let checksum =
            (0x55u16.wrapping_sub(sum) & 0xFF) as u8;

        if checksum != response[16] {
            return Ok(None);
        }

        let battery_level = response[6];

        if battery_level > 100 {
            return Ok(None);
        }

        Ok(Some(battery_level))
    }
}