//! 应用图标：深色圆角底，绿色下行、蓝色上行，和悬浮窗同一套颜色。

use eframe::egui::{self, Color32, CornerRadius, Pos2, Stroke, Vec2};

const SIZE: u32 = 256;
const GREEN: (u8, u8, u8) = (52, 211, 153);
const BLUE: (u8, u8, u8) = (96, 165, 250);
const BG: (u8, u8, u8) = (22, 27, 36);

pub fn icon_data() -> std::sync::Arc<egui::IconData> {
    let rgba = render();
    std::sync::Arc::new(egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    })
}

pub fn paint_mark(painter: &egui::Painter, center: Pos2, size: f32) {
    let rect = egui::Rect::from_center_size(center, Vec2::splat(size));
    let radius = (size * 0.28).round().clamp(2.0, 24.0) as u8;
    painter.rect_filled(
        rect,
        CornerRadius::same(radius),
        Color32::from_rgb(BG.0, BG.1, BG.2),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.0, Color32::from_white_alpha(36)),
        egui::StrokeKind::Inside,
    );
    let span = size * 0.22;
    let width = (size * 0.11).max(1.6);
    chevron(
        painter,
        center + Vec2::new(0.0, -size * 0.2),
        span,
        width,
        GREEN,
        true,
    );
    chevron(
        painter,
        center + Vec2::new(0.0, size * 0.2),
        span,
        width,
        BLUE,
        false,
    );
}

pub fn png_bytes() -> Vec<u8> {
    let rgba = render();
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, SIZE, SIZE);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("写入图标头");
    writer.write_image_data(&rgba).expect("写入图标像素");
    drop(writer);
    out
}

#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    let bytes = png_bytes();
    let data = NSData::with_bytes(&bytes);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    unsafe {
        NSApplication::sharedApplication(marker).setApplicationIconImage(Some(&image));
    }
}

fn chevron(
    painter: &egui::Painter,
    center: Pos2,
    span: f32,
    width: f32,
    color: (u8, u8, u8),
    down: bool,
) {
    let dir = if down { 1.0 } else { -1.0 };
    let left = center + Vec2::new(-span, -dir * span * 0.72);
    let mid = center + Vec2::new(0.0, dir * span * 0.72);
    let right = center + Vec2::new(span, -dir * span * 0.72);
    painter.add(egui::Shape::line(
        vec![left, mid, right],
        Stroke::new(width, Color32::from_rgb(color.0, color.1, color.2)),
    ));
}

fn render() -> Vec<u8> {
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    let margin = 10.0;
    let radius = 58.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let cover = rounded_cover(
                px,
                py,
                margin,
                margin,
                SIZE as f32 - margin,
                SIZE as f32 - margin,
                radius,
            );
            if cover > 0.0 {
                blend(&mut pixels, x, y, BG, cover);
            }
            let edge = rounded_edge(
                px,
                py,
                margin,
                margin,
                SIZE as f32 - margin,
                SIZE as f32 - margin,
                radius,
                2.2,
            );
            if edge > 0.0 {
                blend(&mut pixels, x, y, (255, 255, 255), edge * 0.16);
            }
            let down = chevron_cover(px, py, 128.0, 92.0, 44.0, 18.0, true);
            if down > 0.0 {
                blend(&mut pixels, x, y, GREEN, down);
            }
            let up = chevron_cover(px, py, 128.0, 166.0, 44.0, 18.0, false);
            if up > 0.0 {
                blend(&mut pixels, x, y, BLUE, up);
            }
        }
    }
    pixels
}

fn blend(pixels: &mut [u8], x: u32, y: u32, color: (u8, u8, u8), cover: f32) {
    let cover = cover.clamp(0.0, 1.0);
    if cover <= 0.0 {
        return;
    }
    let index = ((y * SIZE + x) * 4) as usize;
    let dst_a = pixels[index + 3] as f32 / 255.0;
    let src_a = cover;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return;
    }
    for (offset, channel) in [color.0, color.1, color.2].into_iter().enumerate() {
        let dst = pixels[index + offset] as f32 / 255.0;
        let src = channel as f32 / 255.0;
        let out = (src * src_a + dst * dst_a * (1.0 - src_a)) / out_a;
        pixels[index + offset] = (out * 255.0).round() as u8;
    }
    pixels[index + 3] = (out_a * 255.0).round() as u8;
}

fn rounded_cover(x: f32, y: f32, left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> f32 {
    (0.75 - round_box_sdf(x, y, left, top, right, bottom, radius)).clamp(0.0, 1.0)
}

fn rounded_edge(
    x: f32,
    y: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
    width: f32,
) -> f32 {
    let dist = round_box_sdf(x, y, left, top, right, bottom, radius).abs();
    (width * 0.5 - dist + 0.5).clamp(0.0, 1.0)
}

fn round_box_sdf(x: f32, y: f32, left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> f32 {
    let cx = (left + right) * 0.5;
    let cy = (top + bottom) * 0.5;
    let half_w = (right - left) * 0.5;
    let half_h = (bottom - top) * 0.5;
    let qx = (x - cx).abs() - half_w + radius;
    let qy = (y - cy).abs() - half_h + radius;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    ox.hypot(oy) + qx.max(qy).min(0.0) - radius
}

fn chevron_cover(x: f32, y: f32, cx: f32, cy: f32, span: f32, thick: f32, down: bool) -> f32 {
    let dir = if down { 1.0 } else { -1.0 };
    let left = (cx - span, cy - dir * span * 0.62);
    let mid = (cx, cy + dir * span * 0.62);
    let right = (cx + span, cy - dir * span * 0.62);
    let dist = dist_segment(x, y, left, mid).min(dist_segment(x, y, mid, right));
    ((thick * 0.5) - dist + 0.5).clamp(0.0, 1.0)
}

fn dist_segment(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let abx = b.0 - a.0;
    let aby = b.1 - a.1;
    let len2 = abx * abx + aby * aby;
    if len2 <= f32::EPSILON {
        return (x - a.0).hypot(y - a.1);
    }
    let t = ((x - a.0) * abx + (y - a.1) * aby) / len2;
    let t = t.clamp(0.0, 1.0);
    (x - (a.0 + abx * t)).hypot(y - (a.1 + aby * t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_transparent_corners_and_both_arrows() {
        let pixels = render();
        assert_eq!(pixel(&pixels, 0, 0)[3], 0);
        assert!(pixel(&pixels, 128, 128)[3] > 200);
        assert!(pixels.chunks(4).any(|px| {
            px[3] > 200 && px[1] > px[0].saturating_add(40) && px[1] > px[2].saturating_add(20)
        }));
        assert!(pixels.chunks(4).any(|px| {
            px[3] > 200 && px[2] > px[0].saturating_add(40) && px[2] > px[1].saturating_add(20)
        }));
        let png = png_bytes();
        assert!(png.starts_with(b"\x89PNG"));
    }

    fn pixel(pixels: &[u8], x: u32, y: u32) -> &[u8] {
        let index = ((y * SIZE + x) * 4) as usize;
        &pixels[index..index + 4]
    }
}
