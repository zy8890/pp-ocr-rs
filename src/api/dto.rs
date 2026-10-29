use poem_openapi::{Enum, Object};
use serde_json::Value;

#[derive(Debug, Object)]
pub struct HealthResponse {
    pub status: String,
    pub workers: usize,
}

#[derive(Debug, Object)]
pub struct Base64Request {
    /// 原始 base64，或 `data:image/...;base64,` 前缀形式
    pub image_base64: String,
}

#[derive(Debug, Object)]
pub struct TimingsMs {
    pub ocr: f64,
    pub total: f64,
}

#[derive(Debug, Object)]
pub struct Meta {
    pub id_checksum_ok: Option<bool>,
    pub rotation_degrees: u32,
    pub side: String,
    pub timings_ms: TimingsMs,
}

#[derive(Debug, Object)]
pub struct OcrSuccess {
    /// 中文字段：姓名/性别/…
    pub zh: Value,
    /// 英文字段：name/gender/…
    pub en: Value,
    pub meta: Meta,
}

#[derive(Debug, Object)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Object)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Enum)]
pub enum SideName {
    #[oai(rename = "front")]
    Front,
    #[oai(rename = "back")]
    Back,
    #[oai(rename = "unknown")]
    Unknown,
}
