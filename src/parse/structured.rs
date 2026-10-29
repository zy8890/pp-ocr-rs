//! 方案 A：OCR 框为原子元素，按标签坐标配对取值，以模型 score 作置信度。

use rapidocr_core::types::{OcrLine, OcrOutput};

use crate::id_card::{
    FieldValue, IdCardBack, IdCardFront, IdCardResult, IdCardSide, RawLine,
};
use crate::parse::validate::{find_valid_id_number, validate_id_number};

/// 同框「标签+值」拆分时的置信度降权。
const SAME_BOX_PENALTY: f32 = 0.85;

#[derive(Debug, Clone)]
pub struct ParseOptions {
    pub min_confidence: f32,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            min_confidence: 0.80,
        }
    }
}

#[derive(Clone, Debug)]
struct LineItem {
    text: String,
    score: f32,
    cy: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    height: f32,
}

pub fn parse_id_card(output: &OcrOutput) -> IdCardResult {
    parse_id_card_with_options(output, &ParseOptions::default())
}

pub fn parse_id_card_with_options(output: &OcrOutput, options: &ParseOptions) -> IdCardResult {
    let mut warnings = Vec::new();
    let items: Vec<LineItem> = output
        .lines
        .iter()
        .map(line_item)
        .filter(|l| !l.text.is_empty())
        .collect();

    let raw_lines: Vec<RawLine> = items
        .iter()
        .map(|l| RawLine {
            text: l.text.clone(),
            score: l.score,
            x0: l.x0,
            y0: l.y0,
            x1: l.x1,
            y1: l.y1,
        })
        .collect();

    let side = detect_side(&items);

    let (front, back) = match side {
        IdCardSide::Front => (
            Some(parse_front(&items, options, &mut warnings)),
            None,
        ),
        IdCardSide::Back => (None, Some(parse_back(&items, options, &mut warnings))),
        IdCardSide::Unknown => {
            let front = parse_front(&items, options, &mut warnings);
            if front.name.is_some() || front.id_number.is_some() {
                (Some(front), None)
            } else {
                (None, Some(parse_back(&items, options, &mut warnings)))
            }
        }
    };

    let side = if front.as_ref().is_some_and(|f| f.name.is_some() || f.id_number.is_some()) {
        IdCardSide::Front
    } else if back
        .as_ref()
        .is_some_and(|b| b.issuing_authority.is_some() || b.valid_period.is_some())
    {
        IdCardSide::Back
    } else {
        side
    };

    IdCardResult {
        side,
        front,
        back,
        raw_lines,
        warnings,
        min_confidence: options.min_confidence,
        rotation_degrees: 0,
    }
}

fn line_item(line: &OcrLine) -> LineItem {
    let (x0, y0, x1, y1) = quad_bounds(&line.bbox);
    LineItem {
        text: normalize_ocr_text(&line.text),
        score: line.score,
        cy: (y0 + y1) / 2.0,
        x0,
        y0,
        x1,
        y1,
        height: (y1 - y0).max(1.0),
    }
}

