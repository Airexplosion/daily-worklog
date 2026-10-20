//! 全局状态。

use crate::capture::Frozen;
use crate::capture_mode::{Action, LastRect, Origin};
use crate::draft::Draft;
use crate::holidays::Calendar;
use crate::items::Trash;
use crate::model::{Data, Settings};
use crate::recognize::Job;
use crate::storage::Store;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;

/// 这一次截图（框选中）：从哪儿开始的、回车做什么、要不要给升级提示
pub struct Session {
    pub origin: Origin,
    pub primary: Action,
    pub hint: Option<String>,
}

/// 要主窗口做的事。主窗口可能刚建、页面还没加载，所以先存着，页面来取
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum MainIntent {
    EditItem(String),
    OpenSummary,
}

/// 启动时发现的问题，主窗口顶部横幅显示
#[derive(Default, Clone)]
pub struct StartupStatus {
    pub hotkey_error: Option<String>,
    pub recovered_from: Option<String>,
    pub data_lost: bool,
    /// settings.json 损坏，从备份恢复了
    pub settings_recovered: bool,
    /// settings.json 损坏且没有备份，回到了默认设置
    pub settings_reset: bool,
}

pub struct AppState {
    pub store: Store,
    pub data: Mutex<Data>,
    pub settings: Mutex<Settings>,
    pub calendar: Calendar,
    pub frozen: Mutex<Option<Frozen>>,
    /// 框选窗口对应的显示器几何（x, y, w, h），显示器变了就重建窗口
    pub overlay_geometry: Mutex<Vec<(i32, i32, u32, u32)>>,
    /// 正在框选（框选窗口开着），再按热键无效
    pub capturing: AtomicBool,
    pub session: Mutex<Option<Session>>,
    /// 本次截图至少有一块框选画面已经显示
    pub overlay_shown: AtomicBool,
    /// 还没记完的这一条（补一句小窗开着或"再截一张"的小条挂着）
    pub draft: Mutex<Option<Draft>>,
    /// 上一张的选框，"再截一张"时摆回原位
    pub last_rect: Mutex<Option<LastRect>>,
    /// 识别队列
    pub queue: Mutex<Option<UnboundedSender<Job>>>,
    /// 删除后 5 秒内可撤销的条目
    pub trash: Mutex<Trash>,
    pub status: Mutex<StartupStatus>,
    /// 最近一次发现的新版本（横幅用）
    pub update: Mutex<Option<crate::update::UpdateInfo>>,
    pub main_intent: Mutex<Option<MainIntent>>,
    /// 从主窗口点"截一张"时主窗口先藏起来，截完（或取消）再叫回来
    pub restore_main: AtomicBool,
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
            session: Mutex::new(None),
            draft: Mutex::new(None),
            last_rect: Mutex::new(None),
            queue: Mutex::new(None),
            trash: Mutex::new(Trash::default()),
            status: Mutex::new(StartupStatus::default()),
            update: Mutex::new(None),
            main_intent: Mutex::new(None),
            restore_main: AtomicBool::new(false),
        }
    }
}
