use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use idorc::api::{IdCardApi, OcrPool};
use idorc::OcrEngineOptions;
use poem::{listener::TcpListener, EndpointExt, Route, Server};
use poem_openapi::OpenApiService;

#[derive(Parser, Debug)]
#[command(name = "idorc-server", about = "身份证 OCR HTTP API（Poem + OpenAPI/Redoc）")]
struct Args {
    /// 监听地址
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: String,

    /// API Key（也可用环境变量 IDORC_API_KEY）
    #[arg(long)]
    api_key: Option<String>,

    /// OCR worker 数量（并行推理实例）
    #[arg(long, default_value_t = 3)]
    workers: usize,

    /// 最大上传体积（MB）
    #[arg(long, default_value_t = 10)]
    max_upload_mb: usize,

    /// ONNX 模型目录
    #[arg(long, default_value = "models")]
    model_dir: PathBuf,

    /// 模型集
    #[arg(long, default_value = "ppocrv5-ch-server")]
    model_set: String,

    /// 使用快速小模型
    #[arg(long)]
    fast: bool,

    /// 关闭文本行方向分类
    #[arg(long)]
    no_cls: bool,

    /// 字段 reliable 阈值
    #[arg(long, default_value_t = 0.80)]
    min_confidence: f32,

    /// 关闭自动旋转
    #[arg(long)]
    no_rotate: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let api_key = args
        .api_key
        .or_else(|| std::env::var("IDORC_API_KEY").ok())
        .filter(|s| !s.is_empty())
        .context("需要 --api-key 或环境变量 IDORC_API_KEY")?;

    let model_set = if args.fast {
        "ppocrv6-small".to_string()
    } else {
        args.model_set.clone()
    };

    eprintln!(
        "启动 idorc-server bind={} workers={} model={}",
        args.bind, args.workers, model_set
    );

    let pool = Arc::new(OcrPool::new(
        OcrEngineOptions {
            model_dir: args.model_dir,
            model_set,
            use_cls: !args.no_cls,
            min_confidence: args.min_confidence,
            auto_rotate: !args.no_rotate,
            ..Default::default()
        },
        args.workers,
    )?);

    let max_upload_bytes = args
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

    let server_url = format!("http://{}/", args.bind);
    let api_service =
        OpenApiService::new(api, "idorc", env!("CARGO_PKG_VERSION")).server(server_url);
    let ui = api_service.redoc();
    let spec = api_service.spec_endpoint();

    let app = Route::new()
        .nest("/", api_service)
        .nest("/docs", ui)
        .nest("/openapi.json", spec)
        .data(Arc::new(api_key));

    eprintln!("Redoc: http://{}/docs", args.bind);
    Server::new(TcpListener::bind(&args.bind))
        .run(app)
        .await
        .context("server 退出")?;
    Ok(())
}
