# 身份证测试图生成器（idgen）设计

**日期：** 2026-10-29  
**状态：** 待用户审阅  
**目的：** 为 `idorc` OCR 回归测试生成合成「人像面」图片（仅供学习/测试，不用于伪造证件）。

参考实现：[xiamuguizhi/idcard_generator-](https://github.com/xiamuguizhi/idcard_generator-)（底图 + 固定坐标写字）。

## 目标与非目标

**目标**

- 提供 Rust CLI `idgen`，一键生成可用于 OCR 的人像面 PNG
- 字段可随机或手动指定；身份证号校验位符合 GB 11643
- 与 `idorc` 同仓库、同构建方式，便于 `生成 → OCR` 流水线

**非目标**

- 不做 GUI / 自动抠图
- 不默认输出国徽面或正反拼图（可选后续加 `--full`，首版不做）
- 不追求视觉上与真实证件完全一致，够 OCR 测字段即可

## 方案

在本仓库新增 binary `idgen`：

1. 加载资源：`assets/idgen/empty.png` + `fonts/{hei,fzhei,ocrb10bt}.ttf`（自参考仓库拷贝，README 注明来源与用途限制）
2. 按参考仓库坐标在底图上绘制：姓名、性别、民族、年/月/日、住址（按显示宽度约 22 折行）、公民身份号码
3. **默认裁剪人像面**（底图上半部分 / 人像信息区域）并写出 PNG；头像可选（无头像则留底图原样或纯色块）
4. 写出旁路 `*.json` 期望字段（便于和 OCR 结果对比）— 建议默认开启，可用 `--no-meta` 关闭

### CLI 草图

```bash
# 随机人像面
cargo run --release --bin idgen -- --random -o testdata/gen/front.png

# 指定字段
cargo run --release --bin idgen -- \
  --name 鲜丽娟 --sex 女 --nation 汉 \
  --birth 1995-12-08 --addr "四川省…" \
  --id 51082219951208722X \
  -o testdata/gen/front.png

# 可选头像
cargo run --release --bin idgen -- --random --avatar face.png -o out.png
```

### 版面与裁剪

- 底图来自参考仓库 `asserts/empty.png`，尺寸 **2480×3508**（正反面竖向拼在一起；人像面文字最底约 y≈1565，国徽面文字约 y≥2750）
- 文字坐标与参考一致（人像面）：
  - 姓名 `(630,690)` 72px hei
  - 性别 `(630,840)` / 民族 `(1030,840)` 64px hei
  - 年/月/日 `(630|950|1150, 975)` 60px fzhei
  - 住址自 `(630,1115)` 起，行高 100，按显示宽度 22 折行
  - 号码 `(900,1475)` 90px ocrb10bt
- **默认输出**：裁剪人像面，建议矩形 `(0, 0)–(2480, 1754)`（上半幅；实现时可微调，须含头像区与全部正面字段、不含国徽面字段）
- 头像粘贴位参考：`(1500, 690)`，尺寸 `500×670`（可选）
- 注意：根目录 `.gitignore` 当前忽略 `*.png`，需增加例外 `!assets/idgen/**` 以便提交底图

### 随机数据

- 姓名：小词表或简单组合（可移植参考 `name_utils` 的精简版）
- 性别/民族：男|女，默认「汉」
- 出生：合法年月日
- 地址：若干模板串
- 号码：地区码前缀（可固定如 `510822`）+ 出生日期 + 顺序码 + 校验位

### 依赖

- 已有：`image`、`clap`、`serde`/`serde_json`
- 新增：`ab_glyph` 或 `rusttype` + 简单栅格绘制；或 `imageproc`。优先 **`ab_glyph` + 自绘**，避免引入过重 GUI 栈
- 不引入 OpenCV / 抠图

### 仓库卫生

- `_ref_idcard_gen/` 仅作对照，**不提交**（加入 `.gitignore`）或实现完成后删除
- `assets/idgen/` 提交必要底图与字体
- README 增加「测试图生成」小节 + 法律声明（仅测试）

## 验收

1. `--random -o out.png` 生成可读人像面，肉眼可见各字段
2. `idorc --image out.png` 能解析出与 meta JSON 一致的主要字段（姓名、号码等）
3. 默认产物**不含**国徽面有效区域
4. `cargo build --release --bin idgen` 与现有 `idorc` 互不影响

## 风险

- 字体授权：沿用参考仓库自带字体，仅测试用途；若上游许可不明，文档中标明并允许用户替换路径
- 底图裁剪边界需实测，避免切掉住址末行或号码
