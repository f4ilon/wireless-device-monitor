use std::error::Error;

use hidapi::{DeviceInfo, HidApi};

pub mod compx;
pub mod logitech;
pub mod razer;
pub mod standard;
pub mod steelseries;

#[derive(Clone, Debug)]
pub struct BatteryDevice {
    pub name: String,
    pub path: String,
    pub level: u8,
}

/// Интерфейс поставщика данных об аккумуляторе для устройств конкретного производителя.
pub trait BatteryProvider {
    /// Возвращает true, если провайдер работает с данным VID/PID.
    fn supports(&self, vendor_id: u16, product_id: u16) -> bool;

    /// Читает уровни заряда устройства и подключённых к нему приёмников.
    fn get_battery_levels(
        &self,
        api: &HidApi,
        device: &DeviceInfo,
    ) -> Result<Vec<BatteryDevice>, Box<dyn Error>>;
}

/// Список всех подключённых поставщиков.
pub fn providers() -> Vec<Box<dyn BatteryProvider>> {
    vec![
        Box::new(compx::CompxProvider),
        Box::new(logitech::LogitechProvider),
        Box::new(razer::RazerProvider),
        Box::new(steelseries::SteelSeriesProvider),
        Box::new(standard::StandardHidProvider),
    ]
}
