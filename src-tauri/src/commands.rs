//! 前端可调用的命令。

use crate::capture_mode::{Action, Origin};
use crate::flow;
use crate::windows::{self, NoteOpen};
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub fn overlay_init(app: AppHandle, index: usize) -> Option<flow::CaptureInfo> {
    flow::overlay_init(&app, index)
}

#[tauri::command]
pub fn overlay_ready(app: AppHandle, index: usize) {
    flow::overlay_ready(&app, index);
}

/// 没有标注：后端从定格原图裁剪
#[tauri::command]
pub async fn capture_done(app: AppHandle, index: usize, x: i64, y: i64, w: i64, h: i64, action: Action) -> Result<(), String> {
    let result = crate::capture::clamp_rect(x, y, w, h)
        .ok_or_else(|| "选区不在屏幕内".to_string())
        .and_then(|rect| flow::finish_rect(&app, index, rect, action));
    if let Err(e) = &result {
        log::error!("截图失败：{e}");
        flow::cancel(&app);
    }
    result
}

/// 有标注：页面合成好的 RGBA 原始字节。头里带 x-index、x-x、x-y、x-width、x-height、x-action
#[tauri::command]
pub fn capture_pixels(app: AppHandle, request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("没有收到图".into());
    };
    let headers = request.headers();
    let text = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let num = |k: &str| text(k).and_then(|s| s.parse::<u32>().ok());
    let (Some(index), Some(w), Some(h)) = (num("x-index"), num("x-width"), num("x-height")) else {
        return Err("缺少图的尺寸".into());
    };
    let action: Action = text("x-action")
        .and_then(|a| serde_json::from_value(serde_json::Value::String(a)).ok())
        .ok_or("缺少要做的操作")?;
    if w == 0 || h == 0 || bytes.len() != (w as usize) * (h as usize) * 4 {
        return Err("图的大小不对".into());
    }
    let img = image::RgbaImage::from_raw(w, h, bytes.clone()).ok_or("图的大小不对")?;
    let rect = crate::capture::PhysRect { x: num("x-x").unwrap_or(0), y: num("x-y").unwrap_or(0), w, h };
    std::thread::spawn(move || {
        if let Err(e) = flow::finish(&app, index as usize, rect, img, action) {
            log::error!("截图失败：{e}");
            flow::cancel(&app);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn hint_seen(app: AppHandle) {
    flow::hint_seen(&app);
}

#[tauri::command]
pub fn capture_cancel(app: AppHandle) {
    flow::cancel(&app);
}

#[tauri::command]
pub fn note_current(app: AppHandle) -> Option<NoteOpen> {
    flow::note_open(&app, None)
}

#[tauri::command]
pub async fn note_submit(app: AppHandle, text: String) -> Result<(), String> {
    flow::commit_draft(&app, &text)
}

#[tauri::command]
pub async fn note_skip(app: AppHandle) -> Result<(), String> {
    flow::commit_draft(&app, "")
}

#[tauri::command]
pub fn note_more(app: AppHandle) {
    flow::note_more(&app);
}

#[tauri::command]
pub fn note_text(app: AppHandle, text: String) {
    flow::note_text(&app, &text);
}

#[tauri::command]
pub fn note_blur(app: AppHandle) {
    flow::note_blur(&app);
}

#[tauri::command]
pub fn note_remove_shot(app: AppHandle, index: usize) -> Result<(), String> {
    flow::note_remove_shot(&app, index)
}

#[tauri::command]
pub fn note_discard(app: AppHandle) {
    flow::discard_draft(&app);
}

#[tauri::command]
pub fn bar_done(app: AppHandle) {
    flow::bar_done(&app);
}

#[tauri::command]
pub fn bar_current(app: AppHandle) -> Option<windows::BarView> {
    flow::bar_current(&app)
}

/// 拖进来的文件：按文件名排序，逐个读；认不出的计数。一张都没加成时返回提示
#[tauri::command]
pub async fn add_image_paths(app: AppHandle, paths: Vec<String>) -> Option<String> {
    let started = flow::draft_id(&app);
    import_paths(app, paths, started).await
}

/// 读文件、解码（可能好几秒），再交给 add_images；started 是开始时的草稿 id
async fn import_paths(app: AppHandle, paths: Vec<String>, started: Option<String>) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut paths: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
        crate::import::sort_paths(&mut paths);
        let mut imgs = Vec::new();
        let mut unreadable = 0;
        for p in &paths {
            // 不是图片、或者大得离谱的文件（拖进来的视频、压缩包）不往内存里读
            let len = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            let bytes = crate::import::worth_reading(p, len).then(|| std::fs::read(p).ok()).flatten();
            match bytes.and_then(|b| crate::import::decode(&b)) {
                Some(img) => imgs.push(img),
                None => unreadable += 1,
            }
        }
        flow::add_images(&app, imgs, unreadable, started.as_deref())
    })
    .await
    .ok()
    .flatten()
}