fn quad_bounds(q: &rapidocr_core::types::Quad) -> (f32, f32, f32, f32) {
    let mut x0 = f32::INFINITY;
    let mut y0 = f32::INFINITY;
    let mut x1 = f32::NEG_INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    for p in &q.points {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    (x0, y0, x1, y1)
}

fn normalize_ocr_text(s: &str) -> String {
    s.replace([' ', '　'], "").trim().to_string()
}

fn detect_side(items: &[LineItem]) -> IdCardSide {
    let blob: String = items.iter().map(|l| l.text.as_str()).collect();
    let has_id = items.iter().any(|l| find_valid_id_number(&l.text).is_some());
    if has_id || blob.contains("公民身份号码") || blob.contains("姓名") || blob.contains("住址")
    {
        return IdCardSide::Front;
    }
    if blob.contains("签发机关") || blob.contains("有效期限") || blob.contains("中华人民共和国") {
        return IdCardSide::Back;
    }
    IdCardSide::Unknown
}

fn parse_front(
    items: &[LineItem],
    options: &ParseOptions,
    warnings: &mut Vec<String>,
) -> IdCardFront {
    let min_c = options.min_confidence;
    let mut used = vec![false; items.len()];
    let mut front = IdCardFront::default();

    // —— 姓名 ——
    front.name = pair_field(
        items,
        &mut used,
        &["姓名"],
        min_c,
        |s| looks_like_name(s),
        |s| Some(s.to_string()),
    );

    // —— 性别 ——
    front.gender = pair_field(
        items,
        &mut used,
        &["性别"],
        min_c,
        |s| parse_gender(s).is_some(),
        parse_gender,
    );

    // —— 民族（整框保留，可为多字）——
    front.ethnicity = pair_field(
        items,
        &mut used,
        &["民族"],
        min_c,
        |s| looks_like_ethnicity(s),
        |s| Some(normalize_ethnicity(s)),
    );

    // 「性别女民族汉」等同框复合行：性别配对可能已占用该框，补抽民族
    if front.ethnicity.is_none() {
        for item in items {
            if let Some(rest) = item.text.split("民族").nth(1) {
                let rest = rest.trim();
                if looks_like_ethnicity(rest) || looks_like_ethnicity(&format!("民族{rest}")) {
                    front.ethnicity = Some(FieldValue::new(
                        normalize_ethnicity(rest),
                        item.score * SAME_BOX_PENALTY,
                        min_c,
                    ));
                    break;
                }
            }
        }
    }

    // —— 出生 ——
    front.birth_date = pair_field(
        items,
        &mut used,
        &["出生"],
        min_c,
        |s| parse_birth_from_line(s).is_some(),
        parse_birth_from_line,
    );

    // —— 住址（可多行）——
    front.address = pair_address(items, &mut used, min_c);

    // —— 公民身份号码 ——
    front.id_number = pair_id_number(items, &mut used, min_c, warnings);
    front.id_checksum_ok = front
        .id_number
        .as_ref()
        .map(|f| validate_id_number(&f.value));

    if front.id_number.is_none() {
        warnings.push("未找到身份证号字段".into());
    }

    // 兜底：无「姓名」标签时，取版面上部未占用的姓名样文本
    if front.name.is_none() {
        if let Some((idx, item)) = items
            .iter()
            .enumerate()
            .filter(|(i, l)| !used[*i] && looks_like_name(&l.text))
            .min_by(|a, b| a.1.y0.partial_cmp(&b.1.y0).unwrap_or(std::cmp::Ordering::Equal))
        {
            used[idx] = true;
            front.name = Some(FieldValue::new(item.text.clone(), item.score, min_c));
        }
    }

    // 兜底：未配对到的出生/性别（模型未检出标签时）
    if front.birth_date.is_none() {
        if let Some((idx, d)) = items.iter().enumerate().find_map(|(i, l)| {
            if used[i] {
                return None;
            }
            parse_birth_from_line(&l.text).map(|d| (i, (d, l.score)))
        }) {
            used[idx] = true;
            front.birth_date = Some(FieldValue::new(d.0, d.1, min_c));
        }
    }
    if front.gender.is_none() {
        if let Some((idx, g)) = items.iter().enumerate().find_map(|(i, l)| {
            if used[i] {
                return None;
            }
            // 整行仅为「男/女」或极短
            if l.text == "男" || l.text == "女" {
                return Some((i, (l.text.clone(), l.score)));
            }
            None
        }) {
            used[idx] = true;
            front.gender = Some(FieldValue::new(g.0, g.1, min_c));
        }
    }
    if front.ethnicity.is_none() {
        if let Some((idx, e)) = items.iter().enumerate().find_map(|(i, l)| {
            if used[i] {
                return None;
            }
            if looks_like_ethnicity(&l.text) && !l.text.contains("民族") {
                return Some((i, (normalize_ethnicity(&l.text), l.score)));
            }
            None
        }) {
            used[idx] = true;
            front.ethnicity = Some(FieldValue::new(e.0, e.1, min_c));
        }
    }

    // 兜底：无「住址」标签时，收集省市区类文本（排除已占用框）
    if front.address.is_none() {
        front.address = fallback_address(items, &used, min_c);
    }

    let _ = used;
    front
}

fn fallback_address(items: &[LineItem], used: &[bool], min_c: f32) -> Option<FieldValue> {
    let mut parts: Vec<(String, f32)> = items
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            !used[*i]
                && looks_like_address(&l.text)
                && find_valid_id_number(&l.text).is_none()
                && !is_field_label(&l.text)
        })
        .map(|(_, l)| (l.text.clone(), l.score))
        .collect();
    if parts.is_empty() {
        return None;
    }
    // 按阅读顺序：已在 items 中大致为检测顺序；再按 y 稳妥排序
    let confidence = parts.iter().map(|(_, s)| *s).fold(1.0_f32, f32::min);
    let value = parts.drain(..).map(|(t, _)| t).collect::<String>();
    Some(FieldValue::new(value, confidence, min_c))
}

