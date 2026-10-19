//! 前端可调用的命令。

use crate::capture::PhysRect;
use crate::flow;
use crate::windows::{self, NoteOpen};
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub fn overlay_init(app: AppHandle, index: usize) -> Option<String> {
    flow::overlay_init(&app, index)
}

#[tauri::command]
pub fn overlay_ready(app: AppHandle, index: usize) {
    flow::overlay_ready(&app, index);
}

#[tauri::command]
pub async fn capture_done(app: AppHandle, index: usize, x: u32, y: u32, w: u32, h: u32) -> Result<(), String> {
    let result = flow::finish_selection(&app, index, PhysRect { x, y, w, h });
    if let Err(e) = &result {
        log::error!("截图失败：{e}");
        flow::cancel(&app);
    }
    result
}

#[tauri::command]
pub fn capture_cancel(app: AppHandle) {
    flow::cancel(&app);
}

#[tauri::command]
pub fn note_current(app: AppHandle) -> Option<NoteOpen> {
    flow::note_current(&app)
}

#[tauri::command]
pub async fn note_submit(app: AppHandle, text: String) -> Result<(), String> {
    flow::commit_pending(&app, &text)
}

#[tauri::command]
pub async fn note_skip(app: AppHandle) -> Result<(), String> {
    flow::commit_pending(&app, "")
}

#[tauri::command]
pub fn toast_hide(app: AppHandle) {
    windows::hide_toast(&app);
}

/// 提示窗上的"改一下"：打开主窗口，让这条进入编辑
#[tauri::command]
pub fn toast_open_item(app: AppHandle, id: String) {
    windows::hide_toast(&app);
    if windows::show_main(&app).is_some() {
        let _ = app.emit_to("main", "main:edit-item", id);
    }
}

// ───────────── 主窗口：今日 / 条目 ─────────────

use crate::items;
use crate::model::{now_iso, Item, ItemStatus, TagDef, TagOrigin};
use crate::recognize::{self, Job};
use crate::state::AppState;
use crate::tags;
use crate::views::{self, DayView, TagView};
use tauri::Manager;

fn save(state: &AppState, data: &crate::model::Data) -> Result<(), String> {
    state.store.save_data(data).map_err(|e| format!("保存失败：{e}"))
}

fn changed(app: &AppHandle, date: &str) {
    let _ = app.emit("items-changed", date);
}

#[tauri::command]
pub fn today() -> String {
    views::today().format("%Y-%m-%d").to_string()
}

#[tauri::command]
pub fn get_day(app: AppHandle, date: String) -> Result<DayView, String> {
    let state = app.state::<AppState>();
    let d = views::parse_date(&date).ok_or("日期格式不对")?;
    let (remind_time, hotkey) = {
        let s = state.settings.lock().unwrap();
        (s.remind_time.clone(), s.hotkey.clone())
    };
    let data = state.data.lock().unwrap();
    let day = data.days.get(&date).cloned().unwrap_or_default();
    Ok(DayView {
        label: views::day_label(d),
        is_today: d == views::today(),
        stale: views::is_stale(&day),
        muted: day.mute_reminder,
        items: day.items,
        summary: day.summary,
        remind_time,
        hotkey,
        date,
    })
}

#[tauri::command]
pub fn get_tags(app: AppHandle) -> Vec<TagView> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap();
    views::tag_views(&settings.tags, views::today())
}

#[tauri::command]
pub fn add_item(app: AppHandle, date: String, text: String) -> Result<Item, String> {
    if text.trim().is_empty() {
        return Err("内容是空的".into());
    }
    let state = app.state::<AppState>();
    let now = now_iso();
    let id = crate::capture::shot_id(chrono::Local::now().naive_local());
    let item = {
        let mut data = state.data.lock().unwrap();
        let item = items::add_manual(&mut data, &date, &text, &id, &now);
        save(&state, &data)?;
        item
    };
    changed(&app, &date);
    Ok(item)
}

