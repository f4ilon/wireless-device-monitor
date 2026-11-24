use std::error::Error;

use hidapi::{DeviceInfo, HidApi, HidDevice};

use super::{BatteryDevice, BatteryProvider};

const STEELSERIES_VENDOR_ID: u16 = 0x1038;

pub struct SteelSeriesProvider;

#[derive(Clone, Copy)]
enum MouseKind {
    AeroxPrime(u8),
    DirectAerox(u8),
    Rival,
}

struct MouseProfile {
    name: &'static str,
    endpoint: i32,
    kind: MouseKind,
}

struct HeadsetProfile {
    name: &'static str,
    interface: i32,
    usage_page: u16,
    usage: u16,
    request: &'static [u8],
    battery_index: usize,
    mapping: Mapping,
}

#[derive(Clone, Copy)]
enum Mapping {
    Direct,
    FourSteps,
    Arctis9,
    NovaPro,
}

impl BatteryProvider for SteelSeriesProvider {
    fn supports(&self, vendor_id: u16, _product_id: u16) -> bool {
        vendor_id == STEELSERIES_VENDOR_ID
    }

    fn get_battery_levels(
        &self,
        api: &HidApi,
        device: &DeviceInfo,
    ) -> Result<Vec<BatteryDevice>, Box<dyn Error>> {
        if let Some(profile) = mouse_profile(device.product_id()) {
            if interface_number(device) != profile.endpoint {
                return Ok(Vec::new());
            }
            let handle = api.open_path(device.path())?;
            let level = read_mouse_battery(&handle, profile.kind)?;
            return Ok(level
                .map(|level| device_result(device, profile.name, level))
                .into_iter()
                .collect());
        }

        if let Some(profile) = headset_profile(device.product_id()) {
            if interface_number(device) != profile.interface
                || (profile.usage_page != 0 && device.usage_page() != profile.usage_page)
                || (profile.usage != 0 && device.usage() != profile.usage)
            {
                return Ok(Vec::new());
            }
            let handle = api.open_path(device.path())?;
            let level = read_headset_battery(&handle, &profile)?;
            return Ok(level
                .map(|level| device_result(device, profile.name, level))
                .into_iter()
                .collect());
        }

        Ok(Vec::new())
    }
}

fn device_result(device: &DeviceInfo, name: &str, level: u8) -> BatteryDevice {
    BatteryDevice {
        name: name.to_string(),
        path: device.path().to_string_lossy().to_string(),
        level,
    }
}

fn read_mouse_battery(device: &HidDevice, kind: MouseKind) -> Result<Option<u8>, Box<dyn Error>> {
    let request: &[u8] = match kind {
        MouseKind::AeroxPrime(command) | MouseKind::DirectAerox(command) => {
            if command == 0x92 {
                &[0x00, 0x92]
            } else {
                &[0x00, 0xD2]
            }
        }
        MouseKind::Rival => &[0x00, 0xAA, 0x01],
    };
    write_request(device, request)?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        let mut response = [0u8; 64];
        let length = device.read_timeout(&mut response, remaining.as_millis() as i32)?;
        if length < 2 {
            continue;
        }
        let response = &response[..length];
        let level = match kind {
            MouseKind::AeroxPrime(command) => decode_aerox(response, command, false),
            MouseKind::DirectAerox(command) => decode_aerox(response, command, true),
            MouseKind::Rival => decode_rival(response),
        };
        if level.is_some() {
            return Ok(level);
        }
    }
}

fn decode_aerox(response: &[u8], command: u8, direct: bool) -> Option<u8> {
    let battery = if response[0] == command {
        match response {
            [_, 0, battery, ..] => *battery,
            [_, battery, ..] => *battery,
            _ => return None,
        }
    } else if response.len() >= 3 && response[0] == 0 && response[1] == command {
        if response.len() >= 4 && response[2] == 0 {
            response[3]
        } else {
            response[2]
        }
    } else {
        return None;
    };

    let raw = battery & 0x7F;
    if raw == 0 || raw > 100 {
        return None;
    }
    if direct || raw > 21 {
        Some(raw)
    } else {
        Some((raw.saturating_sub(1)) * 5)
    }
}

