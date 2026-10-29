# 方案 A：RapidOCR 框坐标配对 + 置信度

## 目标

不以字符串拆分为主逻辑；每个 OCR 框视为原子元素，按标签右侧/下方配对取值，字段携带模型 `score`。

## 规则摘要

- 标签锚点：姓名 / 性别 / 民族 / 出生 / 住址 / 公民身份号码 / 签发机关 / 有效期限
- 取值优先：同标签行右侧值框 → 下方连续值框（住址）
- 同框标签+值仅作兜底，confidence × 0.85
- `reliable = confidence >= min_confidence`；身份证号校验失败只置 `reliable=false` 与 `id_checksum_ok`
