//! 本地存储：原子写入、启动备份、损坏恢复、截图按天数清理。

use crate::model::{Data, Settings};
use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const MAX_BACKUPS: usize = 7;

/// `%APPDATA%\DailyWorkLog`；设置了环境变量 DWL_DATA_DIR 时用它（测试用）
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("DWL_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("DailyWorkLog")
}

#[derive(Debug)]
pub struct LoadOutcome {
    pub data: Data,
    /// 从哪份备份恢复的（备份文件名）
    pub recovered_from: Option<String>,
    /// data.json 损坏且没有可用备份
    pub lost: bool,
}

#[derive(Debug)]
pub struct SettingsOutcome {
    pub settings: Settings,
    /// 从备份恢复了
    pub recovered: bool,
    /// 损坏且没有备份，回到了默认设置
    pub reset: bool,
}

#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn shots_dir(&self) -> PathBuf {
        self.dir.join("shots")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.dir.join("logs")
    }

    fn data_path(&self) -> PathBuf {
        self.dir.join("data.json")
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    fn backups_dir(&self) -> PathBuf {
        self.dir.join("backups")
    }

    pub fn load_data(&self) -> LoadOutcome {
        let path = self.data_path();
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return LoadOutcome { data: Data::default(), recovered_from: None, lost: false }
            }
            // 读不了（被占用、没权限）也走恢复，绝不能当成"没有数据"再覆盖掉
            Err(e) => {
                log_read_error(&path, &e);
                Vec::new()
            }
        };
        if let Some(data) = parse_data(&bytes) {
            return LoadOutcome { data, recovered_from: None, lost: false };
        }
        // 坏文件留档
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let _ = fs::rename(&path, self.dir.join(format!("data.corrupt-{stamp}.json")));
        for name in self.backup_names().into_iter().rev() {
            let Ok(raw) = fs::read(self.backups_dir().join(&name)) else { continue };
            if let Some(data) = parse_data(&raw) {
                let _ = self.save_data(&data);
                return LoadOutcome { data, recovered_from: Some(name), lost: false };
            }
        }
        LoadOutcome { data: Data::default(), recovered_from: None, lost: true }
    }

    pub fn save_data(&self, data: &Data) -> io::Result<()> {
        write_atomic(&self.data_path(), &serde_json::to_vec_pretty(data)?)
    }

    pub fn load_settings(&self) -> Settings {
        self.load_settings_checked().settings
    }

    /// 读设置；损坏时坏文件留档，从最新的设置备份恢复（API Key、分类都在里面）
    pub fn load_settings_checked(&self) -> SettingsOutcome {
        let fresh = |recovered, reset| SettingsOutcome { settings: Settings::default(), recovered, reset };
        let bytes = match fs::read(self.settings_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return fresh(false, false),
            Err(e) => {
                log_read_error(&self.settings_path(), &e);
                Vec::new()
            }
        };
        if let Some(s) = parse_settings(&bytes) {
            return SettingsOutcome { settings: s, recovered: false, reset: false };
        }
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let _ = fs::rename(self.settings_path(), self.dir.join(format!("settings.corrupt-{stamp}.json")));
        for name in self.backup_names_with("settings-").into_iter().rev() {
            let Ok(raw) = fs::read(self.backups_dir().join(&name)) else { continue };
            if let Some(s) = parse_settings(&raw) {
                let _ = self.save_settings(&s);
                return SettingsOutcome { settings: s, recovered: true, reset: false };
            }
        }
        fresh(false, true)
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        write_atomic(&self.settings_path(), &serde_json::to_vec_pretty(settings)?)
    }

    /// 先确认 data.json 能解析，再复制一份到 backups，最多留 7 份
    pub fn backup_on_startup(&self, now: NaiveDateTime) -> io::Result<()> {
        let Ok(raw) = fs::read(self.data_path()) else { return Ok(()) };
        if parse_data(&raw).is_none() {
            return Ok(());
        }
        fs::create_dir_all(self.backups_dir())?;
        let name = format!("data-{}.json", now.format("%Y%m%d-%H%M%S"));
        write_atomic(&self.backups_dir().join(name), &raw)?;
        if let Ok(raw) = fs::read(self.settings_path()) {
            if parse_settings(&raw).is_some() {
                let name = format!("settings-{}.json", now.format("%Y%m%d-%H%M%S"));
                write_atomic(&self.backups_dir().join(name), &raw)?;
            }
        }
        for prefix in ["data-", "settings-"] {
            let names = self.backup_names_with(prefix);
            if names.len() > MAX_BACKUPS {
                for old in &names[..names.len() - MAX_BACKUPS] {
                    let _ = fs::remove_file(self.backups_dir().join(old));
                }
            }
        }
        Ok(())
    }

    /// data.json 的备份文件名，按时间升序
    fn backup_names(&self) -> Vec<String> {
        self.backup_names_with("data-")
    }

    fn backup_names_with(&self, prefix: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.backups_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.starts_with(prefix) && n.ends_with(".json"))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// 删除早于保留天数的截图（按文件名开头的 YYYYMMDD 判断），返回删除数量
    pub fn purge_shots(&self, retention_days: u32, today: NaiveDate) -> usize {
        let cutoff = today - Duration::days(retention_days as i64);
        let Ok(rd) = fs::read_dir(self.shots_dir()) else { return 0 };
        let mut removed = 0;
        for entry in rd.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(date) = name.get(..8).and_then(|d| NaiveDate::parse_from_str(d, "%Y%m%d").ok()) else {
                continue;
            };
            if date < cutoff && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

/// 解析 data.json 的字节；带 UTF-8 BOM（记事本另存过）也认
fn parse_data(bytes: &[u8]) -> Option<Data> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    serde_json::from_slice::<Data>(body).ok()
}

