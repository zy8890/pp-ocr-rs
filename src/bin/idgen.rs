use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use idorc::gen::{random_fields, render_front, IdCardFields, RenderOptions};
use idorc::parse::validate::{compose_id_number, validate_id_number};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

#[derive(Parser, Debug)]
#[command(name = "idgen", about = "生成二代证人像面测试图（仅供 OCR 测试）")]
struct Args {
    /// 输出 PNG 路径
    #[arg(short, long)]
    output: PathBuf,

    /// 随机生成字段
    #[arg(long)]
    random: bool,

    /// 随机种子（可复现）
    #[arg(long)]
    seed: Option<u64>,

    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    sex: Option<String>,
    #[arg(long)]
    nation: Option<String>,
    /// 出生日期 YYYY-MM-DD
    #[arg(long)]
    birth: Option<String>,
    #[arg(long)]
    addr: Option<String>,
    /// 18 位号码；若给 17 位则自动补校验位
    #[arg(long)]
    id: Option<String>,

    #[arg(long)]
    avatar: Option<PathBuf>,

    #[arg(long, default_value = "assets/idgen")]
    assets: PathBuf,

    #[arg(long)]
    no_meta: bool,
}

fn main() -> Result<()> {
    eprintln!("警告：生成物仅供 OCR 学习测试，禁止用于伪造证件。");
    let args = Args::parse();

    let fields = if args.random {
        let seed = args.seed.unwrap_or_else(|| rand::thread_rng().gen());
        eprintln!("random seed: {seed}");
        let mut rng = StdRng::seed_from_u64(seed);
        let mut f = random_fields(&mut rng);
        overlay_cli(&mut f, &args)?;
        f
    } else {
        build_from_cli(&args)?
    };

    if !validate_id_number(&fields.id_number) {
        bail!("身份证号校验失败: {}", fields.id_number);
    }

    let opts = RenderOptions {
        assets_dir: args.assets,
        avatar: args.avatar,
        front_height: 1754,
    };
    let img = render_front(&fields, &opts)?;
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    img.save(&args.output)
        .with_context(|| format!("写入 {}", args.output.display()))?;
    eprintln!("wrote {}", args.output.display());

    if !args.no_meta {
        let meta_path = args.output.with_extension("json");
        let meta = serde_json::to_string_pretty(&fields.to_meta_json())?;
        std::fs::write(&meta_path, format!("{meta}\n"))?;
        eprintln!("wrote {}", meta_path.display());
    }
    Ok(())
}

fn overlay_cli(fields: &mut IdCardFields, args: &Args) -> Result<()> {
    if let Some(v) = &args.name {
        fields.name = v.clone();
    }
    if let Some(v) = &args.sex {
        fields.sex = v.clone();
    }
    if let Some(v) = &args.nation {
        fields.nation = v.clone();
    }
    if let Some(v) = &args.addr {
        fields.address = v.clone();
    }
    if let Some(b) = &args.birth {
        let (y, m, d) = parse_birth(b)?;
        fields.year = y;
        fields.month = m;
        fields.day = d;
    }
    if let Some(id) = &args.id {
        fields.id_number = normalize_id(id)?;
    }
    Ok(())
}

fn build_from_cli(args: &Args) -> Result<IdCardFields> {
    let name = args
        .name
        .clone()
        .context("--random 或提供 --name 等字段")?;
    let sex = args.sex.clone().unwrap_or_else(|| "男".into());
    let nation = args.nation.clone().unwrap_or_else(|| "汉".into());
    let (year, month, day) = match &args.birth {
        Some(b) => parse_birth(b)?,
        None => bail!("非 --random 时需要 --birth YYYY-MM-DD"),
    };
    let address = args
        .addr
        .clone()
        .unwrap_or_else(|| "四川省成都市武侯区测试路1号".into());
    let id_number = match &args.id {
        Some(id) => normalize_id(id)?,
        None => {
            let body = format!(
                "510822{:04}{:02}{:02}001",
                year.parse::<u32>()?,
                month.parse::<u32>()?,
                day.parse::<u32>()?
            );
            compose_id_number(&body).context("compose id")?
        }
    };
    Ok(IdCardFields {
        name,
        sex,
        nation,
        year,
        month,
        day,
        address,
        id_number,
    })
}

fn parse_birth(s: &str) -> Result<(String, String, String)> {
    let parts: Vec<_> = s.split('-').collect();
    if parts.len() != 3 {
        bail!("birth 格式应为 YYYY-MM-DD");
    }
    let y: u32 = parts[0].parse()?;
    let m: u32 = parts[1].parse()?;
    let d: u32 = parts[2].parse()?;
    Ok((y.to_string(), m.to_string(), d.to_string()))
}

fn normalize_id(id: &str) -> Result<String> {
    let id = id.trim().to_uppercase();
    if id.len() == 17 {
        return compose_id_number(&id).context("无效的 17 位本体");
    }
    if id.len() == 18 && validate_id_number(&id) {
        return Ok(id);
    }
    bail!("无效身份证号: {id}")
}
