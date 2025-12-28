use std::sync::Arc;

use chrono::{DateTime, Utc, Duration};
use reqwest::Client;
use serde_derive::Deserialize;

use crate::errors::AppError;

#[derive(Debug, Deserialize)]
pub struct PagerDutyUser {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct PagerDutyUsersResponse {
    pub users: Vec<PagerDutyUser>,
}

pub struct PagerDuty {
    http_client: Arc<Box<Client>>,
    api_token: String,
    schedule_id: String,
}

impl PagerDuty {
    pub fn new(http_client: Arc<Box<Client>>, api_token: String, schedule_id: String) -> PagerDuty {
        PagerDuty { http_client, api_token, schedule_id }
    }

    fn format_datetime(&self, date_time: &DateTime<Utc>) -> String {
        date_time.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    pub async fn get_on_call_users(&self, from: DateTime<Utc>) -> Result<Vec<PagerDutyUser>, AppError>{
        let url = format!(
            "https://api.pagerduty.com/schedules/{}/users",
            &self.schedule_id
        );

        let since = self.format_datetime(&from);
        let until = self.format_datetime(&(from + Duration::minutes(10)));

        let response = self.http_client
            .get(&url)
            .header("Authorization", format!("Token token={}", &self.api_token))
            // .query(&[("time_zone", "Australia/Melbourne"), ("since", "2023-05-19 09:00"), ("until", "2023-05-20 09:00")])
            .query(&[("time_zone", "UTC"), ("since", since.as_str()), ("until", until.as_str())])
            .send()
            .await?;

        match response.error_for_status() {
            Ok(res) => {
                let users_response: PagerDutyUsersResponse = res.json().await?;
                Ok(users_response.users)
            }

            Err(err) => {
                println!("Error: {:?}", err);
                Err(AppError::PagerDutyError(err.to_string()))
            }
        }
    }
}
