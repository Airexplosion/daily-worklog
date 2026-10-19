//! 本地存储：原子写入、启动备份、损坏恢复、截图按天数清理。

use crate::model::{Data, Settings};
use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const MAX_BACKUPS: usize = 7;

/// `%APPDATA%\DailyWorkLog`
pub fn data_dir() -> PathBuf {
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
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => return LoadOutcome { data: Data::default(), recovered_from: None, lost: false },
        };
        if let Ok(data) = serde_json::from_str::<Data>(&raw) {
            return LoadOutcome { data, recovered_from: None, lost: false };
        }
        // 坏文件留档
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        let _ = fs::rename(&path, self.dir.join(format!("data.corrupt-{stamp}.json")));
        for name in self.backup_names().into_iter().rev() {
            let Ok(raw) = fs::read_to_string(self.backups_dir().join(&name)) else { continue };
            if let Ok(data) = serde_json::from_str::<Data>(&raw) {
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
        fs::read_to_string(self.settings_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        write_atomic(&self.settings_path(), &serde_json::to_vec_pretty(settings)?)
    }

    /// 先确认 data.json 能解析，再复制一份到 backups，最多留 7 份
    pub fn backup_on_startup(&self, now: NaiveDateTime) -> io::Result<()> {
        let Ok(raw) = fs::read_to_string(self.data_path()) else { return Ok(()) };
        if serde_json::from_str::<Data>(&raw).is_err() {
            return Ok(());
        }
        fs::create_dir_all(self.backups_dir())?;
        let name = format!("data-{}.json", now.format("%Y%m%d-%H%M%S"));
        write_atomic(&self.backups_dir().join(name), raw.as_bytes())?;
        let names = self.backup_names();
        if names.len() > MAX_BACKUPS {
            for old in &names[..names.len() - MAX_BACKUPS] {
                let _ = fs::remove_file(self.backups_dir().join(old));
            }
        }
        Ok(())
    }

    /// 备份文件名，按时间升序
    fn backup_names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.backups_dir())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.starts_with("data-") && n.ends_with(".json"))
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
    fs::rename(&tmp, path)
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
    fn settings_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().to_path_buf());
        let mut s = Settings::default();
        s.user_name = "张三".into();
        store.save_settings(&s).unwrap();
        assert_eq!(store.load_settings().user_name, "张三");
    }
}
