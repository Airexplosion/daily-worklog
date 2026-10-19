//! 前端可调用的命令。

use crate::capture::PhysRect;
use crate::flow;
use tauri::AppHandle;

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
    if result.is_err() {
        flow::cancel(&app);
    } else {
        // 补充框在 Task 8 接上；现在先结束流程
        flow::cancel(&app);
    }
    result
}

#[tauri::command]
pub fn capture_cancel(app: AppHandle) {
    flow::cancel(&app);
}
