use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use idorc::api::{IdCardApi, OcrPool};
use idorc::OcrEngineOptions;
use poem::http::Method;
use poem::middleware::Cors;
use poem::{listener::TcpListener, EndpointExt, Route, Server};
use poem_openapi::OpenApiService;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FileConfig {
    #[serde(default = "default_host")]
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_base")]
    base: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default = "default_workers")]
    workers: usize,
    #[serde(default = "default_max_upload_mb")]
    max_upload_mb: usize,
    #[serde(default = "default_model_dir")]
    model_dir: PathBuf,
    #[serde(default = "default_model_set")]
    model_set: String,
    #[serde(default)]
    fast: bool,
    #[serde(default = "default_true")]
    use_cls: bool,
    #[serde(default = "default_true")]
    auto_rotate: bool,
    #[serde(default = "default_min_confidence")]
    min_confidence: f32,
}

fn default_host() -> String {
    "0.0.0.0".into()
}
fn default_port() -> u16 {
    8080
}
fn default_base() -> String {
    "/ocrapi".into()
}
fn default_workers() -> usize {
    3
}
fn default_max_upload_mb() -> usize {
    10
}
fn default_model_dir() -> PathBuf {
    PathBuf::from("models")
}
fn default_model_set() -> String {
    "ppocrv5-ch-server".into()
}
fn default_true() -> bool {
    true
}
fn default_min_confidence() -> f32 {
    0.80
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            base: default_base(),
            api_key: None,
            workers: default_workers(),
            max_upload_mb: default_max_upload_mb(),
            model_dir: default_model_dir(),
            model_set: default_model_set(),
            fast: false,
            use_cls: true,
            auto_rotate: true,
            min_confidence: default_min_confidence(),
        }
    }
}

/// 规范化前缀：空或 `/` → 根；否则保证以 `/` 开头、不以 `/` 结尾。
fn normalize_base(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() || s == "/" {
        return String::new();
    }
    let s = s.trim_end_matches('/');
    if s.starts_with('/') {
        s.to_string()
    } else {
        format!("/{s}")
    }
}

#[derive(Parser, Debug)]
#[command(name = "idorc-server", about = "身份证 OCR HTTP API（Poem + OpenAPI/Redoc）")]
struct Args {
    /// 配置文件路径（默认 ./default.toml）
    #[arg(long, default_value = "default.toml")]
    config: PathBuf,

    /// 覆盖配置：监听 IP
    #[arg(long)]
    host: Option<String>,

    /// 覆盖配置：端口
    #[arg(long)]
    port: Option<u16>,

    /// 覆盖配置：服务前缀，如 /ocrapi
    #[arg(long)]
    base: Option<String>,

    /// 覆盖配置：API Key（也可用环境变量 IDORC_API_KEY，优先级最高）
    #[arg(long)]
    api_key: Option<String>,

    /// 覆盖配置：OCR worker 数
    #[arg(long)]
    workers: Option<usize>,

    /// 覆盖配置：最大上传 MB
    #[arg(long)]
    max_upload_mb: Option<usize>,

    /// 覆盖配置：模型目录
    #[arg(long)]
    model_dir: Option<PathBuf>,

    /// 覆盖配置：模型集
    #[arg(long)]
    model_set: Option<String>,

    /// 覆盖配置：使用快速小模型
    #[arg(long)]
    fast: bool,

    /// 覆盖配置：关闭文本行方向分类
    #[arg(long)]
    no_cls: bool,

    /// 覆盖配置：字段 reliable 阈值
    #[arg(long)]
    min_confidence: Option<f32>,

    /// 覆盖配置：关闭自动旋转
    #[arg(long)]
    no_rotate: bool,
}

#[derive(Debug)]
struct RuntimeConfig {
    host: String,
    port: u16,
    base: String,
    api_key: String,
    workers: usize,
    max_upload_mb: usize,
    model_dir: PathBuf,
    model_set: String,
    use_cls: bool,
    auto_rotate: bool,
    min_confidence: f32,
}

impl RuntimeConfig {
    fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// OpenAPI / 文档里展示的 base URL（0.0.0.0 用 127.0.0.1 方便浏览器）
    fn public_base_url(&self) -> String {
        let host = if self.host == "0.0.0.0" {
            "127.0.0.1"
        } else {
            self.host.as_str()
        };
        format!("http://{}:{}{}/", host, self.port, self.base)
    }
}

