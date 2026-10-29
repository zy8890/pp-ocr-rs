use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use image::RgbImage;
use rapidocr_core::{
    config::{InferenceOptions, PipelineConfig},
    model::{model_set_by_name, ModelCache, ModelDownloadMode, DEFAULT_MODEL_SET_NAME},
    types::{OcrOutput, OcrTimings},
    RapidOcr,
};

use crate::id_card::IdCardResult;
use crate::parse::structured::{parse_id_card_with_options, ParseOptions};
use crate::preprocess::{
    candidate_degrees, downscale_max_side, is_upright_enough, load_rgb_with_exif,
    orientation_score, rotate_clockwise, ORIENT_PROBE_MAX_SIDE,
};

/// OCR 引擎初始化选项。
#[derive(Debug, Clone)]
pub struct OcrEngineOptions {
    /// 模型缓存目录（首次运行可自动下载注册模型）。
    pub model_dir: PathBuf,
    /// 注册模型集名称，默认 `ppocrv5-ch-server`（中文 server，偏准确率）。
    pub model_set: String,
    /// 是否启用文本行方向分类（身份证略有旋转时建议开启）。
    pub use_cls: bool,
    /// ONNX Runtime 算子内线程数。
    pub intra_threads: usize,
    /// 字段 `reliable` 判定阈值（对比 recognition score）。
    pub min_confidence: f32,
    /// 自动尝试 90° 步进旋转矫正（默认开启）。
    pub auto_rotate: bool,
}

impl Default for OcrEngineOptions {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::from("models"),
            model_set: "ppocrv5-ch-server".to_string(),
            use_cls: true,
            intra_threads: 4,
            min_confidence: 0.80,
            auto_rotate: true,
        }
    }
}

/// 一次识别各阶段耗时（毫秒）。
#[derive(Debug, Clone, Default)]
pub struct RecognizeTimings {
    pub load_image_ms: f64,
    pub orient_probe_ms: f64,
    pub orient_probes: u32,
    pub ocr_ms: f64,
    pub ocr_passes: u32,
    pub parse_ms: f64,
    pub total_ms: f64,
    /// 最后一次全分辨率 OCR 的管线细分（若有）。
    pub last_pipeline: Option<OcrTimings>,
}

impl RecognizeTimings {
    pub fn print_stderr(&self) {
        eprintln!("--- 耗时 ---");
        eprintln!("  读图:           {:>7.1} ms", self.load_image_ms);
        if self.orient_probes > 0 {
            eprintln!(
                "  方向探测:       {:>7.1} ms  ({} 次小图 OCR)",
                self.orient_probe_ms, self.orient_probes
            );
        }
        eprintln!(
            "  主 OCR:         {:>7.1} ms  ({} 次全分辨率)",
            self.ocr_ms, self.ocr_passes
        );
        if let Some(t) = &self.last_pipeline {
            eprintln!("    det 推理:     {:>7.1} ms", t.det_inference_ms);
            eprintln!("    cls 推理:     {:>7.1} ms", t.cls_inference_ms);
            eprintln!("    rec 推理:     {:>7.1} ms", t.rec_inference_ms);
            eprintln!("    管线合计:     {:>7.1} ms", t.total_ms);
        }
        eprintln!("  结构化解析:   {:>7.1} ms", self.parse_ms);
        eprintln!("  识别合计:     {:>7.1} ms", self.total_ms);
    }
}

/// 封装 RapidOCR 管道与身份证结构化解析。
pub struct IdCardOcr {
    ocr: RapidOcr,
    min_confidence: f32,
    auto_rotate: bool,
}

impl IdCardOcr {
    pub fn new(options: OcrEngineOptions) -> Result<Self> {
        let model_set = model_set_by_name(&options.model_set).with_context(|| {
            format!(
                "未知模型集 {:?}，可用: {:?}",
                options.model_set,
                rapidocr_core::model::available_model_set_names()
            )
        })?;

        let pipeline = if options.use_cls {
            PipelineConfig::full()
        } else {
            PipelineConfig::without_cls()
        };

        let cache = ModelCache::new(&options.model_dir);
        cache
            .ensure_model_set_for_pipeline(model_set, pipeline, ModelDownloadMode::Missing)
            .context("准备 ONNX 模型失败（检查网络或手动放入 model_dir）")?;

        let cfg = cache
            .config_for(model_set)
            .with_pipeline(pipeline)
            .with_inference_options(InferenceOptions {
                intra_threads: options.intra_threads,
                inter_threads: 1,
                parallel_execution: false,
                ..Default::default()
            });

        let ocr = RapidOcr::from_config(cfg).context("初始化 OCR 管道失败")?;
        Ok(Self {
            ocr,
            min_confidence: options.min_confidence,
            auto_rotate: options.auto_rotate,
        })
    }

