use std::path::{Path, PathBuf};

use ab_glyph::FontRef;
use anyhow::{Context, Result};
use image::{imageops, Rgba, RgbaImage};

use super::draw::draw_text;
use super::fields::{wrap_address, IdCardFields};

pub struct RenderOptions {
    pub assets_dir: PathBuf,
    pub avatar: Option<PathBuf>,
    pub front_height: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            assets_dir: PathBuf::from("assets/idgen"),
            avatar: None,
            front_height: 1754,
        }
    }
}

pub fn render_front(fields: &IdCardFields, opts: &RenderOptions) -> Result<RgbaImage> {
    let empty_path = opts.assets_dir.join("empty.png");
    let mut img = image::open(&empty_path)
        .with_context(|| format!("打开底图 {}", empty_path.display()))?
        .into_rgba8();

    let hei_data = read_font_bytes(&opts.assets_dir.join("fonts/hei.ttf"))?;
    let fzhei_data = read_font_bytes(&opts.assets_dir.join("fonts/fzhei.ttf"))?;
    let ocrb_data = read_font_bytes(&opts.assets_dir.join("fonts/ocrb10bt.ttf"))?;

    let hei = FontRef::try_from_slice(&hei_data).context("解析字体 hei.ttf 失败")?;
    let fzhei = FontRef::try_from_slice(&fzhei_data).context("解析字体 fzhei.ttf 失败")?;
    let ocrb = FontRef::try_from_slice(&ocrb_data).context("解析字体 ocrb10bt.ttf 失败")?;

    let black = Rgba([0, 0, 0, 255]);

    draw_text(&mut img, &hei, &fields.name, 630.0, 690.0, 72.0, black);
    draw_text(&mut img, &hei, &fields.sex, 630.0, 840.0, 64.0, black);
    draw_text(&mut img, &hei, &fields.nation, 1030.0, 840.0, 64.0, black);
    draw_text(&mut img, &fzhei, &fields.year, 630.0, 975.0, 60.0, black);
    draw_text(&mut img, &fzhei, &fields.month, 950.0, 975.0, 60.0, black);
    draw_text(&mut img, &fzhei, &fields.day, 1150.0, 975.0, 60.0, black);

    let mut addr_y = 1115.0;
    for line in wrap_address(&fields.address, 22) {
        draw_text(&mut img, &hei, &line, 630.0, addr_y, 64.0, black);
        addr_y += 100.0;
    }

    draw_text(
        &mut img,
        &ocrb,
        &fields.id_number,
        900.0,
        1475.0,
        90.0,
        black,
    );

    if let Some(avatar_path) = &opts.avatar {
        let av = image::open(avatar_path)
            .with_context(|| format!("打开头像 {}", avatar_path.display()))?
            .into_rgba8();
        let av = imageops::resize(&av, 500, 670, imageops::FilterType::Lanczos3);
        imageops::overlay(&mut img, &av, 1500, 690);
    }

    let h = opts.front_height.min(img.height());
    let front = imageops::crop_imm(&img, 0, 0, img.width(), h).to_image();
    Ok(front)
}

fn read_font_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("读取字体 {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::fields::IdCardFields;

    #[test]
    fn render_smoke_writes_pixels() {
        let fields = IdCardFields {
            name: "测试".into(),
            sex: "男".into(),
            nation: "汉".into(),
            year: "1990".into(),
            month: "1".into(),
            day: "2".into(),
            address: "四川省成都市武侯区益州大道中段722号复城国际".into(),
            id_number: "11010519491231002X".into(),
        };
        let img = render_front(&fields, &RenderOptions::default()).expect("render");
        assert_eq!(img.width(), 2480);
        assert_eq!(img.height(), 1754);
        let mut dark = 0u32;
        for y in 1475..1600 {
            for x in 900..2000 {
                let p = img.get_pixel(x, y);
                if p.0[0] < 200 {
                    dark += 1;
                }
            }
        }
        assert!(dark > 100, "expected drawn id digits, dark={dark}");
    }
}
