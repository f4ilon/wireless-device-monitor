use hidapi::HidApi;

pub fn get_device_name(vendor_id: u16, product_id: u16) -> Result<String, Box<dyn std::error::Error>> {
    let api = HidApi::new()?;

    for device in api.device_list() {
        if device.vendor_id() == vendor_id
            && device.product_id() == product_id
        {
            let name = device
                .product_string()
                .unwrap_or("Unknown device");

            return Ok(name.to_string());
        }
    }

    Ok("Device not found".to_string())
}


// pub fn get_device_battery_level(
//     vendor_id: u16,
//     product_id: u16,
// ) -> Result<Option<u8>, Box<dyn std::error::Error>> {

//     let api = HidApi::new()?;

//     let device_info = api.device_list()
//         .find(|device| {
//             device.vendor_id() == vendor_id
//                 && device.product_id() == product_id
//         });

//     let device_info = match device_info {
//         Some(device) => device,
//         None => return Err("Устройство не найдено".into()),
//     };

//     let device = api.open_path(device_info.path())?;

//     // Буфер Feature Report.
//     // Нулевой байт — Report ID.
//     let mut buffer = [0u8; 2];

//     // Пока не знаем команду производителя.
//     // Этот запрос является только заготовкой.
//     device.get_feature_report(&mut buffer)?;

//     let battery_level = buffer[1];

//     if battery_level <= 100 {
//         Ok(Some(battery_level))
//     } else {
//         Ok(None)
//     }
// }


pub fn test_receiver_interfaces() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;

    for info in api.device_list().filter(|d| {
        d.vendor_id() == 0x3554
            && d.product_id() == 0xF53C
            && (d.usage_page() == 0xFF02 || d.usage_page() == 0xFF04)
    }) {
        println!(
            "\nПроверяем Usage Page: {:04X}",
            info.usage_page()
        );

        match api.open_path(info.path()) {
            Ok(_device) => println!("Успешно открыто!"),
            Err(error) => println!("Не удалось открыть: {}", error),
        }
    }

    Ok(())
}


pub fn inspect_receiver_descriptors() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;

    for info in api.device_list().filter(|d| {
        d.vendor_id() == 0x3554
            && d.product_id() == 0xF53C
            && (d.usage_page() == 0xFF02 || d.usage_page() == 0xFF04)
    }) {
        println!("\n========== HID Descriptor ==========");
        println!("Usage Page: {:04X}", info.usage_page());

        let device = match api.open_path(info.path()) {
            Ok(device) => device,
            Err(error) => {
                println!("Ошибка открытия: {}", error);
                continue;
            }
        };

        let mut buffer = vec![0u8; hidapi::MAX_REPORT_DESCRIPTOR_SIZE];

        match device.get_report_descriptor(&mut buffer) {
            Ok(length) => {
                buffer.truncate(length);

                println!("Размер дескриптора: {} байт", length);
                println!("Содержимое:");

                for chunk in buffer.chunks(16) {
                    for byte in chunk {
                        print!("{:02X} ", byte);
                    }
                    println!();
                }
            }
            Err(error) => {
                println!("Ошибка чтения дескриптора: {}", error);
            }
        }
    }

    Ok(())
}


pub fn read_receiver_feature_report() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;

    for info in api.device_list().filter(|d| {
        d.vendor_id() == 0x3554
            && d.product_id() == 0xF53C
            && d.usage_page() == 0xFF04
    }) {
        let device = api.open_path(info.path())?;

        // Report ID 0x06 + 7 байт данных
        let mut report = [0u8; 8];
        report[0] = 0x06;

        println!("Запрашиваем Feature Report 0x06...");

        match device.get_feature_report(&mut report) {
            Ok(length) => {
                println!("Получено байт: {}", length);
                print!("Ответ: ");

                for byte in report.iter().take(length) {
                    print!("{:02X} ", byte);
                }

                println!();
            }

            Err(error) => {
                println!("Ошибка чтения: {}", error);
            }
        }
    }

    Ok(())
}


pub fn get_device_battery_level(
    vendor_id: u16,
    product_id: u16,
) -> Result<Option<u8>, Box<dyn std::error::Error>> {

    let api = HidApi::new()?;

    let info = api.device_list()
        .find(|d| {
            d.vendor_id() == vendor_id
                && d.product_id() == product_id
                && d.usage_page() == 0xFF02
        })
        .ok_or("HID-интерфейс FF02 не найден")?;

    let device = api.open_path(info.path())?;

    // Compx Battery Level Request
    let mut request = [0u8; 17];

    request[0] = 0x08; // Report ID
    request[1] = 0x04; // Battery Level command
    request[15] = 0xEF;

    // Вычисляем контрольную сумму
    let sum: u16 = request[..16]
        .iter()
        .map(|&b| b as u16)
        .sum();

    request[16] = (0x55u16.wrapping_sub(sum) & 0xFF) as u8;

    println!("Отправляем запрос:");
    for byte in &request {
        print!("{:02X} ", byte);
    }
    println!();

    device.write(&request)?;

    // Читаем ответ приёмника
    let mut response = [0u8; 17];

    let length = device.read_timeout(&mut response, 1500)?;

    if length == 0 {
        println!("Приёмник не ответил");
        return Ok(None);
    }

    println!("Ответ:");
    for byte in response.iter().take(length) {
        print!("{:02X} ", byte);
    }
    println!();

    // Проверяем ответ
    if length >= 7
        && response[0] == 0x08
        && response[1] == 0x04
        && response[6] <= 100
    {
        return Ok(Some(response[6]));
    }

    Ok(None)
}



