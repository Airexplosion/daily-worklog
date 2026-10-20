//! 后台识别队列：截图条目先落盘，再排队调用识图模型。

use crate::ai::client::{self, AiError};
use crate::ai::parse::parse_vision;
use crate::ai::prompt::vision_prompt;
use crate::capture;
use crate::format::clean_item;
use crate::model::{now_iso, ItemStatus, TagDef, TagOrigin};
use crate::state::AppState;
use crate::tags;
use crate::windows::{self, ToastPayload};
use base64::Engine;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

pub const VISION_TIMEOUT: Duration = Duration::from_secs(20);
pub const PLACEHOLDER: &str = "（待补充）";

#[derive(Debug, Clone)]
pub struct Job {
    pub date: String,
    pub id: String,
}

pub fn build_messages(system: &str, note: &str, image_jpeg: Option<&[u8]>) -> Value {
    let note = note.trim();
    let text = format!("补充说明：{}", if note.is_empty() { "（无补充说明）" } else { note });
    let user = match image_jpeg {
        Some(bytes) => {
            let url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
            json!([{ "type": "image_url", "image_url": { "url": url } }, { "type": "text", "text": text }])
        }
        None => Value::String(text),
    };
    json!([{ "role": "system", "content": system }, { "role": "user", "content": user }])
}

/// 网络问题、超时、限流、服务端错误重试一次；配置错误不重试
pub fn should_retry(e: &AiError) -> bool {
    match e {
        AiError::Timeout | AiError::Network(_) => true,
        AiError::Http(code, _) => *code == 429 || *code >= 500,
        _ => false,
    }
}

/// 识别结果写进条目。只在还"识别中"时写：期间用户已手写完成的不覆盖
pub fn apply_to_item(item: &mut crate::model::Item, text: &str, tags: &[String], confidence: f32) -> bool {
    if item.status != ItemStatus::Recognizing {
        return false;
    }
    if !item.edited {
        item.text = text.to_string();
    }
    if item.tags.is_empty() {
        item.tags = tags.to_vec();
    }
    item.confidence = Some(confidence);
    item.status = ItemStatus::Done;
    item.error = None;
    true
}

/// 识别失败转待处理。同样只动还在"识别中"的条目
pub fn pend_item(item: &mut crate::model::Item, reason: &str) -> bool {
    if item.status != ItemStatus::Recognizing {
        return false;
    }
    item.status = ItemStatus::Pending;
    item.error = Some(reason.to_string());
    true
}

pub fn spawn_worker(app: AppHandle) -> mpsc::UnboundedSender<Job> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            process(&app, job).await;
        }
    });
    tx
}

pub fn enqueue(app: &AppHandle, job: Job) {
    let state = app.state::<AppState>();
    let tx = state.queue.lock().unwrap().clone();
    if let Some(tx) = tx {
        let _ = tx.send(job);
    }
}

/// 条目在当天列表里的位置（从 1 开始）
fn position(state: &AppState, job: &Job) -> usize {
    let data = state.data.lock().unwrap();
    data.days
        .get(&job.date)
        .and_then(|d| d.items.iter().position(|i| i.id == job.id))
        .map(|p| p + 1)
        .unwrap_or(0)
}

