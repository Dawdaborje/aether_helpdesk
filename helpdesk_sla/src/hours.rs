//! Business time: how many working minutes lie between two moments, and when a number of working minutes ends.
//!
//! A schedule is a weekly pattern in local time (a UTC offset in minutes), with days off. No pattern means every
//! minute counts. All moments are UTC at the edges.

use aether_sdk::dates::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};

#[derive(Debug, Clone, Default)]
pub struct Schedule {
    /// Local time is UTC plus this many minutes.
    pub offset: i64,
    /// (weekday, start minute of the day, end minute of the day), local.
    pub days: Vec<(Weekday, i64, i64)>,
    /// Local dates with no work.
    pub holidays: Vec<NaiveDate>,
}

const FAR: i64 = 800;

impl Schedule {
    pub fn round_the_clock() -> Self {
        Self::default()
    }

    pub fn is_round_the_clock(&self) -> bool {
        self.days.is_empty()
    }

    fn local(&self, t: NaiveDateTime) -> NaiveDateTime {
        t + Duration::minutes(self.offset)
    }

    fn utc(&self, t: NaiveDateTime) -> NaiveDateTime {
        t - Duration::minutes(self.offset)
    }

    fn intervals(&self, date: NaiveDate) -> Vec<(NaiveDateTime, NaiveDateTime)> {
        if self.holidays.contains(&date) {
            return Vec::new();
        }
        let midnight = date.and_hms_opt(0, 0, 0).unwrap_or_default();
        let mut out: Vec<(NaiveDateTime, NaiveDateTime)> = self
            .days
            .iter()
            .filter(|(wd, ..)| *wd == date.weekday())
            .map(|(_, s, e)| (midnight + Duration::minutes(*s), midnight + Duration::minutes(*e)))
            .filter(|(s, e)| e > s)
            .collect();
        out.sort();
        out
    }

    /// The moment at which `minutes` of working time, counted from `start`, have passed.
    pub fn add(&self, start: NaiveDateTime, minutes: i64) -> NaiveDateTime {
        if minutes <= 0 {
            return start;
        }
        if self.is_round_the_clock() {
            return start + Duration::minutes(minutes);
        }
        let mut cursor = self.local(start);
        let mut left = minutes;
        for _ in 0..FAR {
            let date = cursor.date();
            for (from, to) in self.intervals(date) {
                let from = from.max(cursor);
                if from >= to {
                    continue;
                }
                let room = (to - from).num_minutes();
                if left <= room {
                    return self.utc(from + Duration::minutes(left));
                }
                left -= room;
            }
            cursor = (date + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap_or(cursor);
        }
        self.utc(cursor)
    }

    /// Working minutes from `a` to `b`; zero when `b` is not after `a`.
    pub fn between(&self, a: NaiveDateTime, b: NaiveDateTime) -> i64 {
        if b <= a {
            return 0;
        }
        if self.is_round_the_clock() {
            return (b - a).num_minutes();
        }
        let (la, lb) = (self.local(a), self.local(b));
        let mut total = 0;
        let mut date = la.date();
        let mut steps = 0;
        while date <= lb.date() && steps < FAR * 2 {
            for (from, to) in self.intervals(date) {
                let (from, to) = (from.max(la), to.min(lb));
                if to > from {
                    total += (to - from).num_minutes();
                }
            }
            date += Duration::days(1);
            steps += 1;
        }
        total
    }
}

/// `HH:MM` as minutes since midnight.
pub fn minutes_of(text: &str) -> Option<i64> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some(h * 60 + m)
}

pub fn weekday_of(name: &str) -> Option<Weekday> {
    Some(match name.trim().to_lowercase().get(..3)? {
        "mon" => Weekday::Mon,
        "tue" => Weekday::Tue,
        "wed" => Weekday::Wed,
        "thu" => Weekday::Thu,
        "fri" => Weekday::Fri,
        "sat" => Weekday::Sat,
        "sun" => Weekday::Sun,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    fn office() -> Schedule {
        let days = [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri].iter().map(|d| (*d, 9 * 60, 17 * 60)).collect();
        Schedule { offset: 0, days, holidays: vec![] }
    }

    #[test]
    fn round_the_clock_counts_every_minute() {
        let s = Schedule::round_the_clock();
        assert_eq!(s.add(at("2026-10-16 10:00"), 90), at("2026-10-16 11:30"));
        assert_eq!(s.between(at("2026-10-16 10:00"), at("2026-10-17 10:00")), 1440);
    }

    #[test]
    fn work_inside_one_day() {
        assert_eq!(office().add(at("2026-10-14 10:00"), 120), at("2026-10-14 12:00"));
    }

    #[test]
    fn work_rolls_to_the_next_morning() {
        // Wednesday 16:00 + 120 minutes: 60 left today, 60 more on Thursday from 09:00.
        assert_eq!(office().add(at("2026-10-14 16:00"), 120), at("2026-10-15 10:00"));
    }

    #[test]
    fn the_weekend_is_skipped() {
        // Friday 16:30 + 60: 30 left on Friday, 30 on Monday.
        assert_eq!(office().add(at("2026-10-16 16:30"), 60), at("2026-10-19 09:30"));
    }

    #[test]
    fn starting_out_of_hours_waits_for_opening() {
        assert_eq!(office().add(at("2026-10-17 12:00"), 30), at("2026-10-19 09:30"));
        assert_eq!(office().add(at("2026-10-14 06:00"), 30), at("2026-10-14 09:30"));
    }

    #[test]
    fn a_holiday_is_skipped() {
        let mut s = office();
        s.holidays.push(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap());
        assert_eq!(s.add(at("2026-10-14 16:00"), 120), at("2026-10-16 10:00"));
    }

    #[test]
    fn the_offset_moves_the_working_day() {
        // Local time is UTC+1: 09:00 local is 08:00 UTC.
        let mut s = office();
        s.offset = 60;
        assert_eq!(s.add(at("2026-10-14 06:00"), 30), at("2026-10-14 08:30"));
    }

    #[test]
    fn a_lunch_break_is_not_counted() {
        let mut s = Schedule { offset: 0, days: vec![(Weekday::Wed, 9 * 60, 12 * 60), (Weekday::Wed, 13 * 60, 17 * 60)], holidays: vec![] };
        assert_eq!(s.add(at("2026-10-14 11:00"), 120), at("2026-10-14 14:00"));
        assert_eq!(s.between(at("2026-10-14 11:00"), at("2026-10-14 14:00")), 120);
        s.days.clear();
    }

    #[test]
    fn between_counts_only_working_minutes() {
        let s = office();
        // Friday 16:00 to Monday 10:00: 60 on Friday + 60 on Monday.
        assert_eq!(s.between(at("2026-10-16 16:00"), at("2026-10-19 10:00")), 120);
        assert_eq!(s.between(at("2026-10-17 08:00"), at("2026-10-17 20:00")), 0);
        assert_eq!(s.between(at("2026-10-14 10:00"), at("2026-10-14 09:00")), 0);
    }

    #[test]
    fn add_and_between_agree() {
        let s = office();
        let start = at("2026-10-15 15:20");
        for m in [1, 45, 200, 480, 1000, 2400] {
            assert_eq!(s.between(start, s.add(start, m)), m, "{m}");
        }
    }

    #[test]
    fn times_parse() {
        assert_eq!(minutes_of("09:30"), Some(570));
        assert_eq!(minutes_of("24:00"), None);
        assert_eq!(weekday_of("Friday"), Some(Weekday::Fri));
        assert_eq!(weekday_of("x"), None);
    }
}
