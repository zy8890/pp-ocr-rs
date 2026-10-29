# idorc

基于 [rapidocr-core](https://github.com/White-NX/rapidocr-rs)（RapidOCR-ONNX）的**二代身份证离线 OCR**：检测 + 识别 + 结构化字段解析（姓名、身份证号、住址等）。

## 特性

- 完全离线推理（ONNX Runtime），模型可通过 `ModelCache` 首次自动下载
- 默认 **PP-OCRv5 中文 Server** 模型，偏识别准确率；`--fast` 切换 **PP-OCRv6 Small** 偏速度
- **方案 A**：以 OCR 检测框为原子元素，按标签右侧/下方坐标配对字段；字段带模型 `confidence` / `reliable`
- 18 位身份证号 **GB 11643 校验位**仅作辅助（`id_checksum_ok`），不覆盖模型文本
- 同框「标签+值」仅兜底，并降权置信度；住址支持多行合并（取最低分）

## 构建

```bash
cargo build --release
```

## 使用

```bash
# 首次运行会从网络下载模型到 ./models
cargo run --release -- --image /path/to/id_card_front.jpg

# 更快模型
cargo run --release -- --image id.jpg --fast

# 调试：打印原始 OCR 行
cargo run --release -- --image id.jpg --raw

# 调整字段可信阈值（默认 0.80）
cargo run --release -- --image id.jpg --min-confidence 0.9
```

输出为 JSON：各字段为 `{ value, confidence, reliable }`，另含 `id_checksum_ok`、`raw_lines`、`warnings`。

## 库 API

```rust
use idorc::{IdCardOcr, OcrEngineOptions};

let mut ocr = IdCardOcr::new(OcrEngineOptions::default())?;
let result = ocr.recognize_id_card_path("id_front.png")?;
println!("{:?}", result.front);
```

## 模型与资源

注册模型集由 `rapidocr-core` 维护，与 [RapidAI/RapidOCR](https://github.com/RapidAI/RapidOCR) 的 ONNX 资产一致。也可将已下载的 `.onnx` 与字典文件放入 `models` 目录，避免在线下载。

## 说明

- 拍摄时尽量正面、无强反光、分辨率 ≥ 1280 宽，可显著提高准确率
- 国徽面无身份证号，解析依赖「签发机关」「有效期限」等关键词与版面位置
- 本项目为 OCR + 规则/版面解析，**不**替代公安联网核验
