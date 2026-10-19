//! 给界面用的数据整理：日期标题、总结是否过期、标签"新"标记。

use crate::model::{Day, Item, Summary, TagDef, TagOrigin};
use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, Weekday};
use serde::Serialize;

pub fn weekday_cn(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}

/// "10月15日 周四"
pub fn day_label(date: NaiveDate) -> String {
    format!("{}月{}日 {}", date.month(), date.day(), weekday_cn(date.weekday()))
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn parse_time(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s).ok()
}

/// 生成总结之后又改过条目
pub fn is_stale(day: &Day) -> bool {
    let (Some(summary), Some(changed)) = (&day.summary, &day.items_changed_at) else { return false };
    match (parse_time(changed), parse_time(&summary.generated_at)) {
        (Some(c), Some(g)) => c > g,
        _ => false,
    }
}

/// AI 新建、还没处理、而且是今天建的
pub fn is_new_tag(tag: &TagDef, today: NaiveDate) -> bool {
    tag.pending && parse_time(&tag.created_at).is_some_and(|t| t.date_naive() == today)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayView {
    pub date: String,
    pub label: String,
    pub is_today: bool,
    pub items: Vec<Item>,
    pub summary: Option<Summary>,
    pub stale: bool,
    pub muted: bool,
    pub remind_time: String,
    pub hotkey: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagView {
    pub name: String,
    pub by_ai: bool,
    pub is_new: bool,
}

pub fn tag_views(tags: &[TagDef], today: NaiveDate) -> Vec<TagView> {
    tags.iter()
        .map(|t| TagView { name: t.name.clone(), by_ai: t.created_by == TagOrigin::Ai, is_new: is_new_tag(t, today) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Day, Summary, TagDef, TagOrigin};
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn day_label_format() {
        assert_eq!(day_label(d("2026-10-15")), "10月15日 周四");
        assert_eq!(day_label(d("2026-11-03")), "11月3日 周二");
        assert_eq!(day_label(d("2026-10-18")), "10月18日 周日");
    }

    #[test]
    fn stale_when_items_changed_after_generation() {
        let mut day = Day::default();
        assert!(!is_stale(&day));
        day.summary = Some(Summary { text: "t".into(), generated_at: "2026-10-15T18:05:00+08:00".into(), ..Default::default() });
        day.items_changed_at = Some("2026-10-15T18:00:00+08:00".into());
        assert!(!is_stale(&day));
        day.items_changed_at = Some("2026-10-15T18:06:00+08:00".into());
        assert!(is_stale(&day));
        // 时区不同也按真实时间比较
        day.items_changed_at = Some("2026-10-15T10:04:00+00:00".into());
        assert!(!is_stale(&day));
    }

    #[test]
    fn new_tag_only_while_pending_and_created_today() {
        let t = |pending, created: &str| TagDef { name: "会议".into(), created_by: TagOrigin::Ai, created_at: created.into(), pending };
        let today = d("2026-10-15");
        assert!(is_new_tag(&t(true, "2026-10-15T10:00:00+08:00"), today));
        assert!(!is_new_tag(&t(false, "2026-10-15T10:00:00+08:00"), today));
        assert!(!is_new_tag(&t(true, "2026-10-14T10:00:00+08:00"), today));
    }
}