fn load_file_config(path: &PathBuf) -> Result<FileConfig> {
    if !path.exists() {
        eprintln!("配置文件不存在: {}，使用内置默认值", path.display());
        return Ok(FileConfig::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("读取配置失败: {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("解析 TOML 失败: {}", path.display()))
}

fn merge_config(file: FileConfig, args: &Args) -> Result<RuntimeConfig> {
    let api_key = std::env::var("IDORC_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| args.api_key.clone())
        .or_else(|| file.api_key.clone())
        .filter(|s| !s.is_empty())
        .context("需要在 default.toml 配置 api_key，或设 --api-key / 环境变量 IDORC_API_KEY")?;

    let fast = args.fast || file.fast;
    let model_set = if fast {
        "ppocrv6-small".to_string()
    } else {
        args.model_set.clone().unwrap_or(file.model_set)
    };

    let base = normalize_base(args.base.as_deref().unwrap_or(&file.base));

    Ok(RuntimeConfig {
        host: args.host.clone().unwrap_or(file.host),
        port: args.port.unwrap_or(file.port),
        base,
        api_key,
        workers: args.workers.unwrap_or(file.workers).max(1),
        max_upload_mb: args.max_upload_mb.unwrap_or(file.max_upload_mb),
        model_dir: args.model_dir.clone().unwrap_or(file.model_dir),
        model_set,
        use_cls: if args.no_cls { false } else { file.use_cls },
        auto_rotate: if args.no_rotate {
            false
        } else {
            file.auto_rotate
        },
        min_confidence: args.min_confidence.unwrap_or(file.min_confidence),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let file = load_file_config(&args.config)?;
    let cfg = merge_config(file, &args)?;
    let listen = cfg.listen_addr();

    eprintln!(
        "启动 idorc-server config={} listen={} base={} workers={} model={} model_dir={}",
        args.config.display(),
        listen,
        if cfg.base.is_empty() { "/" } else { &cfg.base },
        cfg.workers,
        cfg.model_set,
        cfg.model_dir.display()
    );

    let pool = Arc::new(OcrPool::new(
        OcrEngineOptions {
            model_dir: cfg.model_dir.clone(),
            model_set: cfg.model_set.clone(),
            use_cls: cfg.use_cls,
            min_confidence: cfg.min_confidence,
            auto_rotate: cfg.auto_rotate,
            ..Default::default()
        },
        cfg.workers,
    )?);

    let max_upload_bytes = cfg
        .max_upload_mb
        .checked_mul(1024 * 1024)
        .context("max_upload_mb 过大")?;
    if max_upload_bytes == 0 {
        bail!("max_upload_mb 必须 > 0");
    }

    let api = IdCardApi {
        pool: pool.clone(),
        max_upload_bytes,
    };

    let api_service = OpenApiService::new(api, "idorc", env!("CARGO_PKG_VERSION"))
        .server(cfg.public_base_url());
    let ui = api_service.redoc();
    let spec_json = api_service.spec_endpoint();
    let spec_alias = api_service.spec_endpoint();

    let cors = Cors::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(["authorization", "content-type", "accept"])
        .allow_credentials(false);

    let api_routes = Route::new()
        .nest("/", api_service)
        .nest("/docs", ui)
        .nest("/spec", spec_alias)
        .nest("/openapi.json", spec_json);

    let app = if cfg.base.is_empty() {
        api_routes.data(Arc::new(cfg.api_key.clone())).with(cors)
    } else {
        Route::new()
            .nest(&cfg.base, api_routes)
            .data(Arc::new(cfg.api_key.clone()))
            .with(cors)
    };

    let prefix = if cfg.base.is_empty() {
        String::new()
    } else {
        cfg.base.clone()
    };
    eprintln!("Redoc:       http://127.0.0.1:{}{}/docs", cfg.port, prefix);
    eprintln!("OpenAPI:     http://127.0.0.1:{}{}/spec", cfg.port, prefix);
    eprintln!(
        "OCR:         POST http://127.0.0.1:{}{}/idcard/ocr",
        cfg.port, prefix
    );

    Server::new(TcpListener::bind(&listen))
        .run(app)
        .await
        .context("server 退出")?;
    Ok(())
}
