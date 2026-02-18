use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use http::Method;
use reqwest::Client;
use serde_derive::Deserialize;
use serde_json::Value;

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

#[derive(Debug, Deserialize)]
pub struct PagerDutySchedule {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct PagerDutySchedulesResponse {
    pub schedules: Vec<PagerDutySchedule>,
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

        let _response: serde_json::Value = 
            self.send_request::<serde_json::Value, ()>("/users/me", Method::GET, None, None).await?;

        Ok(())
    }

    fn format_datetime(&self, date_time: &DateTime<Utc>) -> String {
        date_time.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    pub async fn get_on_call_users(&self, from: Option<DateTime<Utc>>) -> Result<Vec<PagerDutyUser>, AppError> {
        let mut params = vec![("time_zone", "UTC")];

        let since;
        let until;
        if let Some(from_time) = from {
            since = self.format_datetime(&from_time);
            until = self.format_datetime(&(from_time + Duration::minutes(10)));
            params.push(("since", since.as_str()));
            params.push(("until", until.as_str()));
        }

        let url = format!("/schedules/{}/users", &self.schedule_id);
        let users_response: PagerDutyUsersResponse = self.send_request(&url, Method::GET, Some(&params), None).await?;

        Ok(users_response.users)
    }

    pub async fn list_schedules(&self, query: Option<&str>) -> Result<Vec<PagerDutySchedule>, AppError> {
        tracing::info!("Fetching PagerDuty schedules");

        let mut params = vec![("limit", "100")];
        if let Some(search) = query {
            let trimmed = search.trim();
            if !trimmed.is_empty() {
                params.push(("query", trimmed));
            }
        }

        let schedules_response: PagerDutySchedulesResponse =
            self.send_request("/schedules", Method::GET, Some(&params), None).await?;

        Ok(schedules_response.schedules)
    }

    async fn send_request<T, Q>(
        &self,
        endpoint: &str,
        method: Method,
        params: Option<&Q>,
        payload: Option<&Value>,
    ) -> Result<T, AppError>
    where
        T: for<'a> serde::Deserialize<'a>,
        Q: serde::Serialize,
    {
        let url = format!("{}{}", self.base_url, endpoint);

        let mut request_builder = self
            .http_client
            .request(method.clone(), url)
            .header("Authorization", format!("Token token={}", self.api_token))
            .header("Accept", "application/vnd.pagerduty+json;version=2");

        if let Some(params) = params {
            request_builder = request_builder.query(params);
        }

        if let Some(payload) = payload {
            let body: String = payload.to_string();
            request_builder = request_builder.body(body);
        }

        let response = request_builder.send().await?;

        if response.status().is_success() {
            let json_response: T = response.json().await?;
            Ok(json_response)
        } else {
            tracing::error!(status = response.status().as_u16(), endpoint, "Failed sending request to PagerDuty");
            Err(AppError::PagerDutyError(format!("Failed sending request to PagerDuty: {}", response.status())))
        }
    }
}
