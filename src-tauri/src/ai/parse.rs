//! 识图回复解析：JSON 优先，失败时用正则抢救，再失败返回原文。

use crate::format::strip_think;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static ITEM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""item"\s*:\s*"((?:[^"\\]|\\.)*)""#).unwrap());
static TAGS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)"tags"\s*:\s*\[(.*?)\]"#).unwrap());
static QUOTED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""((?:[^"\\]|\\.)*)""#).unwrap());
static CONF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""confidence"\s*:\s*"?([0-9.]+)"#).unwrap());

#[derive(Debug, Clone, PartialEq)]
pub struct VisionReply {
    pub item: String,
    pub tags: Vec<String>,
    pub confidence: f32,
}

fn split_tags(s: &str) -> Vec<String> {
    s.split(['、', ',', '，', '/', '|', ';', '；'])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn from_value(v: &Value) -> Option<VisionReply> {
    let item = v.get("item")?.as_str()?.trim().to_string();
    if item.is_empty() {
        return None;
    }
    let tags = match v.get("tags") {
        Some(Value::Array(a)) => a.iter().filter_map(|t| t.as_str()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
        Some(Value::String(s)) => split_tags(s),
        _ => v.get("project").and_then(|p| p.as_str()).map(split_tags).unwrap_or_default(),
    };
    let confidence = match v.get("confidence") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.5) as f32,
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.5),
        _ => 0.5,
    };
    Some(VisionReply { item, tags, confidence: confidence.clamp(0.0, 1.0) })
}

fn unescape(s: &str) -> String {
    serde_json::from_str::<String>(&format!("\"{s}\"")).unwrap_or_else(|_| s.to_string())
}

/// Err 里是去掉思考段后的原文，用来回退成"原文标黄让用户改"
pub fn parse_vision(raw: &str) -> Result<VisionReply, String> {
    let text = strip_think(raw);
    let text = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if let Some(r) = from_value(&v) {
            return Ok(r);
        }
    }
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if start < end {
            if let Ok(v) = serde_json::from_str::<Value>(&text[start..=end]) {
                if let Some(r) = from_value(&v) {
                    return Ok(r);
                }
            }
        }
    }
    if let Some(c) = ITEM_RE.captures(text) {
        let item = unescape(&c[1]).trim().to_string();
        if !item.is_empty() {
            let tags = TAGS_RE
                .captures(text)
                .map(|t| QUOTED_RE.captures_iter(&t[1]).map(|q| unescape(&q[1])).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            let confidence = CONF_RE.captures(text).and_then(|c| c[1].parse().ok()).unwrap_or(0.5f32);
            return Ok(VisionReply { item, tags, confidence: confidence.clamp(0.0, 1.0) });
        }
    }
    Err(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_json() {
        let r = parse_vision(r#"{"item":"拼多多收入确认填写","tags":["拼多多"],"confidence":0.9}"#).unwrap();
        assert_eq!(r.item, "拼多多收入确认填写");
        assert_eq!(r.tags, vec!["拼多多"]);
        assert!((r.confidence - 0.9).abs() < 1e-6);
    }

    #[test]
    fn fenced_json() {
        let r = parse_vision("```json\n{\"item\":\"百应提现\",\"tags\":[\"百应\"],\"confidence\":0.7}\n```").unwrap();
        assert_eq!(r.item, "百应提现");
    }

    #[test]
    fn think_then_json() {
        let r = parse_vision("<think>看起来是表格</think>{\"item\":\"对账\",\"tags\":[],\"confidence\":0.4}").unwrap();
        assert_eq!(r.item, "对账");
        assert!(r.tags.is_empty());
    }

    #[test]
    fn json_with_chatter_around() {
        let r = parse_vision("好的，结果如下：\n{\"item\":\"团播头像更换\",\"tags\":[\"团播\"],\"confidence\":0.8}\n希望有帮助").unwrap();
        assert_eq!(r.item, "团播头像更换");
    }

    #[test]
    fn tags_as_string() {
        let r = parse_vision(r#"{"item":"沟通锁坏了的事","tags":"直播间、跨部门沟通","confidence":0.8}"#).unwrap();
        assert_eq!(r.tags, vec!["直播间", "跨部门沟通"]);
    }

    #[test]
    fn missing_confidence_defaults_half() {
        let r = parse_vision(r#"{"item":"对账","tags":["百应"]}"#).unwrap();
        assert!((r.confidence - 0.5).abs() < 1e-6);
    }

    #[test]
    fn broken_json_salvaged_by_regex() {
        let r = parse_vision(r#"{"item":"合声计划ppt修改","tags":["合声计划"],"confidence":0.8"#).unwrap();
        assert_eq!(r.item, "合声计划ppt修改");
        assert_eq!(r.tags, vec!["合声计划"]);
    }

    #[test]
    fn legacy_project_field_used_as_tag() {
        let r = parse_vision(r#"{"item":"对账","project":"百应","confidence":0.6}"#).unwrap();
        assert_eq!(r.tags, vec!["百应"]);
    }

    #[test]
    fn single_line_fence() {
        let r = parse_vision("```json {\"item\":\"百应提现\",\"tags\":[\"百应\"],\"confidence\":0.8} ```").unwrap();
        assert_eq!(r.item, "百应提现");
    }

    #[test]
    fn not_json_returns_raw() {
        let e = parse_vision("<think>x</think>我觉得这是在做对账").unwrap_err();
        assert_eq!(e, "我觉得这是在做对账");
    }
}