/// 改文字或标签；用户新写的标签加入标签列表
#[tauri::command]
pub fn update_item(app: AppHandle, id: String, text: Option<String>, tags: Option<Vec<String>>) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(list) = &tags {
        let mut settings = state.settings.lock().unwrap();
        let mut added = false;
        for name in list.iter().filter_map(|t| tags::normalize(t)) {
            if name != tags::OTHER && !settings.tags.iter().any(|t| t.name == name) {
                settings.tags.push(TagDef { name, created_by: TagOrigin::User, created_at: now_iso(), pending: false });
                added = true;
            }
        }
        if added {
            state.store.save_settings(&settings).map_err(|e| format!("保存失败：{e}"))?;
        }
    }
    let date = {
        let mut data = state.data.lock().unwrap();
        let date = items::update(&mut data, &id, text.as_deref(), tags.as_deref(), &now_iso()).ok_or("找不到这一条")?;
        save(&state, &data)?;
        date
    };
    changed(&app, &date);
    Ok(())
}

/// 删除，返回撤销用的凭据；5 秒后才真正删截图文件
#[tauri::command]
pub fn delete_item(app: AppHandle, id: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let removed = {
        let mut data = state.data.lock().unwrap();
        let removed = items::delete(&mut data, &id, &now_iso()).ok_or("找不到这一条")?;
        save(&state, &data)?;
        removed
    };
    let date = removed.date.clone();
    let token = state.trash.lock().unwrap().put(removed, std::time::Instant::now());
    changed(&app, &date);
    Ok(token)
}

#[tauri::command]
pub fn undo_delete(app: AppHandle, token: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let removed = state.trash.lock().unwrap().take(&token).ok_or("已经过了撤销时间")?;
    let date = removed.date.clone();
    {
        let mut data = state.data.lock().unwrap();
        items::undo_delete(&mut data, removed, &now_iso());
        save(&state, &data)?;
    }
    changed(&app, &date);
    Ok(())
}

/// 把待处理的条目重新排队识别
fn requeue(app: &AppHandle, filter: impl Fn(&str, &Item) -> bool) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let mut jobs = Vec::new();
    {
        let mut data = state.data.lock().unwrap();
        for (date, day) in data.days.iter_mut() {
            for item in day.items.iter_mut().filter(|i| i.status == ItemStatus::Pending && filter(date, i)) {
                item.status = ItemStatus::Recognizing;
                item.error = None;
                jobs.push(Job { date: date.clone(), id: item.id.clone() });
            }
        }
        save(&state, &data)?;
    }
    let n = jobs.len();
    for job in jobs {
        changed(app, &job.date);
        recognize::enqueue(app, job);
    }
    Ok(n)
}

#[tauri::command]
pub fn retry_item(app: AppHandle, id: String) -> Result<usize, String> {
    requeue(&app, |_, i| i.id == id)
}

#[tauri::command]
pub fn retry_all(app: AppHandle, date: String) -> Result<usize, String> {
    requeue(&app, |d, _| d == date)
}

#[tauri::command]
pub fn confirm_tag(app: AppHandle, name: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap();
    tags::confirm(&mut settings, &name);
    state.store.save_settings(&settings).map_err(|e| format!("保存失败：{e}"))?;
    let _ = app.emit("tags-changed", ());
    Ok(())
}

/// 改名；新名字已存在即并入
#[tauri::command]
pub fn rename_tag(app: AppHandle, from: String, to: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        let mut data = state.data.lock().unwrap();
        tags::rename(&mut data, &mut settings, &from, &to);
        if let Some(t) = tags::normalize(&to) {
            tags::confirm(&mut settings, &t);
        }
        save(&state, &data)?;
        state.store.save_settings(&settings).map_err(|e| format!("保存失败：{e}"))?;
    }
    let _ = app.emit("tags-changed", ());
    let _ = app.emit("items-changed", "*");
    Ok(())
}

#[tauri::command]
pub fn mute_today(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let date = today();
    let mut data = state.data.lock().unwrap();
    data.days.entry(date.clone()).or_default().mute_reminder = true;
    save(&state, &data)
}

/// 主窗口右下角的"截一张"：先把主窗口藏起来，免得截到它
#[tauri::command]
pub fn capture_from_main(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        flow::start_capture(app);
    });
}

