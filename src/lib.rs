//! 二代身份证离线 OCR：基于 [rapidocr-core] 的检测/识别 + 结构化字段解析。

pub mod api;
pub mod engine;
pub mod format_out;
pub mod gen;
pub mod id_card;
pub mod parse;
pub mod preprocess;

pub use engine::{IdCardOcr, OcrEngineOptions, RecognizeTimings};
pub use format_out::{to_en_json, to_zh_json, to_zh_text};
pub use id_card::{
    FieldValue, IdCardBack, IdCardFront, IdCardResult, IdCardSide,
};
pub use parse::structured::ParseOptions;
