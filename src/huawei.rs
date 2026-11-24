use windows::Devices::Bluetooth::Rfcomm::{RfcommDeviceService, RfcommServiceId};
use windows::Devices::Bluetooth::{BluetoothDevice, BluetoothError};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Networking::Sockets::StreamSocket;
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};
use windows::core::GUID;

use crate::bluetooth::{error, wait, wait_action};
use crate::vendors::BatteryDevice;

const SERIAL_PORT_UUID: GUID = GUID::from_u128(0x00001101_0000_1000_8000_00805F9B34FB);
const BATTERY_COMMAND: [u8; 2] = [0x01, 0x08];

#[derive(Default)]
struct HuaweiBattery {
    global: Option<u8>,
    left: Option<u8>,
    right: Option<u8>,
    case: Option<u8>,
}

pub fn battery_devices() -> Result<Vec<BatteryDevice>, String> {
    let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true).map_err(error)?;
    let information = wait(DeviceInformation::FindAllAsyncAqsFilter(&selector).map_err(error)?)?;
    let mut devices = Vec::new();

    for information in &information {
        let name = information.Name().map_err(error)?.to_string();
        let lower_name = name.to_lowercase();
        if !["freebuds", "freelace", "freeclip", "honor earbuds"]
            .iter()
            .any(|keyword| lower_name.contains(keyword))
        {
            continue;
        }

        let id = information.Id().map_err(error)?;
        let device = match wait(BluetoothDevice::FromIdAsync(&id).map_err(error)?) {
            Ok(device) => device,
            Err(_) => continue,
        };
        if let Ok(battery) = read_battery(&device) {
            if let Some(level) = battery.left {
                devices.push(battery_device(&name, &id, "левый", "left", level));
            }
            if let Some(level) = battery.right {
                devices.push(battery_device(&name, &id, "правый", "right", level));
            }
            if let Some(level) = battery.case {
                devices.push(battery_device(&name, &id, "кейс", "case", level));
            }
            if battery.left.is_none()
                && battery.right.is_none()
                && let Some(level) = battery.global
            {
                devices.push(battery_device(&name, &id, "общий", "global", level));
            }
        }
        let _ = device.Close();
    }

    Ok(devices)
}

fn battery_device(
    name: &str,
    id: &windows::core::HSTRING,
    suffix: &str,
    path_suffix: &str,
    level: u8,
) -> BatteryDevice {
    BatteryDevice {
        name: format!("{name} ({suffix})"),
        path: format!("rfcomm:{id}:{path_suffix}"),
        level,
    }
}

fn read_battery(device: &BluetoothDevice) -> Result<HuaweiBattery, String> {
    let service_id = RfcommServiceId::FromUuid(SERIAL_PORT_UUID).map_err(error)?;
    let services = wait(
        device
            .GetRfcommServicesForIdAsync(&service_id)
            .map_err(error)?,
    )?;
    if services.Error().map_err(error)? != BluetoothError::Success {
        return Err("служба Huawei RFCOMM недоступна".to_string());
    }

    for service in &services.Services().map_err(error)? {
        if let Ok(battery) = read_from_service(&service) {
            return Ok(battery);
        }
    }
    Err("наушники Huawei не вернули уровень заряда".to_string())
}

fn read_from_service(service: &RfcommDeviceService) -> Result<HuaweiBattery, String> {
    let socket = StreamSocket::new().map_err(error)?;
    wait_action(
        socket
            .ConnectAsync(
                &service.ConnectionHostName().map_err(error)?,
                &service.ConnectionServiceName().map_err(error)?,
            )
            .map_err(error)?,
    )?;

    let writer =
        DataWriter::CreateDataWriter(&socket.OutputStream().map_err(error)?).map_err(error)?;
    let request = battery_request();
    writer.WriteBytes(&request).map_err(error)?;
    wait(writer.StoreAsync().map_err(error)?)?;

    let reader =
        DataReader::CreateDataReader(&socket.InputStream().map_err(error)?).map_err(error)?;
    reader
        .SetInputStreamOptions(InputStreamOptions::Partial)
        .map_err(error)?;

    let mut buffer = Vec::new();
    let mut result = None;
    for _ in 0..16 {
        let count = wait(reader.LoadAsync(512).map_err(error)?)? as usize;
        if count == 0 {
            break;
        }
        let mut bytes = vec![0u8; count];
        reader.ReadBytes(&mut bytes).map_err(error)?;
        buffer.extend_from_slice(&bytes);

        while let Some(frame) = next_frame(&mut buffer) {
            if let Some(battery) = parse_battery_frame(&frame) {
                result = Some(battery);
                break;
            }
        }
        if result.is_some() {
            break;
        }
    }

    let _ = reader.Close();
    let _ = writer.Close();
    let _ = socket.Close();
    let _ = service.Close();
    result.ok_or_else(|| "ответ Huawei не содержит данных о заряде".to_string())
}

fn battery_request() -> Vec<u8> {
    let body = [
        BATTERY_COMMAND[0],
        BATTERY_COMMAND[1],
        0x01,
        0x00,
        0x02,
        0x00,
        0x03,
        0x00,
    ];
    let mut request = vec![0x5A, 0x00, (body.len() + 1) as u8, 0x00];
    request.extend_from_slice(&body);
    let checksum = crc16_xmodem(&request);
    request.extend_from_slice(&checksum.to_be_bytes());
    request
}

fn next_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let start = buffer.iter().position(|byte| *byte == 0x5A)?;
    if start > 0 {
        buffer.drain(..start);
    }
    if buffer.len() < 4 {
        return None;
    }
    let frame_length = u16::from_be_bytes([buffer[1], buffer[2]]) as usize + 5;
    if buffer.len() < frame_length {
        return None;
    }
    Some(buffer.drain(..frame_length).collect())
}

fn parse_battery_frame(frame: &[u8]) -> Option<HuaweiBattery> {
    if frame.len() < 10 || frame[0] != 0x5A || frame[3] != 0 {
        return None;
    }
    let expected_checksum = u16::from_be_bytes([frame[frame.len() - 2], frame[frame.len() - 1]]);
    if crc16_xmodem(&frame[..frame.len() - 2]) != expected_checksum {
        return None;
    }
    let body = &frame[4..frame.len() - 2];
    if body.get(..2)? != BATTERY_COMMAND {
        return None;
    }

    let mut battery = HuaweiBattery::default();
    let mut offset = 2;
    while offset + 2 <= body.len() {
        let parameter_type = body[offset];
        let length = body[offset + 1] as usize;
        if offset + 2 + length > body.len() {
            break;
        }
        let value = &body[offset + 2..offset + 2 + length];
        match parameter_type {
            1 if value.len() == 1 && value[0] <= 100 => battery.global = Some(value[0]),
            2 if value.len() == 3 => {
                battery.left = percent(value[0]);
                battery.right = percent(value[1]);
                battery.case = percent(value[2]);
            }
            _ => {}
        }
        offset += 2 + length;
    }

    (battery.global.is_some() || battery.left.is_some() || battery.right.is_some())
        .then_some(battery)
}

fn percent(value: u8) -> Option<u8> {
    (value <= 100).then_some(value)
}

fn crc16_xmodem(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= (*byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}
