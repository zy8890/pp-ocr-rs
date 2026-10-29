# idorc HTTP API 服务设计

**日期：** 2026-10-29  
**状态：** 已批准  
**栈：** Tokio + Poem + poem-openapi + Redoc  

在现有 `IdCardOcr` 库之上提供身份证识别 HTTP API（单进程、可并发接入）。

## 目标与非目标

**目标**

- Binary：`idorc-server`
- OpenAPI + Redoc（`/docs`、`/openapi.json`）
- 上传：`multipart` 文件 + JSON Base64（均可）
- 响应：`zh` / `en` 字段分组
- 鉴权：Bearer API Key
- 单实例部署；HTTP 并发接入；默认 **3** 个 OCR worker 并行推理

**非目标（首版）**

- 多机集群 / 服务发现
- 用户体系、OAuth
- 异步任务回调队列
- 国徽面专用复杂流程（沿用现有解析即可）

## 架构

```
Client → Poem (Tokio)
           ├─ /health（无鉴权）
           ├─ /docs、/openapi.json（无鉴权，便于联调）
           └─ /v1/idcard/ocr*（Bearer）
                    ↓
              decode image bytes
                    ↓
              spawn_blocking + worker pool（默认 3× IdCardOcr）
                    ↓
              { zh, en, meta }
```

- **请求并发：** Tokio 异步接受连接  
- **推理：** `tokio::task::spawn_blocking`，避免阻塞 runtime  
- **Worker 池：** 默认 `workers=3`，每个 worker 持有独立 `IdCardOcr`；用 `tokio::sync::Semaphore`（或等价队列）限制同时进入推理的任务数 ≤ workers；池内引擎可用 `Mutex`/`StdMutex` 按空闲租借  

## CLI / 配置

```bash
cargo run --release --bin idorc-server -- \
  --bind 0.0.0.0:8080 \
  --api-key "$IDORC_API_KEY" \
  --workers 3 \
  --model-dir models \
  --fast   # 可选，等同小模型
```

| 参数 | 默认 | 说明 |
|------|------|------|
| `--bind` | `127.0.0.1:8080` | 监听地址 |
| `--api-key` | 环境变量 `IDORC_API_KEY`（必填其一） | Bearer token |
| `--workers` | **3** | OCR 引擎实例数 |
| `--max-upload-mb` | `10` | 上传上限 |
| `--model-dir` / `--model-set` / `--fast` / `--no-cls` / `--min-confidence` | 与 CLI OCR 对齐 | |

缺少 api-key 时进程启动失败（避免裸奔）。

## API

### `GET /health`

```json
{ "status": "ok", "workers": 3 }
```

### `POST /v1/idcard/ocr`

- Header：`Authorization: Bearer <api-key>`
- Body：`multipart/form-data`，字段名 `file`（图片）

### `POST /v1/idcard/ocr/base64`

- Header：同上  
- Body：

```json
{ "image_base64": "<raw or data-url base64>" }
```

### 成功 `200`

```json
{
  "zh": {
    "姓名": "",
    "性别": "",
    "民族": "",
    "出生": "",
    "住址": "",
    "公民身份号码": ""
  },
  "en": {
    "name": "",
    "gender": "",
    "ethnicity": "",
    "birth_date": "",
    "address": "",
    "id_number": ""
  },
  "meta": {
    "id_checksum_ok": true,
    "rotation_degrees": 0,
    "side": "front",
    "timings_ms": {
      "ocr": 0.0,
      "total": 0.0
    }
  }
}
```

`zh` 复用现有 `to_zh_json`；`en` 由同一 `IdCardResult` 映射。国徽面时 `zh`/`en` 改为签发机关/有效期限对应英文字段（`issuing_authority` / `valid_period`），`side` 标明。

### 错误

```json
{ "error": { "code": "unauthorized", "message": "…" } }
```

| HTTP | code |
|------|------|
| 401 | `unauthorized` |
| 400 | `bad_request` / `bad_image` / `payload_too_large` |
| 503 | `busy`（可选：队列过深时） |
| 500 | `ocr_failed` |

## 模块划分

| 路径 | 职责 |
|------|------|
| `src/bin/idorc-server.rs` | 启动、CLI、跑 Poem |
| `src/api/mod.rs` | 模块出口 |
| `src/api/dto.rs` | OpenAPI 请求/响应类型 |
| `src/api/auth.rs` | Bearer 校验 |
| `src/api/routes.rs` | poem-openapi API 实现 |
| `src/api/pool.rs` | worker 池 + `recognize_bytes` |
| `src/format_out.rs` | 新增 `to_en_json`（或 api 内映射） |

依赖（server 相关）：`tokio`、`poem`、`poem-openapi`、`base64`；可用 feature `server` 或直接加依赖（首版直接加即可）。

## 验收

1. Redoc 可打开并看到两个 OCR 接口  
2. 无/错 Key → 401；合法 Key + `234.png` multipart → 200，且 `zh`/`en` 字段齐全  
3. Base64 路径同等结果  
4. 并发 3+ 请求同时到达不拖死 event loop；`workers=3` 时可并行推理（CPU 允许时）  
5. 现有 `idorc` / `idgen` CLI 不受影响（`default-run = idorc`）

## 风险

- 3 个 Server 模型实例内存占用显著；可用 `--fast` 降内存  
- `IdCardOcr` 是否 `Send`：池设计需确认；不行则每请求临时引擎（更慢）或单线程队列  
- 大图 OOM：靠 `max-upload-mb` 限制  
