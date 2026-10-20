//! 设置页、首次引导用到的命令。

use crate::ai::presets::{self, Preset};
use crate::config;
use crate::flow;
use crate::model::{now_iso, ModelConfig, Settings, TagDef, TagOrigin};
use crate::state::AppState;
use crate::tags;
use crate::views;
use crate::windows;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPrompts {
    pub vision: &'static str,
    pub summary: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// 设置内容，API Key 已掩码
    pub settings: Value,
    pub presets: &'static [Preset],
    pub default_prompts: DefaultPrompts,
    pub version: String,
    pub data_dir: String,
    /// 打包的节假日数据里没有今年
    pub holiday_stale: bool,
}

fn masked(s: &Settings) -> Value {
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    for section in ["vision", "summary"] {
        if let Some(k) = v.pointer_mut(&format!("/{section}/apiKey")) {
            *k = Value::String(config::mask_key(k.as_str().unwrap_or("")));
        }
    }
    v
}

fn save_err(e: std::io::Error) -> String {
    format!("保存失败：{e}")
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsView {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap();
    SettingsView {
        settings: masked(&settings),
        presets: presets::all(),
        default_prompts: DefaultPrompts {
            vision: crate::ai::prompt::default_vision_template(),
            summary: crate::ai::prompt::default_summary_template(),
        },
        version: app.package_info().version.to_string(),
        data_dir: state.store.dir().display().to_string(),
        holiday_stale: state.calendar.stale_for(views::today()),
    }
}

/// 局部保存；快捷键要走 set_hotkey
#[tauri::command]
pub fn save_settings(app: AppHandle, patch: Value) -> Result<Value, String> {
    if patch.get("hotkey").is_some() {
        return Err("快捷键请用「点一下再按新组合」的方式修改".into());
    }
    let state = app.state::<AppState>();
    let (next, autostart_changed) = {
        let mut settings = state.settings.lock().unwrap();
        let next = config::apply_patch(&settings, &patch)?;
        state.store.save_settings(&next).map_err(save_err)?;
        let changed = next.auto_start != settings.auto_start;
        *settings = next.clone();
        (next, changed)
    };
    if autostart_changed && next.onboarding_done {
        crate::system::sync_autostart(&app);
    }
    let _ = app.emit("settings-changed", ());
    Ok(masked(&next))
}

#[tauri::command]
pub fn reveal_api_key(app: AppHandle, which: String) -> String {
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap();
    if which == "summary" {
        s.summary.api_key.clone()
    } else {
        s.vision.api_key.clone()
    }
}

/// 先注销旧的再注册新的；新的注册不上就恢复旧的
#[tauri::command]
pub fn set_hotkey(app: AppHandle, accel: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let old = state.settings.lock().unwrap().hotkey.clone();
    let accel = accel.trim().to_string();
    if accel == old && state.status.lock().unwrap().hotkey_error.is_none() {
        return Ok(());
    }
    let gs = app.global_shortcut();
    let _ = gs.unregister(old.as_str());
    if let Err(e) = gs.register(accel.as_str()) {
        log::warn!("热键 {accel} 注册失败：{e}");
        let _ = gs.register(old.as_str());
        return Err(format!("{accel} 用不了，可能被别的软件占用了，换一个试试"));
    }
    {
        let mut s = state.settings.lock().unwrap();
        s.hotkey = accel;
        state.store.save_settings(&s).map_err(save_err)?;
    }
    state.status.lock().unwrap().hotkey_error = None;
    let _ = app.emit("settings-changed", ());
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub ok: bool,
    pub ms: u128,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    /// None = 不发截图，没测识图
    pub vision: Option<Probe>,
    pub summary: Probe,
}

async fn probe(cfg: &ModelConfig, messages: Value) -> Probe {
    let t0 = Instant::now();
    match crate::ai::client::chat(cfg, messages, Duration::from_secs(25)).await {
        Ok(_) => Probe { ok: true, ms: t0.elapsed().as_millis(), error: None },
        Err(e) => Probe { ok: false, ms: t0.elapsed().as_millis(), error: Some(e.to_string()) },
    }
}

/// 真的发一张小图测识图、发一句话测写总结
#[tauri::command]
pub async fn test_connection(app: AppHandle) -> ConnectionTest {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let vision = if settings.send_image_to_ai {
        let img = image::RgbaImage::from_fn(64, 64, |x, _| {
            if x < 32 { image::Rgba([0, 95, 184, 255]) } else { image::Rgba([255, 255, 255, 255]) }
        });
        let jpeg = crate::capture::ai_jpeg(&img);
        let messages = crate::recognize::build_messages("你是连接测试助手，用一个词回答。", "这张图左半边是什么颜色？", &[jpeg]);
        Some(probe(&settings.vision, messages).await)
    } else {
        None
    };
    let summary = probe(&settings.summary_config(), json!([{ "role": "user", "content": "连接测试，请只回复：好" }])).await;
    ConnectionTest { vision, summary }
}

/// 从贴进来的旧总结里归纳 3-8 个分类
#[tauri::command]
pub async fn suggest_tags(app: AppHandle, samples: String) -> Result<Vec<String>, String> {
    if samples.trim().is_empty() {
        return Err("先贴几份以前写过的总结".into());
    }
    let cfg = app.state::<AppState>().settings.lock().unwrap().summary_config();
    let messages = json!([
        { "role": "system", "content": "你帮用户整理工作分类。根据用户以前写的工作总结，归纳出 3-8 个工作分类标签，每个 2-6 个字，用项目名或业务名。只输出 JSON 字符串数组，不要别的文字。" },
        { "role": "user", "content": samples }
    ]);
    let raw = crate::ai::client::chat(&cfg, messages, Duration::from_secs(30)).await.map_err(|e| e.to_string())?;
    let found = config::parse_tag_list(&raw);
    if found.is_empty() {
        return Err("没认出分类，可以手动添加".into());
    }
    Ok(found)
}

/// 用户在引导页或设置页手动加的标签（直接算正式标签）
#[tauri::command]
pub fn add_tags(app: AppHandle, names: Vec<String>) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut s = state.settings.lock().unwrap();
        for name in names.iter().filter_map(|n| tags::normalize(n)) {
            match s.tags.iter_mut().find(|t| t.name == name) {
                Some(t) => t.pending = false,
                None => s.tags.push(TagDef { name, created_by: TagOrigin::User, created_at: now_iso(), pending: false }),
            }
        }
        state.store.save_settings(&s).map_err(save_err)?;
    }
    let _ = app.emit("tags-changed", ());
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagUsage {
    pub name: String,
    pub count: usize,
    pub by_ai: bool,
    pub is_new: bool,
}