fn looks_like_address(s: &str) -> bool {
    const KEYS: &[&str] = &[
        "省", "市", "区", "县", "旗", "盟", "州", "镇", "乡", "街", "路", "道", "巷", "村", "号",
        "楼", "室",
    ];
    let n = s.chars().count();
    n >= 4 && KEYS.iter().any(|k| s.contains(k))
}

fn parse_back(
    items: &[LineItem],
    options: &ParseOptions,
    warnings: &mut Vec<String>,
) -> IdCardBack {
    let min_c = options.min_confidence;
    let mut used = vec![false; items.len()];
    let mut back = IdCardBack::default();

    back.issuing_authority = pair_field(
        items,
        &mut used,
        &["签发机关"],
        min_c,
        |s| !s.is_empty() && !s.contains("签发"),
        |s| Some(s.to_string()),
    );

    back.valid_period = pair_field(
        items,
        &mut used,
        &["有效期限"],
        min_c,
        |s| s.contains('-') || s.contains("长期") || s.contains('.'),
        |s| Some(normalize_valid_period(s)),
    );

    if back.valid_period.is_none() {
        if let Some((idx, l)) = items.iter().enumerate().find(|(i, l)| {
            !used[*i]
                && (l.text.contains("长期")
                    || (l.text.chars().filter(|c| c.is_ascii_digit()).count() >= 8
                        && (l.text.contains('-') || l.text.contains('.'))))
        }) {
            used[idx] = true;
            back.valid_period = Some(FieldValue::new(
                normalize_valid_period(&l.text),
                l.score,
                min_c,
            ));
        }
    }

    if back.issuing_authority.is_none() && back.valid_period.is_none() {
        warnings.push("国徽面字段解析不完整".into());
    }

    let _ = used;
    back
}

/// 找标签框，再取右侧同行值；否则同框去标签（降权）。
fn pair_field<P, M>(
    items: &[LineItem],
    used: &mut [bool],
    labels: &[&str],
    min_c: f32,
    pred: P,
    map: M,
) -> Option<FieldValue>
where
    P: Fn(&str) -> bool,
    M: Fn(&str) -> Option<String>,
{
    let label_idx = find_label_index(items, used, labels)?;
    let label = &items[label_idx];
    used[label_idx] = true;

    // 同框：标签后仍有内容
    for lab in labels {
        if let Some(rest) = strip_label_prefix(&label.text, lab) {
            if !rest.is_empty() {
                if let Some(v) = map(&rest) {
                    if pred(&rest) || map(&rest).is_some() {
                        return Some(FieldValue::new(v, label.score * SAME_BOX_PENALTY, min_c));
                    }
                }
            }
        }
    }

    // 右侧同行
    if let Some(idx) = find_right_of(items, used, label, &pred) {
        let item = &items[idx];
        used[idx] = true;
        let v = map(&item.text)?;
        return Some(FieldValue::new(v, item.score, min_c));
    }

    // 正下方一行（少见）
    if let Some(idx) = find_below(items, used, label, &pred) {
        let item = &items[idx];
        used[idx] = true;
        let v = map(&item.text)?;
        return Some(FieldValue::new(v, item.score, min_c));
    }

    None
}

