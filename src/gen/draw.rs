use ab_glyph::{Font, FontRef, Point, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};

pub fn draw_text(
    img: &mut RgbaImage,
    font: &FontRef<'_>,
    text: &str,
    x: f32,
    y: f32,
    px: f32,
    color: Rgba<u8>,
) {
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let mut position = Point {
        x,
        y: y + scaled.ascent(),
    };
    for ch in text.chars() {
        let mut glyph = scaled.scaled_glyph(ch);
        glyph.position = position;
        let advance = scaled.h_advance(glyph.id);
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cover| {
                let px_x = bounds.min.x as i32 + gx as i32;
                let px_y = bounds.min.y as i32 + gy as i32;
                if px_x < 0 || px_y < 0 {
                    return;
                }
                let (px_x, px_y) = (px_x as u32, px_y as u32);
                if px_x >= img.width() || px_y >= img.height() {
                    return;
                }
                let cover = cover.clamp(0.0, 1.0);
                let dest = img.get_pixel_mut(px_x, px_y);
                for i in 0..3 {
                    dest.0[i] = (color.0[i] as f32 * cover
                        + dest.0[i] as f32 * (1.0 - cover))
                        .round() as u8;
                }
                dest.0[3] = 255;
            });
        }
        position.x += advance;
    }
}
