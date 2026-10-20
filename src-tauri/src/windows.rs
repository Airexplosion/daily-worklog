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
    ensure_overlays_for(app, &geometry)?;
    // 改了显示缩放、显示器重新接上时几何没变但窗口尺寸可能不对：每次都重新摆一遍
    for m in &frozen.monitors {
        if let Some(w) = app.get_webview_window(&overlay_label(m.index)) {
            let _ = place_physical(&w, m.x, m.y, m.width, m.height);
        }
    }
    Ok(())
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

/// 带上这块屏的物理宽度，页面用它换算缩放比例（比 devicePixelRatio 更可靠）
pub fn frozen_url(frozen: &Frozen, index: usize) -> String {
    let width = frozen.monitors.iter().find(|m| m.index == index).map_or(0, |m| m.width);
    format!("http://frozen.localhost/{}?t={}&w={}", index, frozen.taken_at.timestamp_millis(), width)
}

/// 通知每个框选窗口加载自己那块屏的定格画面；画面加载完页面会调用 overlay_ready。
/// 刚建好、脚本还没跑起来的页面收不到这个通知，它启动时会用 overlay_init 自己来要
pub fn start_overlays<T: Serialize + Clone>(app: &AppHandle, infos: Vec<(usize, T)>) {
    for (index, info) in infos {
        let _ = app.emit_to(overlay_label(index), "capture:start", info);
    }
}

pub fn hide_overlays(app: &AppHandle) {
    for w in overlay_windows(app) {
        let _ = w.hide();
        let _ = w.set_ignore_cursor_events(false);
        let _ = w.emit_to(w.label(), "capture:reset", ());
    }
}

/// 截图那块屏的框选窗口变成补充框的变暗背景（不接收鼠标），其他屏的收起
pub fn keep_backdrop(app: &AppHandle, index: usize) {
    let keep = overlay_label(index);
    for w in overlay_windows(app) {
        if w.label() == keep {
            let _ = w.set_ignore_cursor_events(true);
            let _ = w.emit_to(w.label(), "capture:backdrop", ());
        } else {
            let _ = w.hide();
            let _ = w.emit_to(w.label(), "capture:reset", ());
        }
    }
}

/// 补充框、提示窗都是透明窗口，卡片四周留了画阴影的边距（逻辑像素）
pub const NOTE_SIZE: (f64, f64) = (660.0, 300.0);
pub const TOAST_SIZE: (f64, f64) = (420.0, 150.0);
pub const MAIN_SIZE: (f64, f64) = (900.0, 640.0);
pub const BAR_SIZE: (f64, f64) = (560.0, 64.0);

/// 软件启动时建好补充框和提示窗，用的时候直接显示，省掉加载页面的时间
pub fn create_aux_windows(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window("note").is_none() {
        WebviewWindowBuilder::new(app, "note", WebviewUrl::App("note.html".into()))
            .title("这一条在干啥？")
            .inner_size(NOTE_SIZE.0, NOTE_SIZE.1)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false)
            .build()?;
    }
    if app.get_webview_window("bar").is_none() {
        WebviewWindowBuilder::new(app, "bar", WebviewUrl::App("bar.html".into()))
            .title("再截一张")
            .inner_size(BAR_SIZE.0, BAR_SIZE.1)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .focused(false)
            // 不抢焦点：用户要在下面的窗口里翻聊天记录
            .focusable(false)
            .visible(false)
            .build()?;
    }
    if app.get_webview_window("toast").is_none() {
        WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("toast.html".into()))
            .title("每日工作梳理 · 提示")
            .inner_size(TOAST_SIZE.0, TOAST_SIZE.1)
            .decorations(false)
            .transparent(true)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .focused(false)
            // 不可获得焦点：显示时不会把用户正在打字的窗口的焦点抢走
            .focusable(false)
            .visible(false)
            .build()?;
    }
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteOpen {
    /// 这会是今天第几条
    pub index: usize,
    /// 各张图的地址（http://shot.localhost/文件名），按顺序
    pub shots: Vec<String>,
    /// 已经打的字
    pub note: String,
    /// 一条最多几张
    pub max: usize,
    /// 加图时的提示（多出来的没加、读不出来的文件）
    pub message: Option<String>,
}

