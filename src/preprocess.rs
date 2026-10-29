//! 读图（含 EXIF）与 90° 步进的自动旋转矫正。

use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use image::{imageops, DynamicImage, ImageDecoder, ImageReader, RgbImage};
use rapidocr_core::types::OcrOutput;

/// 方向探测时的最长边（降低分辨率以加速试旋转）。
pub const ORIENT_PROBE_MAX_SIDE: u32 = 960;

/// 加载 RGB 图并应用 EXIF 方向。
pub fn load_rgb_with_exif(path: impl AsRef<Path>) -> Result<RgbImage> {
    let path = path.as_ref();
    let mut decoder = ImageReader::open(path)
        .with_context(|| format!("打开图片失败: {}", path.display()))?
        .into_decoder()
        .with_context(|| format!("解码图片失败: {}", path.display()))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .with_context(|| format!("解码图片失败: {}", path.display()))?;
    image.apply_orientation(orientation);
    Ok(image.to_rgb8())
}

/// 从内存字节加载 RGB 图并应用 EXIF 方向。
pub fn load_rgb_from_memory(bytes: &[u8]) -> Result<RgbImage> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("识别图片格式失败")?;
    let mut decoder = reader
        .into_decoder()
        .context("解码图片失败")?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).context("解码图片失败")?;
    image.apply_orientation(orientation);
    Ok(image.to_rgb8())
}

/// 顺时针旋转 0 / 90 / 180 / 270。
pub fn rotate_clockwise(img: &RgbImage, degrees: u32) -> RgbImage {
    match degrees % 360 {
        0 => img.clone(),
        90 => imageops::rotate90(img),
        180 => imageops::rotate180(img),
        270 => imageops::rotate270(img),
        other => panic!("unsupported rotation {other}"),
    }
}

/// 等比缩小，最长边不超过 `max_side`（已更小则克隆）。
pub fn downscale_max_side(img: &RgbImage, max_side: u32) -> RgbImage {
    let w = img.width();
    let h = img.height();
    let long = w.max(h);
    if long <= max_side {
        return img.clone();
    }
    let scale = max_side as f32 / long as f32;
    let nw = ((w as f32) * scale).round().max(1.0) as u32;
    let nh = ((h as f32) * scale).round().max(1.0) as u32;
    imageops::resize(img, nw, nh, imageops::FilterType::Triangle)
}

fn box_wh(line: &rapidocr_core::types::OcrLine) -> (f32, f32) {
    let mut x0 = f32::INFINITY;
    let mut y0 = f32::INFINITY;
    let mut x1 = f32::NEG_INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    for p in &line.bbox.points {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    ((x1 - x0).max(1.0), (y1 - y0).max(1.0))
}

/// 多数文本框为竖向（高>宽）时，判定为整图可能横竖颠倒。
pub fn mostly_vertical_boxes(output: &OcrOutput) -> bool {
    if output.lines.is_empty() {
        return false;
    }
    let vertical = output
        .lines
        .iter()
        .filter(|l| {
            let (w, h) = box_wh(l);
            h > w * 1.15
        })
        .count();
    vertical * 2 >= output.lines.len()
}

/// 版面方向评分：横向框 + 身份证关键词 + 识别分。
pub fn orientation_score(output: &OcrOutput) -> f64 {
    const KEYWORDS: &[&str] = &[
        "姓名",
        "性别",
        "民族",
        "出生",
        "住址",
        "公民身份号码",
        "签发机关",
        "有效期限",
    ];

    let mut score = 0.0_f64;
    for line in &output.lines {
        let (w, h) = box_wh(line);
        let conf = line.score as f64;
        if w >= h {
            score += conf * 2.0;
        } else {
            score -= conf * 1.5;
        }
        for kw in KEYWORDS {
            if line.text.contains(kw) {
                score += 3.0;
            }
        }
        let digits: String = line
            .text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if digits.len() == 18 {
            score += 4.0;
        }
    }
    score + output.lines.len() as f64 * 0.05
}

/// 是否已足够「正向」：横排为主且有身份证关键字段。
pub fn is_upright_enough(output: &OcrOutput) -> bool {
    if mostly_vertical_boxes(output) {
        return false;
    }
    let blob: String = output.lines.iter().map(|l| l.text.as_str()).collect();
    let has_label = ["姓名", "住址", "公民身份号码", "签发机关", "出生"]
        .iter()
        .any(|k| blob.contains(k));
    has_label && orientation_score(output) >= 10.0
}

/// 决定探测顺序：竖图优先试 270/90（手机竖拍常见），横图先看是否只需 180。
pub fn candidate_degrees(probe0: &OcrOutput, image_w: u32, image_h: u32) -> Vec<u32> {
    if mostly_vertical_boxes(probe0) || image_h > image_w {
        vec![270, 90, 180]
    } else if orientation_score(probe0) < 8.0 {
        vec![180, 90, 270]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapidocr_core::types::{OcrLine, OcrOutput, Quad};

    #[test]
    fn vertical_detection() {
        let output = OcrOutput {
            lines: vec![OcrLine {
                bbox: Quad::from_xyxy(0.0, 0.0, 40.0, 200.0),
                text: "测试".into(),
                score: 0.9,
            }],
        };
        assert!(mostly_vertical_boxes(&output));
        assert!(!is_upright_enough(&output));
    }
}
