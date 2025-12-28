use chrono::{Utc, DateTime};
use chrono_tz::Tz;
use std::str::FromStr;

pub fn get_current_timestamp_utc() -> DateTime<Utc> {
    Utc::now()
}

pub fn get_current_timestamp_tz(tz: String) -> DateTime<Tz> {
    let timezone = Tz::from_str(&tz).unwrap();

    get_current_timestamp(timezone)
}

pub fn get_current_timestamp(tz: Tz) -> DateTime<Tz> {
    Utc::now().with_timezone(&tz)
}

pub fn get_timezone(tz: &str) -> Tz {
    Tz::from_str(&tz).unwrap()
}
