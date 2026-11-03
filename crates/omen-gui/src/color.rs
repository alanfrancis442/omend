// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use omen_common::{RgbColor, hsv_to_rgb, rgb_to_hsv};

pub fn color_wheel(size: u32) -> slint::Image {
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(size, size);
    let center = (size as f32 - 1.0) / 2.0;
    let max_r = center - 1.0;
    let pixels = buffer.make_mut_slice();
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let pixel = &mut pixels[(y * size + x) as usize];
            if dist > max_r {
                *pixel = slint::Rgba8Pixel {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                };
                continue;
            }
            let saturation = dist / max_r;
            let hue = dy.atan2(dx).to_degrees().rem_euclid(360.0);
            let color = hsv_to_rgb(hue, saturation, 1.0);
            *pixel = slint::Rgba8Pixel {
                r: color.r,
                g: color.g,
                b: color.b,
                a: 255,
            };
        }
    }
    slint::Image::from_rgba8(buffer)
}

/// Hue is the angle and saturation is the distance from the center.
pub fn color_at(x: f32, y: f32, width: f32, height: f32) -> (RgbColor, f32, f32) {
    let cx = width / 2.0;
    let cy = height / 2.0;
    let dx = x - cx;
    let dy = y - cy;
    let max_r = width.min(height) / 2.0 - 1.0;
    let dist = (dx * dx + dy * dy).sqrt().min(max_r.max(1.0));
    let saturation = if max_r <= 0.0 { 0.0 } else { dist / max_r };
    let hue = dy.atan2(dx).to_degrees().rem_euclid(360.0);
    let color = hsv_to_rgb(hue, saturation.clamp(0.0, 1.0), 1.0);
    let marker_x = cx + saturation * max_r * hue.to_radians().cos();
    let marker_y = cy + saturation * max_r * hue.to_radians().sin();
    (color, marker_x, marker_y)
}

pub fn marker_for(color: RgbColor, size: f32) -> (f32, f32) {
    let (hue, saturation, _) = rgb_to_hsv(color);
    let center = size / 2.0;
    let max_r = center - 1.0;
    let angle = hue.to_radians();
    (
        center + saturation * max_r * angle.cos(),
        center + saturation * max_r * angle.sin(),
    )
}
