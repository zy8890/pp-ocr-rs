# 身份证测试图生成器（idgen）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 提供 Rust CLI `idgen`，用参考底图+字体合成二代证**人像面** PNG（及期望 JSON），方便 `idorc` OCR 回归测试。

**Architecture:** 库模块 `idorc::gen` 负责字段/号码/折行/渲染；`src/bin/idgen.rs` 只做 CLI。底图与字体放在 `assets/idgen/`。默认裁剪上半幅 `(0,0)-(2480,1754)`。校验位复用并扩展 `parse::validate`。

**Tech Stack:** Rust 2021、`image`、`ab_glyph`、`clap`、`serde`/`serde_json`、`rand`、`anyhow`

## Global Constraints

- 仅供学习/测试，不用于伪造证件；README 须含声明
- 默认只输出人像面；首版不做 `--full` / GUI / 抠图
- 文字坐标与参考仓库一致（见 design spec）
- 根 `.gitignore` 忽略 `*.png`，须例外 `!assets/idgen/**`
- `_ref_idcard_gen/` 不提交（加入 `.gitignore`）；实现后可删本地对照
- Spec: `docs/superpowers/specs/2026-10-29-idcard-generator-design.md`

## File Map

| Path | Responsibility |
|------|----------------|
| `assets/idgen/empty.png` | 正反拼底图 2480×3508 |
| `assets/idgen/fonts/*.ttf` | hei / fzhei / ocrb10bt |
| `src/parse/validate.rs` | 新增 `id_check_digit` / `compose_id_number` |
| `src/gen/mod.rs` | 模块导出 |
| `src/gen/fields.rs` | `IdCardFields`、随机、住址折行、meta JSON |
| `src/gen/render.rs` | 加载底图、贴头像、画字、裁剪人像面 |
| `src/gen/draw.rs` | `ab_glyph` 黑字绘制辅助 |
| `src/bin/idgen.rs` | CLI |
| `src/lib.rs` | `pub mod gen` |
| `Cargo.toml` | 依赖 `ab_glyph`、`rand` |
| `.gitignore` | assets 例外、忽略 `_ref_idcard_gen/`、`testdata/` |
| `README.md` | 测试图生成小节 |

---

### Task 1: 资源与 gitignore

**Files:**
- Create: `assets/idgen/empty.png`, `assets/idgen/fonts/{hei,fzhei,ocrb10bt}.ttf`
- Create: `assets/idgen/README.md`（来源与用途声明）
- Modify: `.gitignore`

**Interfaces:**
- Produces: 资源路径约定 `assets/idgen/empty.png`、`assets/idgen/fonts/*.ttf`

- [ ] **Step 1: 拷贝资源**

```bash
mkdir -p assets/idgen/fonts
cp _ref_idcard_gen/asserts/empty.png assets/idgen/empty.png
cp _ref_idcard_gen/asserts/fonts/hei.ttf assets/idgen/fonts/
cp _ref_idcard_gen/asserts/fonts/fzhei.ttf assets/idgen/fonts/
cp _ref_idcard_gen/asserts/fonts/ocrb10bt.ttf assets/idgen/fonts/
```

Expected: 四个文件存在；`sips -g pixelWidth -g pixelHeight assets/idgen/empty.png` → 2480×3508

- [ ] **Step 2: 写 assets 说明**

Create `assets/idgen/README.md`:

```markdown
# idgen 资源

来源：[xiamuguizhi/idcard_generator-](https://github.com/xiamuguizhi/idcard_generator-) 的 `asserts/`。

**仅供 OCR 学习与自动化测试，禁止用于伪造身份证件或任何违法用途。**
```

- [ ] **Step 3: 更新 `.gitignore`**

将 `.gitignore` 改为：

```
/target
/models
*.jpg
*.jpeg
*.png
!assets/idgen/**
!assets/idgen/**/*.png
.DS_Store
/_ref_idcard_gen/
/testdata/
```

- [ ] **Step 4: 确认 png 可被 git 跟踪**

```bash
git check-ignore -v assets/idgen/empty.png || echo "not ignored OK"
git add -f assets/idgen/README.md assets/idgen/empty.png assets/idgen/fonts/*.ttf .gitignore
git status --short assets/idgen .gitignore
```

