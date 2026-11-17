use std::error::Error;
use hidapi::HidApi;

pub mod compx;

pub trait BatteryProvider {
    fn supports(
        &self,
        vendor_id: u16,
        product_id: u16,
    ) -> bool;

    fn get_battery_level(
        &self,
        api: &HidApi,
        vendor_id: u16,
        product_id: u16,
    ) -> Result<Option<u8>, Box<dyn Error>>;
}

pub fn get_device_battery_level(
    vendor_id: u16,
    product_id: u16,
) -> Result<Option<u8>, Box<dyn Error>> {

    let api = HidApi::new()?;

    let providers: [&dyn BatteryProvider; 1] = [
        &compx::CompxProvider,
    ];

    for provider in providers {
        if provider.supports(vendor_id, product_id) {
            return provider.get_battery_level(
                &api,
                vendor_id,
                product_id,
            );
        }
    }

    Ok(None)
}