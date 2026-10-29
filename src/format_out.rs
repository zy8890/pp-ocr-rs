use serde_json::{Map, Value};

use crate::id_card::{FieldValue, IdCardResult, IdCardSide};

fn field_str(f: &Option<FieldValue>) -> String {
    f.as_ref().map(|v| v.value.clone()).unwrap_or_default()
}

fn insert(map: &mut Map<String, Value>, key: &str, val: String) {
    map.insert(key.to_string(), Value::String(val));
}

/// 中文字段扁平 JSON：`{"姓名":"...","性别":"...",...}`（字段顺序固定）
pub fn to_zh_json(result: &IdCardResult) -> Value {
    let mut map = Map::new();
    match result.side {
        IdCardSide::Back => {
            let b = result.back.as_ref();
            insert(&mut map, "面", "国徽面".into());
            insert(
                &mut map,
                "签发机关",
                b.map(|x| field_str(&x.issuing_authority))
                    .unwrap_or_default(),
            );
            insert(
                &mut map,
                "有效期限",
                b.map(|x| field_str(&x.valid_period)).unwrap_or_default(),
            );
        }
        _ => {
            let f = result.front.as_ref();
            insert(
                &mut map,
                "姓名",
                f.map(|x| field_str(&x.name)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "性别",
                f.map(|x| field_str(&x.gender)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "民族",
                f.map(|x| field_str(&x.ethnicity)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "出生",
                f.map(|x| field_str(&x.birth_date)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "住址",
                f.map(|x| field_str(&x.address)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "公民身份号码",
                f.map(|x| field_str(&x.id_number)).unwrap_or_default(),
            );
        }
    }
    Value::Object(map)
}

/// 英文字段扁平 JSON：`{"name":"...","gender":"...",...}`
pub fn to_en_json(result: &IdCardResult) -> Value {
    let mut map = Map::new();
    match result.side {
        IdCardSide::Back => {
            let b = result.back.as_ref();
            insert(
                &mut map,
                "issuing_authority",
                b.map(|x| field_str(&x.issuing_authority))
                    .unwrap_or_default(),
            );
            insert(
                &mut map,
                "valid_period",
                b.map(|x| field_str(&x.valid_period)).unwrap_or_default(),
            );
        }
        _ => {
            let f = result.front.as_ref();
            insert(
                &mut map,
                "name",
                f.map(|x| field_str(&x.name)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "gender",
                f.map(|x| field_str(&x.gender)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "ethnicity",
                f.map(|x| field_str(&x.ethnicity)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "birth_date",
                f.map(|x| field_str(&x.birth_date)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "address",
                f.map(|x| field_str(&x.address)).unwrap_or_default(),
            );
            insert(
                &mut map,
                "id_number",
                f.map(|x| field_str(&x.id_number)).unwrap_or_default(),
            );
        }
    }
    Value::Object(map)
}

/// 纯文本：`姓名：xxx` 每行一项。
pub fn to_zh_text(result: &IdCardResult) -> String {
    let v = to_zh_json(result);
    let obj = v.as_object().cloned().unwrap_or_default();
    obj.iter()
        .filter(|(k, _)| k.as_str() != "面")
        .map(|(k, val)| {
            let s = val.as_str().unwrap_or("");
            format!("{k}：{s}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id_card::{FieldValue, IdCardFront, IdCardResult, IdCardSide};

    #[test]
    fn en_json_front_keys() {
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
}