/// 补充框放在截图那块屏的中下部，并拿到焦点
pub fn show_note(app: &AppHandle, monitor: (i32, i32, u32, u32, f64), payload: NoteOpen) {
    let Some(w) = app.get_webview_window("note") else { return };
    let (mx, my, mw, mh, scale) = monitor;
    let pw = (NOTE_SIZE.0 * scale).round() as u32;
    let ph = (NOTE_SIZE.1 * scale).round() as u32;
    let x = mx + (mw as i32 - pw as i32) / 2;
    let y = my + (mh as f64 * 0.70) as i32 - ph as i32 / 2;
    let _ = place_physical(&w, x, y, pw, ph);
    let _ = app.emit_to("note", "note:open", payload);
    let _ = w.show();
    let _ = w.set_focus();
    log::info!("note: 显示补充框");
}

pub fn hide_note(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("note") {
        let _ = w.hide();
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarView {
    /// 已经截了几张
    pub count: usize,
    pub hotkey: String,
}

/// "再截一张"的小条：截图那块屏顶部居中，不抢焦点
pub fn show_bar(app: &AppHandle, monitor: (i32, i32, u32, u32, f64), view: BarView) {
    let Some(w) = app.get_webview_window("bar") else { return };
    let (mx, my, mw, _mh, scale) = monitor;
    let pw = (BAR_SIZE.0 * scale).round() as u32;
    let ph = (BAR_SIZE.1 * scale).round() as u32;
    let x = mx + (mw as i32 - pw as i32) / 2;
    let y = my + (16.0 * scale) as i32;
    let _ = place_physical(&w, x, y, pw, ph);
    let _ = app.emit_to("bar", "bar:show", view);
    let _ = w.show();
}

pub fn hide_bar(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("bar") {
        let _ = w.hide();
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToastPayload {
    /// recognizing | done | failed | info | copied
    pub kind: String,
    pub id: String,
    pub index: usize,
    pub text: String,
    pub tags: Vec<String>,
    pub new_tags: Vec<String>,
    pub note: String,
    pub message: String,
    pub low_confidence: bool,
    /// 这条有几张图（识别中提示用）
    pub shots: usize,
}

/// 主屏右下角（避开任务栏），显示时不抢焦点
pub fn show_toast(app: &AppHandle, payload: ToastPayload) {
    let Some(w) = app.get_webview_window("toast") else { return };
    if let Ok(Some(m)) = app.primary_monitor() {
        let scale = m.scale_factor();
        let area = m.work_area();
        let pw = (TOAST_SIZE.0 * scale).round() as u32;
        let ph = (TOAST_SIZE.1 * scale).round() as u32;
        let x = area.position.x + area.size.width as i32 - pw as i32;
        let y = area.position.y + area.size.height as i32 - ph as i32;
        let _ = place_physical(&w, x, y, pw, ph);
    }
    let _ = app.emit_to("toast", "toast:show", payload);
    let _ = w.show();
}

pub fn hide_toast(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("toast") {
        let _ = w.hide();
    }
}

fn build_onboarding(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, "onboarding", WebviewUrl::App("onboarding.html".into()))
        .title("欢迎使用每日工作梳理")
        .inner_size(780.0, 580.0)
        .resizable(false)
        .maximizable(false)
        .theme(Some(tauri::Theme::Light))
        .center()
        .build()
}

/// 建主窗口（先不显示）
pub fn build_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("每日工作梳理")
        .inner_size(MAIN_SIZE.0, MAIN_SIZE.1)
        .min_inner_size(720.0, 520.0)
        .theme(Some(tauri::Theme::Light))
        .center()
        .visible(false)
        .build()
}

fn bring_front(w: &WebviewWindow) {
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
}

/// 打开主窗口并放到前台；还没走完引导时打开引导。
/// 窗口不存在时换个线程去建：托盘、菜单这些回调在主线程上，直接建窗口在 Windows 上可能卡死
pub fn show_main(app: &AppHandle) {
    let onboarded = app.state::<AppState>().settings.lock().unwrap().onboarding_done;
    let label = if onboarded { "main" } else { "onboarding" };
    if let Some(w) = app.get_webview_window(label) {
        bring_front(&w);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let built = if onboarded { build_main(&app) } else { build_onboarding(&app) };
        match built {
            Ok(w) => bring_front(&w),
            Err(e) => log::error!("打开窗口失败：{e}"),
        }
    });
}

/// 当前鼠标位置（物理像素）
pub fn cursor_pos() -> Option<(i32, i32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut p) };
    (ok != 0).then_some((p.x, p.y))
}
