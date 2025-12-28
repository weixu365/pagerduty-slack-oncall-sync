use std::str::FromStr;
use chrono_tz::Tz;
use chrono::{DateTime, Timelike, Datelike};
use cron::Schedule;

#[derive(Debug, Clone, PartialEq)]
pub struct CronSchedule {
    pub cron: String,
    pub timezone: Tz,
    pub next_oneoff_cron: String,
    pub next_timestamp_utc: i64,
    pub next_datetime: DateTime<Tz>,
}

fn one_off_cron(at: &DateTime<Tz>) -> String {
    format!("{} {} {} {} * {}", at.minute(), at.hour(), at.day(), at.month(), at.year())
}

/**
  * Return the next schedule by a given cron expression and from time 
 */
pub fn get_next_schedule_from(cron_expression: &str, from: &DateTime<Tz>) -> Option<CronSchedule> {
    let cron_parts: Vec<_> = cron_expression.split(" ").collect();
    let expression = if cron_parts.len() == 6 {
        format!("0 {}", cron_expression)
    } else {
        cron_expression.to_string()
    };

    let expression_parts: Vec<_> = expression.split(" ").collect();
    let expression_without_seconds = expression_parts[1..].join(" ");

    let schedule = Schedule::from_str(expression.as_str()).unwrap();

    if let Some(next) = schedule.after(from).next() {
        return Some(CronSchedule { 
            cron: expression_without_seconds,
            timezone: from.timezone(),
            next_oneoff_cron: one_off_cron(&next),
            next_timestamp_utc: next.timestamp(),
            next_datetime: next
        })
    }

    None
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use chrono_tz::Tz;
    use crate::cron::get_next_schedule_from;
    use chrono::prelude::*;

    #[test]
    fn test_get_next_schedule_from_sunday() {
        let melbourne_tz = Tz::from_str("Australia/Melbourne").unwrap();
        let from = melbourne_tz.with_ymd_and_hms(2023, 1, 1, 9, 0, 1).unwrap(); // Sunday

        if let Some(next) = get_next_schedule_from("0 0 9 ? * MON-FRI *", &from) {
            assert_eq!(next.cron, "0 9 ? * MON-FRI *");
            assert_eq!(next.next_oneoff_cron, "0 9 2 1 * 2023");
            assert_eq!(next.timezone, melbourne_tz);
            assert_eq!(next.next_datetime, melbourne_tz.with_ymd_and_hms(2023, 1, 2, 9, 0, 0).unwrap()); //Monday
            assert_eq!(next.next_timestamp_utc, 1672610400);
        } else {
            assert_eq!(false, true, "should get next schedule")
        }
    }

    #[test]
    fn test_get_next_schedule_from_sunday_without_seconds_in_cron() {
        let melbourne_tz = Tz::from_str("Australia/Melbourne").unwrap();
        let from = melbourne_tz.with_ymd_and_hms(2023, 1, 1, 9, 0, 1).unwrap(); // Sunday

        let next_schedule = get_next_schedule_from("0 0 9 ? * MON-FRI *", &from);
        let next_schedule_without_seconds = get_next_schedule_from("0 9 ? * MON-FRI *", &from);

        assert_eq!(next_schedule_without_seconds, next_schedule);
    }

    #[test]
    fn test_get_next_schedule_from_friday() {
        let melbourne_tz = Tz::from_str("Australia/Melbourne").unwrap();
        let from = melbourne_tz.with_ymd_and_hms(2023, 1, 6, 9, 0, 1).unwrap(); // Friday

        if let Some(next) = get_next_schedule_from("0 0 9 ? * MON-FRI *", &from) {
            assert_eq!(next.cron, "0 9 ? * MON-FRI *");
            assert_eq!(next.next_oneoff_cron, "0 9 9 1 * 2023");
            assert_eq!(next.timezone, melbourne_tz);
            assert_eq!(next.next_datetime, melbourne_tz.with_ymd_and_hms(2023, 1, 9, 9, 0, 0).unwrap()); //Monday
            assert_eq!(next.next_timestamp_utc, 1673215200);
        } else {
            assert_eq!(false, true, "should get next schedule")
        }
    }
}
