// SPDX-License-Identifier: Apache-2.0

//! UTC timestamp text, `YYYY-MM-DDTHH:MM:SSZ`.

use core::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// The last second that fits a four-digit year: 9999-12-31T23:59:59Z.
const MAX_SECS: u64 = 253_402_300_799;

/// Seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// A moment in time, in whole seconds since 1970-01-01T00:00:00Z.
///
/// `Display` writes it as `YYYY-MM-DDTHH:MM:SSZ` in UTC.
///
/// This replaces upstream `get_date_time_rfc3399` (`src/utils/util.cpp:68-80`)
/// and fixes its defect: upstream formats local time and then adds `Z`, which
/// is wrong on a device whose time zone is not UTC (see note 0003).
///
/// Call `to_string()` when the text must be owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rfc3339Utc {
    secs: u64,
}

impl Rfc3339Utc {
    /// Makes a timestamp from a system time.
    ///
    /// The part of a second is dropped. Returns `None` for any instant before
    /// 1970-01-01T00:00:00Z, and for any instant after 9999-12-31T23:59:59Z.
    #[must_use]
    pub fn new(t: SystemTime) -> Option<Self> {
        let since_epoch = t.duration_since(UNIX_EPOCH).ok()?;
        let secs = since_epoch.as_secs();
        if secs > MAX_SECS {
            return None;
        }
        if secs == MAX_SECS && since_epoch.subsec_nanos() != 0 {
            return None;
        }
        Some(Self { secs })
    }
}

/// Turns a day count (0 is 1970-01-01) into (year, month, day).
///
/// This is Howard Hinnant's `civil_from_days` algorithm. It counts in
/// 400-year eras, which each hold exactly 146097 days.
fn civil_from_days(days_since_epoch: u64) -> (u64, u64, u64) {
    // Shift the zero point to 0000-03-01, so the leap day is the last day of the year.
    let days = days_since_epoch + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // Months count from March: 0 is March, 11 is February.
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let mut year = year_of_era + era * 400;
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

impl fmt::Display for Rfc3339Utc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let days = self.secs / SECS_PER_DAY;
        let rest = self.secs % SECS_PER_DAY;
        let (year, month, day) = civil_from_days(days);
        let hour = rest / 3600;
        let minute = rest % 3600 / 60;
        let second = rest % 60;
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Rfc3339Utc;
    use std::time::{Duration, UNIX_EPOCH};

    fn text_at_secs(secs: u64) -> Option<String> {
        let t = UNIX_EPOCH + Duration::from_secs(secs);
        Rfc3339Utc::new(t).map(|stamp| stamp.to_string())
    }

    #[test]
    fn epoch() {
        assert_eq!(text_at_secs(0).as_deref(), Some("1970-01-01T00:00:00Z"));
    }

    #[test]
    fn leap_day() {
        assert_eq!(
            text_at_secs(1_709_164_800).as_deref(),
            Some("2024-02-29T00:00:00Z")
        );
    }

    #[test]
    fn year_2000_is_a_leap_year() {
        // Divisible by 400: 2000-02-29 exists.
        assert_eq!(
            text_at_secs(951_782_400).as_deref(),
            Some("2000-02-29T00:00:00Z")
        );
        assert_eq!(
            text_at_secs(951_868_800).as_deref(),
            Some("2000-03-01T00:00:00Z")
        );
    }

    #[test]
    fn year_2100_is_not_a_leap_year() {
        // Divisible by 100 but not 400: the day after 2100-02-28 is 2100-03-01.
        assert_eq!(
            text_at_secs(4_107_542_400).as_deref(),
            Some("2100-03-01T00:00:00Z")
        );
        assert_eq!(
            text_at_secs(4_107_542_400 - 86_400).as_deref(),
            Some("2100-02-28T00:00:00Z")
        );
    }

    #[test]
    fn year_end() {
        assert_eq!(
            text_at_secs(1_704_067_199).as_deref(),
            Some("2023-12-31T23:59:59Z")
        );
    }

    #[test]
    fn fixed_recent_instant() {
        assert_eq!(
            text_at_secs(1_795_017_602).as_deref(),
            Some("2026-11-18T16:00:02Z")
        );
    }

    #[test]
    fn sub_second_part_is_dropped() {
        let at_500_ms = UNIX_EPOCH + Duration::from_millis(500);
        let stamp = Rfc3339Utc::new(at_500_ms).expect("after the epoch");
        assert_eq!(stamp.to_string(), "1970-01-01T00:00:00Z");

        let leap_plus =
            UNIX_EPOCH + Duration::from_secs(1_709_164_800) + Duration::from_millis(999);
        let stamp = Rfc3339Utc::new(leap_plus).expect("after the epoch");
        assert_eq!(stamp.to_string(), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn before_the_epoch_is_none() {
        let before = UNIX_EPOCH - Duration::from_millis(500);
        assert_eq!(Rfc3339Utc::new(before), None);
    }

    #[test]
    fn last_four_digit_year_second() {
        assert_eq!(
            text_at_secs(253_402_300_799).as_deref(),
            Some("9999-12-31T23:59:59Z")
        );
        assert_eq!(text_at_secs(253_402_300_800), None);
        let half_second_late =
            UNIX_EPOCH + Duration::from_secs(253_402_300_799) + Duration::from_millis(500);
        assert_eq!(Rfc3339Utc::new(half_second_late), None);
    }
}
