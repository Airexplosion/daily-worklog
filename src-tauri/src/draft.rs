//! 还没记完的这一条：截好或加好的图、已打的字，记下之前随时存成 draft.json。

use crate::model::{Item, ItemStatus, Source};
use serde::{Deserialize, Serialize};

/// 一条最多几张图
pub const MAX_SHOTS: usize = 9;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// 第一张图的 shot_id，也是将来条目的 id
    pub id: String,
    /// YYYY-MM-DD，第一张图截下（或加入）的那天
    pub date: String,
    pub created_at: String,
    /// 相对数据目录的路径，按顺序
    pub shots: Vec<String>,
    #[serde(default)]
    pub note: String,
    /// 补一句小窗和小条放在哪块屏 (x, y, 宽, 高, 缩放)
    pub monitor: (i32, i32, u32, u32, f64),
}

impl Draft {
    pub fn new(id: &str, date: &str, created_at: &str, monitor: (i32, i32, u32, u32, f64)) -> Draft {
        Draft {
            id: id.to_string(),
            date: date.to_string(),
            created_at: created_at.to_string(),
            shots: Vec::new(),
            note: String::new(),
            monitor,
        }
    }

    /// 按顺序加，满 9 张为止；返回实际加了几张（多出来的由调用方删文件、出提示）
    pub fn add_shots(&mut self, rels: &[String]) -> usize {
        let room = MAX_SHOTS.saturating_sub(self.shots.len());
        let n = rels.len().min(room);
        self.shots.extend_from_slice(&rels[..n]);
        n
    }

    /// 去掉第 index 张；只剩一张或越界时不动
    pub fn remove_shot(&mut self, index: usize) -> Option<String> {
        if self.shots.len() <= 1 || index >= self.shots.len() {
            return None;
        }
        Some(self.shots.remove(index))
    }

    pub fn is_full(&self) -> bool {
        self.shots.len() >= MAX_SHOTS
    }

    /// 记成条目：识别中，文字等 AI 写
    pub fn into_item(self) -> Item {
        Item {
            id: self.id,
            created_at: self.created_at,
            text: String::new(),
            note: self.note.trim().to_string(),
            source: Source::Ai,
            edited: false,
            tags: Vec::new(),
            confidence: None,
            shots: self.shots,
            status: ItemStatus::Recognizing,
            error: None,
        }
    }
}

/// 加图（要读文件、解码，可能好几秒）结束时，这些图该去哪儿
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportTarget {
    /// 新建一条
    New,
    /// 加进现在这条
    Existing,
    /// 开始加图时的那条已经记下或不记了：这些图不加
    Gone,
}

/// started = 开始加图时的草稿 id，now = 加完时的草稿 id
pub fn import_target(started: Option<&str>, now: Option<&str>) -> ImportTarget {
    match (started, now) {
        (None, None) => ImportTarget::New,
        (None, Some(_)) => ImportTarget::Existing,
        (Some(a), Some(b)) if a == b => ImportTarget::Existing,
        (Some(_), _) => ImportTarget::Gone,
    }
}

/// 启动恢复：去掉已经不存在的图；一张都不剩就不记
pub fn recover(mut draft: Draft, exists: impl Fn(&str) -> bool) -> Option<Item> {
    draft.shots.retain(|rel| exists(rel));
    if draft.shots.is_empty() {
        return None;
    }
    Some(draft.into_item())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ItemStatus, Source};

    const SCREEN: (i32, i32, u32, u32, f64) = (0, 0, 1920, 1080, 1.0);

    fn rels(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| format!("shots/{n}.png")).collect()
    }

    fn draft_with(names: &[&str]) -> Draft {
        let mut d = Draft::new("first", "2026-10-19", "2026-10-19T10:30:12+08:00", SCREEN);
        d.add_shots(&rels(names));
        d
    }

    #[test]
    fn add_shots_caps_at_nine() {
        let mut d = draft_with(&["a"]);
        let more: Vec<String> = (0..10).map(|i| format!("shots/n{i}.png")).collect();
        assert_eq!(d.add_shots(&more), 8);
        assert_eq!(d.shots.len(), MAX_SHOTS);
        assert_eq!(d.shots[0], "shots/a.png");
        assert_eq!(d.shots[1..], more[..8]);
        assert!(d.is_full());
    }

    #[test]
    fn remove_shot_keeps_last_one() {
        let mut one = draft_with(&["a"]);
        assert_eq!(one.remove_shot(0), None);
        assert_eq!(one.shots.len(), 1);
        let mut three = draft_with(&["a", "b", "c"]);
        assert_eq!(three.remove_shot(1), Some("shots/b.png".to_string()));
        assert_eq!(three.shots, rels(&["a", "c"]));
        assert_eq!(three.remove_shot(5), None);
        assert_eq!(three.shots.len(), 2);
    }

    #[test]
    fn into_item_is_recognizing_with_all_shots() {
        let mut d = draft_with(&["a", "b"]);
        d.note = "  补的一句  ".into();
        let item = d.into_item();
        assert_eq!(item.id, "first");
        assert_eq!(item.created_at, "2026-10-19T10:30:12+08:00");
        assert_eq!(item.note, "补的一句");
        assert_eq!(item.shots, rels(&["a", "b"]));
        assert_eq!(item.status, ItemStatus::Recognizing);
        assert_eq!(item.source, Source::Ai);
        assert_eq!(item.text, "");
        assert!(item.tags.is_empty());
    }

    #[test]
    fn recover_drops_missing_files() {
        let d = draft_with(&["a", "b", "c"]);
        let item = recover(d.clone(), |rel| rel != "shots/b.png").unwrap();
        assert_eq!(item.shots, rels(&["a", "c"]));
        assert_eq!(recover(d, |_| false), None);
    }

    #[test]
    fn import_target_follows_the_draft_seen_at_start() {
        assert_eq!(import_target(None, None), ImportTarget::New);
        assert_eq!(import_target(None, Some("b")), ImportTarget::Existing);
        assert_eq!(import_target(Some("a"), Some("a")), ImportTarget::Existing);
        // 加图途中那一条已经记下或不记了：这些图不能变成新的一条
        assert_eq!(import_target(Some("a"), None), ImportTarget::Gone);
        assert_eq!(import_target(Some("a"), Some("b")), ImportTarget::Gone);
    }
}
