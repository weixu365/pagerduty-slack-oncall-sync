use crate::errors::AppError;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::str::FromStr;

pub fn get_current_timestamp_utc() -> DateTime<Utc> {
    Utc::now()
}

pub fn get_current_timestamp_tz(tz: String) -> Result<DateTime<Tz>, AppError> {
    let timezone = Tz::from_str(&tz)?;

    Ok(get_current_timestamp(timezone))
}

pub fn get_current_timestamp(tz: Tz) -> DateTime<Tz> {
    Utc::now().with_timezone(&tz)
}

pub fn get_timezone(tz: &str) -> Result<Tz, AppError> {
    let timezone = Tz::from_str(tz)?;
    Ok(timezone)
}
