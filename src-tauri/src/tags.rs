//! 标签：名字归一、AI 返回的标签并入、改名/合并、删除、统计。

use crate::model::{Data, Settings};
use std::collections::BTreeMap;

pub const OTHER: &str = "其他";
pub const MAX_TAG_CHARS: usize = 6;
pub const MAX_TAGS_PER_ITEM: usize = 3;

fn to_halfwidth(c: char) -> char {
    match c {
        '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

fn is_trim_char(c: char) -> bool {
    c.is_whitespace() || c.is_ascii_punctuation() || "。，、！？；：“”‘’「」『』（）【】《》〈〉…·—".contains(c)
}

/// 去首尾空白与标点、全角转半角、最多 6 个字；没剩下东西返回 None
pub fn normalize(s: &str) -> Option<String> {
    let half: String = s.chars().map(to_halfwidth).collect();
    let trimmed = half.trim_matches(is_trim_char);
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_TAG_CHARS).collect())
}

/// 比较用的键：归一后转小写
fn key(s: &str) -> Option<String> {
    normalize(s).map(|n| n.to_lowercase())
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiTags {
    pub item_tags: Vec<String>,
    /// 本次新建、需要加入标签列表的
    pub created: Vec<String>,
}

pub fn apply_ai(existing: &[crate::model::TagDef], returned: &[String], allow_new: bool) -> AiTags {
    let mut item_tags: Vec<String> = Vec::new();
    let mut created = Vec::new();
    for raw in returned {
        let Some(norm) = normalize(raw) else { continue };
        let k = norm.to_lowercase();
        let name = match existing.iter().find(|t| key(&t.name).as_deref() == Some(k.as_str())) {
            Some(t) => t.name.clone(),
            None if k == OTHER => OTHER.to_string(),
            None if allow_new => {
                if !created.contains(&norm) {
                    created.push(norm.clone());
                }
                norm
            }
            None => OTHER.to_string(),
        };
        if !item_tags.contains(&name) {
            item_tags.push(name);
        }
        if item_tags.len() == MAX_TAGS_PER_ITEM {
            break;
        }
    }
    created.retain(|c| item_tags.contains(c));
    if item_tags.is_empty() {
        item_tags.push(OTHER.to_string());
    }
    AiTags { item_tags, created }
}

/// 改名；新名字已存在时即合并。所有条目同步更新
pub fn rename(data: &mut Data, settings: &mut Settings, from: &str, to: &str) {
    let Some(to) = normalize(to) else { return };
    if from == to {
        return;
    }
    let to_key = to.to_lowercase();
    let target = settings
        .tags
        .iter()
        .find(|t| t.name != from && key(&t.name).as_deref() == Some(to_key.as_str()))
        .map(|t| t.name.clone());
    match target {
        Some(existing) => {
            settings.tags.retain(|t| t.name != from);
            replace_in_items(data, from, &existing);
        }
        None => {
            for t in settings.tags.iter_mut().filter(|t| t.name == from) {
                t.name = to.clone();
            }
            replace_in_items(data, from, &to);
        }
    }
}

fn replace_in_items(data: &mut Data, from: &str, to: &str) {
    for item in data.days.values_mut().flat_map(|d| d.items.iter_mut()) {
        if item.tags.iter().any(|t| t == from) {
            let mut next: Vec<String> = Vec::new();
            for t in &item.tags {
                let t = if t == from { to.to_string() } else { t.clone() };
                if !next.contains(&t) {
                    next.push(t);
                }
            }
            item.tags = next;
        }
    }
}

/// 从标签列表和所有条目上拿掉；条目一个标签都不剩时补"其他"
pub fn delete(data: &mut Data, settings: &mut Settings, name: &str) {
    settings.tags.retain(|t| t.name != name);
    for item in data.days.values_mut().flat_map(|d| d.items.iter_mut()) {
        if item.tags.iter().any(|t| t == name) {
            item.tags.retain(|t| t != name);
            if item.tags.is_empty() {
                item.tags.push(OTHER.to_string());
            }
        }
    }
}

/// "保留"：AI 新建的标签转为正式标签
pub fn confirm(settings: &mut Settings, name: &str) {
    for t in settings.tags.iter_mut().filter(|t| t.name == name) {
        t.pending = false;
    }
}

pub fn usage(data: &Data) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for t in data.days.values().flat_map(|d| d.items.iter()).flat_map(|i| i.tags.iter()) {
        *out.entry(t.clone()).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Data, Day, Item, ItemStatus, Settings, Source, TagDef, TagOrigin};

    fn def(name: &str) -> TagDef {
        TagDef { name: name.into(), created_by: TagOrigin::User, created_at: String::new(), pending: false }
    }

    fn item(id: &str, tags: &[&str]) -> Item {
        Item {
            id: id.into(),
            created_at: "2026-10-15T10:00:00+08:00".into(),
            text: "x".into(),
            note: String::new(),
            source: Source::Ai,
            edited: false,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            confidence: None,
            shots: vec![],
            status: ItemStatus::Done,
            error: None,
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn normalize_variants() {
        assert_eq!(normalize("直播间 ").as_deref(), Some("直播间"));
        assert_eq!(normalize("直播间。").as_deref(), Some("直播间"));
        assert_eq!(normalize("「直播间」").as_deref(), Some("直播间"));
        assert_eq!(normalize("ＡＢ"), normalize("AB"));
        assert_eq!(normalize("跨部门沟通协调事项").as_deref(), Some("跨部门沟通协"));
        assert_eq!(normalize(" 、。 "), None);
    }

    #[test]
    fn apply_ai_prefers_existing() {
        let existing = vec![def("直播间"), def("AB")];
        let r = apply_ai(&existing, &s(&["直播间。", "ab", "会议"]), true);
        assert_eq!(r.item_tags, s(&["直播间", "AB", "会议"]));
        assert_eq!(r.created, s(&["会议"]));
    }

    #[test]
    fn apply_ai_disallow_maps_to_other() {
        let existing = vec![def("直播间")];
        let r = apply_ai(&existing, &s(&["会议", "直播间"]), false);
        assert_eq!(r.item_tags, s(&["其他", "直播间"]));
        assert!(r.created.is_empty());
        let empty = apply_ai(&existing, &[], true);
        assert_eq!(empty.item_tags, s(&["其他"]));
    }

    #[test]
    fn apply_ai_caps_three_and_dedupes() {
        let r = apply_ai(&[], &s(&["甲", "乙", "甲", "丙", "丁"]), true);
        assert_eq!(r.item_tags, s(&["甲", "乙", "丙"]));
    }

    fn data_with(items: Vec<Item>) -> Data {
        let mut d = Data::default();
        d.days.insert("2026-10-15".into(), Day { items, ..Default::default() });
        d
    }

    #[test]
    fn rename_merges_into_existing_and_updates_items() {
        let mut data = data_with(vec![item("1", &["直播间维护", "直播间"]), item("2", &["直播间维护"])]);
        let mut st = Settings::default();
        st.tags = vec![def("直播间"), def("直播间维护")];
        rename(&mut data, &mut st, "直播间维护", "直播间");
        assert_eq!(st.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["直播间"]);
        let items = &data.days["2026-10-15"].items;
        assert_eq!(items[0].tags, s(&["直播间"]));
        assert_eq!(items[1].tags, s(&["直播间"]));
    }

    #[test]
    fn rename_to_new_name() {
        let mut data = data_with(vec![item("1", &["会议"])]);
        let mut st = Settings::default();
        st.tags = vec![def("会议")];
        rename(&mut data, &mut st, "会议", "例会");
        assert_eq!(st.tags[0].name, "例会");
        assert_eq!(data.days["2026-10-15"].items[0].tags, s(&["例会"]));
    }

    #[test]
    fn rename_case_variant_renames_in_place() {
        let mut data = data_with(vec![item("1", &["ppt"])]);
        let mut st = Settings::default();
        st.tags = vec![def("ppt"), def("百应")];
        rename(&mut data, &mut st, "ppt", "PPT");
        assert_eq!(st.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["PPT", "百应"]);
        assert_eq!(data.days["2026-10-15"].items[0].tags, s(&["PPT"]));
    }

    #[test]
    fn delete_leaves_other_when_empty() {
        let mut data = data_with(vec![item("1", &["会议"]), item("2", &["会议", "百应"])]);
        let mut st = Settings::default();
        st.tags = vec![def("会议"), def("百应")];
        delete(&mut data, &mut st, "会议");
        assert_eq!(st.tags.len(), 1);
        let items = &data.days["2026-10-15"].items;
        assert_eq!(items[0].tags, s(&["其他"]));
        assert_eq!(items[1].tags, s(&["百应"]));
    }

    #[test]
    fn confirm_clears_pending() {
        let mut st = Settings::default();
        st.tags = vec![TagDef { name: "会议".into(), created_by: TagOrigin::Ai, created_at: String::new(), pending: true }];
        confirm(&mut st, "会议");
        assert!(!st.tags[0].pending);
    }

    #[test]
    fn usage_counts() {
        let data = data_with(vec![item("1", &["会议", "百应"]), item("2", &["百应"])]);
        let u = usage(&data);
        assert_eq!(u["百应"], 2);
        assert_eq!(u["会议"], 1);
    }
}