fn decode_rival(response: &[u8]) -> Option<u8> {
    if response.iter().all(|byte| *byte == 0) {
        return None;
    }
    let level = if response[0] == 0xAA {
        if response.get(1) == Some(&0x01) {
            response.get(2).copied()?
        } else {
            response[1]
        }
    } else if response.len() >= 3 && response[0] == 0 && response[1] == 0xAA {
        if response.get(2) == Some(&0x01) {
            response.get(3).copied()?
        } else {
            response[2]
        }
    } else if response[0] == 0 {
        response[1]
    } else {
        response[0]
    };
    (1..=100).contains(&level).then_some(level)
}

fn read_headset_battery(
    device: &HidDevice,
    profile: &HeadsetProfile,
) -> Result<Option<u8>, Box<dyn Error>> {
    write_request(device, profile.request)?;
    let mut response = [0u8; 64];
    let length = device.read_timeout(&mut response, 700)?;
    if length <= profile.battery_index {
        return Ok(None);
    }

    let raw = response[profile.battery_index];
    let level = match profile.mapping {
        Mapping::Direct => raw,
        Mapping::FourSteps => {
            if raw > 4 {
                100
            } else {
                raw * 25
            }
        }
        Mapping::Arctis9 => {
            if raw > 0x9A {
                100
            } else {
                ((raw.saturating_sub(0x64) as u16 * 100) / (0x9A - 0x64)) as u8
            }
        }
        Mapping::NovaPro => {
            if raw > 8 {
                100
            } else {
                (raw as u16 * 100 / 8) as u8
            }
        }
    };
    Ok((level <= 100).then_some(level))
}

fn write_request(device: &HidDevice, request: &[u8]) -> Result<(), Box<dyn Error>> {
    if device.write(request).is_ok() {
        return Ok(());
    }

    let mut padded = [0u8; 65];
    padded[..request.len()].copy_from_slice(request);
    if device.write(&padded[..64]).is_ok() || device.write(&padded).is_ok() {
        return Ok(());
    }
    device.send_feature_report(request)?;
    Ok(())
}

fn interface_number(device: &DeviceInfo) -> i32 {
    if device.interface_number() >= 0 {
        return device.interface_number();
    }
    let path = device.path().to_string_lossy().to_lowercase();
    path.find("mi_")
        .and_then(|position| {
            path.get(position + 3..).map(|rest| {
                rest.chars()
                    .take_while(char::is_ascii_hexdigit)
                    .collect::<String>()
            })
        })
        .and_then(|value| i32::from_str_radix(&value, 16).ok())
        .unwrap_or(-1)
}

fn mouse_profile(product_id: u16) -> Option<MouseProfile> {
    let (name, endpoint, kind) = match product_id {
        0x1838 => (
            "SteelSeries Aerox 3 Wireless",
            3,
            MouseKind::AeroxPrime(0xD2),
        ),
        0x1878 => (
            "SteelSeries Aerox 3 Wireless CS2 Dragon Lore",
            3,
            MouseKind::AeroxPrime(0xD2),
        ),
        0x183A | 0x187A => (
            "SteelSeries Aerox 3 Wireless (wired)",
            3,
            MouseKind::AeroxPrime(0x92),
        ),
        0x1890 => (
            "SteelSeries Aerox 3 Gen 2 Wireless",
            3,
            MouseKind::DirectAerox(0xD2),
        ),
        0x1892 => (
            "SteelSeries Aerox 3 Gen 2 Wireless (wired)",
            3,
            MouseKind::DirectAerox(0x92),
        ),
        0x1852 | 0x185C | 0x1860 => (
            "SteelSeries Aerox 5 Wireless",
            3,
            MouseKind::AeroxPrime(0xD2),
        ),
        0x1854 | 0x185E | 0x1862 => (
            "SteelSeries Aerox 5 Wireless (wired)",
            3,
            MouseKind::AeroxPrime(0x92),
        ),
        0x1858 | 0x1874 => (
            "SteelSeries Aerox 9 Wireless",
            3,
            MouseKind::AeroxPrime(0xD2),
        ),
        0x185A | 0x1876 => (
            "SteelSeries Aerox 9 Wireless (wired)",
            3,
            MouseKind::AeroxPrime(0x92),
        ),
        0x1840 | 0x1848 => ("SteelSeries Prime Wireless", 3, MouseKind::AeroxPrime(0xD2)),
        0x1842 | 0x184A => (
            "SteelSeries Prime Wireless (wired)",
            3,
            MouseKind::AeroxPrime(0x92),
        ),
        0x1830 | 0x1872 => ("SteelSeries Rival 3 Wireless", 3, MouseKind::Rival),
        0x1726 | 0x172B => ("SteelSeries Rival 650 Wireless", 0, MouseKind::Rival),
        0x1880 => ("SteelSeries Rival Pro", 2, MouseKind::DirectAerox(0x92)),
        _ => return None,
    };
    Some(MouseProfile {
        name,
        endpoint,
        kind,
    })
}