Expected: `assets/idgen/*` 显示为 staged（或可 add），不被 ignore

- [ ] **Step 5: Commit**

```bash
git add assets/idgen .gitignore
git commit -m "$(cat <<'EOF'
chore: vendor idgen template assets for test card generation

EOF
)"
```

---

### Task 2: 身份证号校验位生成 API

**Files:**
- Modify: `src/parse/validate.rs`
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Consumes: 现有 `validate_id_number`
- Produces:
  - `pub fn id_check_digit(body17: &str) -> Option<char>`
  - `pub fn compose_id_number(body17: &str) -> Option<String>`（17 位数字 → 18 位）

- [ ] **Step 1: 写失败测试**

在 `src/parse/validate.rs` 的 `tests` 模块追加：

```rust
#[test]
fn compose_check_digit_matches_known() {
    // 公开测试向量
    assert_eq!(id_check_digit("11010519491231002"), Some('X'));
    assert_eq!(
        compose_id_number("11010519491231002").as_deref(),
        Some("11010519491231002X")
    );
    assert!(validate_id_number(
        &compose_id_number("51082219951208722").unwrap()
    ));
}

#[test]
fn compose_rejects_bad_body() {
    assert!(id_check_digit("123").is_none());
    assert!(id_check_digit("1101051949123100a").is_none());
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cargo test --lib parse::validate::tests::compose_check_digit_matches_known -- --nocapture
```

Expected: FAIL（`id_check_digit` / `compose_id_number` 未定义）

- [ ] **Step 3: 实现**

在 `validate_id_number` 上方或下方加入：

```rust
/// 由 17 位本体计算 GB 11643 校验位。
pub fn id_check_digit(body17: &str) -> Option<char> {
    let body17 = body17.trim();
    if body17.len() != 17 || !body17.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let weights: [u32; 17] = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
    let check_map = ['1', '0', 'X', '9', '8', '7', '6', '5', '4', '3', '2'];
    let sum: u32 = body17
        .chars()
        .zip(weights.iter())
        .map(|(c, w)| c.to_digit(10).unwrap() * w)
        .sum();
    Some(check_map[(sum % 11) as usize])
}

/// `body17` + 校验位 → 18 位号码。
pub fn compose_id_number(body17: &str) -> Option<String> {
    let d = id_check_digit(body17)?;
    Some(format!("{body17}{d}"))
}
```

可选：让 `validate_id_number` 内部调用 `id_check_digit`，避免重复权重表（保持行为不变即可）。

- [ ] **Step 4: 测试通过**

```bash
cargo test --lib parse::validate -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/parse/validate.rs
git commit -m "$(cat <<'EOF'
feat: add id number check-digit composition helpers

EOF
)"
```

---

### Task 3: 字段模型、折行与随机数据

**Files:**
- Create: `src/gen/mod.rs`, `src/gen/fields.rs`
- Modify: `src/lib.rs`（`pub mod gen;`）
- Modify: `Cargo.toml`（`rand = "0.8"`）

**Interfaces:**
- Consumes: `crate::parse::validate::compose_id_number`
- Produces:
  - `pub struct IdCardFields { name, sex, nation, year, month, day, address, id_number }`
  - `pub fn display_width(s: &str) -> usize`
  - `pub fn wrap_address(addr: &str, max_width: usize) -> Vec<String>`（默认 max_width=22）
  - `pub fn random_fields(rng: &mut impl Rng) -> IdCardFields`
  - `impl IdCardFields { pub fn to_meta_json(&self) -> serde_json::Value }`

- [ ] **Step 1: 加依赖并挂模块**

`Cargo.toml` `[dependencies]` 增加：

```toml
rand = "0.8"
```

`src/lib.rs` 增加：`pub mod gen;`

`src/gen/mod.rs`:

```rust
pub mod fields;

pub use fields::{display_width, random_fields, wrap_address, IdCardFields};
```

- [ ] **Step 2: 写 fields 失败测试**

Create `src/gen/fields.rs` 先只放测试与空壳（或完整写测试，实现留空让编译失败）：