#[tauri::command]
pub fn tag_usage(app: AppHandle) -> Vec<TagUsage> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap();
    let usage = tags::usage(&state.data.lock().unwrap());
    views::tag_views(&settings.tags, views::today())
        .into_iter()
        .map(|t| TagUsage { count: usage.get(&t.name).copied().unwrap_or(0), name: t.name, by_ai: t.by_ai, is_new: t.is_new })
        .collect()
}

#[tauri::command]
pub fn delete_tag(app: AppHandle, name: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        let mut data = state.data.lock().unwrap();
        tags::delete(&mut data, &mut settings, &name);
        state.store.save_data(&data).map_err(save_err)?;
        state.store.save_settings(&settings).map_err(save_err)?;
    }
    let _ = app.emit("tags-changed", ());
    let _ = app.emit("items-changed", "*");
    Ok(())
}

/// 引导完成：记下来、按选择设开机自启、打开主窗口；选了"试截一张"就直接开始截图。
/// 会新建窗口，必须是 async：Windows 上在同步命令里建窗口会卡死
#[tauri::command]
pub async fn finish_onboarding(app: AppHandle, try_capture: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut s = state.settings.lock().unwrap();
        onboarding_defaults(&mut s);
        s.onboarding_done = true;
        state.store.save_settings(&s).map_err(save_err)?;
    }
    crate::system::sync_autostart(&app);
    if let Some(w) = app.get_webview_window("onboarding") {
        let _ = w.close();
    }
    if try_capture {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            flow::start_capture(app, crate::capture_mode::Origin::Onboarding);
        });
    } else {
        windows::show_main(&app);
    }
    Ok(())
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = app.state::<AppState>().store.dir().display().to_string();
    app.opener().open_path(dir, None::<&str>).map_err(|e| e.to_string())
}

/// 只允许打开 https 链接（申请 Key 教程、GitHub 发布页）
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !url.starts_with("https://") {
        return Err("只能打开 https 链接".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

// ───────────── 检查更新 ─────────────

#[tauri::command]
pub async fn check_update(app: AppHandle, manual: bool) -> Result<crate::update::UpdateInfo, String> {
    crate::update::check(&app, manual).await
}

/// 横幅上的"稍后"：这个版本 3 天内不再提示
#[tauri::command]
pub fn snooze_update(app: AppHandle, version: String) -> Result<(), String> {
    let v = semver::Version::parse(&version).map_err(|e| e.to_string())?;
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap();
    crate::update::snooze(&mut s.update, &v, chrono::Local::now().naive_local());
    state.store.save_settings(&s).map_err(save_err)?;
    *state.update.lock().unwrap() = None;
    Ok(())
}

/// 主窗口打开时问一下：之前查到过的新版本还要不要显示
#[tauri::command]
pub fn pending_update(app: AppHandle) -> Option<crate::update::UpdateInfo> {
    app.state::<AppState>().update.lock().unwrap().clone()
}

/// 走完引导时，没选过"框选后按回车"的按"复制"
pub fn onboarding_defaults(s: &mut Settings) {
    if s.enter_action.is_none() {
        s.enter_action = Some(crate::model::EnterAction::Copy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EnterAction, Settings};

    #[test]
    fn onboarding_defaults_sets_copy_once() {
        let mut s = Settings::default();
        onboarding_defaults(&mut s);
        assert_eq!(s.enter_action, Some(EnterAction::Copy));
        s.enter_action = Some(EnterAction::Record);
        onboarding_defaults(&mut s);
        assert_eq!(s.enter_action, Some(EnterAction::Record));
    }
}