fn pair_address(items: &[LineItem], used: &mut [bool], min_c: f32) -> Option<FieldValue> {
    let label_idx = find_label_index(items, used, &["住址"])?;
    let label = &items[label_idx];
    used[label_idx] = true;

    let id_top = items
        .iter()
        .find(|l| l.text.contains("公民身份号码") || find_valid_id_number(&l.text).is_some())
        .map(|l| l.y0)
        .unwrap_or(f32::INFINITY);

    let mut parts: Vec<(String, f32)> = Vec::new();

    if let Some(rest) = strip_label_prefix(&label.text, "住址") {
        if !rest.is_empty() {
            parts.push((rest, label.score * SAME_BOX_PENALTY));
        }
    }

    // 右侧同行
    if let Some(idx) = find_right_of(items, used, label, &|s| {
        !is_field_label(s) && find_valid_id_number(s).is_none()
    }) {
        let item = &items[idx];
        used[idx] = true;
        parts.push((item.text.clone(), item.score));
    }

    // 下方连续行，直到号码区
    let mut below: Vec<(usize, &LineItem)> = items
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            !used[*i]
                && l.y0 >= label.y1 - label.height * 0.2
                && l.y1 < id_top
                && l.x0 >= label.x0 - label.height * 2.0
                && !is_field_label(&l.text)
                && find_valid_id_number(&l.text).is_none()
                && parse_birth_from_line(&l.text).is_none()
        })
        .map(|(i, l)| (i, l))
        .collect();
    below.sort_by(|a, b| a.1.cy.partial_cmp(&b.1.cy).unwrap_or(std::cmp::Ordering::Equal));

    for (i, l) in below {
        used[i] = true;
        parts.push((l.text.clone(), l.score));
    }

    if parts.is_empty() {
        return None;
    }

    let confidence = parts.iter().map(|(_, s)| *s).fold(1.0_f32, f32::min);
    let value = parts.into_iter().map(|(t, _)| t).collect::<String>();
    Some(FieldValue::new(value, confidence, min_c))
}

fn pair_id_number(
    items: &[LineItem],
    used: &mut [bool],
    min_c: f32,
    warnings: &mut Vec<String>,
) -> Option<FieldValue> {
    let label_idx = find_label_index(items, used, &["公民身份号码"]);
    if let Some(label_idx) = label_idx {
        let label = &items[label_idx];
        used[label_idx] = true;

        if let Some(rest) = strip_label_prefix(&label.text, "公民身份号码") {
            if let Some(id) = find_valid_id_number(&rest).or_else(|| extract_id_digits(&rest)) {
                let ok = validate_id_number(&id);
                if !ok {
                    warnings.push("身份证号校验位未通过".into());
                }
                return Some(
                    FieldValue::new(id, label.score * SAME_BOX_PENALTY, min_c).with_reliable(ok),
                );
            }
        }

        if let Some(idx) = find_right_of(items, used, label, &|s| {
            find_valid_id_number(s).is_some() || extract_id_digits(s).is_some()
        }) {
            let item = &items[idx];
            used[idx] = true;
            let id = find_valid_id_number(&item.text)
                .or_else(|| extract_id_digits(&item.text))?;
            let ok = validate_id_number(&id);
            if !ok {
                warnings.push("身份证号校验位未通过".into());
            }
            return Some(FieldValue::new(id, item.score, min_c).with_reliable(ok));
        }
    }

    // 无标签时：任意含合法号码的框
    for (i, l) in items.iter().enumerate() {
        if used[i] {
            continue;
        }
        if let Some(id) = find_valid_id_number(&l.text) {
            used[i] = true;
            let ok = validate_id_number(&id);
            if !ok {
                warnings.push("身份证号校验位未通过".into());
            }
            return Some(FieldValue::new(id, l.score, min_c).with_reliable(ok));
        }
    }

    None
}

