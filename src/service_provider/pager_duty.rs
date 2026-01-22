use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
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
    http_client: Arc<Client>,
    api_token: String,
    schedule_id: String,
    base_url: String,
}

impl PagerDuty {
    pub fn new(http_client: Arc<Client>, api_token: String, schedule_id: String) -> PagerDuty {
        PagerDuty {
            http_client,
            api_token,
            schedule_id,
            base_url: "https://api.pagerduty.com".to_string(),
        }
    }

    pub async fn validate_token(&self) -> Result<(), AppError> {
        tracing::info!("Validating PagerDuty API token");

        let response = self
            .http_client
            .get(&format!("{}/users/me", self.base_url))
            .header("Authorization", format!("Token token={}", self.api_token))
            // .header("Accept", "application/vnd.pagerduty+json;version=2")
            .send()
            .await?;

        response.error_for_status().map_err(|err| {
            tracing::error!(%err, "Error validating PagerDuty API token");
            AppError::PagerDutyError("Invalid PagerDuty API token".to_string())
        })?;

        Ok(())
    }

    fn format_datetime(&self, date_time: &DateTime<Utc>) -> String {
        date_time.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    pub async fn get_on_call_users(&self, from: DateTime<Utc>) -> Result<Vec<PagerDutyUser>, AppError> {
        let since = self.format_datetime(&from);
        let until = self.format_datetime(&(from + Duration::minutes(10)));

        let response = self
            .http_client
            .get(&format!("{}/schedules/{}/users", self.base_url, &self.schedule_id))
            .header("Authorization", format!("Token token={}", &self.api_token))
            // .query(&[("time_zone", "Australia/Melbourne"), ("since", "2023-05-19 09:00"), ("until", "2023-05-20 09:00")])
            .query(&[
                ("time_zone", "UTC"),
                ("since", since.as_str()),
                ("until", until.as_str()),
            ])
            .send()
            .await?;

        match response.error_for_status() {
            Ok(res) => {
                let users_response: PagerDutyUsersResponse = res.json().await?;
                Ok(users_response.users)
            }

            Err(err) => {
                tracing::error!(%err, "Error calling PagerDuty API");
                Err(AppError::PagerDutyError(err.to_string()))
            }
        }
    }
}
