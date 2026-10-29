# idorc HTTP API Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 提供 `idorc-server`：Tokio + Poem + poem-openapi + Redoc，Bearer 鉴权，multipart/base64 识别，返回 `zh`/`en`，默认 3 OCR workers 并发推理。

**Architecture:** 库内 `api` 模块（DTO/鉴权/路由/worker 池）+ `bin/idorc-server` 启动；`to_en_json` 与 `recognize_id_card_bytes`；池内每 worker 一把 `std::sync::Mutex<IdCardOcr>`，外层 `Semaphore(workers)`，识别在 `spawn_blocking` 中执行。

**Tech Stack:** `tokio` (rt-multi-thread, macros)、`poem`、`poem-openapi` (features: `swagger-ui` 或 `redoc`)、`base64`、现有 `idorc` 库

## Global Constraints

- Spec: `docs/superpowers/specs/2026-10-29-idorc-api-server-design.md`
- 默认 `--workers 3`；单进程；HTTP 并发 + spawn_blocking
- 鉴权：`Authorization: Bearer <api-key>`；health/docs/openapi 免鉴权
- 上传：`POST /v1/idcard/ocr` multipart `file`；`POST /v1/idcard/ocr/base64` JSON
- 响应：`{ zh, en, meta }`；错误 `{ error: { code, message } }`
- 缺 api-key（CLI 与 `IDORC_API_KEY` 皆无）则拒绝启动
- 不破坏现有 `idorc` / `idgen`（`default-run = idorc`）
- 依赖版本以 crates.io 可解析为准；`poem-openapi` 启用 Redoc 相关 feature

## File Map

| Path | Responsibility |
|------|----------------|
| `src/format_out.rs` | 新增 `to_en_json` |
| `src/engine.rs` | `recognize_id_card_bytes` / from `RgbImage` |
| `src/api/mod.rs` | api 模块 |
| `src/api/pool.rs` | `OcrPool` |
| `src/api/dto.rs` | OpenAPI 对象 |
| `src/api/auth.rs` | Bearer SecurityScheme |
| `src/api/routes.rs` | OpenApi impl |
| `src/bin/idorc-server.rs` | CLI + Server |
| `src/lib.rs` | `pub mod api` |
| `Cargo.toml` | deps |
| `README.md` | API 小节 |

---

### Task 1: `to_en_json` + 内存识别 API