fn find_label_index(items: &[LineItem], used: &[bool], labels: &[&str]) -> Option<usize> {
    // 优先：整框恰为标签，或「标签+值」以标签开头且较短标签占比大
    for (i, item) in items.iter().enumerate() {
        if used[i] {
            continue;
        }
        for lab in labels {
            if item.text == *lab {
                return Some(i);
            }
        }
    }
    for (i, item) in items.iter().enumerate() {
        if used[i] {
            continue;
        }
        for lab in labels {
            if item.text.starts_with(lab) {
                return Some(i);
            }
        }
    }
    // 弱匹配：包含标签（竖排误识等场景慎用，仅当无更强匹配）
    for (i, item) in items.iter().enumerate() {
        if used[i] {
            continue;
        }
        for lab in labels {
            if item.text.contains(lab) {
                return Some(i);
            }
        }
    }
    None
}

fn find_right_of<P>(items: &[LineItem], used: &[bool], label: &LineItem, pred: &P) -> Option<usize>
where
    P: Fn(&str) -> bool,
{
    // 同行用中心 y；右侧用中心 x（允许标签/值框轻微水平重叠，如「姓名」与「丹丹」）
    let row_tol = label.height.max(8.0) * 0.9;
    let mut best: Option<(usize, f32)> = None;
    for (i, item) in items.iter().enumerate() {
        if used[i] {
            continue;
        }
        if (item.cy - label.cy).abs() > row_tol {
            continue;
        }
        let item_cx = (item.x0 + item.x1) / 2.0;
        let label_cx = (label.x0 + label.x1) / 2.0;
        if item_cx <= label_cx {
            continue;
        }
        // 值框左缘不应远在标签左缘左侧（排除左侧其它字段）
        if item.x1 <= label.x0 {
            continue;
        }
        if !pred(&item.text) {
            continue;
        }
        let dist = item_cx - label_cx;
        if best.map(|(_, d)| dist < d).unwrap_or(true) {
            best = Some((i, dist));
        }
    }
    best.map(|(i, _)| i)
}

fn find_below<P>(items: &[LineItem], used: &[bool], label: &LineItem, pred: &P) -> Option<usize>
where
    P: Fn(&str) -> bool,
{
    let mut best: Option<(usize, f32)> = None;
    for (i, item) in items.iter().enumerate() {
        if used[i] {
            continue;
        }
        if item.cy <= label.cy {
            continue;
        }
        if (item.x0 - label.x0).abs() > label.height * 8.0 {
            continue;
        }
        if !pred(&item.text) {
            continue;
        }
        let dist = item.cy - label.cy;
        if best.map(|(_, d)| dist < d).unwrap_or(true) {
            best = Some((i, dist));
        }
    }
    best.map(|(i, _)| i)
}

fn strip_label_prefix(text: &str, label: &str) -> Option<String> {
    let rest = text.strip_prefix(label)?;
    let rest = rest.trim_start_matches([':', '：']);
    Some(rest.to_string())
}

fn is_field_label(s: &str) -> bool {
    const LABELS: &[&str] = &[
        "姓名",
        "性别",
        "民族",
        "出生",
        "住址",
        "公民身份号码",
        "签发机关",
        "有效期限",
    ];
    LABELS.iter().any(|l| s == *l || s.starts_with(l) && strip_label_prefix(s, l).is_some_and(|r| r.is_empty()))
}

fn looks_like_name(s: &str) -> bool {
    let s = s.trim();
    let n = s.chars().count();
    (2..=8).contains(&n)
        && !s.chars().any(|c| c.is_ascii_digit())
        && !contains_field_keyword(s)
        && s != "男"
        && s != "女"
}

