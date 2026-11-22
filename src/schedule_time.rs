use std::time::{SystemTime, UNIX_EPOCH};

const DAY: i64 = 86_400;
const HOUR: i64 = 3_600;
const CENTRAL_STANDARD: i64 = -6 * HOUR;
const CENTRAL_DAYLIGHT: i64 = -5 * HOUR;
const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    Daily,
    Weekly,
    Monthly,
}

impl Cadence {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "monthly" => Some(Self::Monthly),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
        }
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn parse_local(value: &str, zone: &str) -> Result<u64, String> {
    let date = DateTime::parse(value).ok_or("Enter a valid date and time.")?;
    resolve_local(&date, zone, false)
}

fn resolve_local(date: &DateTime, zone: &str, recurring: bool) -> Result<u64, String> {
    let wall_time = date.seconds();
    let utc = match zone {
        "UTC" => wall_time,
        "central-standard" => wall_time - CENTRAL_STANDARD,
        "central-daylight" => wall_time - CENTRAL_DAYLIGHT,
        "America/Chicago" if date.year >= 2007 => {
            let standard = wall_time - CENTRAL_STANDARD;
            let daylight = wall_time - CENTRAL_DAYLIGHT;
            match (central_offset(standard, date.year), central_offset(daylight, date.year)) {
                (CENTRAL_STANDARD, CENTRAL_STANDARD) => standard,
                (CENTRAL_DAYLIGHT, CENTRAL_DAYLIGHT) => daylight,
                (CENTRAL_STANDARD, CENTRAL_DAYLIGHT) if recurring => daylight,
                (CENTRAL_STANDARD, CENTRAL_DAYLIGHT) => return Err("This Central Time hour occurs twice. Choose Central daylight time for the first occurrence or Central standard time for the second.".into()),
                // The standard offset maps a skipped time to one hour later in daylight time.
                _ if recurring => standard,
                _ => return Err("This Central Time hour is skipped when clocks move forward. Choose another time.".into()),
            }
        }
        "America/Chicago" => return Err("Choose a Central Time date in 2007 or later.".into()),
        _ => return Err("Choose a supported time zone.".into()),
    };
    u64::try_from(utc).map_err(|_| "Choose a date and time in 1970 or later.".into())
}

pub fn next_occurrence(anchor_at: u64, zone: &str, cadence: Cadence, after: u64) -> Option<u64> {
    let (anchor, _) = local_datetime(anchor_at, zone)?;
    if after < anchor_at {
        return Some(anchor_at);
    }

    // Jump directly to the period containing the boundary, even after a long outage.
    let (boundary, _) = local_datetime(after, zone)?;
    let days = boundary.days() - anchor.days();
    let period = match cadence {
        Cadence::Daily => days,
        Cadence::Weekly => days / 7,
        Cadence::Monthly => boundary.month_index() - anchor.month_index(),
    }
    .max(1);
    [period, period + 1].into_iter().find_map(|period| {
        let date = match cadence {
            Cadence::Daily => DateTime::from_seconds(anchor.seconds() + period * DAY)?,
            Cadence::Weekly => DateTime::from_seconds(anchor.seconds() + period * 7 * DAY)?,
            Cadence::Monthly => {
                let index = anchor.month_index() + period;
                let year = index.div_euclid(12);
                let month = index.rem_euclid(12) as usize + 1;
                if year > 9999 {
                    return None;
                }
                DateTime {
                    year,
                    month,
                    day: anchor.day.min(days_in_month(year, month)),
                    ..anchor
                }
            }
        };
        resolve_local(&date, zone, true)
            .ok()
            .filter(|&timestamp| timestamp > after)
    })
}

pub fn to_local(timestamp: u64, zone: &str) -> String {
    let Some((local, _)) = local_datetime(timestamp, zone) else {
        return String::new();
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}",
        local.year, local.month, local.day, local.hour, local.minute
    )
}

pub fn display(timestamp: u64, zone: &str) -> String {
    let Some((local, abbreviation)) = local_datetime(timestamp, zone) else {
        return String::new();
    };
    let period = if local.hour < 12 { "AM" } else { "PM" };
    let hour = (local.hour + 11) % 12 + 1;
    format!(
        "{} {}, {} at {hour}:{:02} {period} {abbreviation}",
        MONTH_NAMES[local.month - 1],
        local.day,
        local.year,
        local.minute
    )
}

