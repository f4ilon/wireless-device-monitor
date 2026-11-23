use std::error::Error;

use hidapi::{DeviceInfo, HidApi};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HIDP_CAPS, HIDP_REPORT_TYPE, HIDP_VALUE_CAPS, HidD_FreePreparsedData, HidD_GetFeature,
    HidD_GetInputReport, HidD_GetPreparsedData, HidP_Feature, HidP_GetCaps, HidP_GetUsageValue,
    HidP_GetValueCaps, HidP_Input, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::core::PCWSTR;

use super::{BatteryDevice, BatteryProvider};

const GENERIC_DEVICE_CONTROLS_PAGE: u16 = 0x0006;
const BATTERY_STRENGTH_USAGE: u16 = 0x0020;

pub struct StandardHidProvider;

impl BatteryProvider for StandardHidProvider {
    fn supports(&self, _vendor_id: u16, _product_id: u16) -> bool {
        true
    }

    fn get_battery_levels(
        &self,
        _api: &HidApi,
        device: &DeviceInfo,
    ) -> Result<Vec<BatteryDevice>, Box<dyn Error>> {
        if device.usage_page() != GENERIC_DEVICE_CONTROLS_PAGE {
            return Ok(Vec::new());
        }

        let level = read_battery_level(&device.path().to_string_lossy())?;
        Ok(level
            .map(|level| {
                vec![BatteryDevice {
                    name: device.product_string().unwrap_or("Unknown").to_string(),
                    path: device.path().to_string_lossy().to_string(),
                    level,
                }]
            })
            .unwrap_or_default())
    }
}

fn read_battery_level(path: &str) -> Result<Option<u8>, Box<dyn Error>> {
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )?
    };
    let handle = OwnedHandle(handle);

    let mut preparsed = PHIDP_PREPARSED_DATA::default();
    if !unsafe { HidD_GetPreparsedData(handle.0, &mut preparsed) } {
        return Ok(None);
    }
    let preparsed = PreparsedData(preparsed);

    let mut capabilities = HIDP_CAPS::default();
    if unsafe { HidP_GetCaps(preparsed.0, &mut capabilities) }.0 < 0 {
        return Ok(None);
    }

    if let Some(level) = read_report(
        handle.0,
        preparsed.0,
        HidP_Feature,
        capabilities.NumberFeatureValueCaps,
        capabilities.FeatureReportByteLength,
    ) {
        return Ok(Some(level));
    }

    Ok(read_report(
        handle.0,
        preparsed.0,
        HidP_Input,
        capabilities.NumberInputValueCaps,
        capabilities.InputReportByteLength,
    ))
}

fn read_report(
    handle: HANDLE,
    preparsed: PHIDP_PREPARSED_DATA,
    report_type: HIDP_REPORT_TYPE,
    value_caps_count: u16,
    report_length: u16,
) -> Option<u8> {
    if value_caps_count == 0 || report_length == 0 {
        return None;
    }

    let mut count = value_caps_count;
    let mut value_caps = vec![HIDP_VALUE_CAPS::default(); count as usize];
    if unsafe { HidP_GetValueCaps(report_type, value_caps.as_mut_ptr(), &mut count, preparsed) }.0
        < 0
    {
        return None;
    }

    for value_cap in value_caps.into_iter().take(count as usize) {
        if value_cap.UsagePage != GENERIC_DEVICE_CONTROLS_PAGE
            || value_cap.IsRange
            || value_cap.IsAlias
            || unsafe { value_cap.Anonymous.NotRange.Usage } != BATTERY_STRENGTH_USAGE
        {
            continue;
        }

        let mut report = vec![0u8; report_length as usize];
        report[0] = value_cap.ReportID;
        let read = unsafe {
            if report_type == HidP_Feature {
                HidD_GetFeature(handle, report.as_mut_ptr().cast(), report_length as u32)
            } else {
                HidD_GetInputReport(handle, report.as_mut_ptr().cast(), report_length as u32)
            }
        };
        if !read {
            continue;
        }

        let mut value = 0u32;
        if unsafe {
            HidP_GetUsageValue(
                report_type,
                GENERIC_DEVICE_CONTROLS_PAGE,
                Some(value_cap.LinkCollection),
                BATTERY_STRENGTH_USAGE,
                &mut value,
                preparsed,
                &report,
            )
        }
        .0 < 0
        {
            continue;
        }

        let minimum = value_cap.LogicalMin as i64;
        let maximum = value_cap.LogicalMax as i64;
        let value = value as i64;
        if maximum <= minimum || !(minimum..=maximum).contains(&value) {
            continue;
        }
        let level = (value - minimum) * 100 / (maximum - minimum);
        return Some(level.clamp(0, 100) as u8);
    }

    None
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct PreparsedData(PHIDP_PREPARSED_DATA);

impl Drop for PreparsedData {
    fn drop(&mut self) {
        let _ = unsafe { HidD_FreePreparsedData(self.0) };
    }
}
