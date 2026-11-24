use windows::Devices::Bluetooth::Rfcomm::RfcommServiceId;
use windows::Devices::Bluetooth::{BluetoothDevice, BluetoothError};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Networking::Sockets::StreamSocket;
use windows::Storage::Streams::{DataReader, DataWriter, InputStreamOptions};
use windows::core::GUID;

use crate::bluetooth::{error, wait, wait_action};
use crate::vendors::BatteryDevice;

const XIAOMI_SERVICE_UUID: GUID = GUID::from_u128(0x0000FD2D_0000_1000_8000_00805F9B34FB);
const GET_TARGET_INFO: [u8; 13] = [
    0xFE, 0xDC, 0xBA, 0xC0, 0x02, 0x00, 0x05, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xEF,
];

pub fn battery_devices() -> Result<Vec<BatteryDevice>, String> {
    let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true).map_err(error)?;
    let information = wait(DeviceInformation::FindAllAsyncAqsFilter(&selector).map_err(error)?)?;
    let mut devices = Vec::new();

    for information in &information {
        let name = information.Name().map_err(error)?.to_string();
        if !name.to_lowercase().contains("redmi buds 6 lite") {
            continue;
        }

        let id = information.Id().map_err(error)?;
        let device = match wait(BluetoothDevice::FromIdAsync(&id).map_err(error)?) {
            Ok(device) => device,
            Err(_) => continue,
        };
        if let Ok(readings) = read_battery(&device) {
            for ((suffix, path_suffix), level) in
                [("левый", "left"), ("правый", "right"), ("кейс", "case")]
                    .into_iter()
                    .zip(readings)
            {
                let Some(level) = level else {
                    continue;
                };
                devices.push(BatteryDevice {
                    name: format!("{name} ({suffix})"),
                    path: format!("rfcomm:{id}:{path_suffix}"),
                    level,
                });
            }
        }
        let _ = device.Close();
    }

    Ok(devices)
}

fn read_battery(device: &BluetoothDevice) -> Result<[Option<u8>; 3], String> {
    let service_id = RfcommServiceId::FromUuid(XIAOMI_SERVICE_UUID).map_err(error)?;
    let services = wait(
        device
            .GetRfcommServicesForIdAsync(&service_id)
            .map_err(error)?,
    )?;
    if services.Error().map_err(error)? != BluetoothError::Success {
        return Err("служба Xiaomi RFCOMM недоступна".to_string());
    }
    let service = services
        .Services()
        .map_err(error)?
        .into_iter()
        .next()
        .ok_or_else(|| "служба Xiaomi RFCOMM не найдена".to_string())?;

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
    writer.WriteBytes(&GET_TARGET_INFO).map_err(error)?;
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
            if let Some(levels) = parse_battery_frame(&frame) {
                result = Some(levels);
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
    result.ok_or_else(|| "наушники Xiaomi не вернули уровень заряда".to_string())
}

fn next_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        let start = buffer
            .windows(3)
            .position(|header| header == [0xFE, 0xDC, 0xBA])?;
        if start > 0 {
            buffer.drain(..start);
        }
        if buffer.len() < 7 {
            return None;
        }

        let body_length = u16::from_be_bytes([buffer[5], buffer[6]]) as usize;
        let frame_length = body_length + 8;
        if buffer.len() < frame_length {
            return None;
        }
        if buffer[frame_length - 1] != 0xEF {
            buffer.remove(0);
            continue;
        }
        return Some(buffer.drain(..frame_length).collect());
    }
}

fn parse_battery_frame(frame: &[u8]) -> Option<[Option<u8>; 3]> {
    if frame.len() < 9 {
        return None;
    }
    let flags = frame[3];
    let opcode = frame[4];
    if !matches!(opcode, 0x02 | 0x0E) {
        return None;
    }

    let body_length = u16::from_be_bytes([frame[5], frame[6]]) as usize;
    let body = frame.get(7..7 + body_length)?;
    let mut offset = if flags & 0x80 == 0 { 2 } else { 1 };
    while offset + 2 <= body.len() {
        let length = body[offset] as usize;
        let attribute_type = body[offset + 1];
        if length < 1 || offset + 1 + length > body.len() {
            break;
        }
        let value = &body[offset + 2..offset + 1 + length];
        if ((opcode == 0x02 && attribute_type == 0x07)
            || (opcode == 0x0E && attribute_type == 0x00))
            && value.len() >= 3
        {
            return Some([
                battery_percent(value[0]),
                battery_percent(value[1]),
                battery_percent(value[2]),
            ]);
        }
        offset += 1 + length;
    }
    None
}

fn battery_percent(value: u8) -> Option<u8> {
    let level = value & 0x7F;
    (value != 0xFF && level <= 100).then_some(level)
}