// ───────────── 生成总结 ─────────────

use crate::summary::{self, SummaryResult};
use tauri::ipc::Channel;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// 流式生成：先推标题，再推模型输出的片段；完成后返回清洗过的全文和检查结果
#[tauri::command]
pub async fn generate_summary(app: AppHandle, date: String, on_delta: Channel<String>) -> Result<SummaryResult, String> {
    let state = app.state::<AppState>();
    let d = views::parse_date(&date).ok_or("日期格式不对")?;
    let settings = state.settings.lock().unwrap().clone();
    let day = state.data.lock().unwrap().days.get(&date).cloned().unwrap_or_default();
    let (sources, skipped) = summary::sources(&day);
    let title = crate::format::title(settings.user_name.trim(), &settings.title_suffix, d);
    let _ = on_delta.send(format!("{title}\n"));
    let raw = if sources.is_empty() {
        String::new()
    } else {
        let cfg = settings.summary_config();
        let messages = summary::build_messages(&settings, &sources);
        crate::ai::client::chat_stream(&cfg, messages, std::time::Duration::from_secs(60), |delta| {
            let _ = on_delta.send(delta.to_string());
        })
        .await
        .map_err(|e| e.to_string())?
    };
    let result = summary::finalize(&raw, &settings, d, &sources, skipped);
    {
        let mut data = state.data.lock().unwrap();
        let entry = data.days.entry(date.clone()).or_default();
        summary::set_generated(entry, &result.text, &now_iso());
        save(&state, &data)?;
    }
    log::info!("总结已生成：{date}，{} 条", sources.len());
    let _ = app.emit("summary-generated", &date);
    Ok(result)
}

#[tauri::command]
pub fn save_summary(app: AppHandle, date: String, text: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut data = state.data.lock().unwrap();
    let day = data.days.get_mut(&date).ok_or("这天还没有总结")?;
    summary::save_edit(day, &text, &now_iso());
    save(&state, &data)
}

/// 撤销重新生成，返回恢复后的文字
#[tauri::command]
pub fn undo_regenerate(app: AppHandle, date: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    let mut data = state.data.lock().unwrap();
    let day = data.days.get_mut(&date).ok_or("这天还没有总结")?;
    if !summary::undo_regenerate(day) {
        return Err("没有可以撤销的".into());
    }
    let text = day.summary.as_ref().map(|s| s.text.clone()).unwrap_or_default();
    save(&state, &data)?;
    Ok(text)
}

/// 复制纯文本：只去掉 Markdown 符号，换行统一 \r\n
#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<String, String> {
    let clean = crate::format::for_clipboard(&text);
    app.clipboard().write_text(clean.clone()).map_err(|e| format!("复制失败：{e}"))?;
    Ok(clean)
}

// ───────────── 横幅：热键失败、数据恢复、漏生成 ─────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissedView {
    pub date: String,
    pub label: String,
    pub count: usize,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub hotkey_error: Option<String>,
    pub recovered_from: Option<String>,
    pub data_lost: bool,
    pub missed: Option<MissedView>,
}

#[tauri::command]
pub fn get_status(app: AppHandle) -> StatusView {
    let state = app.state::<AppState>();
    let status = state.status.lock().unwrap().clone();
    let missed = {
        let data = state.data.lock().unwrap();
        crate::reminder::find_missed(&data, views::today()).map(|(d, count)| MissedView {
            date: d.format("%Y-%m-%d").to_string(),
            label: views::missed_label(d, views::today()),
            count,
        })
    };
    StatusView {
        hotkey_error: status.hotkey_error,
        recovered_from: status.recovered_from,
        data_lost: status.data_lost,
        missed,
    }
}

#[tauri::command]
pub fn dismiss_missed(app: AppHandle, date: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut data = state.data.lock().unwrap();
    data.days.entry(date).or_default().missed_banner_dismissed = true;
    save(&state, &data)
}

#[tauri::command]
pub fn dismiss_recovered(app: AppHandle) {
    let state = app.state::<AppState>();
    let mut status = state.status.lock().unwrap();
    status.recovered_from = None;
    status.data_lost = false;
}
