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
        if seq % 2 == 1 {
            seq - 1
        } else {
            seq
        }
    } else if seq % 2 == 0 {
        (seq + 1) % 1000
    } else {
        seq
    };
    let body = format!("510822{:04}{:02}{:02}{:03}", year, month, day, seq);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::validate::validate_id_number;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

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
