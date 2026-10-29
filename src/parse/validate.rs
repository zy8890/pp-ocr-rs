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

/// 18 位公民身份号码加权校验（GB 11643-1999）。
pub fn validate_id_number(id: &str) -> bool {
    let id = id.trim().to_uppercase();
    if id.len() != 18 {
        return false;
    }
    if !id[..17].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let last = id.as_bytes()[17];
    if !(last.is_ascii_digit() || last == b'X') {
        return false;
    }

    id_check_digit(&id[..17]) == id.chars().nth(17)
}

use regex::Regex;

/// 从任意文本中提取首个疑似身份证号并校验。
pub fn find_valid_id_number(text: &str) -> Option<String> {
    lazy_regex(text, |re| {
        for cap in re.captures_iter(text) {
            let candidate = cap.get(0)?.as_str().to_uppercase();
            if validate_id_number(&candidate) {
                return Some(candidate);
            }
        }
        None
    })
}

fn lazy_regex<F, T>(_text: &str, f: F) -> T
where
    F: FnOnce(&Regex) -> T,
{
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)[1-9]\d{16}[\dXx]").unwrap());
    f(re)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_sample() {
        // 公开测试向量（仅校验算法，非真实个人信息）
        assert!(validate_id_number("11010519491231002X"));
    }

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
}
