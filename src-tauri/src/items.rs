//! 条目操作：手动添加、修改、删除（可撤销）。不依赖界面，便于测试。

use crate::model::{Data, Item, ItemStatus, Source};
use crate::tags::{normalize, MAX_TAGS_PER_ITEM};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 删除后可撤销的时间
pub const UNDO_WINDOW: Duration = Duration::from_secs(5);

pub fn add_manual(data: &mut Data, date: &str, text: &str, id: &str, now: &str) -> Item {
    let item = Item {
        id: id.to_string(),
        created_at: now.to_string(),
        text: text.trim().to_string(),
        note: String::new(),
        source: Source::Manual,
        edited: false,
        tags: Vec::new(),
        confidence: None,
        shots: Vec::new(),
        status: ItemStatus::Done,
        error: None,
    };
    let day = data.days.entry(date.to_string()).or_default();
    day.items.push(item.clone());
    day.items_changed_at = Some(now.to_string());
    item
}

/// 改文字和/或标签，返回条目所在日期；找不到返回 None
pub fn update(data: &mut Data, id: &str, text: Option<&str>, tags: Option<&[String]>, now: &str) -> Option<String> {
    let (date, day) = data.days.iter_mut().find(|(_, d)| d.items.iter().any(|i| i.id == id))?;
    let item = day.items.iter_mut().find(|i| i.id == id)?;
    let mut changed = false;
    if let Some(text) = text.map(str::trim) {
        if text != item.text {
            item.text = text.to_string();
            item.edited = true;
            changed = true;
            // 待处理的条目手写了内容，就算处理完了
            if item.status != ItemStatus::Done && !text.is_empty() {
                item.status = ItemStatus::Done;
                item.source = Source::Manual;
                item.error = None;
            }
        }
    }
    if let Some(tags) = tags {
        let mut next: Vec<String> = Vec::new();
        for t in tags.iter().filter_map(|t| normalize(t)) {
            if !next.contains(&t) && next.len() < MAX_TAGS_PER_ITEM {
                next.push(t);
            }
        }
        if next != item.tags {
            item.tags = next;
            changed = true;
        }
    }
    if changed {
        day.items_changed_at = Some(now.to_string());
    }
    Some(date.clone())
}

/// 删掉的条目和它原来的位置，用来撤销
#[derive(Debug, Clone)]
pub struct Removed {
    pub date: String,
    pub index: usize,
    pub item: Item,
}

pub fn delete(data: &mut Data, id: &str, now: &str) -> Option<Removed> {
    let (date, day) = data.days.iter_mut().find(|(_, d)| d.items.iter().any(|i| i.id == id))?;
    let index = day.items.iter().position(|i| i.id == id)?;
    let item = day.items.remove(index);
    day.items_changed_at = Some(now.to_string());
    Some(Removed { date: date.clone(), index, item })
}

pub fn undo_delete(data: &mut Data, removed: Removed, now: &str) {
    let day = data.days.entry(removed.date).or_default();
    let index = removed.index.min(day.items.len());
    let mut item = removed.item;
    // 删除时还在识别：那次识别已经作废，转成待处理，用户可以点重试
    if item.status == ItemStatus::Recognizing {
        item.status = ItemStatus::Pending;
        item.error = Some("删除时还没识别完，点重试再识别一次".into());
    }
    day.items.insert(index, item);
    day.items_changed_at = Some(now.to_string());
}

/// 删除后 5 秒内可撤销；过期的交给调用方删截图文件
#[derive(Default)]
pub struct Trash {
    entries: HashMap<String, (Removed, Instant)>,
    seq: u64,
}

impl Trash {
    pub fn put(&mut self, removed: Removed, now: Instant) -> String {
        self.seq += 1;
        let token = format!("{}-{}", removed.item.id, self.seq);
        self.entries.insert(token.clone(), (removed, now));
        token
    }

    pub fn take(&mut self, token: &str) -> Option<Removed> {
        self.entries.remove(token).map(|(r, _)| r)
    }

