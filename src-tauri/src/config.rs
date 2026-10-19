//! 设置页用的整理：API Key 掩码、局部更新、AI 归纳标签的解析。

use crate::format::strip_think;
use crate::model::Settings;
use crate::tags::normalize;
use serde_json::Value;

const MASK: &str = "****";

/// sk-abcdef1234 → sk-****1234；太短的全部遮住
pub fn mask_key(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 8 {
        return MASK.to_string();
    }
    let prefix: String = match key.find('-') {
        Some(i) if i <= 4 => key[..=i].to_string(),
        _ => chars[..3].iter().collect(),
    };
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}{MASK}{tail}")
}

pub fn is_masked(s: &str) -> bool {
    s.contains(MASK)
}

/// 递归合并 JSON 对象
fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// 把设置页传来的局部修改合进现有设置；掩码过的 Key 视为没改
pub fn apply_patch(current: &Settings, patch: &Value) -> Result<Settings, String> {
    let mut patch = patch.clone();
    for section in ["vision", "summary"] {
        if let Some(obj) = patch.get_mut(section).and_then(|v| v.as_object_mut()) {
            match obj.get("apiKey").and_then(|k| k.as_str()) {
                Some(k) if is_masked(k) => {
                    obj.remove("apiKey");
                }
                Some(k) => {
                    let trimmed = k.trim().to_string();
                    obj.insert("apiKey".into(), Value::String(trimmed));
                }
                None => {}
            }
        }
    }
    let mut value = serde_json::to_value(current).map_err(|e| e.to_string())?;
    merge(&mut value, &patch);
    let next: Settings = serde_json::from_value(value).map_err(|e| format!("设置格式不对：{e}"))?;
    if chrono::NaiveTime::parse_from_str(&next.remind_time, "%H:%M").is_err() {
        return Err("提醒时间要写成 18:00 这样".into());
    }
    if next.shots_retention_days == 0 {
        return Err("截图至少保留 1 天".into());
    }
    if patch.get("userName").is_some() && next.user_name.trim().is_empty() {
        return Err("名字不能空着".into());
    }
    Ok(next)
}

/// 解析模型归纳出的标签：JSON 数组或顿号/逗号/换行分隔，归一后去重，最多 8 个
pub fn parse_tag_list(raw: &str) -> Vec<String> {
    let text = strip_think(raw);
    let text = text.trim();
    let items: Vec<String> = match serde_json::from_str::<Vec<String>>(text) {
        Ok(v) => v,
        Err(_) => match (text.find('['), text.rfind(']')) {
            (Some(a), Some(b)) if a < b => serde_json::from_str::<Vec<String>>(&text[a..=b]).unwrap_or_default(),
            _ => text.split(['、', ',', '，', '\n', ';', '；']).map(|s| s.to_string()).collect(),
        },
    };
    let mut out: Vec<String> = Vec::new();
    for t in items.iter().filter_map(|t| normalize(t)) {
        if !out.contains(&t) {
            out.push(t);
        }
        if out.len() == 8 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Settings;
    use serde_json::json;

    #[test]
    fn mask_key_variants() {
        assert_eq!(mask_key("sk-abcdef1234"), "sk-****1234");
        assert_eq!(mask_key("abcdefghijklmn"), "abc****klmn");
        assert_eq!(mask_key("short"), "****");
        assert_eq!(mask_key(""), "");
        assert!(is_masked("sk-****1234"));
        assert!(!is_masked("sk-abcdef1234"));
    }

    #[test]
    fn patch_merges_and_keeps_real_key_when_masked() {
        let mut s = Settings::default();
        s.vision.api_key = "sk-realkey9999".into();
        s.user_name = "某某".into();
        let next = apply_patch(&s, &json!({
            "titleSuffix": "日报",
            "vision": { "apiKey": "sk-****9999", "model": "qwen-vl-max" },
            "weekdayOnly": false
        }))
        .unwrap();
        assert_eq!(next.title_suffix, "日报");
        assert_eq!(next.vision.api_key, "sk-realkey9999", "掩码不能覆盖真 Key");
        assert_eq!(next.vision.model, "qwen-vl-max");
        assert_eq!(next.user_name, "某某");
        assert!(!next.weekday_only);
        let changed = apply_patch(&s, &json!({ "vision": { "apiKey": " sk-new " } })).unwrap();
        assert_eq!(changed.vision.api_key, "sk-new");
    }

    #[test]
    fn patch_validates_remind_time_and_retention() {
        let s = Settings::default();
        assert!(apply_patch(&s, &json!({ "remindTime": "25:00" })).is_err());
        assert_eq!(apply_patch(&s, &json!({ "remindTime": "17:30" })).unwrap().remind_time, "17:30");
        assert!(apply_patch(&s, &json!({ "shotsRetentionDays": 0 })).is_err());
        assert!(apply_patch(&s, &json!({ "userName": "  " })).is_err());
    }

    #[test]
    fn suggest_tags_parse() {
        assert_eq!(parse_tag_list(r#"["项目甲","项目乙","项目丙"]"#), vec!["项目甲", "项目乙", "项目丙"]);
        assert_eq!(parse_tag_list("```json\n[\"项目丁\", \"会议室。\"]\n```"), vec!["项目丁", "会议室"]);
        assert_eq!(parse_tag_list("项目甲、项目乙，项目丙\n项目丁"), vec!["项目甲", "项目乙", "项目丙", "项目丁"]);
        let many = parse_tag_list(r#"["a1","a2","a3","a4","a5","a6","a7","a8","a9","a1"]"#);
        assert_eq!(many.len(), 8);
    }
}
