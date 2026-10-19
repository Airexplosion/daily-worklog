//! 生成总结：取当天已完成的条目 → 汇总模型 → 代码清洗、拼标题、检查。

use crate::ai::prompt::summary_prompt;
use crate::format::{self, Check};
use crate::model::{Day, ItemStatus, Settings, Summary};
use crate::recognize::PLACEHOLDER;
use chrono::NaiveDate;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryResult {
    pub text: String,
    pub checks: Vec<Check>,
    /// 代码删掉的多余内容（模型自己写的标题、前言、总结语）
    pub removed: Vec<String>,
    /// 没参与汇总的条目数（待处理、识别中、待补充）
    pub skipped_pending: usize,
    pub notice: Option<String>,
}

/// 参与汇总的条目文字（按记录顺序），以及没参与的条数
pub fn sources(day: &Day) -> (Vec<String>, usize) {
    let mut out = Vec::new();
    let mut skipped = 0;
    for item in &day.items {
        let text = item.text.trim();
        if item.status == ItemStatus::Done && !text.is_empty() && text != PLACEHOLDER {
            out.push(text.to_string());
        } else {
            skipped += 1;
        }
    }
    (out, skipped)
}

pub fn build_messages(settings: &Settings, sources: &[String]) -> Value {
    let list: Vec<String> = sources.iter().enumerate().map(|(i, s)| format!("{}.{}", i + 1, s)).collect();
    json!([
        { "role": "system", "content": summary_prompt(settings) },
        { "role": "user", "content": format!("当天记录的工作事项（按记录顺序）：\n{}", list.join("\n")) }
    ])
}

/// 清洗模型输出、拼上代码生成的标题、做检查
pub fn finalize(raw: &str, settings: &Settings, date: NaiveDate, sources: &[String], skipped: usize) -> SummaryResult {
    let title = format::title(settings.user_name.trim(), &settings.title_suffix, date);
    if sources.is_empty() {
        return SummaryResult {
            text: title,
            checks: Vec::new(),
            removed: Vec::new(),
            skipped_pending: skipped,
            notice: Some("今天还没记录".into()),
        };
    }
    let cleaned = format::clean_summary(raw);
    SummaryResult {
        text: format::compose(&title, &cleaned.lines),
        checks: format::check(&cleaned.lines, sources, &settings.banned_words),
        removed: cleaned.removed,
        skipped_pending: skipped,
        notice: None,
    }
}

/// 存为新生成的总结；原来那版留着，供"撤销重新生成"
pub fn set_generated(day: &mut Day, text: &str, now: &str) {
    let previous = day.summary.as_ref().map(|s| s.text.clone());
    day.summary = Some(Summary { text: text.to_string(), generated_at: now.to_string(), edited_at: None, previous_text: previous });
}

pub fn save_edit(day: &mut Day, text: &str, now: &str) {
    if let Some(s) = day.summary.as_mut() {
        s.text = text.to_string();
        s.edited_at = Some(now.to_string());
    }
}

/// 回到重新生成之前的那版；没有可撤销的返回 false
pub fn undo_regenerate(day: &mut Day) -> bool {
    let Some(s) = day.summary.as_mut() else { return false };
    let Some(prev) = s.previous_text.take() else { return false };
    s.text = prev;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Day, Item, ItemStatus, Settings, Source, Summary};
    use chrono::NaiveDate;

    fn item(text: &str, status: ItemStatus) -> Item {
        Item {
            id: text.into(),
            created_at: String::new(),
            text: text.into(),
            note: String::new(),
            source: Source::Ai,
            edited: false,
            tags: vec![],
            confidence: None,
            shots: vec![],
            status,
            error: None,
        }
    }

    fn settings() -> Settings {
        let mut s = Settings::default();
        s.user_name = "张三".into();
        s
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
    }

    #[test]
    fn summary_input_excludes_pending() {
        let day = Day {
            items: vec![
                item("拼多多收入确认填写", ItemStatus::Done),
                item("", ItemStatus::Pending),
                item("（待补充）", ItemStatus::Done),
                item("百应提现，计算。", ItemStatus::Done),
                item("识别中", ItemStatus::Recognizing),
            ],
            ..Default::default()
        };
        let (sources, skipped) = sources(&day);
        assert_eq!(sources, vec!["拼多多收入确认填写", "百应提现，计算。"]);
        assert_eq!(skipped, 3);
    }

    #[test]
    fn messages_list_items_in_order() {
        let m = build_messages(&settings(), &["甲事情".to_string(), "乙事情".to_string()]);
        assert!(m[0]["content"].as_str().unwrap().contains("你是张三的助手"));
        assert_eq!(m[1]["content"], "当天记录的工作事项（按记录顺序）：\n1.甲事情\n2.乙事情");
    }

    #[test]
    fn finalize_adds_title_and_cleans() {
        let sources = vec!["拼多多收入确认填写".to_string(), "百应提现，计算。".to_string()];
        let r = finalize("张三10.15工作梳理\n1. **拼多多收入确认填写**\n2. 百应提现，计算。", &settings(), date(), &sources, 1);
        assert_eq!(r.text, "张三10.15工作梳理\r\n1.拼多多收入确认填写\r\n2.百应提现，计算。");
        assert_eq!(r.removed, vec!["张三10.15工作梳理"]);
        assert_eq!(r.skipped_pending, 1);
        assert!(r.notice.is_none());
    }

    #[test]
    fn zero_items_title_only() {
        let r = finalize("", &settings(), date(), &[], 0);
        assert_eq!(r.text, "张三10.15工作梳理");
        assert_eq!(r.notice.as_deref(), Some("今天还没记录"));
    }

    #[test]
    fn regenerate_keeps_previous_for_undo() {
        let mut day = Day::default();
        set_generated(&mut day, "第一版", "2026-10-15T18:00:00+08:00");
        save_edit(&mut day, "第一版（手改）", "2026-10-15T18:01:00+08:00");
        set_generated(&mut day, "第二版", "2026-10-15T18:02:00+08:00");
        assert_eq!(day.summary.as_ref().unwrap().text, "第二版");
        assert!(undo_regenerate(&mut day));
        let s: &Summary = day.summary.as_ref().unwrap();
        assert_eq!(s.text, "第一版（手改）");
        assert_eq!(s.previous_text, None);
        assert!(!undo_regenerate(&mut day));
    }
}
