//! 截图从哪儿开始、框选完做什么：主按钮、Tab 的含义、升级提示、"沿用上次的框"、另存为的文件名。

use crate::capture::PhysRect;
use crate::model::{EnterAction, Settings};
use serde::{Deserialize, Serialize};

/// 这次截图是从哪儿开始的
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Hotkey,
    Tray,
    /// 主窗口右下角"截一张"
    Main,
    /// 首次引导最后的"试截一张"
    Onboarding,
}

/// 框选完以后图去哪儿
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Copy,
    Record,
    Save,
    /// 加到还没记完的这一条
    Add,
}

/// 开始截图时有没有没记完的一条
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftState {
    None,
    /// 有，还能加
    Open,
    /// 有，已经满 9 张
    Full,
}

/// 回车（主按钮）做什么：有没记完的一条时是加到这条（满了就只能复制）；从主窗口、引导开始的是记录；其他按设置
pub fn primary(origin: Origin, draft: DraftState, enter: Option<EnterAction>) -> Action {
    match draft {
        DraftState::Open => return Action::Add,
        DraftState::Full => return Action::Copy,
        DraftState::None => {}
    }
    match origin {
        Origin::Main | Origin::Onboarding => Action::Record,
        Origin::Hotkey | Origin::Tray => match enter {
            Some(EnterAction::Record) => Action::Record,
            _ => Action::Copy,
        },
    }
}

/// Tab 和"记录"：有草稿时就是加到这条；"加到这条"在草稿没了以后退回记录
pub fn resolve(action: Action, has_draft: bool) -> Action {
    match (action, has_draft) {
        (Action::Record, true) => Action::Add,
        (Action::Add, false) => Action::Record,
        (a, _) => a,
    }
}

pub const UPGRADE_HINT: &str = "现在回车是复制，记一条请按 Tab；想改回回车记录，去设置里改";

/// 老用户升级后第一次截图：走完了引导，但还没有"框选后按回车"这一项
pub fn needs_upgrade_hint(s: &Settings) -> bool {
    s.onboarding_done && s.enter_action.is_none()
}

/// 上一张的选框（物理像素），"再截一张"时摆回原位
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LastRect {
    pub index: usize,
    /// 那块屏当时的 (x, y, 宽, 高)
    pub monitor: (i32, i32, u32, u32),
    pub rect: PhysRect,
}

/// 只在同一块屏、屏幕几何没变时沿用（改了缩放或换了显示器就不预放）
pub fn last_rect_for(last: Option<&LastRect>, index: usize, monitor: (i32, i32, u32, u32)) -> Option<PhysRect> {
    last.filter(|l| l.index == index && l.monitor == monitor).map(|l| l.rect)
}

/// 另存为最后写到哪个文件：用户选的不是 .png 就改成 .png；改了名的那个文件已经有了，就加 (2)、(3)，
/// 不悄悄覆盖（选 .png 时对话框自己问过要不要覆盖）
pub fn save_target(picked: &std::path::Path, exists: impl Fn(&std::path::Path) -> bool) -> std::path::PathBuf {
    let is_png = picked.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"));
    if is_png {
        return picked.to_path_buf();
    }
    let png = picked.with_extension("png");
    if !exists(&png) {
        return png;
    }
    let stem = png.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    (2..)
        .map(|n| png.with_file_name(format!("{stem} ({n}).png")))
        .find(|p| !exists(p))
        .expect("总能找到没用过的名字")
}

/// 另存为的默认文件名
pub fn save_file_name(t: chrono::NaiveDateTime) -> String {
    format!("截图_{}.png", t.format("%Y%m%d_%H%M%S"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::PhysRect;
    use crate::model::{EnterAction, Settings};
    use chrono::NaiveDate;
    use std::path::Path;

    #[test]
    fn primary_by_origin() {
        assert_eq!(primary(Origin::Hotkey, DraftState::None, None), Action::Copy);
        assert_eq!(primary(Origin::Hotkey, DraftState::None, Some(EnterAction::Record)), Action::Record);
        assert_eq!(primary(Origin::Tray, DraftState::None, Some(EnterAction::Copy)), Action::Copy);
        assert_eq!(primary(Origin::Main, DraftState::None, Some(EnterAction::Copy)), Action::Record);
        assert_eq!(primary(Origin::Onboarding, DraftState::None, None), Action::Record);
        for o in [Origin::Hotkey, Origin::Tray, Origin::Main, Origin::Onboarding] {
            assert_eq!(primary(o, DraftState::Open, Some(EnterAction::Copy)), Action::Add);
            // 满 9 张就加不进去了：只能复制或另存
            assert_eq!(primary(o, DraftState::Full, Some(EnterAction::Record)), Action::Copy);
        }
    }

    #[test]
    fn resolve_record_and_add() {
        assert_eq!(resolve(Action::Record, true), Action::Add);
        assert_eq!(resolve(Action::Add, false), Action::Record);
        assert_eq!(resolve(Action::Record, false), Action::Record);
        assert_eq!(resolve(Action::Copy, true), Action::Copy);
        assert_eq!(resolve(Action::Save, true), Action::Save);
    }

    #[test]
    fn upgrade_hint_only_for_onboarded_without_choice() {
        let mut s = Settings::default();
        assert!(!needs_upgrade_hint(&s));
        s.onboarding_done = true;
        assert!(needs_upgrade_hint(&s));
        s.enter_action = Some(EnterAction::Copy);
        assert!(!needs_upgrade_hint(&s));
    }

    #[test]
    fn last_rect_same_monitor_only() {
        let rect = PhysRect { x: 10, y: 20, w: 300, h: 200 };
        let monitor = (-1920, 0, 1920, 1080);
        let last = LastRect { index: 1, monitor, rect };
        assert_eq!(last_rect_for(Some(&last), 1, monitor), Some(rect));
        assert_eq!(last_rect_for(Some(&last), 0, monitor), None);
        assert_eq!(last_rect_for(Some(&last), 1, (-2880, 0, 2880, 1620)), None);
        assert_eq!(last_rect_for(None, 1, monitor), None);
    }

    #[test]
    fn save_file_name_format() {
        let t = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap().and_hms_opt(10, 30, 12).unwrap();
        assert_eq!(save_file_name(t), "截图_20261019_103012.png");
    }

    #[test]
    fn actions_and_origins_use_lowercase_names() {
        assert_eq!(serde_json::to_value(Action::Add).unwrap(), "add");
        assert_eq!(serde_json::from_value::<Action>("copy".into()).unwrap(), Action::Copy);
        assert_eq!(serde_json::from_value::<Origin>("onboarding".into()).unwrap(), Origin::Onboarding);
    }

    #[test]
    fn save_target_never_silently_overwrites_after_renaming() {
        let taken = |names: &'static [&'static str]| move |p: &Path| names.iter().any(|n| p.ends_with(n));
        let dir = Path::new("C:/pics");
        // 对话框里选的就是 .png：对话框自己问过要不要覆盖
        assert_eq!(save_target(&dir.join("a.png"), taken(&["a.png"])), dir.join("a.png"));
        assert_eq!(save_target(&dir.join("a.jpg"), taken(&[])), dir.join("a.png"));
        assert_eq!(save_target(&dir.join("a.jpg"), taken(&["a.png"])), dir.join("a (2).png"));
        assert_eq!(save_target(&dir.join("a.jpg"), taken(&["a.png", "a (2).png"])), dir.join("a (3).png"));
        assert_eq!(save_target(&dir.join("a"), taken(&[])), dir.join("a.png"));
    }
}
