//! 截图主流程：热键 → 定格 → 框选 → 裁剪存档 → 补充说明 → 落盘 → 后台识别。

use crate::capture::{self, PhysRect};
use crate::model::{now_iso, Item, ItemStatus, Source};
use crate::recognize::{self, Job};
use crate::state::{AppState, PendingShot};
use crate::windows::{self, NoteOpen, ToastPayload};
use base64::Engine;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

/// 热键触发。截图流程进行中时再按无效；补充框开着时把它重新拿到前台
pub fn start_capture(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.capturing.swap(true, Ordering::SeqCst) {
        if state.pending_shot.lock().unwrap().is_some() {
            if let Some(w) = app.get_webview_window("note") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        return;
    }
    let toast_was_visible = app.get_webview_window("toast").and_then(|w| w.is_visible().ok()).unwrap_or(false);
    windows::hide_toast(&app);
    std::thread::spawn(move || {
        // 上一条的提示窗刚藏起来，等它真的消失再截
        if toast_was_visible {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        let t0 = Instant::now();
        let state = app.state::<AppState>();
        let frozen = match capture::grab_all() {
            Ok(f) => f,
            Err(e) => {
                log::error!("{e}");
                state.capturing.store(false, Ordering::SeqCst);
                return;
            }
        };
        log::info!("capture: grab {} 块屏 {}ms", frozen.monitors.len(), t0.elapsed().as_millis());
        if let Err(e) = windows::ensure_overlays(&app, &frozen) {
            log::error!("创建框选窗口失败：{e}");
            state.capturing.store(false, Ordering::SeqCst);
            return;
        }
        let taken_at = frozen.taken_at;
        *state.frozen.lock().unwrap() = Some(frozen);
        state.overlay_shown.store(false, Ordering::SeqCst);
        let urls: Vec<(usize, String)> = state
            .frozen
            .lock()
            .unwrap()
            .as_ref()
            .map(|f| f.monitors.iter().map(|m| (m.index, windows::frozen_url(f, m.index))).collect())
            .unwrap_or_default();
        windows::start_overlays(&app, urls);
        // 兜底：5 秒内一块框选画面都没出来，就结束这次截图，免得后面的热键一直无效
        std::thread::sleep(std::time::Duration::from_secs(5));
        let same_capture = state.frozen.lock().unwrap().as_ref().is_some_and(|f| f.taken_at == taken_at);
        if same_capture && !state.overlay_shown.load(Ordering::SeqCst) {
            log::error!("capture: 框选画面 5 秒内没有出现，已取消");
            cancel(&app);
        }
    });
}

/// 框选页面启动时调用：如果正在截图，返回这块屏的定格画面地址
pub fn overlay_init(app: &AppHandle, index: usize) -> Option<String> {
    let state = app.state::<AppState>();
    let frozen = state.frozen.lock().unwrap();
    frozen.as_ref().filter(|f| f.monitors.iter().any(|m| m.index == index)).map(|f| windows::frozen_url(f, index))
}

/// 某块屏的定格画面加载好了：显示这块屏的框选窗口，鼠标所在的那块拿焦点
pub fn overlay_ready(app: &AppHandle, index: usize) {
    let state = app.state::<AppState>();
    let Some(w) = app.get_webview_window(&windows::overlay_label(index)) else { return };
    if state.frozen.lock().unwrap().is_none() {
        return;
    }
    let _ = w.show();
    state.overlay_shown.store(true, Ordering::SeqCst);
    let under_cursor = {
        let frozen = state.frozen.lock().unwrap();
        let cursor = windows::cursor_pos();
        frozen.as_ref().and_then(|f| f.monitors.iter().find(|m| m.index == index)).is_some_and(|m| {
            cursor.is_some_and(|(cx, cy)| {
                cx >= m.x && cx < m.x + m.width as i32 && cy >= m.y && cy < m.y + m.height as i32
            })
        })
    };
    if under_cursor {
        let _ = w.set_focus();
    }
    let taken_at = state.frozen.lock().unwrap().as_ref().map(|f| f.taken_at);
    if let Some(t) = taken_at {
        let ms = (chrono::Local::now() - t).num_milliseconds();
        log::info!("capture: 屏 {index} 框选画面出现，距按热键 {ms}ms");
    }
}

pub fn cancel(app: &AppHandle) {
    let state = app.state::<AppState>();
    windows::hide_overlays(app);
    windows::hide_note(app);
    *state.frozen.lock().unwrap() = None;
    *state.pending_shot.lock().unwrap() = None;
    state.capturing.store(false, Ordering::SeqCst);
    restore_main_if_needed(app);
}

/// 从主窗口点"截一张"的，截完或取消后把主窗口叫回来
fn restore_main_if_needed(app: &AppHandle) {
    if app.state::<AppState>().restore_main.swap(false, Ordering::SeqCst) {
        windows::show_main(app);
    }
}

/// 框选完成：从原图裁剪，PNG 存档，准备好缩略图和发给 AI 的图
pub fn finish_selection(app: &AppHandle, index: usize, rect: PhysRect) -> Result<(), String> {
    let state = app.state::<AppState>();
    let frozen = state.frozen.lock().unwrap().take().ok_or("截图已失效")?;
    // 截图那块屏的框选窗口留着当补充框的变暗背景，其他屏的收起
    windows::keep_backdrop(app, index);
    let img = capture::crop(&frozen, index, rect).ok_or("选区不在屏幕内")?;
    let local = frozen.taken_at;
    let id = capture::shot_id(local.naive_local());
    let shot_rel = format!("shots/{id}.png");
    let path = state.store.dir().join(&shot_rel);
    crate::storage::write_atomic(&path, &capture::png_bytes(&img)).map_err(|e| format!("保存截图失败：{e}"))?;
    log::info!("capture: 屏 {index} 选区 {:?} → {}", rect, shot_rel);
    let thumb = capture::thumbnail_fit(&img, 240, 150);
    let m = frozen.monitors.iter().find(|m| m.index == index).ok_or("选区不在屏幕内")?;
    let monitor = (m.x, m.y, m.width, m.height, m.scale);
    let thumb_jpeg = capture::thumb_jpeg(&thumb);
    let date = local.format("%Y-%m-%d").to_string();
    let next_index = state.data.lock().unwrap().days.get(&date).map_or(0, |d| d.items.len()) + 1;
    let thumb_url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&thumb_jpeg));
    *state.pending_shot.lock().unwrap() = Some(PendingShot {
        id,
        date,
        created_at: local.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        shot_rel,
        monitor_index: index,
        monitor,
        thumb_jpeg,
    });
    windows::show_note(&app, monitor, NoteOpen { thumb: thumb_url, index: next_index });
    Ok(())
}