**Files:**
- Modify: `src/format_out.rs`, `src/engine.rs`, `src/lib.rs`（如需 re-export）
- Test: `src/format_out.rs` 或相邻 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub fn to_en_json(result: &IdCardResult) -> serde_json::Value`
  - `pub fn recognize_id_card_bytes_timed(&mut self, bytes: &[u8]) -> Result<(IdCardResult, RecognizeTimings)>`
- Consumes: existing parse/orientation path（bytes → `image::load_from_memory` → EXIF 若可行，否则 RGB → `run_image_with_orientation`）

- [ ] **Step 1: 失败测试 `to_en_json`**

在 `format_out.rs` 测试模块（或新建测试）加入：

```rust
#[test]
fn en_json_front_keys() {
    use crate::id_card::{FieldValue, IdCardFront, IdCardResult, IdCardSide};
    let mut front = IdCardFront::default();
    front.name = Some(FieldValue::new("张三", 0.99, 0.8));
    front.gender = Some(FieldValue::new("男", 0.99, 0.8));
    front.ethnicity = Some(FieldValue::new("汉族", 0.99, 0.8));
    front.birth_date = Some(FieldValue::new("1990-01-02", 0.99, 0.8));
    front.address = Some(FieldValue::new("北京市", 0.99, 0.8));
    front.id_number = Some(FieldValue::new("11010519491231002X", 0.99, 0.8));
    let r = IdCardResult {
        side: IdCardSide::Front,
        front: Some(front),
        back: None,
        raw_lines: vec![],
        warnings: vec![],
        min_confidence: 0.8,
        rotation_degrees: 0,
    };
    let v = to_en_json(&r);
    assert_eq!(v["name"], "张三");
    assert_eq!(v["gender"], "男");
    assert_eq!(v["ethnicity"], "汉族");
    assert_eq!(v["birth_date"], "1990-01-02");
    assert_eq!(v["address"], "北京市");
    assert_eq!(v["id_number"], "11010519491231002X");
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cargo test --lib format_out -- --nocapture
```

Expected: FAIL（`to_en_json` 未定义）

- [ ] **Step 3: 实现 `to_en_json`**

镜像 `to_zh_json`：front 用上表英文字段；back 用 `issuing_authority` / `valid_period`（及可选 `"side": "back"` 仅放 meta，不在 en 对象内重复，按 spec en 对象只含字段）。

- [ ] **Step 4: 实现 `recognize_id_card_bytes_timed`**

```rust
pub fn recognize_id_card_bytes_timed(
    &mut self,
    bytes: &[u8],
) -> Result<(IdCardResult, RecognizeTimings)> {
    let total = Instant::now();
    let mut timings = RecognizeTimings::default();
    let t_load = Instant::now();
    let image = image::load_from_memory(bytes)
        .context("解码图片失败")?
        .to_rgb8();
    // 若已有基于 DynamicImage 的 EXIF 工具可复用；否则先 to_rgb8
    timings.load_image_ms = t_load.elapsed().as_secs_f64() * 1000.0;
    let (output, rotation_degrees) = self.run_image_with_orientation(&image, &mut timings)?;
    // 同 path 路径：parse + rotation warnings + total_ms
    ...
}
```

将 `run_image_with_orientation` 保持私有，bytes 路径复用它。

- [ ] **Step 5: 测试通过并提交**

```bash
cargo test --lib -- --nocapture
git add src/format_out.rs src/engine.rs src/lib.rs
git commit -m "feat: add to_en_json and recognize_id_card_bytes_timed"
```

---

### Task 2: OCR worker 池

**Files:**
- Create: `src/api/mod.rs`, `src/api/pool.rs`
- Modify: `src/lib.rs` → `pub mod api;`
- Modify: `Cargo.toml` → `tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }`

**Interfaces:**
- Produces:

```rust
pub struct OcrPool { /* sem + Vec<Mutex<IdCardOcr>> */ }

