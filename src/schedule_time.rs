use std::time::{SystemTime, UNIX_EPOCH};

const DAY: i64 = 86_400;
const HOUR: i64 = 3_600;
const CENTRAL_STANDARD: i64 = -6 * HOUR;
const CENTRAL_DAYLIGHT: i64 = -5 * HOUR;
const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn parse_local(value: &str, zone: &str) -> Result<u64, String> {
    let date = DateTime::parse(value).ok_or("Enter a valid date and time.")?;
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
                (CENTRAL_STANDARD, CENTRAL_DAYLIGHT) => return Err("This Central Time hour occurs twice. Choose Central daylight time for the first occurrence or Central standard time for the second.".into()),
                _ => return Err("This Central Time hour is skipped when clocks move forward. Choose another time.".into()),
            }
        }
        "America/Chicago" => return Err("Choose a Central Time date in 2007 or later.".into()),
        _ => return Err("Choose a supported time zone.".into()),
    };
    u64::try_from(utc).map_err(|_| "Choose a date and time in 1970 or later.".into())
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
        date_days(self.year, self.month, self.day) * DAY
            + self.hour * HOUR
            + self.minute * 60
            + self.second
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