fn looks_like_ethnicity(s: &str) -> bool {
    let s = s.trim();
    if s == "民族" || contains_field_keyword(s) && !s.starts_with("民族") {
        return false;
    }
    let body = strip_label_prefix(s, "民族").unwrap_or_else(|| s.to_string());
    let n = body.chars().count();
    // 常见民族简称或带「族」后缀；拒绝「别生」一类竖排误识
    if !(1..=8).contains(&n) || body.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    if body == "男" || body == "女" {
        return false;
    }
    body.ends_with('族')
        || matches!(
            body.as_str(),
            "汉" | "回" | "蒙" | "藏" | "维" | "苗" | "彝" | "壮" | "朝鲜" | "满" | "侗" | "瑶"
                | "白" | "土家" | "哈尼" | "哈萨克" | "傣" | "黎" | "傈僳" | "佤" | "畲" | "高山"
                | "拉祜" | "水" | "东乡" | "纳西" | "景颇" | "柯尔克孜" | "土" | "达斡尔" | "仫佬"
                | "羌" | "布朗" | "撒拉" | "毛南" | "仡佬" | "锡伯" | "阿昌" | "普米" | "塔吉克"
                | "怒" | "乌孜别克" | "俄罗斯" | "鄂温克" | "德昂" | "保安" | "裕固" | "京" | "塔塔尔"
                | "独龙" | "鄂伦春" | "赫哲" | "门巴" | "珞巴" | "基诺"
        )
}

fn contains_field_keyword(s: &str) -> bool {
    ["姓名", "性别", "民族", "出生", "住址", "公民", "身份", "号码", "签发", "有效"]
        .iter()
        .any(|k| s.contains(k))
}

fn normalize_ethnicity(s: &str) -> String {
    let s = strip_label_prefix(s, "民族").unwrap_or_else(|| s.to_string());
    let s = s.trim();
    if s.is_empty() {
        return s.to_string();
    }
    if s.ends_with('族') {
        s.to_string()
    } else {
        format!("{s}族")
    }
}

fn parse_gender(s: &str) -> Option<String> {
    let s = strip_label_prefix(s, "性别").unwrap_or_else(|| s.to_string());
    if s.contains('男') {
        Some("男".into())
    } else if s.contains('女') {
        Some("女".into())
    } else {
        None
    }
}

fn parse_birth_from_line(s: &str) -> Option<String> {
    use regex::Regex;
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?x)
            (?P<y>(?:19|20)\d{2})
            \s*[年./\-]?\s*
            (?P<m>1[0-2]|0?[1-9])
            \s*[月./\-]?\s*
            (?P<d>3[01]|[12]\d|0?[1-9])
            \s*日?
        ",
        )
        .unwrap()
    });
    let caps = re.captures(s)?;
    let y = caps.name("y")?.as_str();
    let m = caps.name("m")?.as_str().parse::<u32>().ok()?;
    let d = caps.name("d")?.as_str().parse::<u32>().ok()?;
    Some(format!("{y}-{m:02}-{d:02}"))
}

fn extract_id_digits(s: &str) -> Option<String> {
    let compact: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    if compact.len() == 18 {
        Some(compact)
    } else {
        find_valid_id_number(s)
    }
}