/// 补充框的内容（页面刚加载、错过 note:open 时用）
pub fn note_current(app: &AppHandle) -> Option<NoteOpen> {
    let state = app.state::<AppState>();
    let pending = state.pending_shot.lock().unwrap();
    let shot = pending.as_ref()?;
    let next_index = state.data.lock().unwrap().days.get(&shot.date).map_or(0, |d| d.items.len()) + 1;
    Some(NoteOpen {
        thumb: format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&shot.thumb_jpeg)),
        index: next_index,
    })
}

/// 回车或跳过：条目立刻落盘（识别中），补充框关掉，右下角提示，后台排队识别
pub fn commit_pending(app: &AppHandle, note: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let shot = state.pending_shot.lock().unwrap().take().ok_or("没有待记录的截图")?;
    windows::hide_note(app);
    windows::hide_overlays(app);
    state.capturing.store(false, Ordering::SeqCst);
    let item = Item {
        id: shot.id.clone(),
        created_at: shot.created_at.clone(),
        text: String::new(),
        note: note.trim().to_string(),
        source: Source::Ai,
        edited: false,
        tags: Vec::new(),
        confidence: None,
        shots: vec![shot.shot_rel.clone()],
        status: ItemStatus::Recognizing,
        error: None,
    };
    let index = {
        let mut data = state.data.lock().unwrap();
        let day = data.days.entry(shot.date.clone()).or_default();
        day.items.push(item);
        day.items_changed_at = Some(now_iso());
        let index = day.items.len();
        // 存盘失败（比如被杀毒软件锁住）也照常排队识别，识别完会再存一次
        if let Err(e) = state.store.save_data(&data) {
            log::error!("保存失败：{e}");
        }
        index
    };
    let _ = app.emit("items-changed", &shot.date);
    windows::show_toast(
        app,
        ToastPayload {
            kind: "recognizing".into(),
            id: shot.id.clone(),
            index,
            text: String::new(),
            tags: Vec::new(),
            new_tags: Vec::new(),
            note: note.trim().to_string(),
            message: String::new(),
            low_confidence: false,
        },
    );
    recognize::enqueue(app, Job { date: shot.date, id: shot.id });
    restore_main_if_needed(app);
    Ok(())
}
