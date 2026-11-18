use image::{Rgba, RgbaImage};

/// Рисует иконку батареи (RGBA 8-бит) заданного размера.
/// `level` — заряд 0–100; `None` рисует только контур.
pub fn battery_icon(size: u32, level: Option<u8>) -> RgbaImage {
    let w = size;
    let h = size;
    let mut img = RgbaImage::new(w, h);

    // Пропорции подобраны под квадрат 16x16 и масштабируются линейно.
    let body_x0 = w / 8;
    let body_x1 = w.saturating_sub(w / 4).max(body_x0 + 1);
    let body_y0 = h / 4;
    let body_y1 = h.saturating_sub(h / 4).max(body_y0 + 1);

    let nib_x0 = body_x1; // «полюс» справа
    let nib_x1 = w.saturating_sub(1).max(nib_x0);
    let nib_y0 = h / 3;
    let nib_y1 = h.saturating_sub(h / 3).max(nib_y0);

    let border = Rgba([70, 70, 70, 255]);
    let empty = Rgba([0, 0, 0, 0]);
    let fill = match level {
        Some(v) if v <= 20 => Rgba([220, 40, 40, 255]),
        Some(v) if v <= 50 => Rgba([255, 170, 0, 255]),
        _ => Rgba([0, 200, 80, 255]),
    };

    // Контур корпуса.
    for x in body_x0..=body_x1 {
        for y in body_y0..=body_y1 {
            let on_edge = x == body_x0 || x == body_x1 || y == body_y0 || y == body_y1;
            img.put_pixel(x, y, if on_edge { border } else { empty });
        }
    }

    // Полюс батареи.
    for x in nib_x0..=nib_x1 {
        for y in nib_y0..=nib_y1 {
            img.put_pixel(x, y, border);
        }
    }

    // Заполнение уровнем заряда (слева направо внутри корпуса).
    if let Some(level) = level {
        let inner_w = body_x1.saturating_sub(body_x0 + 1);
        let filled = (inner_w * level as u32 / 100).max(if level > 0 { 1 } else { 0 });
        let to = (body_x0 + 1 + filled).min(body_x1.saturating_sub(1));
        for x in (body_x0 + 1)..=to {
            for y in (body_y0 + 1)..body_y1 {
                img.put_pixel(x, y, fill);
            }
        }
    }

    img
}