fn local_datetime(timestamp: u64, zone: &str) -> Option<(DateTime, &'static str)> {
    let utc = i64::try_from(timestamp).ok()?;
    let offset = match zone {
        "UTC" => 0,
        "central-standard" => CENTRAL_STANDARD,
        "central-daylight" => CENTRAL_DAYLIGHT,
        "America/Chicago" => {
            let year = DateTime::from_seconds(utc + CENTRAL_STANDARD)?.year;
            if year < 2007 {
                return None;
            }
            central_offset(utc, year)
        }
        _ => return None,
    };
    let local = DateTime::from_seconds(utc + offset)?;
    let abbreviation = match offset {
        CENTRAL_STANDARD => "CST",
        CENTRAL_DAYLIGHT => "CDT",
        _ => "UTC",
    };
    Some((local, abbreviation))
}

fn central_offset(utc: i64, year: i64) -> i64 {
    // NIST's U.S. rule since 2007: second Sunday in March at 2 a.m. standard,
    // first Sunday in November at 2 a.m. daylight.
    let starts = sunday(year, 3, 2) * DAY + 2 * HOUR - CENTRAL_STANDARD;
    let ends = sunday(year, 11, 1) * DAY + 2 * HOUR - CENTRAL_DAYLIGHT;
    if (starts..ends).contains(&utc) {
        CENTRAL_DAYLIGHT
    } else {
        CENTRAL_STANDARD
    }
}

fn sunday(year: i64, month: usize, occurrence: i64) -> i64 {
    let first = date_days(year, month, 1);
    // Unix day zero was a Thursday. Index Sunday as zero.
    first + (-(first + 4)).rem_euclid(7) + (occurrence - 1) * 7
}

#[derive(Clone, Copy)]
struct DateTime {
    year: i64,
    month: usize,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
}

impl DateTime {
    fn parse(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if !matches!(bytes.len(), 16 | 19)
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || bytes[10] != b'T'
            || bytes[13] != b':'
            || (bytes.len() == 19 && bytes[16] != b':')
        {
            return None;
        }
        let year = digits(&bytes[..4])?;
        let month = digits(&bytes[5..7])? as usize;
        let day = digits(&bytes[8..10])?;
        let hour = digits(&bytes[11..13])?;
        let minute = digits(&bytes[14..16])?;
        let second = if bytes.len() == 19 {
            digits(&bytes[17..])?
        } else {
            0
        };
        if !(1..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || !(1..=days_in_month(year, month)).contains(&day)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return None;
        }
        Some(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }

    fn seconds(&self) -> i64 {
        self.days() * DAY + self.hour * HOUR + self.minute * 60 + self.second
    }

    fn days(&self) -> i64 {
        date_days(self.year, self.month, self.day)
    }

    fn month_index(&self) -> i64 {
        self.year * 12 + self.month as i64 - 1
    }

    fn from_seconds(seconds: i64) -> Option<Self> {
        let day = seconds.div_euclid(DAY) + days_before_year(1970);
        if !(0..days_before_year(10000)).contains(&day) {
            return None;
        }
        let mut year = 1;
        let mut end = 10000;
        while year + 1 < end {
            let middle = (year + end) / 2;
            if days_before_year(middle) <= day {
                year = middle;
            } else {
                end = middle;
            }
        }
        let mut year_day = day - days_before_year(year);
        let mut month = 1;
        while year_day >= days_in_month(year, month) {
            year_day -= days_in_month(year, month);
            month += 1;
        }
        let time = seconds.rem_euclid(DAY);
        Some(Self {
            year,
            month,
            day: year_day + 1,
            hour: time / HOUR,
            minute: time / 60 % 60,
            second: time % 60,
        })
    }
}

fn digits(bytes: &[u8]) -> Option<i64> {
    bytes.iter().try_fold(0, |value, &byte| {
        byte.is_ascii_digit()
            .then(|| 10 * value + i64::from(byte - b'0'))
    })
}

fn date_days(year: i64, month: usize, day: i64) -> i64 {
    let earlier_months: i64 = (1..month).map(|month| days_in_month(year, month)).sum();
    days_before_year(year) - days_before_year(1970) + earlier_months + day - 1
}

fn days_before_year(year: i64) -> i64 {
    let completed = year - 1;
    completed * 365 + completed / 4 - completed / 100 + completed / 400
}

fn days_in_month(year: i64, month: usize) -> i64 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    MONTH_DAYS[month - 1] + i64::from(month == 2 && leap)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHICAGO: &str = "America/Chicago";

