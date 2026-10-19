//! 截图主流程：热键 → 定格 → 框选 → 裁剪存档。

use crate::capture::{self, PhysRect};
use crate::state::{AppState, PendingShot};
use crate::windows;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tauri::{AppHandle, Manager};

/// 热键触发。截图流程进行中时再按无效
pub fn start_capture(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.capturing.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
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
        if let Some(f) = state.frozen.lock().unwrap().as_ref() {
            windows::start_overlays(&app, f);
        }
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
    *state.frozen.lock().unwrap() = None;
    *state.pending_shot.lock().unwrap() = None;
    state.capturing.store(false, Ordering::SeqCst);
}

/// 框选完成：从原图裁剪，PNG 存档，准备好缩略图和发给 AI 的图
pub fn finish_selection(app: &AppHandle, index: usize, rect: PhysRect) -> Result<(), String> {
    let state = app.state::<AppState>();
    let frozen = state.frozen.lock().unwrap().take().ok_or("截图已失效")?;
    windows::hide_overlays(app);
    let img = capture::crop(&frozen, index, rect).ok_or("选区不在屏幕内")?;
    let local = frozen.taken_at;
    let id = capture::shot_id(local.naive_local());
    let shot_rel = format!("shots/{id}.png");
    let path = state.store.dir().join(&shot_rel);
    crate::storage::write_atomic(&path, &capture::png_bytes(&img)).map_err(|e| format!("保存截图失败：{e}"))?;
    log::info!("capture: 屏 {index} 选区 {:?} → {}", rect, shot_rel);
    let thumb = capture::thumbnail_fit(&img, 240, 150);
    *state.pending_shot.lock().unwrap() = Some(PendingShot {
        id,
        date: local.format("%Y-%m-%d").to_string(),
        created_at: local.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        shot_rel,
        monitor_index: index,
        thumb_jpeg: capture::thumb_jpeg(&thumb),
        ai_jpeg: capture::ai_jpeg(&img),
    });
    Ok(())
}
