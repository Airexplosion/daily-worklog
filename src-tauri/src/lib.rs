pub mod ai;
pub mod capture;
pub mod commands;
pub mod flow;
pub mod format;
pub mod holidays;
pub mod model;
pub mod reminder;
pub mod state;
pub mod storage;
pub mod tags;
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
        .setup(move |app| {
            let loaded = store.load_data();
            let settings = store.load_settings();
            let hotkey = settings.hotkey.clone();
            app.manage(AppState::new(store.clone(), loaded.data, settings));
            if let Err(e) = app.global_shortcut().register(hotkey.as_str()) {
                log::error!("热键 {hotkey} 注册失败：{e}");
            }
            if let Err(e) = windows::ensure_overlays_for(app.handle(), &windows::monitor_geometry()) {
                log::error!("创建框选窗口失败：{e}");
            }
            log::info!("启动完成，数据目录 {}", store.dir().display());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::overlay_init,
            commands::overlay_ready,
            commands::capture_done,
            commands::capture_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