    /// 使用默认模型集（与 rapidocr 示例一致的小模型，更快）。
    pub fn new_fast(model_dir: impl AsRef<Path>) -> Result<Self> {
        Self::new(OcrEngineOptions {
            model_dir: model_dir.as_ref().to_path_buf(),
            model_set: DEFAULT_MODEL_SET_NAME.to_string(),
            use_cls: true,
            intra_threads: 4,
            min_confidence: 0.80,
            auto_rotate: true,
        })
    }

    pub fn run_raw_path(&mut self, path: impl AsRef<Path>) -> Result<OcrOutput> {
        let (out, _, _) = self.run_with_orientation(path)?;
        Ok(out)
    }

    pub fn recognize_id_card_path(&mut self, path: impl AsRef<Path>) -> Result<IdCardResult> {
        let (result, _timings) = self.recognize_id_card_path_timed(path)?;
        Ok(result)
    }

    pub fn recognize_id_card_path_timed(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<(IdCardResult, RecognizeTimings)> {
        let total = Instant::now();
        let (output, rotation_degrees, mut timings) = self.run_with_orientation(path)?;

        let t_parse = Instant::now();
        let mut result = parse_id_card_with_options(
            &output,
            &ParseOptions {
                min_confidence: self.min_confidence,
            },
        );
        timings.parse_ms = t_parse.elapsed().as_secs_f64() * 1000.0;

        result.rotation_degrees = rotation_degrees;
        if rotation_degrees != 0 {
            result
                .warnings
                .push(format!("已顺时针旋转 {rotation_degrees}° 后识别"));
        }
        timings.total_ms = total.elapsed().as_secs_f64() * 1000.0;
        Ok((result, timings))
    }

    fn run_with_orientation(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<(OcrOutput, u32, RecognizeTimings)> {
        let mut timings = RecognizeTimings::default();

        let t_load = Instant::now();
        let image = load_rgb_with_exif(path).context("加载图片失败")?;
        timings.load_image_ms = t_load.elapsed().as_secs_f64() * 1000.0;

        let (out, deg) = self.run_image_with_orientation(&image, &mut timings)?;
        Ok((out, deg, timings))
    }

    fn run_image_with_orientation(
        &mut self,
        image: &RgbImage,
        timings: &mut RecognizeTimings,
    ) -> Result<(OcrOutput, u32)> {
        // 正向图：只跑 1 次全分辨率 OCR。仅当判定非正向时，才小图试角度 + 再跑 1 次。
        let t_ocr = Instant::now();
        let timed0 = self
            .ocr
            .run_image_timed(image)
            .context("OCR 推理失败")?;
        timings.ocr_ms += t_ocr.elapsed().as_secs_f64() * 1000.0;
        timings.ocr_passes += 1;
        timings.last_pipeline = Some(timed0.timings);
        let out0 = timed0.output;

        if !self.auto_rotate || is_upright_enough(&out0) {
            return Ok((out0, 0));
        }

        let probe = downscale_max_side(image, ORIENT_PROBE_MAX_SIDE);
        let mut best_deg = 0_u32;
        let mut best_score = orientation_score(&out0);

        for deg in candidate_degrees(&out0, image.width(), image.height()) {
            let rotated = rotate_clockwise(&probe, deg);
            let t_probe = Instant::now();
            let out = self
                .ocr
                .run_image(&rotated)
                .with_context(|| format!("OCR 方向探测失败（旋转 {deg}°）"))?;
            timings.orient_probe_ms += t_probe.elapsed().as_secs_f64() * 1000.0;
            timings.orient_probes += 1;

            let score = orientation_score(&out);
            if score > best_score {
                best_score = score;
                best_deg = deg;
            }
            if is_upright_enough(&out) {
                best_deg = deg;
                break;
            }
        }

        if best_deg == 0 {
            return Ok((out0, 0));
        }

        let final_img = rotate_clockwise(image, best_deg);
        let t_ocr2 = Instant::now();
        let timed = self
            .ocr
            .run_image_timed(&final_img)
            .context("OCR 推理失败（矫正后）")?;
        timings.ocr_ms += t_ocr2.elapsed().as_secs_f64() * 1000.0;
        timings.ocr_passes += 1;
        timings.last_pipeline = Some(timed.timings);

        Ok((timed.output, best_deg))
    }
}
