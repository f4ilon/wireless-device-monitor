use image::{Rgba, RgbaImage};

/// Рисует иконку батареи (RGBA 8-бит) заданного размера.
/// `level` — заряд 0–100; `None` рисует только контур.
pub fn battery_icon(size: u32, level: Option<u8>, dark_taskbar: bool) -> RgbaImage {
    let w = size.max(16);
    let h = size.max(16);
    let mut img = RgbaImage::new(w, h);

    let border = if dark_taskbar {
        Rgba([248, 248, 248, 255])
    } else {
        Rgba([24, 24, 24, 255])
    };
    let empty = Rgba([0, 0, 0, 0]);
    let fill = match level {
        Some(v) if v <= 20 => Rgba([220, 40, 40, 255]),
        Some(v) if v <= 50 => Rgba([255, 170, 0, 255]),
        _ => Rgba([0, 200, 80, 255]),
    };

    let body_x0 = w * 3 / 16;
    let body_x1 = w * 13 / 16;
    let body_y0 = h * 5 / 16;
    let body_y1 = h * 11 / 16;
    let terminal_x0 = body_x1;
    let terminal_x1 = (body_x1 + w * 2 / 16).min(w.saturating_sub(1));
    let terminal_y0 = h * 7 / 16;
    let terminal_y1 = h * 9 / 16;

    // Battery body: one-pixel outline plus softly rounded corners.
    for y in body_y0..=body_y1 {
        for x in body_x0..=body_x1 {
            let corner = (x == body_x0 || x == body_x1) && (y == body_y0 || y == body_y1);
            let edge = x == body_x0
                || x == body_x1
                || y == body_y0
                || y == body_y1;
            if corner {
                img.put_pixel(x, y, empty);
            } else if edge {
                img.put_pixel(x, y, border);
            }
        }
    }

    // Small terminal nub, as in Windows battery indicators.
    for y in terminal_y0..=terminal_y1 {
        for x in terminal_x0..=terminal_x1 {
            img.put_pixel(x, y, border);
        }
    }

    if let Some(level) = level {
        let inner_x0 = body_x0 + 2;
        let inner_x1 = body_x1.saturating_sub(2);
        let inner_y0 = body_y0 + 2;
        let inner_y1 = body_y1.saturating_sub(2);
        let inner_width = inner_x1.saturating_sub(inner_x0 + 1);
        let filled = if level == 0 {
            0
        } else {
            ((inner_width as u32 * level as u32 + 99) / 100)
                .clamp(1, inner_width as u32)
        };
        let fill_x1 = (inner_x0 + filled as u32).min(inner_x1 as u32);

        for y in inner_y0..=inner_y1 {
            for x in inner_x0..fill_x1 as u32 {
                img.put_pixel(x, y, fill);
            }
        }
    }

    img
}
