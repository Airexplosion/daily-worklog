//! 系统集成：托盘、关闭到托盘、下班提醒循环、开机自启同步。

use crate::model::now_iso;
use crate::reminder;
use crate::state::AppState;
use crate::windows::{self, ToastPayload};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;

#[derive(Debug, PartialEq, Eq)]
pub enum CloseAction {
    /// 主窗口：藏到托盘
    HideMain,
    /// 补充框（Alt+F4）：当作"跳过"
    SkipNote,
    /// 框选窗口（Alt+F4）：取消这次截图
    CancelCapture,
    /// 提示窗：藏起来，窗口留着下次用
    Hide,
    Allow,
}

/// 各窗口点关闭（含 Alt+F4）时怎么办。辅助窗口只建一次，不能真的被关掉
pub fn close_action(label: &str) -> CloseAction {
    match label {
        "main" => CloseAction::HideMain,
        "note" => CloseAction::SkipNote,
        "toast" | "bar" => CloseAction::Hide,
        l if l.starts_with("capture-") => CloseAction::CancelCapture,
        _ => CloseAction::Allow,
    }
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "capture", "截一张", true, None::<&str>)?,
            &MenuItem::with_id(app, "open", "打开主窗口", true, None::<&str>)?,
            &MenuItem::with_id(app, "summary", "生成今日总结", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("每日工作梳理")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture" => {
                // 等托盘菜单淡出再抓屏，免得被截进定格画面（框选窗口能先显示的话会马上显示）
                let app = app.clone();
                std::thread::spawn(move || {
                    crate::flow::start_capture_after(app, crate::capture_mode::Origin::Tray, std::time::Duration::from_millis(250));
                });
            }
            "open" => {
                windows::show_main(app);
            }
            "summary" => crate::commands::open_main_with(app, crate::state::MainIntent::OpenSummary),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event {
                windows::show_main(tray.app_handle());
            }
        });
    // 托盘用 32px 的图，缩放 125%/150% 时系统缩小得更清楚（默认窗口图标取的是 256px）
    match tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png")) {
        Ok(icon) => builder = builder.icon(icon),
        Err(_) => {
            if let Some(icon) = app.default_window_icon() {
                builder = builder.icon(icon.clone());
            }
        }
    }
    builder.build(app)?;
    Ok(())
}

/// 主窗口点关闭 = 藏到托盘（第一次给个提示）；辅助窗口按 close_action 处理
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else { return };
    let action = close_action(window.label());
    if action == CloseAction::Allow {
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    let app = window.app_handle().clone();
    match action {
        CloseAction::SkipNote => {
            std::thread::spawn(move || {
                if crate::flow::commit_draft(&app, "").is_err() {
                    crate::flow::cancel(&app);
                }
            });
        }
        CloseAction::CancelCapture => {
            std::thread::spawn(move || crate::flow::cancel(&app));
        }
        CloseAction::HideMain => first_close_tip(&app),
        _ => {}
    }
}

/// 第一次把主窗口关到托盘时提示一下
fn first_close_tip(app: &AppHandle) {
    let state = app.state::<AppState>();
    let first = {
        let mut s = state.settings.lock().unwrap();
        let first = !s.close_to_tray_tip_shown;
        if first {
            s.close_to_tray_tip_shown = true;
            let _ = state.store.save_settings(&s);
        }
        first
    };
    if first {
        windows::show_toast(
            app,
            ToastPayload {
                kind: "info".into(),
                id: String::new(),
                index: 0,
                text: "已最小化到托盘，不会退出".into(),
                tags: Vec::new(),
                new_tags: Vec::new(),
                note: String::new(),
                message: "双击右下角托盘图标可以重新打开".into(),
                low_confidence: false,
                shots: 0,
            },
        );
    }
}

