pub mod ai;
pub mod capture;
pub mod commands;
pub mod config;
pub mod flow;
pub mod format;
pub mod holidays;
pub mod items;
pub mod model;
pub mod recognize;
pub mod reminder;
pub mod settings_cmds;
pub mod state;
pub mod storage;
pub mod summary;
pub mod system;
pub mod tags;
pub mod views;
pub mod windows;

use state::AppState;
use storage::Store;
use tauri::http::Response;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};

pub fn run() {
    let store = Store::new(storage::data_dir());
    let logs_dir = store.logs_dir();

    tauri::Builder::default()
        // 重复打开 exe：不开新进程，只把已有的主窗口叫出来
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_main(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(Target::new(TargetKind::Folder { path: logs_dir, file_name: Some("app".into()) }))
                .target(Target::new(TargetKind::Stdout))
                .level(log::LevelFilter::Info)
                .timezone_strategy(TimezoneStrategy::UseLocal)
                .max_file_size(1_000_000)
                .rotation_strategy(RotationStrategy::KeepSome(5))
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .on_window_event(system::on_window_event)
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        flow::start_capture(app.clone());
                    }
                })
                .build(),
        )
        // 框选窗口的背景：http://frozen.localhost/{屏序号}，返回定格画面 BMP
        .register_asynchronous_uri_scheme_protocol("frozen", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let index: usize = request.uri().path().trim_start_matches('/').parse().unwrap_or(usize::MAX);
            std::thread::spawn(move || {
                let t0 = std::time::Instant::now();
                let state = app.state::<AppState>();
                let bmp = {
                    let frozen = state.frozen.lock().unwrap();
                    frozen
                        .as_ref()
                        .and_then(|f| f.monitors.iter().find(|m| m.index == index))
                        .map(|m| capture::to_bmp(&m.image))
                };
                let response = match bmp {
                    Some(body) => Response::builder()
                        .header("Content-Type", "image/bmp")
                        .header("Cache-Control", "no-store")
                        .body(body),
                    None => Response::builder().status(404).body(Vec::new()),
                };
                log::info!("capture: 屏 {index} 定格画面编码 {}ms", t0.elapsed().as_millis());
                responder.respond(response.unwrap());
            });
        })
        // 截图原图：http://shot.localhost/{文件名}，只允许 shots 目录下的 png
        .register_asynchronous_uri_scheme_protocol("shot", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let name = request.uri().path().rsplit('/').next().unwrap_or("").to_string();
            std::thread::spawn(move || {
                let valid = name.ends_with(".png") && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
                let body = valid
                    .then(|| std::fs::read(app.state::<AppState>().store.shots_dir().join(&name)).ok())
                    .flatten();
                let response = match body {
                    Some(b) => Response::builder().header("Content-Type", "image/png").header("Cache-Control", "max-age=86400").body(b),
                    None => Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(response.unwrap());
            });
        })
        .setup(move |app| {
            let mut loaded = store.load_data();
            // 上次退出时还在识别的条目，改成待处理，等用户手动重试
            let mut interrupted = false;
            for item in loaded.data.days.values_mut().flat_map(|d| d.items.iter_mut()) {
                if item.status == model::ItemStatus::Recognizing {
                    item.status = model::ItemStatus::Pending;
                    item.error = Some("上次没识别完".into());
                    interrupted = true;
                }
            }
            if interrupted {
                let _ = store.save_data(&loaded.data);
            }
            let settings = store.load_settings();
            let hotkey = settings.hotkey.clone();
            let retention = settings.shots_retention_days;
            app.manage(AppState::new(store.clone(), loaded.data, settings));
            {
                let state = app.state::<AppState>();
                let mut status = state.status.lock().unwrap();
                status.recovered_from = loaded.recovered_from.clone();
                status.data_lost = loaded.lost;
            }
            if let Some(name) = &loaded.recovered_from {
                log::warn!("data.json 损坏，已从备份 {name} 恢复");
            }
            if let Err(e) = store.backup_on_startup(chrono::Local::now().naive_local()) {
                log::error!("启动备份失败：{e}");
            }
            let purged = store.purge_shots(retention, chrono::Local::now().date_naive());
            if purged > 0 {
                log::info!("清理了 {purged} 张超过 {retention} 天的截图");
            }
            let worker = recognize::spawn_worker(app.handle().clone());
            *app.state::<AppState>().queue.lock().unwrap() = Some(worker);
            spawn_trash_sweeper(app.handle().clone());
            if let Err(e) = windows::create_aux_windows(app.handle()) {
                log::error!("创建补充框/提示窗失败：{e}");
            }
            if let Err(e) = app.global_shortcut().register(hotkey.as_str()) {
                log::error!("热键 {hotkey} 注册失败：{e}");
                app.state::<AppState>().status.lock().unwrap().hotkey_error = Some(hotkey.clone());
            }
            if let Err(e) = system::build_tray(app.handle()) {
                log::error!("托盘图标创建失败：{e}");
            }
            system::spawn_reminder_loop(app.handle().clone());
            if app.state::<AppState>().settings.lock().unwrap().onboarding_done {
                system::sync_autostart(app.handle());
            }
            if let Err(e) = windows::ensure_overlays_for(app.handle(), &windows::monitor_geometry()) {
                log::error!("创建框选窗口失败：{e}");
            }
            // 开机自启（带 --autostart）只进托盘，手动打开时显示主窗口
            if !std::env::args().any(|a| a == "--autostart") {
                windows::show_main(app.handle());
            }
            log::info!("启动完成，数据目录 {}", store.dir().display());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::overlay_init,
            commands::overlay_ready,
            commands::capture_done,
            commands::capture_cancel,
            commands::note_current,
            commands::note_submit,
            commands::note_skip,
            commands::toast_hide,
            commands::toast_open_item,
            commands::today,
            commands::get_day,
            commands::get_tags,
            commands::add_item,
            commands::update_item,
            commands::delete_item,
            commands::undo_delete,
            commands::retry_item,
            commands::retry_all,
            commands::confirm_tag,
            commands::rename_tag,
            commands::mute_today,
            commands::capture_from_main,
            commands::generate_summary,
            commands::save_summary,
            commands::undo_regenerate,
            commands::copy_text,
            commands::get_status,
            commands::list_days,
            settings_cmds::get_settings,
            settings_cmds::save_settings,
            settings_cmds::reveal_api_key,
            settings_cmds::set_hotkey,
            settings_cmds::test_connection,
            settings_cmds::suggest_tags,
            settings_cmds::add_tags,
            settings_cmds::tag_usage,
            settings_cmds::delete_tag,
            settings_cmds::finish_onboarding,
            settings_cmds::open_data_dir,
            settings_cmds::open_url,
            commands::dismiss_missed,
            commands::dismiss_recovered,
        ])
        .run(tauri::generate_context!())
        .expect("启动失败");
}

/// 每秒检查一次：过了撤销期的删除，真正删掉截图文件
fn spawn_trash_sweeper(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tick.tick().await;
            let state = app.state::<AppState>();
            let expired = state.trash.lock().unwrap().expired(std::time::Instant::now());
            for removed in expired {
                for shot in &removed.item.shots {
                    let _ = std::fs::remove_file(state.store.dir().join(shot));
                }
            }
        }
    });
}