/// "加图片…""从图片添加"：选文件对话框，可以多选
#[tauri::command]
pub async fn pick_images(app: AppHandle, window: tauri::WebviewWindow) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let started = flow::draft_id(&app);
    let dialog = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog
            .dialog()
            .file()
            .set_title("选择图片")
            .add_filter("图片", &["png", "jpg", "jpeg", "bmp", "webp"])
            .set_parent(&window)
            .blocking_pick_files()
    })
    .await
    .ok()
    .flatten()?;
    let paths = picked.into_iter().filter_map(|f| f.into_path().ok()).map(|p| p.display().to_string()).collect();
    import_paths(app, paths, started).await
}

/// 粘贴的图片：body 是图片文件的字节
#[tauri::command]
pub fn add_image_bytes(app: AppHandle, request: tauri::ipc::Request<'_>) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("没有收到图".into());
    };
    let bytes = bytes.clone();
    let started = flow::draft_id(&app);
    std::thread::spawn(move || {
        let (imgs, unreadable) = match crate::import::decode(&bytes) {
            Some(img) => (vec![img], 0),
            None => (Vec::new(), 1),
        };
        if let Some(msg) = flow::add_images(&app, imgs, unreadable, started.as_deref()) {
            log::warn!("粘贴的图没加：{msg}");
        }
    });
    Ok(())
}

#[tauri::command]
pub fn toast_hide(app: AppHandle) {
    windows::hide_toast(&app);
}

/// 提示窗上的"改一下"：打开主窗口，让这条进入编辑
#[tauri::command]
pub async fn toast_open_item(app: AppHandle, id: String) {
    windows::hide_toast(&app);
    open_main_with(&app, crate::state::MainIntent::EditItem(id));
}

/// 记下要主窗口做的事，打开主窗口并通知它来取（刚建的页面加载完也会自己来取）
pub fn open_main_with(app: &AppHandle, intent: crate::state::MainIntent) {
    *app.state::<crate::state::AppState>().main_intent.lock().unwrap() = Some(intent);
    windows::show_main(app);
    let _ = app.emit_to("main", "main:intent", ());
}

#[tauri::command]
pub fn take_main_intent(app: AppHandle) -> Option<crate::state::MainIntent> {
    app.state::<crate::state::AppState>().main_intent.lock().unwrap().take()
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

/// 网页里发起截图。主窗口右下角的"截一张"先把主窗口藏起来，免得截到它，截完再叫回来
#[tauri::command]
pub fn capture_start(app: AppHandle, origin: Origin) {
    let delay = if origin == Origin::Main {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
        app.state::<AppState>().restore_main.store(true, std::sync::atomic::Ordering::SeqCst);
        250
    } else {
        0
    };
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay));
        flow::start_capture(app, origin);
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
    let started_at = now_iso();
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
    let result = summary::finalize_for(&raw, &settings, d, &sources, skipped, d == views::today())?;
    {
        let mut data = state.data.lock().unwrap();
        summary::record_generated(&mut data, &date, &result.text, &started_at);
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
    pub settings_recovered: bool,
    pub settings_reset: bool,
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
        settings_recovered: status.settings_recovered,
        settings_reset: status.settings_reset,
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
    status.settings_recovered = false;
    status.settings_reset = false;
}

// ───────────── 历史 ─────────────

#[tauri::command]
pub fn list_days(app: AppHandle) -> Vec<views::DayListItem> {
    let state = app.state::<AppState>();
    let data = state.data.lock().unwrap();
    views::list_days(&data)
}
