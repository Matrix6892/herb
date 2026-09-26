//! Calendar dates and UTC timestamps without a time library. The build is
//! reproducible, so dates always come from data, never from the clock; the
//! clock is read only by the binaries that pass it in.

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a date in YYYY-MM-DD form")]
pub struct DateError(pub String);

/// A proleptic Gregorian calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

const fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

const fn days_in_month(y: i32, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    pub fn new(year: i32, month: u8, day: u8) -> Option<Date> {
        if !(1900..=9999).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Date { year, month, day })
    }

    pub fn parse(s: &str) -> Result<Date, DateError> {
        let err = || DateError(s.to_owned());
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return Err(err());
        }
        let num = |r: core::ops::Range<usize>| -> Result<u32, DateError> {
            let part = &s[r];
            if !part.bytes().all(|c| c.is_ascii_digit()) {
                return Err(err());
            }
            part.parse().map_err(|_| err())
        };
        let y = i32::try_from(num(0..4)?).map_err(|_| err())?;
        let m = u8::try_from(num(5..7)?).map_err(|_| err())?;
        let d = u8::try_from(num(8..10)?).map_err(|_| err())?;
        Date::new(y, m, d).ok_or_else(err)
    }

    pub const fn year(self) -> i32 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    pub const fn day(self) -> u8 {
        self.day
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    pub fn days_since_epoch(self) -> i64 {
        let m = i64::from(self.month);
        let d = i64::from(self.day);
        let y = i64::from(self.year) - i64::from(m <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// Inverse of [`Date::days_since_epoch`] (`civil_from_days`).
    pub fn from_days_since_epoch(days: i64) -> Option<Date> {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        Date::new(i32::try_from(y).ok()?, u8::try_from(m).ok()?, u8::try_from(d).ok()?)
    }

    pub fn add_days(self, days: i64) -> Option<Date> {
        Date::from_days_since_epoch(self.days_since_epoch() + days)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Seconds since the Unix epoch, UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcTimestamp(i64);

impl UtcTimestamp {
    pub const fn from_unix(secs: i64) -> UtcTimestamp {
        UtcTimestamp(secs)
    }

    pub const fn unix(self) -> i64 {
        self.0
    }

    pub fn date(self) -> Option<Date> {
        Date::from_days_since_epoch(self.0.div_euclid(86_400))
    }

    /// `(hour, minute)` of the day.
    pub fn hour_minute(self) -> (u8, u8) {
        let s = self.0.rem_euclid(86_400);
        let h = u8::try_from(s / 3600).expect("hour < 24");
        let m = u8::try_from(s % 3600 / 60).expect("minute < 60");
        (h, m)
    }
}

impl fmt::Display for UtcTimestamp {
    /// `2026-09-26 06:04 UTC`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (h, m) = self.hour_minute();
        match self.date() {
            Some(d) => write!(f, "{d} {h:02}:{m:02} UTC"),
            None => write!(f, "@{}", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates() {
        assert_eq!(Date::parse("2026-09-26").unwrap().to_string(), "2026-09-26");
        assert!(Date::parse("2026-02-29").is_err());
        assert!(Date::parse("2024-02-29").is_ok());
        assert!(Date::parse("2026-13-01").is_err());
        assert!(Date::parse("2026-9-26").is_err());
        assert!(Date::parse("2026-09-26T00").is_err());
    }

    #[test]
    fn epoch_round_trip() {
        assert_eq!(Date::parse("1970-01-01").unwrap().days_since_epoch(), 0);
        let d = Date::parse("2026-09-26").unwrap();
        assert_eq!(d.days_since_epoch(), 20_722);
        for days in [-25_000_i64, -1, 0, 59, 60, 11_016, 20_722, 2_932_896] {
            let date = Date::from_days_since_epoch(days).unwrap();
            assert_eq!(date.days_since_epoch(), days);
        }
        assert_eq!(d.add_days(-30).unwrap().to_string(), "2026-08-27");
    }

    #[test]
    fn formats_timestamps() {
        let t = UtcTimestamp::from_unix(20_722 * 86_400 + 6 * 3600 + 4 * 60 + 59);
        assert_eq!(t.to_string(), "2026-09-26 06:04 UTC");
    }
}