```rust
use rand::Rng;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdCardFields {
    pub name: String,
    pub sex: String,
    pub nation: String,
    pub year: String,
    pub month: String,
    pub day: String,
    pub address: String,
    pub id_number: String,
}

impl IdCardFields {
    pub fn birth_iso(&self) -> String {
        format!(
            "{}-{:0>2}-{:0>2}",
            self.year,
            self.month.parse::<u32>().unwrap_or(0),
            self.day.parse::<u32>().unwrap_or(0)
        )
    }

    pub fn to_meta_json(&self) -> Value {
        json!({
            "姓名": self.name,
            "性别": self.sex,
            "民族": self.nation,
            "出生": self.birth_iso(),
            "住址": self.address,
            "公民身份号码": self.id_number,
        })
    }
}

/// 中文显示宽度：非 ASCII 计 2，ASCII 计 1（与参考 utils.get_show_len 一致）。
pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| if c.is_ascii() { 1 } else { 2 })
        .sum()
}

pub fn wrap_address(addr: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut w = 0usize;
    for ch in addr.chars() {
        let cw = if ch.is_ascii() { 1 } else { 2 };
        if w + cw > max_width && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            w = 0;
        }
        cur.push(ch);
        w += cw;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

pub fn random_fields(rng: &mut impl Rng) -> IdCardFields {
    todo!("Task 3")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::validate::validate_id_number;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn wrap_chinese_address() {
        let addr = "四川省成都市武侯区益州大道中段722号复城国际";
        let lines = wrap_address(addr, 22);
        assert!(lines.len() >= 2);
        for line in &lines {
            assert!(display_width(line) <= 22);
        }
        assert_eq!(lines.concat(), addr);
    }

    #[test]
    fn random_id_valid() {
        let mut rng = StdRng::seed_from_u64(42);
        let f = random_fields(&mut rng);
        assert!(validate_id_number(&f.id_number), "{}", f.id_number);
        assert!(f.sex == "男" || f.sex == "女");
        assert!(!f.name.is_empty());
    }
}
```

注意：Step 2 若直接包含完整 `wrap_*` 实现，则 Step 3 只实现 `random_fields`。按 TDD：可先让 `random_fields` 为 `todo!()`，`wrap` 已实现（折行逻辑简单，与随机分开测）。

- [ ] **Step 3: 实现 `random_fields`**

```rust
pub fn random_fields(rng: &mut impl Rng) -> IdCardFields {
    const SURNAMES: &[&str] = &["张", "王", "李", "赵", "刘", "陈", "杨", "黄", "周", "吴"];
    const MALE: &[&str] = &["伟", "强", "磊", "洋", "勇", "军", "杰", "涛", "斌", "超"];
    const FEMALE: &[&str] = &["芳", "娜", "敏", "静", "丽", "艳", "娟", "霞", "秀", "英"];
    const ADDRS: &[&str] = &[
        "四川省成都市武侯区益州大道中段722号复城国际",
        "四川省青川县马鹿镇衡柏村3组34号",
        "北京市东城区东华门街道某某胡同1号",
    ];

    let female = rng.gen_bool(0.5);
    let name = format!(
        "{}{}",
        SURNAMES[rng.gen_range(0..SURNAMES.len())],
        if female {
            FEMALE[rng.gen_range(0..FEMALE.len())]
        } else {
            MALE[rng.gen_range(0..MALE.len())]
        }
    );
    let sex = if female { "女" } else { "男" }.to_string();
    let year = rng.gen_range(1970..=2005);
    let month = rng.gen_range(1..=12);
    let day = rng.gen_range(1..=28);
    let seq = rng.gen_range(0..=999);
    // 顺序码奇偶与性别：男奇女偶（简化）
    let seq = if female {
        if seq % 2 == 1 { seq - 1 } else { seq }
    } else if seq % 2 == 0 {
        (seq + 1) % 1000
    } else {
        seq
    };
    let body = format!(
        "510822{:04}{:02}{:02}{:03}",
        year, month, day, seq
    );
    let id_number = crate::parse::validate::compose_id_number(&body)
        .expect("valid 17-digit body");

    IdCardFields {
        name,
        sex,
        nation: "汉".into(),
        year: year.to_string(),
        month: month.to_string(),
        day: day.to_string(),
        address: ADDRS[rng.gen_range(0..ADDRS.len())].into(),
        id_number,
    }
}
```

