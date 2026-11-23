use std::sync::mpsc;
use std::time::Duration;

use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice, BluetoothUuidHelper};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::RuntimeType;
use windows_future::{AsyncOperationCompletedHandler, IAsyncOperation};

use crate::vendors::BatteryDevice;

const BATTERY_SERVICE: u32 = 0x180F;
const BATTERY_LEVEL_CHARACTERISTIC: u32 = 0x2A19;
const ASYNC_TIMEOUT: Duration = Duration::from_secs(4);

pub fn battery_devices() -> Result<Vec<BatteryDevice>, String> {
    std::thread::spawn(|| {
        let _apartment = ComApartment::new()?;
        read_battery_devices()
    })
    .join()
    .map_err(|_| "поток сканирования Bluetooth завершился с ошибкой".to_string())?
}

fn read_battery_devices() -> Result<Vec<BatteryDevice>, String> {
    let selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true).map_err(error)?;
    let information = wait(DeviceInformation::FindAllAsyncAqsFilter(&selector).map_err(error)?)?;
    let mut devices = Vec::new();

    for information in &information {
        let id = information.Id().map_err(error)?;
        let name = information.Name().map_err(error)?.to_string();
        let device = match wait(BluetoothLEDevice::FromIdAsync(&id).map_err(error)?) {
            Ok(device) => device,
            Err(_) => continue,
        };
        if let Ok(Some(level)) = read_battery_level(&device) {
            devices.push(BatteryDevice {
                name: if name.is_empty() {
                    "Bluetooth устройство".to_string()
                } else {
                    name
                },
                path: format!("bluetooth:{id}"),
                level,
            });
        }
        let _ = device.Close();
    }

    Ok(devices)
}

fn read_battery_level(device: &BluetoothLEDevice) -> Result<Option<u8>, String> {
    let service_uuid = BluetoothUuidHelper::FromShortId(BATTERY_SERVICE).map_err(error)?;
    let services = wait(
        device
            .GetGattServicesForUuidWithCacheModeAsync(service_uuid, BluetoothCacheMode::Cached)
            .map_err(error)?,
    )?;
    if services.Status().map_err(error)? != GattCommunicationStatus::Success {
        return Ok(None);
    }

    let characteristic_uuid =
        BluetoothUuidHelper::FromShortId(BATTERY_LEVEL_CHARACTERISTIC).map_err(error)?;
    for service in &services.Services().map_err(error)? {
        let characteristics = wait(
            service
                .GetCharacteristicsForUuidWithCacheModeAsync(
                    characteristic_uuid,
                    BluetoothCacheMode::Cached,
                )
                .map_err(error)?,
        )?;
        if characteristics.Status().map_err(error)? != GattCommunicationStatus::Success {
            continue;
        }

        for characteristic in &characteristics.Characteristics().map_err(error)? {
            let result = wait(
                characteristic
                    .ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)
                    .map_err(error)?,
            )?;
            if result.Status().map_err(error)? != GattCommunicationStatus::Success {
                continue;
            }

            let reader = DataReader::FromBuffer(&result.Value().map_err(error)?).map_err(error)?;
            if reader.UnconsumedBufferLength().map_err(error)? == 0 {
                continue;
            }
            let level = reader.ReadByte().map_err(error)?;
            if level <= 100 {
                return Ok(Some(level));
            }
        }
    }

    Ok(None)
}

fn wait<T: RuntimeType + Send>(operation: IAsyncOperation<T>) -> Result<T, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    operation
        .SetCompleted(&AsyncOperationCompletedHandler::new(move |operation, _| {
            let result = operation.ok().and_then(|operation| operation.GetResults());
            let _ = sender.send(result);
            Ok(())
        }))
        .map_err(error)?;

    match receiver.recv_timeout(ASYNC_TIMEOUT) {
        Ok(result) => result.map_err(error),
        Err(error) => {
            let _ = operation.Cancel();
            Err(error.to_string())
        }
    }
}

fn error(error: windows::core::Error) -> String {
    error.to_string()
}

struct ComApartment;

impl ComApartment {
    fn new() -> Result<Self, String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(error)?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}
