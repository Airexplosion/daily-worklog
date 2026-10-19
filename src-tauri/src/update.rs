//! 检查更新：请求 github.com/{仓库}/releases/latest 不跟随跳转，从 Location 里取出最新 tag。
//! 不走 GitHub API（不登录每小时只能查 60 次，同单位共用一个公网 IP 容易超）。

use crate::model::UpdateState;
use crate::state::AppState;
use chrono::{Duration, NaiveDateTime};
use semver::Version;
use serde::Serialize;
use std::sync::LazyLock;
use tauri::{AppHandle, Emitter, Manager};

/// 构建时用环境变量 DWL_UPDATE_REPO 注入（GitHub Actions 里是 ${{ github.repository }}）
pub const UPDATE_REPO: &str = match option_env!("DWL_UPDATE_REPO") {
    Some(r) => r,
    None => "OWNER/REPO",
};
/// GitHub 打不开时，横幅上提示"找谁要安装包"
pub const PUBLISHER: &str = match option_env!("DWL_PUBLISHER") {
    Some(p) => p,
    None => "发布人",
};

pub fn is_configured(repo: &str) -> bool {
    !repo.is_empty() && repo != "OWNER/REPO" && repo.contains('/')
}

/// ".../releases/tag/v1.2.0" → 1.2.0
pub fn parse_location(loc: &str) -> Option<Version> {
    let tag = loc.split("/releases/tag/").nth(1)?.split(['?', '#', '/']).next()?;
    Version::parse(tag.trim_start_matches('v')).ok()
}

pub fn is_newer(current: &Version, latest: &Version) -> bool {
    latest > current
}

/// 点过"稍后"的这个版本 3 天内不再提示；更新的版本不受影响
pub fn should_show(latest: &Version, st: &UpdateState, now: NaiveDateTime) -> bool {
    let snoozed_this = st.snoozed_version.as_deref() == Some(&latest.to_string());
    let until = st.snoozed_until.as_deref().and_then(|u| NaiveDateTime::parse_from_str(u, "%Y-%m-%d %H:%M:%S").ok());
    !(snoozed_this && until.is_some_and(|u| now < u))
}

pub fn snooze(st: &mut UpdateState, version: &Version, now: NaiveDateTime) {
    st.snoozed_version = Some(version.to_string());
    st.snoozed_until = Some((now + Duration::days(3)).format("%Y-%m-%d %H:%M:%S").to_string());
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("DailyWorkLog")
        .build()
        .expect("HTTP 客户端初始化失败")
});

/// 最新 Release 的版本号；还没有 Release 时返回 None
pub async fn fetch_latest(repo: &str) -> Result<Option<Version>, String> {
    let url = format!("https://github.com/{repo}/releases/latest");
    let resp = CLIENT.get(&url).send().await.map_err(|e| format!("连不上 GitHub：{}", e.without_url()))?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    let loc = resp.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("");
    Ok(parse_location(loc))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: Option<String>,
    pub newer: bool,
    pub url: String,
    pub publisher: String,
}

fn current_version(app: &AppHandle) -> Version {
    let v = app.package_info().version.to_string();
    Version::parse(&v).unwrap_or_else(|_| Version::new(0, 0, 0))
}

/// 检查一次。自动检查时：有新版且没被"稍后"压住，就发 update-available 事件
pub async fn check(app: &AppHandle, manual: bool) -> Result<UpdateInfo, String> {
    let current = current_version(app);
    let mut info = UpdateInfo {
        current: current.to_string(),
        latest: None,
        newer: false,
        url: format!("https://github.com/{UPDATE_REPO}/releases"),
        publisher: PUBLISHER.to_string(),
    };
    if !is_configured(UPDATE_REPO) {
        return if manual { Err("这个版本没有配置更新地址".into()) } else { Ok(info) };
    }
    let latest = fetch_latest(UPDATE_REPO).await?;
    let state = app.state::<AppState>();
    {
        let mut s = state.settings.lock().unwrap();
        s.update.last_check_at = Some(crate::model::now_iso());
        let _ = state.store.save_settings(&s);
    }
    if let Some(latest) = latest {
        info.newer = is_newer(&current, &latest);
        info.url = format!("https://github.com/{UPDATE_REPO}/releases/tag/v{latest}");
        info.latest = Some(latest.to_string());
        let show = {
            let s = state.settings.lock().unwrap();
            should_show(&latest, &s.update, chrono::Local::now().naive_local())
        };
        if info.newer && (manual || show) {
            *state.update.lock().unwrap() = Some(info.clone());
            let _ = app.emit("update-available", &info);
        }
    }
    Ok(info)
}

/// 启动 1 分钟后查一次，之后每 24 小时查一次；失败只写日志
pub fn spawn_checker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        loop {
            if let Err(e) = check(&app, false).await {
                log::info!("检查更新失败：{e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate};

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    #[test]
    fn parse_location_variants() {
        assert_eq!(parse_location("https://github.com/a/b/releases/tag/v1.2.0"), Some(v("1.2.0")));
        assert_eq!(parse_location("/a/b/releases/tag/1.10.3"), Some(v("1.10.3")));
        assert_eq!(parse_location("https://github.com/a/b/releases/tag/v1.2"), None);
        assert_eq!(parse_location("https://github.com/a/b/releases"), None);
    }

    #[test]
    fn newer_compares_semver() {
        assert!(is_newer(&v("0.1.0"), &v("0.2.0")));
        assert!(is_newer(&v("1.9.0"), &v("1.10.0")));
        assert!(!is_newer(&v("1.2.0"), &v("1.2.0")));
        assert!(!is_newer(&v("1.3.0"), &v("1.2.9")));
    }

    #[test]
    fn snooze_3_days() {
        let now = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap().and_hms_opt(9, 0, 0).unwrap();
        let mut st = crate::model::UpdateState::default();
        assert!(should_show(&v("1.2.0"), &st, now));
        snooze(&mut st, &v("1.2.0"), now);
        assert!(!should_show(&v("1.2.0"), &st, now + Duration::days(2)));
        assert!(should_show(&v("1.2.0"), &st, now + Duration::days(3) + Duration::minutes(1)));
        // 出了更新的版本立刻提示
        assert!(should_show(&v("1.3.0"), &st, now + Duration::hours(1)));
    }

    #[test]
    fn repo_placeholder_disables_check() {
        assert!(!is_configured("OWNER/REPO"));
        assert!(!is_configured(""));
        assert!(is_configured("zhaoding/daily-worklog"));
    }
}
