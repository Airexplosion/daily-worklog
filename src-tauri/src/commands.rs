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