/// 自启时没有主窗口，快捷键注册失败要用右下角提示告诉用户（等提示窗页面加载好再发）
pub fn notify_hotkey_failed(app: AppHandle, hotkey: String) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(4));
        windows::show_toast(
            &app,
            ToastPayload {
                kind: "info".into(),
                id: String::new(),
                index: 0,
                text: format!("截图快捷键 {hotkey} 用不了"),
                tags: Vec::new(),
                new_tags: Vec::new(),
                note: String::new(),
                message: "可能被别的软件占用了，打开主窗口在设置里换一个".into(),
                low_confidence: false,
                shots: 0,
            },
        );
    });
}

/// 每 30 秒看一次钟：改系统时间、睡眠唤醒后都能正确触发；启动后立刻检查一次（补弹）
pub fn spawn_reminder_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tick.tick().await;
            check_reminder(&app);
        }
    });
}

/// 一直挂在托盘里的也要清理过期截图：日期变了就清一次
fn purge_daily(app: &AppHandle) {
    static LAST: std::sync::Mutex<Option<chrono::NaiveDate>> = std::sync::Mutex::new(None);
    let today = chrono::Local::now().date_naive();
    let mut last = LAST.lock().unwrap();
    if *last == Some(today) {
        return;
    }
    *last = Some(today);
    let state = app.state::<AppState>();
    let days = state.settings.lock().unwrap().shots_retention_days;
    let n = state.store.purge_shots(days, today);
    if n > 0 {
        log::info!("清理了 {n} 张超过 {days} 天的截图");
    }
}

fn check_reminder(app: &AppHandle) {
    purge_daily(app);
    let state = app.state::<AppState>();
    // 正在截图、还有一条没记完时先不打扰，下一轮再看
    if state.capturing.load(std::sync::atomic::Ordering::SeqCst) || state.draft.lock().unwrap().is_some() {
        return;
    }
    let now = chrono::Local::now().naive_local();
    let date = now.date().format("%Y-%m-%d").to_string();
    let settings = state.settings.lock().unwrap().clone();
    if !settings.onboarding_done {
        return;
    }
    let count = {
        let mut data = state.data.lock().unwrap();
        let day = data.days.get(&date);
        if !reminder::should_remind(now, &settings, day, &state.calendar) {
            return;
        }
        let entry = data.days.entry(date.clone()).or_default();
        entry.reminded_at = Some(now_iso());
        let count = entry.items.len();
        if let Err(e) = state.store.save_data(&data) {
            log::error!("保存失败：{e}");
        }
        count
    };
    log::info!("下班提醒：{date}，已记录 {count} 条");
    windows::show_main(app);
    let _ = app.emit_to("main", "main:remind", count);
    if let Err(e) = app
        .notification()
        .builder()
        .title("该下班了")
        .body(format!("今天的工作总结可以生成了，已记录 {count} 条"))
        .show()
    {
        log::warn!("系统通知没发出去：{e}");
    }
}

/// 设置里的开机自启和系统里的保持一致。调试版和测试数据目录下不碰注册表
pub fn sync_autostart(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    if cfg!(debug_assertions) || std::env::var_os("DWL_DATA_DIR").is_some() {
        return;
    }
    let want = app.state::<AppState>().settings.lock().unwrap().auto_start;
    let launcher = app.autolaunch();
    let has = launcher.is_enabled().unwrap_or(false);
    let result = match (want, has) {
        (true, false) => launcher.enable(),
        (false, true) => launcher.disable(),
        _ => Ok(()),
    };
    if let Err(e) = result {
        log::warn!("开机自启设置失败：{e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_action_per_window() {
        assert_eq!(close_action("main"), CloseAction::HideMain);
        assert_eq!(close_action("note"), CloseAction::SkipNote);
        assert_eq!(close_action("capture-0"), CloseAction::CancelCapture);
        assert_eq!(close_action("capture-12"), CloseAction::CancelCapture);
        assert_eq!(close_action("toast"), CloseAction::Hide);
        assert_eq!(close_action("bar"), CloseAction::Hide);
        assert_eq!(close_action("onboarding"), CloseAction::Allow);
    }
}