fn normalize_valid_period(s: &str) -> String {
    let s = strip_label_prefix(s, "有效期限").unwrap_or_else(|| s.to_string());
    s.replace([' ', '—', '－'], "-")
        .replace('.', "-")
        .replace('至', "-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapidocr_core::types::{OcrLine, OcrOutput, Quad};

    fn line(text: &str, x0: f32, y0: f32, x1: f32, y1: f32, score: f32) -> OcrLine {
        OcrLine {
            bbox: Quad::from_xyxy(x0, y0, x1, y1),
            text: text.into(),
            score,
        }
    }

    #[test]
    fn separate_boxes_like_layout_diagram() {
        // 模拟示意图：标签与值分框
        let output = OcrOutput {
            lines: vec![
                line("姓名", 80.0, 40.0, 160.0, 70.0, 0.99),
                line("王飞", 180.0, 38.0, 280.0, 72.0, 0.99),
                line("性别", 80.0, 90.0, 160.0, 120.0, 0.99),
                line("女", 180.0, 90.0, 220.0, 120.0, 0.99),
                line("民族", 280.0, 90.0, 360.0, 120.0, 0.99),
                line("汉", 380.0, 90.0, 420.0, 120.0, 0.99),
                line("出生", 80.0, 140.0, 160.0, 170.0, 0.99),
                line("1992年8月18日", 180.0, 140.0, 420.0, 170.0, 0.99),
                line("住址", 80.0, 200.0, 160.0, 230.0, 0.99),
                line("辽宁省大连市甘井子区", 180.0, 200.0, 500.0, 230.0, 0.99),
                line("某某街道1号", 180.0, 240.0, 360.0, 270.0, 0.97),
                line("公民身份号码", 80.0, 320.0, 280.0, 350.0, 0.99),
                line("522530199208180048", 300.0, 320.0, 620.0, 350.0, 0.99),
            ],
        };
        let r = parse_id_card(&output);
        let f = r.front.expect("front");
        assert_eq!(f.name.as_ref().map(|v| v.value.as_str()), Some("王飞"));
        assert!(f.name.as_ref().unwrap().reliable);
        assert_eq!(f.gender.as_ref().map(|v| v.value.as_str()), Some("女"));
        assert_eq!(f.ethnicity.as_ref().map(|v| v.value.as_str()), Some("汉族"));
        assert_eq!(
            f.birth_date.as_ref().map(|v| v.value.as_str()),
            Some("1992-08-18")
        );
        let addr = f.address.as_ref().unwrap();
        assert!(addr.value.contains("辽宁省大连市甘井子区"));
        assert!(addr.value.contains("某某街道1号"));
        assert!((addr.confidence - 0.97).abs() < 1e-3); // min of lines
        assert_eq!(
            f.id_number.as_ref().map(|v| v.value.as_str()),
            Some("522530199208180048")
        );
        assert_eq!(f.id_checksum_ok, Some(true));
    }

    #[test]
    fn same_box_fallback_with_penalty() {
        let output = OcrOutput {
            lines: vec![
                line("姓名张三", 80.0, 40.0, 200.0, 70.0, 1.0),
                line("性别男", 80.0, 80.0, 160.0, 110.0, 1.0),
                line("民族汉", 180.0, 80.0, 280.0, 110.0, 1.0),
                line("出生1990年5月12日", 80.0, 120.0, 320.0, 150.0, 1.0),
                line("住址北京市朝阳区某某路1号", 80.0, 160.0, 400.0, 190.0, 1.0),
                line("公民身份号码", 80.0, 220.0, 220.0, 250.0, 1.0),
                line("11010519491231002X", 240.0, 220.0, 420.0, 250.0, 1.0),
            ],
        };
        let r = parse_id_card(&output);
        let f = r.front.expect("front");
        assert_eq!(f.name.as_ref().map(|v| v.value.as_str()), Some("张三"));
        assert!((f.name.as_ref().unwrap().confidence - SAME_BOX_PENALTY).abs() < 1e-4);
        assert_eq!(f.gender.as_ref().map(|v| v.value.as_str()), Some("男"));
        assert_eq!(f.ethnicity.as_ref().map(|v| v.value.as_str()), Some("汉族"));
        assert_eq!(
            f.birth_date.as_ref().map(|v| v.value.as_str()),
            Some("1990-05-12")
        );
        assert!(f.id_number.is_some());
    }

    #[test]
    fn low_score_marked_unreliable() {
        let output = OcrOutput {
            lines: vec![
                line("姓名", 80.0, 40.0, 160.0, 70.0, 0.99),
                line("李四", 180.0, 40.0, 260.0, 70.0, 0.5),
                line("公民身份号码", 80.0, 220.0, 220.0, 250.0, 0.99),
                line("11010519491231002X", 240.0, 220.0, 420.0, 250.0, 0.99),
            ],
        };
        let r = parse_id_card_with_options(
            &output,
            &ParseOptions {
                min_confidence: 0.8,
            },
        );
        let f = r.front.expect("front");
        assert_eq!(f.name.as_ref().map(|v| v.value.as_str()), Some("李四"));
        assert!(!f.name.as_ref().unwrap().reliable);
    }
}
