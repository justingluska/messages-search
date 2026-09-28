//! Local time for dates across the whole history. A single offset taken at
//! startup would put every message from the other half of the year (DST) an
//! hour off: wrong hour buckets, wrong days around midnight, shifted date
//! filters. `Tz::System` asks the OS time zone for each instant.

use chrono::{Local, NaiveDate, Offset, TimeZone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tz {
    /// A fixed UTC offset in seconds (tests, reproducible tools).
    Fixed(i64),
    /// The system time zone, DST included.
    System,
}

impl From<i64> for Tz {
    fn from(offset_s: i64) -> Self {
        Tz::Fixed(offset_s)
    }
}

impl Tz {
    /// UTC offset in seconds at the instant `ms` (unix ms).
    pub fn offset_at(&self, ms: i64) -> i64 {
        match self {
            Tz::Fixed(o) => *o,
            Tz::System => Local
                .timestamp_millis_opt(ms)
                .single()
                .map_or(0, |d| i64::from(d.offset().fix().local_minus_utc())),
        }
    }

    /// Unix ms of local midnight starting `y-m-d`.
    pub fn local_midnight_ms(&self, y: i64, m: i64, d: i64) -> Option<i64> {
        let date = NaiveDate::from_ymd_opt(
            i32::try_from(y).ok()?,
            u32::try_from(m).ok()?,
            u32::try_from(d).ok()?,
        )?;
        match self {
            Tz::Fixed(o) => Some((date.and_hms_opt(0, 0, 0)?.and_utc().timestamp() - o) * 1000),
            // earliest(): on a DST gap at midnight, the first valid instant.
            Tz::System => Local
                .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
                .earliest()
                .map(|t| t.timestamp_millis()),
        }
    }

    /// Local days since 1970-01-01 for the instant `ms`.
    pub fn local_day(&self, ms: i64) -> i64 {
        (ms.div_euclid(1000) + self.offset_at(ms)).div_euclid(86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_offsets() {
        let tz = Tz::Fixed(-5 * 3600);
        assert_eq!(
            tz.local_midnight_ms(2024, 1, 1),
            Some((1_704_067_200 + 5 * 3600) * 1000)
        );
        assert_eq!(tz.local_day(1_704_067_200_000), 19_722); // Dec 31 locally
        assert_eq!(Tz::Fixed(0).local_day(1_704_067_200_000), 19_723);
        assert_eq!(tz.local_midnight_ms(2024, 2, 30), None);
    }
}
