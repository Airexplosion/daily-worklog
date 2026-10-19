//! 下班提醒的判定，以及"漏生成那天"的查找。

use crate::holidays::Calendar;
use crate::model::{Data, Day, Settings};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

pub fn remind_time(s: &Settings) -> NaiveTime {
    NaiveTime::parse_from_str(s.remind_time.trim(), "%H:%M")
        .unwrap_or_else(|_| NaiveTime::from_hms_opt(18, 0, 0).unwrap())
}

/// 全部满足才提醒：工作日（按设置）、已到点、没生成、没提醒过、没点"今天不再提醒"
pub fn should_remind(now: NaiveDateTime, s: &Settings, day: Option<&Day>, cal: &Calendar) -> bool {
    if s.weekday_only && !cal.is_workday(now.date()) {
        return false;
    }
    if now.time() < remind_time(s) {
        return false;
    }
    match day {
        Some(d) => d.summary.is_none() && d.reminded_at.is_none() && !d.mute_reminder,
        None => true,
    }
}

/// 今天之前最近一个有记录的日子；它没生成总结、也没被忽略时返回（日期，条数）
pub fn find_missed(data: &Data, today: NaiveDate) -> Option<(NaiveDate, usize)> {
    let (key, day) = data.days.iter().rev().find(|(k, d)| {
        !d.items.is_empty() && NaiveDate::parse_from_str(k, "%Y-%m-%d").map(|dt| dt < today).unwrap_or(false)
    })?;
    if day.summary.is_some() || day.missed_banner_dismissed {
        return None;
    }
    Some((NaiveDate::parse_from_str(key, "%Y-%m-%d").ok()?, day.items.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holidays::Calendar;
    use crate::model::{Data, Day, Item, ItemStatus, Settings, Source, Summary};
    use chrono::{NaiveDate, NaiveDateTime};

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn item() -> Item {
        Item {
            id: "x".into(),
            created_at: String::new(),
            text: "对账".into(),
            note: String::new(),
            source: Source::Manual,
            edited: false,
            tags: vec![],
            confidence: None,
            shots: vec![],
            status: ItemStatus::Done,
            error: None,
        }
    }

    fn day_with(n: usize) -> Day {
        Day { items: (0..n).map(|_| item()).collect(), ..Default::default() }
    }

    fn generated(mut d: Day) -> Day {
        d.summary = Some(Summary { text: "t".into(), generated_at: "x".into(), ..Default::default() });
        d
    }

    #[test]
    fn remind_time_boundaries() {
        let cal = Calendar::bundled();
        let s = Settings::default();
        // 2026-10-15 周四
        assert!(!should_remind(at("2026-10-15 17:59"), &s, None, &cal));
        assert!(should_remind(at("2026-10-15 18:00"), &s, None, &cal));
        assert!(should_remind(at("2026-10-15 23:30"), &s, Some(&day_with(3)), &cal));
    }

    #[test]
    fn no_remind_when_generated_reminded_or_muted() {
        let cal = Calendar::bundled();
        let s = Settings::default();
        let now = at("2026-10-15 18:30");
        assert!(!should_remind(now, &s, Some(&generated(day_with(2))), &cal));
        let mut reminded = day_with(2);
        reminded.reminded_at = Some("2026-10-15T18:00:05+08:00".into());
        assert!(!should_remind(now, &s, Some(&reminded), &cal));
        let mut muted = day_with(2);
        muted.mute_reminder = true;
        assert!(!should_remind(now, &s, Some(&muted), &cal));
    }

    #[test]
    fn weekend_and_holidays() {
        let cal = Calendar::bundled();
        let mut s = Settings::default();
        assert!(!should_remind(at("2026-10-18 18:30"), &s, None, &cal)); // 周日
        assert!(!should_remind(at("2026-10-05 18:30"), &s, None, &cal)); // 国庆周一
        assert!(should_remind(at("2026-10-10 18:30"), &s, None, &cal)); // 调休周六
        s.weekday_only = false;
        assert!(should_remind(at("2026-10-18 18:30"), &s, None, &cal));
    }

    #[test]
    fn custom_remind_time() {
        let cal = Calendar::bundled();
        let mut s = Settings::default();
        s.remind_time = "17:30".into();
        assert!(should_remind(at("2026-10-15 17:30"), &s, None, &cal));
        s.remind_time = "乱写".into(); // 解析不了按 18:00
        assert!(!should_remind(at("2026-10-15 17:30"), &s, None, &cal));
    }

    fn data(days: Vec<(&str, Day)>) -> Data {
        let mut d = Data::default();
        for (k, v) in days {
            d.days.insert(k.into(), v);
        }
        d
    }

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn find_missed_most_recent_only() {
        let d = data(vec![
            ("2026-10-14", day_with(5)),
            ("2026-10-16", day_with(4)),
            ("2026-10-19", day_with(1)), // 今天，不算
        ]);
        assert_eq!(find_missed(&d, date("2026-10-19")), Some((date("2026-10-16"), 4)));
    }

    #[test]
    fn find_missed_none_when_latest_generated_or_dismissed() {
        let d = data(vec![("2026-10-14", day_with(5)), ("2026-10-16", generated(day_with(4)))]);
        assert_eq!(find_missed(&d, date("2026-10-19")), None);
        let mut dismissed = day_with(4);
        dismissed.missed_banner_dismissed = true;
        let d2 = data(vec![("2026-10-16", dismissed)]);
        assert_eq!(find_missed(&d2, date("2026-10-19")), None);
    }

    #[test]
    fn find_missed_skips_empty_days() {
        let d = data(vec![("2026-10-15", day_with(2)), ("2026-10-16", day_with(0))]);
        assert_eq!(find_missed(&d, date("2026-10-19")), Some((date("2026-10-15"), 2)));
    }
}