impl OcrPool {
    pub fn new(options: OcrEngineOptions, workers: usize) -> anyhow::Result<Self>;
    pub fn workers(&self) -> usize;
    pub async fn recognize_bytes(
        &self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<(IdCardResult, RecognizeTimings)>;
}
```

- [ ] **Step 1: 实现 `OcrPool`**

```rust
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

pub struct OcrPool {
    engines: Vec<Mutex<IdCardOcr>>,
    sem: Arc<Semaphore>,
    workers: usize,
}

impl OcrPool {
    pub fn new(options: OcrEngineOptions, workers: usize) -> anyhow::Result<Self> {
        let workers = workers.max(1);
        let mut engines = Vec::with_capacity(workers);
        for _ in 0..workers {
            engines.push(Mutex::new(IdCardOcr::new(options.clone())?));
        }
        Ok(Self {
            engines,
            sem: Arc::new(Semaphore::new(workers)),
            workers,
        })
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    pub async fn recognize_bytes(
        &self,
        bytes: Vec<u8>,
    ) -> anyhow::Result<(IdCardResult, RecognizeTimings)> {
        let _permit = self.sem.acquire().await.expect("semaphore closed");
        // 租借空闲引擎：短自旋 try_lock
        let engines = &self.engines;
        tokio::task::spawn_blocking(move || {
            // 注意：不能直接 move &self；应 Arc<OcrPool> 或传入 engines Arc
            ...
        })
        .await?
    }
}
```

**实现要点（必须按此语义）：**

- `OcrPool` 放进 `Arc`
- `recognize_bytes`：`acquire` permit → `spawn_blocking` 闭包内 `loop { for m in &engines { if let Ok(mut g) = m.try_lock() { return g.recognize_id_card_bytes_timed(&bytes); } } }`（permit 已保证有空闲；若 try_lock 全失败则 `lock()` 第一个）
- `IdCardOcr` / `Mutex` 必须 `Send`；若编译失败，在报告中 `BLOCKED` 并说明，改用「每 worker 一线程 + channel」备选（仅阻塞时启用）

- [ ] **Step 2: 单元测试（可选轻量）**

不加载真实大模型亦可：用 `#[ignore]` 的集成测试占位，或仅测试 `workers()` getter。首版允许无 ignore 的 pool smoke。

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock src/api src/lib.rs
git commit -m "feat: add OCR worker pool for API server"
```

---

### Task 3: OpenAPI DTO + Bearer + Routes + Binary

**Files:**
- Create: `src/api/dto.rs`, `src/api/auth.rs`, `src/api/routes.rs`, `src/bin/idorc-server.rs`
- Modify: `src/api/mod.rs`, `Cargo.toml`, `README.md`

**Deps 加入 Cargo.toml：**

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }
poem = "3"
poem-openapi = { version = "5", features = ["redoc", "swagger-ui"] }
base64 = "0.22"
```

（若 poem 3 与 openapi 5 不兼容，按 crates.io 配对调整，在报告中记录最终版本。）

**Interfaces:**
- `BearerAuth` SecurityScheme checker 比对 `Arc<String>` api_key
- `IdCardApi { pool: Arc<OcrPool>, max_upload_bytes: usize }`
- OpenAPI paths 与 spec 一致

- [ ] **Step 1: `dto.rs`**

```rust
#[derive(Object)]
pub struct HealthResponse {
    pub status: String,
    pub workers: usize,
}

#[derive(Object)]
pub struct Base64Request {
    pub image_base64: String,
}

#[derive(Object)]
pub struct TimingsMs {
    pub ocr: f64,
    pub total: f64,
}

#[derive(Object)]
pub struct Meta {
    pub id_checksum_ok: Option<bool>,
    pub rotation_degrees: u32,
    pub side: String,
    pub timings_ms: TimingsMs,
}

#[derive(Object)]
pub struct OcrSuccess {
    /// 中文字段对象（用 serde_json::Value 或 Map）
    pub zh: serde_json::Value,
    pub en: serde_json::Value,
    pub meta: Meta,
}

#[derive(Object)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Object)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}
```

若 `Object` 对 `serde_json::Value` 支持不佳，改用显式 `ZhFront`/`EnFront` 结构体字段（与 spec 键一致）。

- [ ] **Step 2: `auth.rs`**

```rust
#[derive(SecurityScheme)]
#[oai(
    ty = "bearer",
    checker = "check_bearer"
)]
pub struct BearerAuth(());

async fn check_bearer(req: &poem::Request, token: poem_openapi::auth::Bearer) -> Option<()> {
    let expected = req.data::<Arc<String>>()?;
    (token.token == expected.as_str()).then_some(())
}
```

在 `main` 里 `app.data(Arc::new(api_key))`。

- [ ] **Step 3: `routes.rs`**

```rust
#[OpenApi]
impl IdCardApi {
    #[oai(path = "/health", method = "get")]
    async fn health(&self) -> Json<HealthResponse> { ... }

    #[oai(path = "/v1/idcard/ocr", method = "post")]
    async fn ocr_multipart(
        &self,
        _auth: BearerAuth,
        upload: OcrUpload, // Multipart { file: Upload }
    ) -> Result<Json<OcrSuccess>, OcrApiError> { ... }

    #[oai(path = "/v1/idcard/ocr/base64", method = "post")]
    async fn ocr_base64(
        &self,
        _auth: BearerAuth,
        body: Json<Base64Request>,
    ) -> Result<Json<OcrSuccess>, OcrApiError> { ... }
}
```

辅助：`decode_base64` 去掉 `data:*;base64,` 前缀；校验 `bytes.len() <= max_upload_bytes`；调用 `pool.recognize_bytes`；组装 `OcrSuccess`（`to_zh_json`/`to_en_json` + meta）。

`OcrApiError` 用 `ApiResponse` 映射 401/400/500。

- [ ] **Step 4: `idorc-server.rs`**

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let api_key = args.api_key.or_else(|| std::env::var("IDORC_API_KEY").ok())
        .context("需要 --api-key 或环境变量 IDORC_API_KEY")?;
    let pool = Arc::new(OcrPool::new(engine_opts_from(&args), args.workers)?);
    let api = IdCardApi { pool: pool.clone(), max_upload_bytes: args.max_upload_mb * 1024 * 1024 };
    let spec = OpenApiService::new(api, "idorc", env!("CARGO_PKG_VERSION"))
        .server(format!("http://{}/", args.bind));
    let ui = spec.redoc();
    let app = Route::new()
        .nest("/", spec)
        .nest("/docs", ui)
        .data(Arc::new(api_key));
    Server::new(TcpListener::bind(&args.bind)).run(app).await?;
    Ok(())
}
```

确认 Redoc 路径与 openapi json 导出方式符合 poem-openapi 5 惯例（常见：`spec.as_json()` / `spec.swagger_ui()`）；以官方 example 为准，文档写清最终 URL。

- [ ] **Step 5: README 小节 + Commit**

```markdown
## HTTP API（idorc-server）

```bash
export IDORC_API_KEY=dev-secret
cargo run --release --bin idorc-server -- --bind 127.0.0.1:8080 --workers 3 --fast
# Redoc: http://127.0.0.1:8080/docs
curl -H "Authorization: Bearer $IDORC_API_KEY" \
  -F file=@234.png http://127.0.0.1:8080/v1/idcard/ocr
```
```

```bash
git add Cargo.toml Cargo.lock src/api src/bin/idorc-server.rs README.md
git commit -m "feat: add idorc-server Poem OpenAPI with Redoc and worker pool"
```

---

### Task 4: 手工验收

**Files:** 无代码（或修 bug）

- [ ] **Step 1: 启动**

```bash
export IDORC_API_KEY=test-key
cargo run --release --bin idorc-server -- --bind 127.0.0.1:8080 --workers 3 --fast
```

- [ ] **Step 2: health / 401 / multipart / base64**

```bash
curl -s http://127.0.0.1:8080/health
curl -s -o /dev/null -w "%{http_code}" -F file=@234.png http://127.0.0.1:8080/v1/idcard/ocr
# expect 401
curl -s -H "Authorization: Bearer test-key" -F file=@234.png http://127.0.0.1:8080/v1/idcard/ocr | jq .
B64=$(base64 -i 234.png | tr -d '\n')
curl -s -H "Authorization: Bearer test-key" -H "Content-Type: application/json" \
  -d "{\"image_base64\":\"$B64\"}" http://127.0.0.1:8080/v1/idcard/ocr/base64 | jq .
```

Expected：`zh`/`en` 均有姓名等字段；打开 `/docs` 可见接口。

- [ ] **Step 3: 并发烟测**

```bash
for i in 1 2 3; do
  curl -s -H "Authorization: Bearer test-key" -F file=@234.png \
    http://127.0.0.1:8080/v1/idcard/ocr -o /tmp/ocr$i.json &
done
wait
jq '.en.name' /tmp/ocr1.json /tmp/ocr2.json /tmp/ocr3.json
```

- [ ] **Step 4: 若有修复则提交；更新 spec 状态已实现（可选）**

---

## Spec coverage

| Spec | Task |
|------|------|
| to_en_json / bytes 识别 | 1 |
| workers=3 pool + spawn_blocking | 2 |
| Poem/OpenAPI/Redoc/Bearer/双上传 | 3 |
| 验收 curl | 4 |
| CLI 不受影响 | 3（独立 bin） |

## Self-review

无 TBD；`Value` 与 OpenAPI Object 冲突时改用显式结构体（已注明）。池 `Send` 风险有备选升级路径。