- [ ] **Step 4: 测试通过**

```bash
cargo test --lib gen::fields -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/gen/
git commit -m "$(cat <<'EOF'
feat: add idgen field model, address wrap, and random data

EOF
)"
```

---

### Task 4: 文字绘制 + 人像面渲染

**Files:**
- Create: `src/gen/draw.rs`, `src/gen/render.rs`
- Modify: `src/gen/mod.rs`
- Modify: `Cargo.toml`（`ab_glyph = "0.2"`）

**Interfaces:**
- Consumes: `IdCardFields`, `wrap_address`, assets under `assets/idgen/`
- Produces:
  - `pub struct RenderOptions { pub assets_dir: PathBuf, pub avatar: Option<PathBuf>, pub front_height: u32 }`（默认 front_height=1754）
  - `pub fn render_front(fields: &IdCardFields, opts: &RenderOptions) -> anyhow::Result<image::RgbaImage>`
  - 坐标常量与 design spec 一致

- [ ] **Step 1: 加 `ab_glyph`，更新 mod**

```toml
ab_glyph = "0.2"
```

`src/gen/mod.rs`:

```rust
pub mod draw;
pub mod fields;
pub mod render;

pub use fields::{display_width, random_fields, wrap_address, IdCardFields};
pub use render::{render_front, RenderOptions};
```

- [ ] **Step 2: 实现 `draw.rs`**

```rust
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};

pub fn draw_text(
    img: &mut RgbaImage,
    font: &FontRef<'_>,
    text: &str,
    x: f32,
    y: f32,
    px: f32,
    color: Rgba<u8>,
) {
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let mut caret = x;
    let baseline = y + scaled.ascent();
    for ch in text.chars() {
        let glyph = scaled.scaled_glyph(ch);
        let advance = scaled.h_advance(glyph.id);
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cover| {
                let px_x = (bounds.min.x + gx as f32).round() as i32;
                let px_y = (bounds.min.y + gy as f32).round() as i32;
                if px_x < 0 || px_y < 0 {
                    return;
                }
                let (px_x, px_y) = (px_x as u32, px_y as u32);
                if px_x >= img.width() || px_y >= img.height() {
                    return;
                }
                let cover = cover.clamp(0.0, 1.0);
                let p = img.get_pixel_mut(px_x, px_y);
                for i in 0..3 {
                    p.0[i] = ((color.0[i] as f32) * cover
                        + (p.0[i] as f32) * (1.0 - cover)) as u8;
                }
                p.0[3] = 255;
            });
        }
        caret += advance;
        // 重新定位下一个 glyph：ab_glyph position
        // 上面 scaled_glyph 未带位置；改用：
        let _ = caret;
    }
}
```

**重要：** 上面草图的 caret 用法不完整。实现时必须用官方模式：

```rust
use ab_glyph::{Font, FontRef, Glyph, Point, PxScale, ScaleFont};

pub fn draw_text(
    img: &mut RgbaImage,
    font: &FontRef<'_>,
    text: &str,
    x: f32,
    y: f32,
    px: f32,
    color: Rgba<u8>,
) {
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let mut position = Point { x, y: y + scaled.ascent() };
    for ch in text.chars() {
        let mut glyph = scaled.scaled_glyph(ch);
        glyph.position = position;
        let advance = scaled.h_advance(glyph.id);
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cover| {
                let px_x = bounds.min.x as i32 + gx as i32;
                let px_y = bounds.min.y as i32 + gy as i32;
                if px_x < 0 || px_y < 0 {
                    return;
                }
                let (px_x, px_y) = (px_x as u32, px_y as u32);
                if px_x >= img.width() || px_y >= img.height() {
                    return;
                }
                let cover = cover.clamp(0.0, 1.0);
                let dest = img.get_pixel_mut(px_x, px_y);
                for i in 0..3 {
                    dest.0[i] = (color.0[i] as f32 * cover
                        + dest.0[i] as f32 * (1.0 - cover))
                        .round() as u8;
                }
                dest.0[3] = 255;
            });
        }
        position.x += advance;
    }
}
```

