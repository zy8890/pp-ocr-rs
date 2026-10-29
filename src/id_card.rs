use serde::{Deserialize, Serialize};

/// 识别到的身份证面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdCardSide {
    Front,
    Back,
    Unknown,
}

/// 带模型置信度的字段值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldValue {
    pub value: String,
    /// RapidOCR recognition score（多行字段取最低分；同框拆分会降权）。
    pub confidence: f32,
    /// `confidence >=` 解析阈值（身份证号另受校验位影响）。
    pub reliable: bool,
}

impl FieldValue {
    pub fn new(value: impl Into<String>, confidence: f32, min_confidence: f32) -> Self {
        let confidence = confidence.clamp(0.0, 1.0);
        Self {
            value: value.into(),
            confidence,
            reliable: confidence >= min_confidence,
        }
    }

    pub fn with_reliable(mut self, reliable: bool) -> Self {
        self.reliable = reliable && self.reliable;
        self
    }
}

/// 身份证人像面结构化字段。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdCardFront {
    pub name: Option<FieldValue>,
    pub gender: Option<FieldValue>,
    pub ethnicity: Option<FieldValue>,
    /// 出生日期，格式 `YYYY-MM-DD`（由 OCR 文本归一化）。
    pub birth_date: Option<FieldValue>,
    pub address: Option<FieldValue>,
    pub id_number: Option<FieldValue>,
    /// 校验位是否通过（辅助信号，不覆盖模型文本）。
    pub id_checksum_ok: Option<bool>,
}

/// 身份证国徽面结构化字段。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IdCardBack {
    pub issuing_authority: Option<FieldValue>,
    /// 有效期限原文（略作分隔符归一）。
    pub valid_period: Option<FieldValue>,
}

/// 完整识别结果（含原始行与置信度摘要）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdCardResult {
    pub side: IdCardSide,
    pub front: Option<IdCardFront>,
    pub back: Option<IdCardBack>,
    /// 全部 OCR 文本行（调试用）。
    pub raw_lines: Vec<RawLine>,
    pub warnings: Vec<String>,
    /// 本次解析使用的最低可信阈值。
    pub min_confidence: f32,
    /// 相对原图（EXIF 校正后）顺时针旋转的角度：0 / 90 / 180 / 270。
    #[serde(default)]
    pub rotation_degrees: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawLine {
    pub text: String,
    pub score: f32,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}
