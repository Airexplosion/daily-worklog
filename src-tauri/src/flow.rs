//! 截图主流程：热键 → 定格 → 框选（可标注）→ 复制 / 另存 / 记录进草稿 → 补充说明 → 落盘 → 后台识别。

use crate::capture::{self, Frozen, PhysRect};
use crate::capture_mode::{self, Action, LastRect, Origin};
use crate::draft::{Draft, MAX_SHOTS};
use crate::model::{now_iso, EnterAction};
use crate::recognize::{self, Job};
use crate::state::{AppState, Session};
use crate::windows::{self, BarView, NoteOpen, ToastPayload};
use image::RgbaImage;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

/// 框选页面要的：定格画面、主按钮、第几张、上次的框、升级提示
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInfo {
    pub url: String,
    pub primary: Action,
    /// 有没记完的一条时是第几张（草稿张数 + 1），否则 1
    pub shot_no: usize,
    /// 物理像素；只在"加到这条"且同屏同几何时给
    pub last_rect: Option<RectView>,
    pub hint: Option<String>,
    /// 没记完的那条已经满 9 张：不能再加，只能复制或另存
    pub full: bool,
}

#[derive(Clone, Copy, Serialize)]
pub struct RectView {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl From<PhysRect> for RectView {
    fn from(r: PhysRect) -> Self {
        RectView { x: r.x, y: r.y, w: r.w, h: r.h }
    }
}

/// 热键、托盘、主窗口、引导都从这里开始。截图流程进行中时再按无效
pub fn start_capture(app: AppHandle, origin: Origin) {
    let state = app.state::<AppState>();
    if state.capturing.swap(true, Ordering::SeqCst) {
        return;
    }
    let draft_state = match state.draft.lock().unwrap().as_ref() {
        None => capture_mode::DraftState::None,
        Some(d) if d.is_full() => capture_mode::DraftState::Full,
        Some(_) => capture_mode::DraftState::Open,
    };
    let has_draft = draft_state != capture_mode::DraftState::None;
    if has_draft {
        // 还没记完的一条：小窗、小条、变暗背景先收起来，这一张截下来加到这条里
        windows::hide_note(&app);
        windows::hide_bar(&app);
        windows::hide_overlays(&app);
    }
    let session = {
        let s = state.settings.lock().unwrap();
        // 升级提示只给回车是"复制"的那种截图（热键、托盘，且不是在加到这条）；页面真显示了才算看过（hint_seen）
        let by_key = matches!(origin, Origin::Hotkey | Origin::Tray) && !has_draft;
        let hint = (by_key && capture_mode::needs_upgrade_hint(&s)).then(|| capture_mode::UPGRADE_HINT.to_string());
        Session { origin, primary: capture_mode::primary(origin, draft_state, s.enter_action), hint }
    };
    *state.session.lock().unwrap() = Some(session);
    let toast_was_visible = app.get_webview_window("toast").and_then(|w| w.is_visible().ok()).unwrap_or(false);
    windows::hide_toast(&app);
    std::thread::spawn(move || {
        // 刚藏起来的提示窗、补一句小窗，等它们真的消失再截
        if toast_was_visible || has_draft {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        let t0 = Instant::now();
        let state = app.state::<AppState>();
        let frozen = match capture::grab_all() {
            Ok(f) => f,
            Err(e) => {
                log::error!("{e}");
                // 收起的小窗、主窗口都要回来，不能让没记完的那一条看不见
                cancel(&app);
                return;
            }
        };
        log::info!("capture: grab {} 块屏 {}ms", frozen.monitors.len(), t0.elapsed().as_millis());
        if let Err(e) = windows::ensure_overlays(&app, &frozen) {
            log::error!("创建框选窗口失败：{e}");
            cancel(&app);
            return;
        }
        let taken_at = frozen.taken_at;
        let infos: Vec<(usize, CaptureInfo)> =
            frozen.monitors.iter().map(|m| (m.index, capture_info(&state, &frozen, m.index))).collect();
        *state.frozen.lock().unwrap() = Some(frozen);
        state.overlay_shown.store(false, Ordering::SeqCst);
        windows::start_overlays(&app, infos);
        // 兜底：5 秒内一块框选画面都没出来，就结束这次截图，免得后面的热键一直无效
        std::thread::sleep(std::time::Duration::from_secs(5));
        let same_capture = state.frozen.lock().unwrap().as_ref().is_some_and(|f| f.taken_at == taken_at);
        if same_capture && !state.overlay_shown.load(Ordering::SeqCst) {
            log::error!("capture: 框选画面 5 秒内没有出现，已取消");
            cancel(&app);
        }
    });
}

fn end_session(state: &AppState) {
    *state.session.lock().unwrap() = None;
    state.capturing.store(false, Ordering::SeqCst);
}

fn capture_info(state: &AppState, frozen: &Frozen, index: usize) -> CaptureInfo {
    let (primary, hint) =
        state.session.lock().unwrap().as_ref().map(|s| (s.primary, s.hint.clone())).unwrap_or((Action::Copy, None));
    let (shot_no, full) = state.draft.lock().unwrap().as_ref().map_or((1, false), |d| (d.shots.len() + 1, d.is_full()));
    let geometry = frozen.monitors.iter().find(|m| m.index == index).map(|m| (m.x, m.y, m.width, m.height));
    let last_rect = match (primary, geometry) {
        (Action::Add, Some(g)) => capture_mode::last_rect_for(state.last_rect.lock().unwrap().as_ref(), index, g).map(RectView::from),
        _ => None,
    };
    CaptureInfo { url: windows::frozen_url(frozen, index), primary, shot_no, last_rect, hint, full }
}

/// 框选页面启动时调用：如果正在截图，返回这块屏的定格画面和主按钮等
pub fn overlay_init(app: &AppHandle, index: usize) -> Option<CaptureInfo> {
    let state = app.state::<AppState>();
    let frozen = state.frozen.lock().unwrap();
    let f = frozen.as_ref().filter(|f| f.monitors.iter().any(|m| m.index == index))?;
    Some(capture_info(&state, f, index))
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

/// Esc、右键、关窗口、出错：结束这次截图；有没记完的一条就回到补一句小窗
pub fn cancel(app: &AppHandle) {
    let state = app.state::<AppState>();
    windows::hide_overlays(app);
    *state.frozen.lock().unwrap() = None;
    end_session(&state);
    after_capture(app);
}

/// 这次截图结束（复制、另存、取消）：有没记完的一条就回到它；没有就把主窗口叫回来（如果是从主窗口开始的）
fn after_capture(app: &AppHandle) {
    let has_draft = app.state::<AppState>().draft.lock().unwrap().is_some();
    if has_draft {
        show_draft_note(app, false, None);
    } else {
        restore_main_if_needed(app);
    }
}

/// 从主窗口点"截一张"的，截完（或取消）把主窗口叫回来
fn restore_main_if_needed(app: &AppHandle) {
    if app.state::<AppState>().restore_main.swap(false, Ordering::SeqCst) {
        windows::show_main(app);
    }
}

/// 没有标注：从定格原图裁剪
pub fn finish_rect(app: &AppHandle, index: usize, rect: PhysRect, action: Action) -> Result<(), String> {
    let img = {
        let state = app.state::<AppState>();
        let frozen = state.frozen.lock().unwrap();
        let f = frozen.as_ref().ok_or("截图已失效")?;
        capture::crop(f, index, rect).ok_or("选区不在屏幕内")?
    };
    finish(app, index, rect, img, action)
}

/// 框选完：复制、另存、记录进草稿
pub fn finish(app: &AppHandle, index: usize, rect: PhysRect, img: RgbaImage, action: Action) -> Result<(), String> {
    let state = app.state::<AppState>();
    let has_draft = state.draft.lock().unwrap().is_some();
    let action = capture_mode::resolve(action, has_draft);
    let frozen = state.frozen.lock().unwrap().take().ok_or("截图已失效")?;
    *state.session.lock().unwrap() = None;
    let m = frozen.monitors.iter().find(|m| m.index == index).ok_or("选区不在屏幕内")?;
    let monitor = (m.x, m.y, m.width, m.height, m.scale);
    log::info!("capture: 屏 {index} 选区 {:?} → {:?}", rect, action);
    match action {
        Action::Copy => {
            windows::hide_overlays(app);
            state.capturing.store(false, Ordering::SeqCst);
            let (w, h) = img.dimensions();
            app.clipboard()
                .write_image(&tauri::image::Image::new_owned(img.into_raw(), w, h))
                .map_err(|e| format!("复制失败：{e}"))?;
            flash(app, "已复制");
            after_capture(app);
            Ok(())
        }
        Action::Save => {
            windows::hide_overlays(app);
            state.capturing.store(false, Ordering::SeqCst);
            let app = app.clone();
            std::thread::spawn(move || {
                save_as(&app, &img);
                after_capture(&app);
            });
            Ok(())
        }
        Action::Record => {
            let taken_at = frozen.taken_at;
            let id = capture::shot_id(taken_at.naive_local());
            let rel = save_shot(&state, &id, &img)?;
            {
                // 同一把锁里判断和写入：截图期间别处（比如加图）刚建了草稿，就加进去，不覆盖
                let mut draft = state.draft.lock().unwrap();
                let d = draft.get_or_insert_with(|| {
                    Draft::new(
                        &id,
                        &taken_at.format("%Y-%m-%d").to_string(),
                        &taken_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
                        monitor,
                    )
                });
                d.add_shots(&[rel]);
                if let Err(e) = state.store.save_draft(d) {
                    log::error!("保存草稿失败：{e}");
                }
            }
            *state.last_rect.lock().unwrap() = Some(LastRect { index, monitor: (m.x, m.y, m.width, m.height), rect });
            // 截图那块屏的框选窗口留着当补一句小窗的变暗背景，其他屏的收起
            windows::keep_backdrop(app, index);
            state.capturing.store(false, Ordering::SeqCst);
            show_draft_note(app, true, None);
            Ok(())
        }
        Action::Add => {
            let full = state.draft.lock().unwrap().as_ref().is_none_or(|d| d.is_full());
            if full {
                windows::hide_overlays(app);
                state.capturing.store(false, Ordering::SeqCst);
                show_draft_note(app, false, Some(format!("一条最多 {MAX_SHOTS} 张，多出来的没加")));
                return Ok(());
            }
            let rel = save_shot(&state, &capture::shot_id(frozen.taken_at.naive_local()), &img)?;
            if let Some(d) = state.draft.lock().unwrap().as_mut() {
                d.add_shots(&[rel]);
                if let Err(e) = state.store.save_draft(d) {
                    log::error!("保存草稿失败：{e}");
                }
            }
            *state.last_rect.lock().unwrap() = Some(LastRect { index, monitor: (m.x, m.y, m.width, m.height), rect });
            windows::keep_backdrop(app, index);
            state.capturing.store(false, Ordering::SeqCst);
            show_draft_note(app, true, None);
            Ok(())
        }
    }
}

/// 存档用的 PNG 原图，返回相对数据目录的路径
fn save_shot(state: &AppState, id: &str, img: &RgbaImage) -> Result<String, String> {
    let rel = format!("shots/{id}.png");
    crate::storage::write_atomic(&state.store.dir().join(&rel), &capture::png_bytes(img))
        .map_err(|e| format!("保存截图失败：{e}"))?;
    Ok(rel)
}

/// 另存为：默认上次存的文件夹，第一次是"图片"文件夹
fn save_as(app: &AppHandle, img: &RgbaImage) {
    let state = app.state::<AppState>();
    let last = state.settings.lock().unwrap().last_save_dir.clone();
    let dir = last.map(std::path::PathBuf::from).filter(|p| p.is_dir()).or_else(|| app.path().picture_dir().ok());
    let mut dialog = app
        .dialog()
        .file()
        .set_title("另存为")
        .set_file_name(capture_mode::save_file_name(chrono::Local::now().naive_local()))
        .add_filter("PNG 图片", &["png"]);
    if let Some(d) = dir {
        dialog = dialog.set_directory(d);
    }
    let Some(picked) = dialog.blocking_save_file() else { return };
    let Ok(picked) = picked.into_path() else { return };
    let path = capture_mode::save_target(&picked, |p| p.exists());
    match std::fs::write(&path, capture::png_bytes(img)) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                let mut s = state.settings.lock().unwrap();
                s.last_save_dir = Some(parent.display().to_string());
                if let Err(e) = state.store.save_settings(&s) {
                    log::error!("保存设置失败：{e}");
                }
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            flash(app, &format!("已保存到 {name}"));
        }
        Err(e) => {
            log::error!("另存为失败：{e}");
            flash(app, "保存失败，换个文件夹试试");
        }
    }
}

/// 右下角闪一下（约 1.5 秒）
fn flash(app: &AppHandle, text: &str) {
    windows::show_toast(
        app,
        ToastPayload {
            kind: "copied".into(),
            id: String::new(),
            index: 0,
            text: text.into(),
            tags: Vec::new(),
            new_tags: Vec::new(),
            note: String::new(),
            message: String::new(),
            low_confidence: false,
            shots: 0,
        },
    );
}

fn shot_url(rel: &str) -> String {
    format!("http://shot.localhost/{}", rel.rsplit('/').next().unwrap_or(rel))
}

/// 补一句小窗的内容（页面刚加载、错过 note:open 时也用它）
pub fn note_open(app: &AppHandle, message: Option<String>) -> Option<NoteOpen> {
    let state = app.state::<AppState>();
    let draft = state.draft.lock().unwrap();
    let d = draft.as_ref()?;
    let index = state.data.lock().unwrap().days.get(&d.date).map_or(0, |day| day.items.len()) + 1;
    Some(NoteOpen {
        index,
        shots: d.shots.iter().map(|r| shot_url(r)).collect(),
        note: d.note.clone(),
        max: MAX_SHOTS,
        message,
    })
}

/// 显示补一句小窗。backdrop = 背后留着截图那块屏的变暗定格画面
pub fn show_draft_note(app: &AppHandle, backdrop: bool, message: Option<String>) {
    let state = app.state::<AppState>();
    let Some(monitor) = state.draft.lock().unwrap().as_ref().map(|d| d.monitor) else { return };
    let Some(open) = note_open(app, message) else { return };
    if !backdrop {
        windows::hide_overlays(app);
    }
    windows::show_note(app, monitor, open);
}

/// 回车或跳过：条目立刻落盘（识别中），补一句小窗关掉，右下角提示，后台排队识别
pub fn commit_draft(app: &AppHandle, note: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut d = state.draft.lock().unwrap().take().ok_or("没有待记录的截图")?;
    windows::hide_note(app);
    windows::hide_bar(app);
    windows::hide_overlays(app);
    d.note = note.to_string();
    let date = d.date.clone();
    let shots = d.shots.len();
    let item = d.into_item();
    let id = item.id.clone();
    let note = item.note.clone();
    let index = {
        let mut data = state.data.lock().unwrap();
        let day = data.days.entry(date.clone()).or_default();
        day.items.push(item);
        day.items_changed_at = Some(now_iso());
        // 存盘失败（比如被杀毒软件锁住）也照常排队识别，识别完会再存一次
        if let Err(e) = state.store.save_data(&data) {
            log::error!("保存失败：{e}");
        }
        day_len(&data, &date)
    };
    // 条目存好以后再删草稿：中间断电，下次启动按 id 去重
    state.store.clear_draft();
    let _ = app.emit("items-changed", &date);
    windows::show_toast(
        app,
        ToastPayload {
            kind: "recognizing".into(),
            id: id.clone(),
            index,
            text: String::new(),
            tags: Vec::new(),
            new_tags: Vec::new(),
            note,
            message: String::new(),
            low_confidence: false,
            shots,
        },
    );
    recognize::enqueue(app, Job { date, id });
    restore_main_if_needed(app);
    Ok(())
}

fn day_len(data: &crate::model::Data, date: &str) -> usize {
    data.days.get(date).map_or(0, |d| d.items.len())
}

/// 刷新补一句小窗的内容，不改位置、不抢焦点
fn refresh_note(app: &AppHandle, message: Option<String>) {
    if let Some(open) = note_open(app, message) {
        let _ = app.emit_to("note", "note:open", open);
    }
}

fn bar_view(state: &AppState) -> Option<BarView> {
    let count = state.draft.lock().unwrap().as_ref()?.shots.len();
    Some(BarView { count, hotkey: state.settings.lock().unwrap().hotkey.clone() })
}

/// "再截一张"：收起小窗和变暗背景，屏幕恢复可操作，顶部挂一个小条
pub fn note_more(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(monitor) = state.draft.lock().unwrap().as_ref().map(|d| d.monitor) else { return };
    let Some(view) = bar_view(&state) else { return };
    windows::hide_note(app);
    windows::hide_overlays(app);
    windows::show_bar(app, monitor, view);
}

pub fn bar_current(app: &AppHandle) -> Option<BarView> {
    bar_view(&app.state::<AppState>())
}

/// 小条上的"不截了，去写说明"
pub fn bar_done(app: &AppHandle) {
    windows::hide_bar(app);
    show_draft_note(app, false, None);
}

/// 输入框里的字随时存进草稿（"再截一张"来回、崩溃后都还在）
pub fn note_text(app: &AppHandle, text: &str) {
    let state = app.state::<AppState>();
    let mut draft = state.draft.lock().unwrap();
    if let Some(d) = draft.as_mut() {
        d.note = text.to_string();
        if let Err(e) = state.store.save_draft(d) {
            log::error!("保存草稿失败：{e}");
        }
    }
}

/// 小窗失去焦点（用户切去别的窗口找图）：收起变暗背景，下面的窗口才看得见、拖得过来
pub fn note_blur(app: &AppHandle) {
    if !app.state::<AppState>().capturing.load(Ordering::SeqCst) {
        windows::hide_overlays(app);
    }
}

/// 缩略图上的 ×：去掉这张并删文件（只剩一张时不动）
pub fn note_remove_shot(app: &AppHandle, index: usize) -> Result<(), String> {
    let state = app.state::<AppState>();
    let removed = {
        let mut draft = state.draft.lock().unwrap();
        let d = draft.as_mut().ok_or("没有正在记的这一条")?;
        let removed = d.remove_shot(index).ok_or("至少要留一张")?;
        if let Err(e) = state.store.save_draft(d) {
            log::error!("保存草稿失败：{e}");
        }
        removed
    };
    let _ = std::fs::remove_file(state.store.dir().join(&removed));
    refresh_note(app, None);
    Ok(())
}

/// "不记了"：这条的图删掉、草稿作废，什么都不记，也不发给 AI
pub fn discard_draft(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(d) = state.draft.lock().unwrap().take() else { return };
    windows::hide_note(app);
    windows::hide_bar(app);
    windows::hide_overlays(app);
    for rel in &d.shots {
        let _ = std::fs::remove_file(state.store.dir().join(rel));
    }
    state.store.clear_draft();
    restore_main_if_needed(app);
}

/// 现在这条草稿的 id（加图开始时记下，加完时用来判断那条还在不在）
pub fn draft_id(app: &AppHandle) -> Option<String> {
    app.state::<AppState>().draft.lock().unwrap().as_ref().map(|d| d.id.clone())
}

/// 电脑里已有的图：有没记完的一条就加进去（收起小条、把小窗叫到前面）；没有就新建一条，
/// 日期按现在，补一句小窗放在主窗口那块屏，背后不变暗。一张都没加成时返回给调用方的提示。
/// started = 开始加图时的草稿 id：加图途中那条已经记下或不记了，这些图就不加
pub fn add_images(app: &AppHandle, imgs: Vec<RgbaImage>, unreadable: usize, started: Option<&str>) -> Option<String> {
    let state = app.state::<AppState>();
    let now = chrono::Local::now();
    if crate::draft::import_target(started, draft_id(app).as_deref()) == crate::draft::ImportTarget::Gone {
        log::info!("加图途中那一条已经记下或不记了，{} 张图没加", imgs.len());
        return Some("那一条已经记下了，这几张图没加".into());
    }
    let room = state.draft.lock().unwrap().as_ref().map_or(MAX_SHOTS, |d| MAX_SHOTS.saturating_sub(d.shots.len()));
    let over = imgs.len().saturating_sub(room);
    let mut rels = Vec::new();
    for img in imgs.into_iter().take(room) {
        match save_shot(&state, &capture::shot_id(now.naive_local()), &img) {
            Ok(rel) => rels.push(rel),
            Err(e) => log::error!("{e}"),
        }
    }
    let message = crate::import::outcome_message(over, unreadable);
    {
        let mut draft = state.draft.lock().unwrap();
        if draft.is_none() {
            let Some(first) = rels.first() else { return message };
            let id = first.trim_start_matches("shots/").trim_end_matches(".png").to_string();
            *draft = Some(Draft::new(
                &id,
                &now.format("%Y-%m-%d").to_string(),
                &now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
                main_monitor(app),
            ));
        }
        let Some(d) = draft.as_mut() else { return message };
        d.add_shots(&rels);
        if let Err(e) = state.store.save_draft(d) {
            log::error!("保存草稿失败：{e}");
        }
    }
    log::info!("加了 {} 张已有的图", rels.len());
    // 正在框选：窗口一个都不动，截图结束后（after_capture）会把补一句小窗叫出来
    if state.capturing.load(Ordering::SeqCst) {
        return None;
    }
    windows::hide_bar(app);
    show_draft_note(app, false, message);
    None
}

/// 主窗口所在那块屏；主窗口不在就用主屏
fn main_monitor(app: &AppHandle) -> (i32, i32, u32, u32, f64) {
    let m = app
        .get_webview_window("main")
        .filter(|w| w.is_visible().unwrap_or(false))
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    m.map_or((0, 0, 1920, 1080, 1.0), |m| {
        (m.position().x, m.position().y, m.size().width, m.size().height, m.scale_factor())
    })
}

/// 框选页真的把升级提示显示出来了：记下"复制"，以后不再提示
pub fn hint_seen(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap();
    if s.enter_action.is_none() {
        s.enter_action = Some(EnterAction::Copy);
        if let Err(e) = state.store.save_settings(&s) {
            log::error!("保存设置失败：{e}");
        }
    }
}
