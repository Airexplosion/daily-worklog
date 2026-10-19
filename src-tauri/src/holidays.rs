//! 法定节假日：数据来自 holiday-cn（NateScarlet/holiday-cn），随版本打包。

use chrono::{Datelike, NaiveDate, Weekday};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};

/// 新一年的数据一般在每年 11 月公布，12 月随新版本加进来
const BUNDLED: &[&str] = &[
    include_str!("../resources/holidays/2025.json"),
    include_str!("../resources/holidays/2026.json"),
];

#[derive(Deserialize)]
struct YearFile {
    year: i32,
    days: Vec<DayEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DayEntry {
    date: NaiveDate,
    is_off_day: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Calendar {
    /// true = 放假，false = 调休上班
    special: HashMap<NaiveDate, bool>,
    years: BTreeSet<i32>,
}

impl Calendar {
    pub fn bundled() -> Calendar {
        let mut cal = Calendar::default();
        for raw in BUNDLED {
            let Ok(file) = serde_json::from_str::<YearFile>(raw) else { continue };
            if file.days.is_empty() {
                continue;
            }
            cal.years.insert(file.year);
            for d in file.days {
                cal.special.insert(d.date, d.is_off_day);
            }
        }
        cal
    }

    /// 打包的数据里没有今年：只能按周一到周五判断，需要更新软件
    pub fn stale_for(&self, today: NaiveDate) -> bool {
        !self.has_year(today.year())
    }

    pub fn has_year(&self, year: i32) -> bool {
        self.years.contains(&year)
    }

    pub fn is_workday(&self, date: NaiveDate) -> bool {
        match self.special.get(&date) {
            Some(off) => !off,
            None => !matches!(date.weekday(), Weekday::Sat | Weekday::Sun),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn national_day_is_off() {
        let cal = Calendar::bundled();
        assert!(!cal.is_workday(d(2026, 10, 1))); // 周四，国庆
        assert!(!cal.is_workday(d(2026, 10, 5))); // 周一，国庆
    }

    #[test]
    fn makeup_saturday_is_workday() {
        let cal = Calendar::bundled();
        assert!(cal.is_workday(d(2026, 10, 10))); // 周六，调休上班
    }

    #[test]
    fn ordinary_days() {
        let cal = Calendar::bundled();
        assert!(cal.is_workday(d(2026, 10, 15))); // 周四
        assert!(!cal.is_workday(d(2026, 10, 18))); // 周日
    }

    #[test]
    fn data_stale_when_year_missing() {
        let cal = Calendar::bundled();
        assert!(!cal.stale_for(d(2026, 10, 15)));
        assert!(cal.stale_for(d(2030, 1, 2)));
    }

    #[test]
    fn year_without_data_falls_back_to_weekdays() {
        let cal = Calendar::bundled();
        assert!(cal.has_year(2026));
        assert!(!cal.has_year(2030));
        assert!(cal.is_workday(d(2030, 3, 4))); // 周一
        assert!(!cal.is_workday(d(2030, 3, 9))); // 周六
    }
}