fn headset_profile(product_id: u16) -> Option<HeadsetProfile> {
    const OLD: (&[u8], u16, u16, i32) = (&[0x06, 0x18], 0, 0, 5);
    const EASY: (&[u8], u16, u16, i32) = (&[0x06, 0x12], 0xFF43, 0x0202, 3);
    const NOVA: (&[u8], u16, u16, i32) = (&[0x00, 0xB0], 0xFFC0, 0x0001, 3);

    let (name, request, usage_page, usage, interface, battery_index, mapping): (
        &str,
        &[u8],
        u16,
        u16,
        i32,
        usize,
        Mapping,
    ) = match product_id {
        0x1290 => (
            "SteelSeries Arctis Pro Wireless",
            &[0x40u8, 0xAA],
            0,
            0,
            0,
            0,
            Mapping::FourSteps,
        ),
        0x12E0 => (
            "SteelSeries Arctis Nova Pro Wireless",
            &[0x06u8, 0xB0],
            0,
            0,
            4,
            6,
            Mapping::NovaPro,
        ),
        0x1260 => (
            "SteelSeries Arctis 7 2017",
            OLD.0,
            OLD.1,
            OLD.2,
            OLD.3,
            2,
            Mapping::Direct,
        ),
        0x12AD => (
            "SteelSeries Arctis 7 2019",
            OLD.0,
            OLD.1,
            OLD.2,
            OLD.3,
            2,
            Mapping::Direct,
        ),
        0x1252 => (
            "SteelSeries Arctis Pro 2019",
            OLD.0,
            OLD.1,
            OLD.2,
            OLD.3,
            2,
            Mapping::Direct,
        ),
        0x1280 => (
            "SteelSeries Arctis Pro GameDac",
            OLD.0,
            OLD.1,
            OLD.2,
            OLD.3,
            2,
            Mapping::Direct,
        ),
        0x12C2 => (
            "SteelSeries Arctis 9",
            &[0x00u8, 0x20],
            0,
            0,
            0,
            3,
            Mapping::Arctis9,
        ),
        0x12B3 => (
            "SteelSeries Arctis 1 Wireless",
            EASY.0,
            EASY.1,
            EASY.2,
            EASY.3,
            3,
            Mapping::Direct,
        ),
        0x12B6 => (
            "SteelSeries Arctis 1 Xbox",
            EASY.0,
            EASY.1,
            EASY.2,
            EASY.3,
            3,
            Mapping::Direct,
        ),
        0x12D7 => (
            "SteelSeries Arctis 7X",
            EASY.0,
            EASY.1,
            EASY.2,
            EASY.3,
            3,
            Mapping::Direct,
        ),
        0x12D5 => (
            "SteelSeries Arctis 7P",
            EASY.0,
            EASY.1,
            EASY.2,
            EASY.3,
            3,
            Mapping::Direct,
        ),
        0x220E => (
            "SteelSeries Arctis 7 Plus",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        0x2236 => (
            "SteelSeries Arctis 7 Destiny Plus",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        0x2212 => (
            "SteelSeries Arctis 7P Plus",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        0x2216 => (
            "SteelSeries Arctis 7X Plus",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        0x2202 => (
            "SteelSeries Arctis Nova 7",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        0x2206 => (
            "SteelSeries Arctis Nova 7X",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        0x220A => (
            "SteelSeries Arctis Nova 7P",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        0x2258 => (
            "SteelSeries Arctis Nova 7X V2",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        0x223A => (
            "SteelSeries Arctis Nova 7 Diablo IV",
            NOVA.0,
            NOVA.1,
            NOVA.2,
            NOVA.3,
            2,
            Mapping::FourSteps,
        ),
        _ => return None,
    };
    Some(HeadsetProfile {
        name,
        interface,
        usage_page,
        usage,
        request,
        battery_index,
        mapping,
    })
}
