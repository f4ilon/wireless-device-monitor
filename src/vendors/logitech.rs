use std::error::Error;
use std::time::{Duration, Instant};

use hidapi::{DeviceInfo, HidApi, HidDevice};

use super::{BatteryDevice, BatteryProvider};

const LOGITECH_VENDOR_ID: u16 = 0x046D;
const HIDPP_USAGE_PAGE: u16 = 0xFF00;
const HIDPP_USAGE: u16 = 0x0001;
const LONG_REPORT_ID: u8 = 0x11;
const LONG_REPORT_LEN: usize = 20;
const DIRECT_DEVICE_INDEX: u8 = 0xFF;
const SOFTWARE_ID: u8 = 0x09;
const ROOT_FEATURE: u8 = 0x00;
const DEVICE_NAME_FEATURE: u16 = 0x0005;
const BATTERY_STATUS_FEATURE: u16 = 0x1000;
const UNIFIED_BATTERY_FEATURE: u16 = 0x1004;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(800);
const PING_TIMEOUT: Duration = Duration::from_millis(250);

pub struct LogitechProvider;

impl BatteryProvider for LogitechProvider {
    fn supports(&self, vendor_id: u16, _product_id: u16) -> bool {
        vendor_id == LOGITECH_VENDOR_ID
    }

    fn get_battery_levels(
        &self,
        api: &HidApi,
        device: &DeviceInfo,
    ) -> Result<Vec<BatteryDevice>, Box<dyn Error>> {
        if device.usage_page() != HIDPP_USAGE_PAGE || device.usage() != HIDPP_USAGE {
            return Ok(Vec::new());
        }

        let handle = api.open_path(device.path())?;
        let path = device.path().to_string_lossy();
        if is_receiver(device.product_id()) {
            let mut devices = Vec::new();
            for index in 1..=6 {
                if !ping(&handle, index)? {
                    continue;
                }
                if let Some(level) = read_battery(&handle, index)? {
                    let name = read_name(&handle, index)?
                        .unwrap_or_else(|| format!("Logitech (слот {index})"));
                    devices.push(BatteryDevice {
                        name,
                        path: format!("{path}#logitech_slot={index}"),
                        level,
                    });
                }
            }
            Ok(devices)
        } else {
            let level = read_battery(&handle, DIRECT_DEVICE_INDEX)?;
            Ok(level
                .map(|level| {
                    vec![BatteryDevice {
                        name: device.product_string().unwrap_or("Logitech").to_string(),
                        path: path.into_owned(),
                        level,
                    }]
                })
                .unwrap_or_default())
        }
    }
}

fn is_receiver(product_id: u16) -> bool {
    (0xC500..=0xC5FF).contains(&product_id)
}

fn ping(device: &HidDevice, device_index: u8) -> Result<bool, Box<dyn Error>> {
    const PING_MARK: u8 = 0xA5;

    Ok(request(
        device,
        device_index,
        ROOT_FEATURE,
        1,
        &[0, 0, PING_MARK],
        PING_TIMEOUT,
    )?
    .is_some_and(|response| response[0] >= 2 && response[2] == PING_MARK))
}

fn read_battery(device: &HidDevice, device_index: u8) -> Result<Option<u8>, Box<dyn Error>> {
    if let Some(feature) = find_feature(device, device_index, UNIFIED_BATTERY_FEATURE)? {
        let capabilities = request(device, device_index, feature, 0, &[], REQUEST_TIMEOUT)?;
        if let Some(capabilities) = capabilities
            && capabilities[1] & 0x02 != 0
        {
            let status = request(device, device_index, feature, 1, &[], REQUEST_TIMEOUT)?;
            return Ok(status.and_then(|response| (response[0] <= 100).then_some(response[0])));
        }
    }

    if let Some(feature) = find_feature(device, device_index, BATTERY_STATUS_FEATURE)? {
        let status = request(device, device_index, feature, 0, &[], REQUEST_TIMEOUT)?;
        return Ok(status.and_then(|response| {
            let charging = matches!(response[2], 1 | 2 | 4);
            (response[0] <= 100 && !(charging && response[0] == 0)).then_some(response[0])
        }));
    }

    Ok(None)
}

fn find_feature(
    device: &HidDevice,
    device_index: u8,
    feature_id: u16,
) -> Result<Option<u8>, Box<dyn Error>> {
    let response = request(
        device,
        device_index,
        ROOT_FEATURE,
        0,
        &feature_id.to_be_bytes(),
        REQUEST_TIMEOUT,
    )?;
    Ok(response.and_then(|response| (response[0] != 0).then_some(response[0])))
}

fn read_name(device: &HidDevice, device_index: u8) -> Result<Option<String>, Box<dyn Error>> {
    let Some(feature) = find_feature(device, device_index, DEVICE_NAME_FEATURE)? else {
        return Ok(None);
    };
    let Some(length) = request(device, device_index, feature, 0, &[], REQUEST_TIMEOUT)?
        .map(|response| response[0] as usize)
    else {
        return Ok(None);
    };

    let mut name = Vec::with_capacity(length);
    while name.len() < length {
        let Some(fragment) = request(
            device,
            device_index,
            feature,
            1,
            &[name.len() as u8],
            REQUEST_TIMEOUT,
        )?
        else {
            return Ok(None);
        };
        name.extend_from_slice(&fragment[..(length - name.len()).min(fragment.len())]);
    }

    Ok(String::from_utf8(name).ok().filter(|name| !name.is_empty()))
}

fn request(
    device: &HidDevice,
    device_index: u8,
    feature_index: u8,
    function: u8,
    parameters: &[u8],
    timeout: Duration,
) -> Result<Option<[u8; 16]>, Box<dyn Error>> {
    let function_and_software_id = (function << 4) | SOFTWARE_ID;
    let mut request = [0u8; LONG_REPORT_LEN];
    request[0] = LONG_REPORT_ID;
    request[1] = device_index;
    request[2] = feature_index;
    request[3] = function_and_software_id;
    request[4..4 + parameters.len()].copy_from_slice(parameters);
    device.write(&request)?;

    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }

        let mut response = [0u8; 32];
        let length = device.read_timeout(&mut response, remaining.as_millis() as i32)?;
        if length < LONG_REPORT_LEN || response[0] != LONG_REPORT_ID {
            continue;
        }
        if response[1] != device_index && !(device_index == DIRECT_DEVICE_INDEX && response[1] == 0)
        {
            continue;
        }
        if response[2] == 0xFF
            && response[3] == feature_index
            && response[4] == function_and_software_id
        {
            return Ok(None);
        }
        if response[2] == feature_index && response[3] == function_and_software_id {
            let mut parameters = [0u8; 16];
            parameters.copy_from_slice(&response[4..LONG_REPORT_LEN]);
            return Ok(Some(parameters));
        }
    }
}
