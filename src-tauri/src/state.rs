//! 全局状态。

use crate::capture::Frozen;
use crate::holidays::Calendar;
use crate::items::Trash;
use crate::model::{Data, Settings};
use crate::recognize::Job;
use crate::storage::Store;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;

/// 已经截好、等补充说明的那张图
pub struct PendingShot {
    pub id: String,
    /// YYYY-MM-DD，按按下热键的时刻算
    pub date: String,
    pub created_at: String,
    /// 相对数据目录的路径，如 shots/20261015-183012-a1b2.png
    pub shot_rel: String,
    pub monitor_index: usize,
    /// 截图那块屏的 (x, y, 宽, 高, 缩放)，补充框放在这块屏上
    pub monitor: (i32, i32, u32, u32, f64),
    /// 补充框里显示的缩略图（JPEG）
    pub thumb_jpeg: Vec<u8>,
}

/// 启动时发现的问题，主窗口顶部横幅显示
#[derive(Default, Clone)]
pub struct StartupStatus {
    pub hotkey_error: Option<String>,
    pub recovered_from: Option<String>,
    pub data_lost: bool,
}

pub struct AppState {
    pub store: Store,
    pub data: Mutex<Data>,
    pub settings: Mutex<Settings>,
    pub calendar: Calendar,
    pub frozen: Mutex<Option<Frozen>>,
    /// 框选窗口对应的显示器几何（x, y, w, h），显示器变了就重建窗口
    pub overlay_geometry: Mutex<Vec<(i32, i32, u32, u32)>>,
    /// 截图流程进行中（框选或补充框），再按热键无效
    pub capturing: AtomicBool,
    /// 本次截图至少有一块框选画面已经显示
    pub overlay_shown: AtomicBool,
    pub pending_shot: Mutex<Option<PendingShot>>,
    /// 识别队列
    pub queue: Mutex<Option<UnboundedSender<Job>>>,
    /// 删除后 5 秒内可撤销的条目
    pub trash: Mutex<Trash>,
    pub status: Mutex<StartupStatus>,
}

impl AppState {
    pub fn new(store: Store, data: Data, settings: Settings) -> AppState {
        AppState {
            store,
            data: Mutex::new(data),
            settings: Mutex::new(settings),
            calendar: Calendar::bundled(),
            frozen: Mutex::new(None),
            overlay_geometry: Mutex::new(Vec::new()),
            capturing: AtomicBool::new(false),
            overlay_shown: AtomicBool::new(false),
            pending_shot: Mutex::new(None),
            queue: Mutex::new(None),
            trash: Mutex::new(Trash::default()),
            status: Mutex::new(StartupStatus::default()),
        }
    }
}
