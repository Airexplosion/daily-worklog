//! 各窗口的创建、定位与显示。

use crate::capture::Frozen;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub fn overlay_label(index: usize) -> String {
    format!("capture-{index}")
}

fn overlay_windows(app: &AppHandle) -> Vec<WebviewWindow> {
    app.webview_windows().into_iter().filter(|(l, _)| l.starts_with("capture-")).map(|(_, w)| w).collect()
}

/// 当前显示器几何（物理像素），软件启动时用来提前建好框选窗口
pub fn monitor_geometry() -> Vec<(i32, i32, u32, u32)> {
    xcap::Monitor::all()
        .map(|ms| {
            ms.iter()
                .map(|m| (m.x().unwrap_or(0), m.y().unwrap_or(0), m.width().unwrap_or(0), m.height().unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn ensure_overlays(app: &AppHandle, frozen: &Frozen) -> tauri::Result<()> {
    let geometry: Vec<(i32, i32, u32, u32)> = frozen.monitors.iter().map(|m| (m.x, m.y, m.width, m.height)).collect();
    ensure_overlays_for(app, &geometry)
}

/// 每块屏一个框选窗口，按这块屏的物理范围摆放；显示器变了就重建
pub fn ensure_overlays_for(app: &AppHandle, geometry: &[(i32, i32, u32, u32)]) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let mut current = state.overlay_geometry.lock().unwrap();
    if *current == geometry && overlay_windows(app).len() == geometry.len() {
        return Ok(());
    }
    for w in overlay_windows(app) {
        let _ = w.destroy();
    }
    for (index, &(x, y, width, height)) in geometry.iter().enumerate() {
        let w = WebviewWindowBuilder::new(app, overlay_label(index), WebviewUrl::App(format!("capture.html?i={index}").into()))
            .title("截图")
            .transparent(true)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false)
            .focused(false)
            .build()?;
        place_physical(&w, x, y, width, height)?;
    }
    *current = geometry.to_vec();
    Ok(())
}

/// 先移到目标屏再设尺寸：跨缩放比例的屏移动时系统会按新 DPI 改一次尺寸，所以尺寸放后面
pub fn place_physical(w: &WebviewWindow, x: i32, y: i32, width: u32, height: u32) -> tauri::Result<()> {
    w.set_position(PhysicalPosition::new(x, y))?;
    w.set_size(PhysicalSize::new(width, height))?;
    w.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}

#[derive(Clone, Serialize)]
struct CaptureStart {
    url: String,
}

pub fn frozen_url(frozen: &Frozen, index: usize) -> String {
    format!("http://frozen.localhost/{}?t={}", index, frozen.taken_at.timestamp_millis())
}

/// 通知每个框选窗口加载自己那块屏的定格画面；画面加载完页面会调用 overlay_ready。
/// 刚建好、脚本还没跑起来的页面收不到这个通知，它启动时会用 overlay_init 自己来要
pub fn start_overlays(app: &AppHandle, frozen: &Frozen) {
    for m in &frozen.monitors {
        let _ = app.emit_to(overlay_label(m.index), "capture:start", CaptureStart { url: frozen_url(frozen, m.index) });
    }
}

pub fn hide_overlays(app: &AppHandle) {
    for w in overlay_windows(app) {
        let _ = w.hide();
        let _ = w.emit_to(w.label(), "capture:reset", ());
    }
}

/// 当前鼠标位置（物理像素）
pub fn cursor_pos() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut p) };
    (ok != 0).then_some((p.x, p.y))
}

/// 显示但不抢焦点（右下角提示窗用）
pub fn show_no_activate(w: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
    match w.hwnd() {
        Ok(hwnd) => unsafe {
            ShowWindow(hwnd.0 as _, SW_SHOWNOACTIVATE);
        },
        Err(_) => {
            let _ = w.show();
        }
    }
}