    /// 取出并移除所有已过撤销期的
    pub fn expired(&mut self, now: Instant) -> Vec<Removed> {
        let keys: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, (_, at))| now.duration_since(*at) > UNDO_WINDOW)
            .map(|(k, _)| k.clone())
            .collect();
        keys.into_iter().filter_map(|k| self.take(&k)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Data, Day, Item, ItemStatus, Source};
    use std::time::{Duration, Instant};

    fn item(id: &str, status: ItemStatus) -> Item {
        Item {
            id: id.into(),
            created_at: "2026-10-15T10:00:00+08:00".into(),
            text: format!("事项{id}"),
            note: String::new(),
            source: Source::Ai,
            edited: false,
            tags: vec![],
            confidence: Some(0.9),
            shots: vec![format!("shots/{id}.png")],
            status,
            error: None,
        }
    }

    fn data() -> Data {
        let mut d = Data::default();
        d.days.insert(
            "2026-10-15".into(),
            Day { items: vec![item("a", ItemStatus::Done), item("b", ItemStatus::Done), item("c", ItemStatus::Done)], ..Default::default() },
        );
        d
    }

    #[test]
    fn add_manual_item_appends() {
        let mut d = data();
        let it = add_manual(&mut d, "2026-10-15", "  手动补一条工作  ", "id-x", "2026-10-15T20:00:00+08:00");
        assert_eq!(it.source, Source::Manual);
        assert_eq!(it.status, ItemStatus::Done);
        assert_eq!(it.text, "手动补一条工作");
        let day = &d.days["2026-10-15"];
        assert_eq!(day.items.last().unwrap().id, "id-x");
        assert!(day.items_changed_at.is_some());
    }

    #[test]
    fn update_text_marks_edited_and_resolves_pending() {
        let mut d = data();
        d.days.get_mut("2026-10-15").unwrap().items[1].status = ItemStatus::Pending;
        d.days.get_mut("2026-10-15").unwrap().items[1].error = Some("没联网".into());
        let date = update(&mut d, "b", Some("手写的这一条"), None, "t").unwrap();
        assert_eq!(date, "2026-10-15");
        let it = &d.days["2026-10-15"].items[1];
        assert!(it.edited);
        assert_eq!(it.status, ItemStatus::Done);
        assert_eq!(it.source, Source::Manual);
        assert_eq!(it.error, None);
        assert_eq!(d.days["2026-10-15"].items_changed_at.as_deref(), Some("t"));
    }

    #[test]
    fn update_same_text_is_not_an_edit() {
        let mut d = data();
        update(&mut d, "a", Some("事项a"), None, "t").unwrap();
        assert!(!d.days["2026-10-15"].items[0].edited);
        assert_eq!(d.days["2026-10-15"].items_changed_at, None);
    }

    #[test]
    fn update_tags_caps_three_and_dedupes() {
        let mut d = data();
        update(&mut d, "a", None, Some(&["甲".into(), "乙".into(), "甲".into(), "丙".into(), "丁".into()]), "t").unwrap();
        assert_eq!(d.days["2026-10-15"].items[0].tags, vec!["甲", "乙", "丙"]);
        assert!(update(&mut d, "nope", Some("x"), None, "t").is_none());
    }

    #[test]
    fn delete_then_undo_restores_position() {
        let mut d = data();
        let removed = delete(&mut d, "b", "t1").unwrap();
        assert_eq!(d.days["2026-10-15"].items.len(), 2);
        undo_delete(&mut d, removed, "t2");
        let ids: Vec<&str> = d.days["2026-10-15"].items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn delete_finalizes_after_window_removes_shot() {
        let mut d = data();
        let mut trash = Trash::default();
        let t0 = Instant::now();
        let token = trash.put(delete(&mut d, "a", "t").unwrap(), t0);
        assert!(trash.expired(t0 + Duration::from_secs(4)).is_empty());
        let gone = trash.expired(t0 + Duration::from_secs(6));
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].item.shots, vec!["shots/a.png"]);
        assert!(trash.take(&token).is_none(), "过期后不能再撤销");
    }

    #[test]
    fn undo_of_recognizing_item_becomes_pending() {
        let mut d = data();
        d.days.get_mut("2026-10-15").unwrap().items[0].status = ItemStatus::Recognizing;
        let removed = delete(&mut d, "a", "t").unwrap();
        undo_delete(&mut d, removed, "t");
        let it = &d.days["2026-10-15"].items[0];
        assert_eq!(it.status, ItemStatus::Pending);
        assert!(it.error.is_some());
    }

    #[test]
    fn undo_within_window() {
        let mut d = data();
        let mut trash = Trash::default();
        let token = trash.put(delete(&mut d, "c", "t").unwrap(), Instant::now());
        let back = trash.take(&token).unwrap();
        undo_delete(&mut d, back, "t");
        assert_eq!(d.days["2026-10-15"].items.len(), 3);
    }

    #[test]
    fn delete_and_undo_keep_every_shot_of_a_multi_image_item() {
        let mut d = data();
        let shots: Vec<String> = (1..=3).map(|i| format!("shots/m{i}.png")).collect();
        d.days.get_mut("2026-10-15").unwrap().items[1].shots = shots.clone();
        let removed = delete(&mut d, "b", "t1").unwrap();
        // 撤销期过后后台按 removed.item.shots 删文件：三张都要在里面
        assert_eq!(removed.item.shots, shots);
        undo_delete(&mut d, removed, "t2");
        assert_eq!(d.days["2026-10-15"].items[1].shots, shots);
    }
}
