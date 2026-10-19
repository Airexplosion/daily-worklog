//! 系统集成：托盘、关闭到托盘、下班提醒循环、开机自启同步。

use crate::model::now_iso;
use crate::reminder;
use crate::state::AppState;
use crate::windows::{self, ToastPayload};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;

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
            "capture" => crate::flow::start_capture(app.clone()),
            "open" => {
                windows::show_main(app);
            }
            "summary" => {
                if windows::show_main(app).is_some() {
                    let _ = app.emit_to("main", "main:open-summary", ());
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event {
                windows::show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// 主窗口点关闭 = 藏到托盘；第一次给个提示
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.hide();
        let app = window.app_handle();
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
                },
            );
        }
    }
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

fn check_reminder(app: &AppHandle) {
    let state = app.state::<AppState>();
    // 正在截图时先不打扰，下一轮再看
    if state.capturing.load(std::sync::atomic::Ordering::SeqCst) {
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
