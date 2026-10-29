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
