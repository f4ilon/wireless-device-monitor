use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

use hidapi::{DeviceInfo, HidApi};
use windows::Win32::Devices::HumanInterfaceDevice::{HidD_GetFeature, HidD_SetFeature};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::core::PCWSTR;

use super::{BatteryDevice, BatteryProvider};

const RAZER_VENDOR_ID: u16 = 0x1532;
const REPORT_LEN: usize = 91;
const TRANSACTION_IDS: [u8; 4] = [0x1F, 0x3F, 0xFF, 0x9F];

pub struct RazerProvider;

impl BatteryProvider for RazerProvider {
    fn supports(&self, vendor_id: u16, _product_id: u16) -> bool {
        vendor_id == RAZER_VENDOR_ID
    }

    fn get_battery_levels(
        &self,
        _api: &HidApi,
        device: &DeviceInfo,
    ) -> Result<Vec<BatteryDevice>, Box<dyn Error>> {
        if device.usage_page() != 0x0001 || !matches!(device.usage(), 0x0002 | 0x0006) {
            return Ok(Vec::new());
        }

        let level = read_battery(&device.path().to_string_lossy())?;
        Ok(level
            .map(|level| {
                vec![BatteryDevice {
                    name: device.product_string().unwrap_or("Razer").to_string(),
                    path: device.path().to_string_lossy().to_string(),
                    level,
                }]
            })
            .unwrap_or_default())
    }
}

fn read_battery(path: &str) -> Result<Option<u8>, Box<dyn Error>> {
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?
    };
    let handle = OwnedHandle(handle);

    for transaction_id in TRANSACTION_IDS {
        let mut request = [0u8; REPORT_LEN];
        request[2] = transaction_id;
        request[6] = 0x02;
        request[7] = 0x07;
        request[8] = 0x80;
        let crc = request[3..=88].iter().fold(0, |crc, byte| crc ^ byte);
        request[89] = crc;

        if !unsafe { HidD_SetFeature(handle.0, request.as_ptr().cast(), REPORT_LEN as u32) } {
            continue;
        }

        for delay in [80, 160] {
            sleep(Duration::from_millis(delay));
            let mut response = [0u8; REPORT_LEN];
            if !unsafe {
                HidD_GetFeature(handle.0, response.as_mut_ptr().cast(), REPORT_LEN as u32)
            } {
                break;
            }
            if response[1] == 0x01 {
                continue;
            }
            if response[1] != 0x02
                || response[2] != transaction_id
                || response[7] != 0x07
                || response[8] != 0x80
            {
                break;
            }

            let raw = response[10];
            if raw != 0 {
                return Ok(Some(((raw as u32 * 100 + 127) / 255) as u8));
            }
            break;
        }
    }

    Ok(None)
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}
