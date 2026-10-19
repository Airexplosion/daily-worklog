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
/// 存为当天的总结。生成时间用"开始生成"的时刻：生成过程中识别完的条目会让它显示为过期
pub fn record_generated(data: &mut crate::model::Data, date: &str, text: &str, started_at: &str) {
    set_generated(data.days.entry(date.to_string()).or_default(), text, started_at);
}

pub fn finalize(raw: &str, settings: &Settings, date: NaiveDate, sources: &[String], skipped: usize) -> Result<SummaryResult, String> {
    finalize_for(raw, settings, date, sources, skipped, true)
}

/// is_today 只影响没有记录时的说法
pub fn finalize_for(
    raw: &str,
    settings: &Settings,
    date: NaiveDate,
    sources: &[String],
    skipped: usize,
    is_today: bool,
) -> Result<SummaryResult, String> {
    let title = format::title(settings.user_name.trim(), &settings.title_suffix, date);
    if sources.is_empty() {
        // 有条目但都还没识别出来：生成只有标题的总结会让人以为生成好了
        if skipped > 0 {
            return Err("条目都还没识别出来，先重试或手动写一句".into());
        }
        return Ok(SummaryResult {
            text: title,
            checks: Vec::new(),
            removed: Vec::new(),
            skipped_pending: 0,
            notice: Some(if is_today { "今天还没记录" } else { "这天没有记录" }.into()),
        });
    }
    let cleaned = format::clean_summary(raw);
    // 有记录却没整理出任何一条：多半是 AI 中途断了或只输出了思考，别当成功保存
    if cleaned.lines.is_empty() {
        return Err("AI 没有返回内容，再生成一次试试".into());
    }
    Ok(SummaryResult {
        text: format::compose(&title, &cleaned.lines),
        checks: format::check(&cleaned.lines, sources, &settings.banned_words),
        removed: cleaned.removed,
        skipped_pending: skipped,
        notice: None,
    })
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
        let r = finalize("张三10.15工作梳理\n1. **拼多多收入确认填写**\n2. 百应提现，计算。", &settings(), date(), &sources, 1).unwrap();
        assert_eq!(r.text, "张三10.15工作梳理\r\n1.拼多多收入确认填写\r\n2.百应提现，计算。");
        assert_eq!(r.removed, vec!["张三10.15工作梳理"]);
        assert_eq!(r.skipped_pending, 1);
        assert!(r.notice.is_none());
    }

    #[test]
    fn empty_reply_with_sources_is_error() {
        let sources = vec!["拼多多收入确认填写".to_string()];
        assert!(finalize("", &settings(), date(), &sources, 0).is_err());
        assert!(finalize("<think>想了想</think>", &settings(), date(), &sources, 0).is_err());
    }

    #[test]
    fn item_finished_during_generation_marks_stale() {
        let mut data = crate::model::Data::default();
        // 开始生成时（18:00:00）读了当天；生成期间（18:00:05）有条目识别完
        data.days.insert("2026-10-15".into(), Day { items_changed_at: Some("2026-10-15T18:00:05+08:00".into()), ..Default::default() });
        record_generated(&mut data, "2026-10-15", "张三10.15工作梳理", "2026-10-15T18:00:00+08:00");
        assert!(crate::views::is_stale(&data.days["2026-10-15"]));
    }

    #[test]
    fn zero_items_title_only() {
        let r = finalize("", &settings(), date(), &[], 0).unwrap();
        assert_eq!(r.text, "张三10.15工作梳理");
        assert_eq!(r.notice.as_deref(), Some("今天还没记录"));
    }

    #[test]
    fn notice_wording_by_day() {
        let past = finalize_for("", &settings(), date(), &[], 0, false).unwrap();
        assert_eq!(past.notice.as_deref(), Some("这天没有记录"));
        let today = finalize_for("", &settings(), date(), &[], 0, true).unwrap();
        assert_eq!(today.notice.as_deref(), Some("今天还没记录"));
        // 有条目但都没识别出来：不能生成只有标题的总结
        assert!(finalize_for("", &settings(), date(), &[], 2, true).is_err());
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