fn parse_settings(bytes: &[u8]) -> Option<Settings> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    serde_json::from_slice::<Settings>(body).ok()
}

fn log_read_error(path: &Path, e: &io::Error) {
    log::warn!("读取 {} 失败：{e}", path.display());
}

/// 先写 .tmp 并刷盘，再改名覆盖，断电时不会只写一半
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    // 杀毒软件、同步盘偶尔会短暂占用文件，重试几次
    let mut tries = 0;
    loop {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if tries < 5 && e.kind() == io::ErrorKind::PermissionDenied => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Data, Day, Settings};
    use chrono::{NaiveDate, NaiveDateTime};
    use std::fs;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn sample_data(tag: &str) -> Data {
        let mut d = Data::default();
        d.days.insert(tag.to_string(), Day::default());
        d
    }

    #[test]
    fn save_is_atomic_no_tmp_left() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        store.save_data(&sample_data("2026-10-15")).unwrap();
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
        let loaded = store.load_data();
        assert!(loaded.data.days.contains_key("2026-10-15"));
        assert_eq!(loaded.recovered_from, None);
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let loaded = store.load_data();
        assert!(loaded.data.days.is_empty());
        assert_eq!(loaded.recovered_from, None);
        assert!(!loaded.lost);
        assert_eq!(store.load_settings(), Settings::default());
    }

    #[test]
    fn corrupt_data_restores_latest_valid_backup() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let b = dir.path().join("backups");
        fs::create_dir_all(&b).unwrap();
        fs::write(b.join("data-20261013-090000.json"), serde_json::to_string(&sample_data("old")).unwrap()).unwrap();
        fs::write(b.join("data-20261014-090000.json"), serde_json::to_string(&sample_data("good")).unwrap()).unwrap();
        fs::write(b.join("data-20261015-090000.json"), "{ broken").unwrap();
        fs::write(dir.path().join("data.json"), "{\"days\": {\"2026-10-15\": ").unwrap();

        let loaded = store.load_data();
        assert!(loaded.data.days.contains_key("good"));
        assert_eq!(loaded.recovered_from.as_deref(), Some("data-20261014-090000.json"));
        // 恢复后 data.json 已是好的
        let again = store.load_data();
        assert!(again.data.days.contains_key("good"));
        assert_eq!(again.recovered_from, None);
    }

    #[test]
    fn corrupt_data_without_backup_is_reported_lost() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        fs::write(dir.path().join("data.json"), "garbage").unwrap();
        let loaded = store.load_data();
        assert!(loaded.lost);
        assert!(loaded.data.days.is_empty());
        // 坏文件留档，不直接删
        let kept = fs::read_dir(dir.path())
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().starts_with("data.corrupt-"));
        assert!(kept);
    }

    #[test]
    fn backup_skipped_when_data_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        fs::write(dir.path().join("data.json"), "garbage").unwrap();
        store.backup_on_startup(at("2026-10-15 09:00:00")).unwrap();
        let b = dir.path().join("backups");
        let count = fs::read_dir(&b).map(|r| r.count()).unwrap_or(0);
        assert_eq!(count, 0);
    }

    #[test]
    fn backups_capped_at_7() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        store.save_data(&sample_data("x")).unwrap();
        for d in 1..=9 {
            store.backup_on_startup(at(&format!("2026-10-{d:02} 09:00:00"))).unwrap();
        }
        let mut names: Vec<String> = fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names.len(), 7);
        assert_eq!(names[0], "data-20261003-090000.json");
        assert_eq!(names[6], "data-20261009-090000.json");
    }

    #[test]
    fn purge_shots_removes_only_older_than_retention() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let shots = store.shots_dir();
        fs::create_dir_all(&shots).unwrap();
        for name in [
            "20260716-100000-aaaa.png", // 91 天前，删
            "20260717-100000-bbbb.png", // 刚好 90 天，留
            "20261015-100000-cccc.png", // 今天，留
            "readme.txt",               // 不认识的文件不动
        ] {
            fs::write(shots.join(name), b"x").unwrap();
        }
        let today = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        let removed = store.purge_shots(90, today);
        assert_eq!(removed, 1);
        assert!(!shots.join("20260716-100000-aaaa.png").exists());
        assert!(shots.join("20260717-100000-bbbb.png").exists());
        assert!(shots.join("20261015-100000-cccc.png").exists());
        assert!(shots.join("readme.txt").exists());
    }

    #[test]
    fn truncated_utf8_data_recovers_from_backup() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let b = dir.path().join("backups");
        fs::create_dir_all(&b).unwrap();
        fs::write(b.join("data-20261014-090000.json"), serde_json::to_string(&sample_data("good")).unwrap()).unwrap();
        // 在中文字符中间截断：不是合法 UTF-8
        let full = serde_json::to_string(&sample_data("沟通物业")).unwrap().into_bytes();
        let cut = full.iter().position(|&c| c >= 0x80).unwrap() + 1;
        fs::write(dir.path().join("data.json"), &full[..cut]).unwrap();
        let loaded = store.load_data();
        assert!(loaded.data.days.contains_key("good"), "要从备份恢复，而不是当成没有数据");
        assert_eq!(loaded.recovered_from.as_deref(), Some("data-20261014-090000.json"));
    }

    #[test]
    fn bom_prefixed_data_loads() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend(serde_json::to_vec(&sample_data("2026-10-15")).unwrap());
        fs::write(dir.path().join("data.json"), bytes).unwrap();
        let loaded = store.load_data();
        assert!(loaded.data.days.contains_key("2026-10-15"));
        assert_eq!(loaded.recovered_from, None);
        assert!(!loaded.lost);
        store.backup_on_startup(at("2026-10-15 09:00:00")).unwrap();
        assert_eq!(fs::read_dir(dir.path().join("backups")).unwrap().count(), 1, "带 BOM 的好文件也要备份");
    }

    #[test]
    fn corrupt_settings_recovers_from_backup() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let mut s = Settings::default();
        s.user_name = "某某".into();
        s.vision.api_key = "sk-keep".into();
        store.save_settings(&s).unwrap();
        store.save_data(&sample_data("x")).unwrap();
        store.backup_on_startup(at("2026-10-15 09:00:00")).unwrap();
        fs::write(dir.path().join("settings.json"), "{ broken").unwrap();
        let out = store.load_settings_checked();
        assert_eq!(out.settings.vision.api_key, "sk-keep");
        assert!(out.recovered);
        assert!(fs::read_dir(dir.path()).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with("settings.corrupt-")));
    }

    #[test]
    fn corrupt_settings_without_backup_resets_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        fs::write(dir.path().join("settings.json"), "garbage").unwrap();
        let out = store.load_settings_checked();
        assert_eq!(out.settings, Settings::default());
        assert!(out.reset);
    }

    #[test]
    fn settings_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let mut s = Settings::default();
        s.user_name = "某某".into();
        store.save_settings(&s).unwrap();
        assert_eq!(store.load_settings().user_name, "某某");
    }
}