    fn at(local: &str, zone: &str) -> u64 {
        parse_local(local, zone).unwrap()
    }

    #[test]
    fn daily_and_weekly_skip_missed_slots_and_preserve_the_first_instant() {
        let anchor = at("2024-01-01T09:15:17", "UTC");
        assert_eq!(
            next_occurrence(anchor, "UTC", Cadence::Daily, anchor - 1),
            Some(anchor)
        );
        assert_eq!(
            next_occurrence(anchor, "UTC", Cadence::Daily, anchor),
            Some(at("2024-01-02T09:15:17", "UTC"))
        );
        assert_eq!(
            next_occurrence(anchor, "UTC", Cadence::Daily, at("2094-12-31T11:01", "UTC")),
            Some(at("2095-01-01T09:15:17", "UTC"))
        );
        assert_eq!(
            next_occurrence(
                anchor,
                "UTC",
                Cadence::Weekly,
                at("2024-01-23T08:00", "UTC")
            ),
            Some(at("2024-01-29T09:15:17", "UTC"))
        );
    }

    #[test]
    fn monthly_short_months_clamp_temporarily_and_respect_leap_years() {
        let anchor = at("2024-01-31T12:00", "UTC");
        for (after, expected) in [
            ("2024-01-31T12:00", "2024-02-29T12:00"),
            ("2024-02-29T12:00", "2024-03-31T12:00"),
            ("2025-02-28T11:59", "2025-02-28T12:00"),
            ("2025-02-28T12:00", "2025-03-31T12:00"),
        ] {
            assert_eq!(
                next_occurrence(anchor, "UTC", Cadence::Monthly, at(after, "UTC")),
                Some(at(expected, "UTC"))
            );
        }
    }

    #[test]
    fn chicago_gap_shifts_one_slot_and_overlap_uses_the_earlier_instant() {
        let spring = at("2026-03-07T02:30", CHICAGO);
        let shifted = at("2026-03-08T03:30", CHICAGO);
        assert_eq!(
            next_occurrence(spring, CHICAGO, Cadence::Daily, spring),
            Some(shifted)
        );
        assert_eq!(
            next_occurrence(spring, CHICAGO, Cadence::Daily, shifted),
            Some(at("2026-03-09T02:30", CHICAGO))
        );

        let fall = at("2026-10-25T01:30", CHICAGO);
        let first = at("2026-11-01T01:30", "central-daylight");
        let second = at("2026-11-01T01:30", "central-standard");
        assert_eq!(
            next_occurrence(fall, CHICAGO, Cadence::Weekly, fall),
            Some(first)
        );
        assert_eq!(
            next_occurrence(fall, CHICAGO, Cadence::Weekly, second),
            Some(at("2026-11-08T01:30", CHICAGO))
        );
        assert_eq!(
            next_occurrence(second, CHICAGO, Cadence::Weekly, first),
            Some(second)
        );
    }

    #[test]
    fn fixed_offsets_and_supported_year_boundaries() {
        let zone = "central-standard";
        let anchor = at("2026-03-07T02:30", zone);
        assert_eq!(
            next_occurrence(anchor, zone, Cadence::Daily, anchor),
            Some(anchor + 86_400)
        );

        let last = at("9999-12-31T09:15", "UTC");
        for cadence in [Cadence::Daily, Cadence::Weekly, Cadence::Monthly] {
            assert_eq!(next_occurrence(last, "UTC", cadence, last), None);
        }
        assert_eq!(
            next_occurrence(anchor, "UTC", Cadence::Monthly, u64::MAX),
            None
        );
        assert_eq!(next_occurrence(anchor, "unknown", Cadence::Daily, 0), None);
    }
}
