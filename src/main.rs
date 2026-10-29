use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use idorc::{to_zh_json, to_zh_text, IdCardOcr, OcrEngineOptions};

#[derive(Clone, Debug, ValueEnum)]
enum OutputFormat {
    /// 中文字段扁平 JSON（默认）：姓名/性别/民族…
    Zh,
    /// 纯文本：姓名：xxx
    Text,
    /// 完整调试 JSON（含 confidence / raw_lines）
    Full,
}

#[derive(Parser, Debug)]
#[command(name = "idorc", about = "中国二代身份证离线 OCR")]
struct Args {
    /// 身份证图片路径
    #[arg(short, long)]
    image: PathBuf,

    /// ONNX 模型缓存目录
    #[arg(long, default_value = "models")]
    model_dir: PathBuf,

    /// 模型集：ppocrv5-ch-server（默认，偏准确率）、ppocrv6-small（偏速度）等
    #[arg(long, default_value = "ppocrv5-ch-server")]
    model_set: String,

    /// 关闭文本行方向分类
    #[arg(long)]
    no_cls: bool,

    /// 输出原始 OCR 行到 stderr
    #[arg(long)]
    raw: bool,

    /// 使用快速小模型（等同 --model-set ppocrv6-small）
    #[arg(long)]
    fast: bool,

    /// 字段 reliable 判定阈值（对比 recognition score）
    #[arg(long, default_value_t = 0.80)]
    min_confidence: f32,

    /// 关闭自动旋转矫正
    #[arg(long)]
    no_rotate: bool,

    /// 输出格式
    #[arg(long, value_enum, default_value_t = OutputFormat::Zh)]
    format: OutputFormat,

    /// 将结果写入文件（默认打印到 stdout）
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let wall = Instant::now();

    let model_set = if args.fast {
        "ppocrv6-small".to_string()
    } else {
        args.model_set
    };

    let t_init = Instant::now();
    let mut engine = IdCardOcr::new(OcrEngineOptions {
        model_dir: args.model_dir,
        model_set: model_set.clone(),
        use_cls: !args.no_cls,
        min_confidence: args.min_confidence,
        auto_rotate: !args.no_rotate,
        ..Default::default()
    })?;
    let init_ms = t_init.elapsed().as_secs_f64() * 1000.0;

    let (result, timings) = engine.recognize_id_card_path_timed(&args.image)?;

    eprintln!("模型: {model_set}");
    eprintln!("引擎初始化: {init_ms:>7.1} ms");
    if result.rotation_degrees != 0 {
        eprintln!("旋转矫正: 顺时针 {}°", result.rotation_degrees);
    } else {
        eprintln!("旋转矫正: 无需旋转（正向）");
    }
    timings.print_stderr();
    eprintln!("端到端合计: {:>7.1} ms", wall.elapsed().as_secs_f64() * 1000.0);

    if args.raw {
        eprintln!("--- raw OCR ---");
        for line in &result.raw_lines {
            eprintln!(
                "{:.4}\t[{:.0},{:.0}-{:.0},{:.0}]\t{}",
                line.score, line.x0, line.y0, line.x1, line.y1, line.text
            );
        }
    }

    let out = match args.format {
        OutputFormat::Zh => serde_json::to_string_pretty(&to_zh_json(&result))?,
        OutputFormat::Text => to_zh_text(&result),
        OutputFormat::Full => serde_json::to_string_pretty(&result)?,
    };

    if let Some(path) = args.output {
        std::fs::write(&path, format!("{out}\n"))?;
        eprintln!("已写入 {}", path.display());
    } else {
        println!("{out}");
    }

    Ok(())
}