- [ ] **Step 3: 实现 `render.rs`**

```rust
use std::path::{Path, PathBuf};

use ab_glyph::FontRef;
use anyhow::{Context, Result};
use image::{imageops, Rgba, RgbaImage};

use super::draw::draw_text;
use super::fields::{wrap_address, IdCardFields};

pub struct RenderOptions {
    pub assets_dir: PathBuf,
    pub avatar: Option<PathBuf>,
    pub front_height: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            assets_dir: PathBuf::from("assets/idgen"),
            avatar: None,
            front_height: 1754,
        }
    }
}

pub fn render_front(fields: &IdCardFields, opts: &RenderOptions) -> Result<RgbaImage> {
    let empty_path = opts.assets_dir.join("empty.png");
    let mut img = image::open(&empty_path)
        .with_context(|| format!("打开底图 {}", empty_path.display()))?
        .into_rgba8();

    let hei = load_font(&opts.assets_dir.join("fonts/hei.ttf"))?;
    let fzhei = load_font(&opts.assets_dir.join("fonts/fzhei.ttf"))?;
    let ocrb = load_font(&opts.assets_dir.join("fonts/ocrb10bt.ttf"))?;
    let black = Rgba([0, 0, 0, 255]);

    draw_text(&mut img, &hei, &fields.name, 630.0, 690.0, 72.0, black);
    draw_text(&mut img, &hei, &fields.sex, 630.0, 840.0, 64.0, black);
    draw_text(&mut img, &hei, &fields.nation, 1030.0, 840.0, 64.0, black);
    draw_text(&mut img, &fzhei, &fields.year, 630.0, 975.0, 60.0, black);
    draw_text(&mut img, &fzhei, &fields.month, 950.0, 975.0, 60.0, black);
    draw_text(&mut img, &fzhei, &fields.day, 1150.0, 975.0, 60.0, black);

    let mut addr_y = 1115.0;
    for line in wrap_address(&fields.address, 22) {
        draw_text(&mut img, &hei, &line, 630.0, addr_y, 64.0, black);
        addr_y += 100.0;
    }

    draw_text(
        &mut img,
        &ocrb,
        &fields.id_number,
        900.0,
        1475.0,
        90.0,
        black,
    );

    if let Some(avatar_path) = &opts.avatar {
        let av = image::open(avatar_path)
            .with_context(|| format!("打开头像 {}", avatar_path.display()))?
            .into_rgba8();
        let av = imageops::resize(&av, 500, 670, imageops::FilterType::Lanczos3);
        imageops::overlay(&mut img, &av, 1500, 690);
    }

    let h = opts.front_height.min(img.height());
    let front = imageops::crop_imm(&img, 0, 0, img.width(), h).to_image();
    Ok(front)
}

fn load_font(path: &Path) -> Result<FontRef<'static>> {
    let data = std::fs::read(path)
        .with_context(|| format!("读取字体 {}", path.display()))?;
    // FontRef needs owned data — use Box::leak for process-lifetime fonts in CLI
    let leaked: &'static [u8] = Box::leak(data.into_boxed_slice());
    FontRef::try_from_slice(leaked).context("解析字体失败")
}
```

（若不想 leak，可改为在 `render_front` 内用局部 `Vec<u8>` + `FontRef::try_from_slice` 生命周期绑在函数内。）

- [ ] **Step 4: 渲染烟雾测试（集成测试）**

在 `src/gen/render.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::fields::IdCardFields;

    #[test]
    fn render_smoke_writes_pixels() {
        let fields = IdCardFields {
            name: "测试".into(),
            sex: "男".into(),
            nation: "汉".into(),
            year: "1990".into(),
            month: "1".into(),
            day: "2".into(),
            address: "四川省成都市武侯区益州大道中段722号复城国际".into(),
            id_number: "11010519491231002X".into(),
        };
        let img = render_front(&fields, &RenderOptions::default()).expect("render");
        assert_eq!(img.width(), 2480);
        assert_eq!(img.height(), 1754);
        // 号码区域应有非白像素
        let mut dark = 0u32;
        for y in 1475..1600 {
            for x in 900..2000 {
                let p = img.get_pixel(x, y);
                if p.0[0] < 200 {
                    dark += 1;
                }
            }
        }
        assert!(dark > 100, "expected drawn id digits, dark={dark}");
    }
}
```

