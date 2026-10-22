//! Calendar months, the unit of crawling.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// The first month in which Chess.com archives can contain games.
pub const CHESSCOM_FIRST_MONTH: Month = Month { year: 2007, month: 1 };

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month {
    pub year: i32,
    pub month: u8,
}

impl Month {
    pub fn new(year: i32, month: u8) -> Option<Self> {
        ((1..=12).contains(&month) && (1900..=9999).contains(&year)).then_some(Month { year, month })
    }

    /// Parses `YYYY-MM` (a `/` or `.` separator is also accepted).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (y, m) = text.split_once(['-', '/', '.'])?;
        if y.len() != 4 || m.is_empty() || m.len() > 2 {
            return None;
        }
        Month::new(y.parse().ok()?, m.parse().ok()?)
    }

    pub fn next(self) -> Self {
        if self.month == 12 {
            Month { year: self.year + 1, month: 1 }
        } else {
            Month { year: self.year, month: self.month + 1 }
        }
    }

    pub fn prev(self) -> Self {
        if self.month == 1 {
            Month { year: self.year - 1, month: 12 }
        } else {
            Month { year: self.year, month: self.month - 1 }
        }
    }

    /// Whole months from `other` to `self` (positive when `self` is later).
    pub fn months_since(self, other: Month) -> i32 {
        (self.year - other.year) * 12 + i32::from(self.month) - i32::from(other.month)
    }

    pub fn from_unix(seconds: i64) -> Self {
        let (y, m, _) = civil_from_days(seconds.div_euclid(86_400));
        Month { year: y, month: m }
    }

    pub fn current_utc() -> Self {
        Month::from_unix(unix_now())
    }

    /// The month before the current one: the most recent complete month.
    pub fn previous_utc() -> Self {
        Month::current_utc().prev()
    }

    /// The `YYYY/MM` path segment used by the Chess.com archive URLs.
    pub fn api_path(self) -> String {
        format!("{:04}/{:02}", self.year, self.month)
    }
}

impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

pub fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
pub fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        assert_eq!(Month::parse("2026-09"), Some(Month { year: 2026, month: 9 }));
        assert_eq!(Month::parse(" 2026/1 "), Some(Month { year: 2026, month: 1 }));
        assert_eq!(Month::parse("2026-13"), None);
        assert_eq!(Month::parse("26-01"), None);
        assert_eq!(Month::parse("2026-09").unwrap().to_string(), "2026-09");
        assert_eq!(Month::parse("2026-09").unwrap().api_path(), "2026/09");
    }

    #[test]
    fn steps_across_years() {
        let jan = Month::parse("2026-01").unwrap();
        assert_eq!(jan.prev().to_string(), "2025-12");
        assert_eq!(jan.prev().next(), jan);
        assert_eq!(jan.months_since(Month::parse("2025-10").unwrap()), 3);
    }

    #[test]
    fn converts_unix_time() {
        assert_eq!(Month::from_unix(1_788_880_073).to_string(), "2026-09");
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }
}
