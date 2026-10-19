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