async fn process(app: &AppHandle, job: Job) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let found = {
        let data = state.data.lock().unwrap();
        data.days
            .get(&job.date)
            .and_then(|d| d.items.iter().find(|i| i.id == job.id))
            .map(|i| (i.note.clone(), i.shots.first().cloned()))
    };
    let Some((note, shot)) = found else { return };
    let cfg = settings.vision.clone();
    if cfg.base_url.trim().is_empty() || cfg.api_key.trim().is_empty() || cfg.model.trim().is_empty() {
        mark_pending(app, &job, &AiError::NoKey.to_string(), "先去设置页填接口信息，或手动写这一条");
        return;
    }
    let with_image = settings.send_image_to_ai && shot.is_some();
    if !with_image && note.trim().is_empty() {
        mark_pending(app, &job, "没有补充说明，截图也不发送给 AI", "截图已保存，有空手动写一句");
        return;
    }
    let image = if with_image {
        let path = state.store.dir().join(shot.unwrap_or_default());
        tauri::async_runtime::spawn_blocking(move || {
            image::open(&path).ok().map(|img| capture::ai_jpeg(&img.to_rgba8()))
        })
        .await
        .ok()
        .flatten()
    } else {
        None
    };
    if with_image && image.is_none() && note.trim().is_empty() {
        mark_pending(app, &job, "截图文件读不出来", "截图已保存，有空手动写一句");
        return;
    }
    let messages = build_messages(&vision_prompt(&settings, image.is_some()), &note, image.as_deref());
    let mut result = client::chat(&cfg, messages.clone(), VISION_TIMEOUT).await;
    if let Err(e) = &result {
        if should_retry(e) {
            log::warn!("识图失败，重试一次：{e}");
            result = client::chat(&cfg, messages, VISION_TIMEOUT).await;
        }
    }
    match result {
        Ok(raw) => {
            log::info!("识图完成：{}", job.id);
            apply_result(app, &job, &raw)
        }
        Err(e) => {
            log::error!("识图失败：{e}");
            mark_pending(app, &job, &e.to_string(), "截图已保存，有空手动写一句");
        }
    }
}

fn apply_result(app: &AppHandle, job: &Job, raw: &str) {
    let state = app.state::<AppState>();
    let (text, returned_tags, confidence) = match parse_vision(raw) {
        Ok(r) => (clean_item(&r.item), r.tags, r.confidence),
        Err(raw) => (clean_item(&raw), Vec::new(), 0.0),
    };
    let text = if text.is_empty() { PLACEHOLDER.to_string() } else { text };
    let confidence = if text == PLACEHOLDER { 0.0 } else { confidence };
    // 条目已被删除或用户已手写完成：不再建标签、不再改
    let still_recognizing = {
        let data = state.data.lock().unwrap();
        data.days
            .get(&job.date)
            .and_then(|d| d.items.iter().find(|i| i.id == job.id))
            .is_some_and(|i| i.status == ItemStatus::Recognizing)
    };
    if !still_recognizing {
        return;
    }
    let ai_tags = {
        let mut settings = state.settings.lock().unwrap();
        let ai_tags = tags::apply_ai(&settings.tags, &returned_tags, settings.allow_ai_new_tags);
        if !ai_tags.created.is_empty() {
            for name in &ai_tags.created {
                settings.tags.push(TagDef { name: name.clone(), created_by: TagOrigin::Ai, created_at: now_iso(), pending: true });
            }
            if let Err(e) = state.store.save_settings(&settings) {
                log::error!("保存设置失败：{e}");
            }
        }
        ai_tags
    };
    let updated = {
        let mut data = state.data.lock().unwrap();
        let Some(day) = data.days.get_mut(&job.date) else { return };
        let Some(item) = day.items.iter_mut().find(|i| i.id == job.id) else { return };
        if !apply_to_item(item, &text, &ai_tags.item_tags, confidence) {
            return;
        }
        let snapshot = item.clone();
        day.items_changed_at = Some(now_iso());
        if let Err(e) = state.store.save_data(&data) {
            log::error!("保存数据失败：{e}");
        }
        snapshot
    };
    let _ = app.emit("items-changed", &job.date);
    windows::show_toast(
        app,
        ToastPayload {
            kind: "done".into(),
            id: job.id.clone(),
            index: position(&state, job),
            text: updated.text,
            tags: updated.tags,
            new_tags: ai_tags.created,
            note: String::new(),
            message: String::new(),
            low_confidence: confidence < 0.5,
            shots: 0,
        },
    );
}