```bash
cargo test --lib gen::render -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/gen/
git commit -m "$(cat <<'EOF'
feat: render id card portrait face from template and fonts

EOF
)"
```

---

### Task 5: CLI `idgen`

**Files:**
- Create: `src/bin/idgen.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: `idorc::gen::{random_fields, render_front, IdCardFields, RenderOptions}`
- Produces: PNG + 默认同名 `.json` meta；`--no-meta` 可关

- [ ] **Step 1: 实现 CLI**

Create `src/bin/idgen.rs`:

```rust
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
        // 允许 random 后再被 CLI 覆盖
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
                "510822{:0>4}{:0>2}{:0>2}001",
                year, month, day
            );
            // month/day 可能非零填充；统一：
            let body = format!(
                "510822{:04}{:02}{:02}001",
                year.parse::<u32>()?,
                month.parse::<u32>()?,
                day.parse::<u32>()?
            );
            let _ = body;
            compose_id_number(&format!(
                "510822{:04}{:02}{:02}001",
                year.parse::<u32>()?,
                month.parse::<u32>()?,
                day.parse::<u32>()?
            ))
            .context("compose id")?
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
    Ok((
        parts[0].to_string(),
        parts[1].trim_start_matches('0').to_string().pipe_month(),
        parts[2].trim_start_matches('0').to_string().pipe_day(),
    ))
}
```

实现时 **不要** 使用不存在的 `.pipe_month()`；月日按参考 GUI 习惯可存不带前导零（`1` 而非 `01`），与 `draw` 一致：

```rust
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
```

`build_from_cli` 里自动生成号码时只保留一份 `compose_id_number` 调用，删除重复 `body` 草稿。

- [ ] **Step 2: 构建并生成样例**

```bash
cargo build --release --bin idgen
mkdir -p testdata/gen
cargo run --release --bin idgen -- --random --seed 1 -o testdata/gen/front.png
ls -la testdata/gen/front.png testdata/gen/front.json
```

Expected: 两文件存在；png 宽 2480、高 1754

- [ ] **Step 3: 用 idorc 抽检**

```bash
cargo run --release --bin idorc -- --image testdata/gen/front.png --format zh 2>testdata/gen/ocr.err | tee testdata/gen/ocr.json
cat testdata/gen/front.json
```

Expected: OCR JSON 中「姓名」「公民身份号码」与 `front.json` 大体一致（允许个别字符偏差，但号码应一致或高度接近）

- [ ] **Step 4: 更新 README**

在 `README.md` 末尾追加：

```markdown
## 测试图生成（idgen）

基于公开模板合成**人像面**测试图，仅供 OCR 学习与自动化测试，**禁止用于伪造证件**。

```bash
cargo run --release --bin idgen -- --random -o testdata/gen/front.png
cargo run --release -- --image testdata/gen/front.png
```

资源见 `assets/idgen/`（源自 [idcard_generator-](https://github.com/xiamuguizhi/idcard_generator-)）。
```

- [ ] **Step 5: Commit**

```bash
git add src/bin/idgen.rs README.md
git commit -m "$(cat <<'EOF'
feat: add idgen CLI to synthesize portrait-face test cards

EOF
)"
```

---

## Spec coverage (self-review)

| Spec 项 | Task |
|---------|------|
| Rust CLI idgen | Task 5 |
| 人像面默认裁剪 | Task 4/5 `front_height=1754` |
| 随机/手填字段 | Task 3/5 |
| GB 11643 校验位 | Task 2 |
| 资源拷贝 + gitignore 例外 | Task 1 |
| meta JSON + `--no-meta` | Task 5 |
| 无 GUI/抠图/full | 全局约束，未实现 |
| README 声明 | Task 5 |
| `_ref` 不提交 | Task 1 gitignore |
| 验收：生成+OCR | Task 5 Step 3 |

无 TBD/占位；`parse_birth` 草图中的错误 API 已在同任务内纠正说明。
