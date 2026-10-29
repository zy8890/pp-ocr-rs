use std::sync::Arc;

use base64::Engine as _;
use poem_openapi::{
    payload::Json,
    types::multipart::Upload,
    ApiResponse, Multipart, OpenApi,
};

use super::auth::BearerAuth;
use super::dto::{
    Base64Request, ErrorBody, ErrorDetail, HealthResponse, Meta, OcrSuccess, TimingsMs,
};
use super::pool::OcrPool;
use crate::format_out::{to_en_json, to_zh_json};
use crate::id_card::{IdCardResult, IdCardSide};
use crate::RecognizeTimings;

pub struct IdCardApi {
    pub pool: Arc<OcrPool>,
    pub max_upload_bytes: usize,
}

#[derive(Debug, Multipart)]
struct OcrUpload {
    file: Upload,
}

#[derive(ApiResponse)]
enum OcrHttpResponse {
    #[oai(status = 200)]
    Ok(Json<OcrSuccess>),
    #[oai(status = 400)]
    BadRequest(Json<ErrorBody>),
    #[oai(status = 413)]
    TooLarge(Json<ErrorBody>),
    #[oai(status = 500)]
    Internal(Json<ErrorBody>),
}

fn err_body(code: &str, message: impl Into<String>) -> Json<ErrorBody> {
    Json(ErrorBody {
        error: ErrorDetail {
            code: code.into(),
            message: message.into(),
        },
    })
}

fn side_str(side: IdCardSide) -> String {
    match side {
        IdCardSide::Front => "front".into(),
        IdCardSide::Back => "back".into(),
        IdCardSide::Unknown => "unknown".into(),
    }
}

fn checksum_ok(result: &IdCardResult) -> Option<bool> {
    result
        .front
        .as_ref()
        .and_then(|f| f.id_checksum_ok)
}

fn to_success(result: IdCardResult, timings: RecognizeTimings) -> OcrSuccess {
    OcrSuccess {
        zh: to_zh_json(&result),
        en: to_en_json(&result),
        meta: Meta {
            id_checksum_ok: checksum_ok(&result),
            rotation_degrees: result.rotation_degrees,
            side: side_str(result.side),
            timings_ms: TimingsMs {
                ocr: timings.ocr_ms,
                total: timings.total_ms,
            },
        },
    }
}

fn decode_base64_image(raw: &str) -> Result<Vec<u8>, String> {
    let s = raw.trim();
    let s = if let Some(idx) = s.find("base64,") {
        &s[idx + "base64,".len()..]
    } else {
        s
    };
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(s.trim()))
        .map_err(|e| format!("base64 解码失败: {e}"))
}

#[OpenApi]
impl IdCardApi {
    /// 健康检查（无需鉴权）
    #[oai(path = "/health", method = "get")]
    async fn health(&self) -> Json<HealthResponse> {
        Json(HealthResponse {
            status: "ok".into(),
            workers: self.pool.workers(),
        })
    }

    /// 上传图片识别（multipart，字段名 `file`）
    #[oai(path = "/idcard/ocr", method = "post")]
    async fn ocr_multipart(&self, _auth: BearerAuth, upload: OcrUpload) -> OcrHttpResponse {
        let bytes = match upload.file.into_vec().await {
            Ok(b) => b,
            Err(e) => {
                return OcrHttpResponse::BadRequest(err_body("bad_request", format!("读取上传失败: {e}")));
            }
        };
        self.recognize(bytes).await
    }

    /// Base64 图片识别
    #[oai(path = "/idcard/ocr/base64", method = "post")]
    async fn ocr_base64(&self, _auth: BearerAuth, body: Json<Base64Request>) -> OcrHttpResponse {
        let bytes = match decode_base64_image(&body.0.image_base64) {
            Ok(b) => b,
            Err(e) => return OcrHttpResponse::BadRequest(err_body("bad_image", e)),
        };
        self.recognize(bytes).await
    }
}

impl IdCardApi {
    async fn recognize(&self, bytes: Vec<u8>) -> OcrHttpResponse {
        if bytes.is_empty() {
            return OcrHttpResponse::BadRequest(err_body("bad_image", "空图片"));
        }
        if bytes.len() > self.max_upload_bytes {
            return OcrHttpResponse::TooLarge(err_body(
                "payload_too_large",
                format!(
                    "图片超过上限 {} bytes（实际 {}）",
                    self.max_upload_bytes,
                    bytes.len()
                ),
            ));
        }
        match self.pool.recognize_bytes(bytes).await {
            Ok((result, timings)) => OcrHttpResponse::Ok(Json(to_success(result, timings))),
            Err(e) => OcrHttpResponse::Internal(err_body("ocr_failed", e.to_string())),
        }
    }
}