fn mark_pending(app: &AppHandle, job: &Job, reason: &str, toast_message: &str) {
    let state = app.state::<AppState>();
    let note = {
        let mut data = state.data.lock().unwrap();
        let Some(day) = data.days.get_mut(&job.date) else { return };
        let Some(item) = day.items.iter_mut().find(|i| i.id == job.id) else { return };
        if !pend_item(item, reason) {
            return;
        }
        let note = item.note.clone();
        day.items_changed_at = Some(now_iso());
        if let Err(e) = state.store.save_data(&data) {
            log::error!("保存数据失败：{e}");
        }
        note
    };
    let _ = app.emit("items-changed", &job.date);
    windows::show_toast(
        app,
        ToastPayload {
            kind: "failed".into(),
            id: job.id.clone(),
            index: position(&state, job),
            text: String::new(),
            tags: Vec::new(),
            new_tags: Vec::new(),
            note,
            message: toast_message.to_string(),
            low_confidence: false,
            shots: 0,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::prompt::{vision_prompt, VISION_TAIL};
    use crate::model::Settings;

    #[test]
    fn build_messages_with_image() {
        let s = Settings::default();
        let m = build_messages(&vision_prompt(&s, true), "沟通物业3号楼会议室锁坏了", Some(&[0xFF, 0xD8, 0xFF]));
        let arr = m.as_array().unwrap();
        assert_eq!(arr[0]["role"], "system");
        assert!(arr[0]["content"].as_str().unwrap().ends_with(VISION_TAIL));
        let content = arr[1]["content"].as_array().unwrap();
        let url = content[0]["image_url"]["url"].as_str().unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"), "{url}");
        assert_eq!(content[1]["type"], "text");
        assert!(content[1]["text"].as_str().unwrap().contains("沟通物业3号楼会议室锁坏了"));
    }

    #[test]
    fn build_messages_text_only_and_empty_note() {
        let s = Settings::default();
        let m = build_messages(&vision_prompt(&s, false), "整理合同", None);
        assert_eq!(m[1]["content"].as_str().unwrap(), "补充说明：整理合同");
        let empty = build_messages(&vision_prompt(&s, true), "  ", Some(&[1]));
        assert_eq!(empty[1]["content"][1]["text"], "补充说明：（无补充说明）");
    }

    fn item(status: crate::model::ItemStatus, text: &str, edited: bool) -> crate::model::Item {
        crate::model::Item {
            id: "x".into(),
            created_at: String::new(),
            text: text.into(),
            note: "补的话".into(),
            source: crate::model::Source::Ai,
            edited,
            tags: vec![],
            confidence: None,
            shots: vec![],
            status,
            error: None,
        }
    }

    #[test]
    fn result_only_applies_while_recognizing() {
        use crate::model::ItemStatus;
        let mut it = item(ItemStatus::Recognizing, "", false);
        assert!(apply_to_item(&mut it, "项目乙报销核算", &["项目乙".to_string()], 0.9));
        assert_eq!(it.status, ItemStatus::Done);
        assert_eq!(it.text, "项目乙报销核算");
        // 识别期间用户已经手写完成：不覆盖
        let mut done = item(ItemStatus::Done, "我自己写的", true);
        assert!(!apply_to_item(&mut done, "AI 的", &[], 0.9));
        assert_eq!(done.text, "我自己写的");
    }

    #[test]
    fn failure_only_pends_while_recognizing() {
        use crate::model::ItemStatus;
        let mut it = item(ItemStatus::Recognizing, "", false);
        assert!(pend_item(&mut it, "没联网"));
        assert_eq!(it.status, ItemStatus::Pending);
        let mut done = item(ItemStatus::Done, "我自己写的", true);
        assert!(!pend_item(&mut done, "没联网"));
        assert_eq!(done.status, ItemStatus::Done);
        assert_eq!(done.error, None);
    }

    #[test]
    fn should_retry_rules() {
        use crate::ai::client::AiError;
        assert!(should_retry(&AiError::Timeout));
        assert!(should_retry(&AiError::Network("x".into())));
        assert!(should_retry(&AiError::Http(502, "".into())));
        assert!(should_retry(&AiError::Http(429, "".into())));
        assert!(!should_retry(&AiError::Http(401, "".into())));
        assert!(!should_retry(&AiError::NoKey));
    }
}
