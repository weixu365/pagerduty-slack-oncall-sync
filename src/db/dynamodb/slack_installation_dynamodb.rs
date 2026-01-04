use async_trait::async_trait;
use aws_sdk_dynamodb::{types::AttributeValue, Client};
use chrono::Utc;

use crate::config::Config;
use crate::db::slack_installation::{SlackInstallation, SlackInstallationRepository};
use crate::utils::dynamodb_client::{get_attribute, get_encrypted_attribute, get_optional_encrypted_attribute};
use crate::{encryptor::Encryptor, errors::AppError};

pub struct SlackInstallationsDynamoDb {
    client: Client,
    table_name: String,
    encryptor: Encryptor,
}

impl SlackInstallationsDynamoDb {
    pub fn new(config: &Config, encryptor: Encryptor) -> SlackInstallationsDynamoDb {
        SlackInstallationsDynamoDb {
            client: Client::new(&config.aws_config),
            table_name: config.installations_table_name.to_string(),
            encryptor,
        }
    }

    fn installation_id(&self, slack_team_id: &str, slack_enterprise_id: &str) -> String {
        format!("{}:{}", slack_team_id, slack_enterprise_id)
    }

    fn parse_installation(
        &self,
        item: &std::collections::HashMap<String, AttributeValue>,
    ) -> Result<SlackInstallation, AppError> {
        Ok(SlackInstallation {
            team_id: get_attribute(item, "team_id")?,
            team_name: get_attribute(item, "team_name")?,
            enterprise_id: get_attribute(item, "enterprise_id")?,
            enterprise_name: get_attribute(item, "enterprise_name")?,
            is_enterprise_install: get_attribute(item, "is_enterprise_install")?.eq_ignore_ascii_case("true"),

            access_token: get_encrypted_attribute(item, "access_token", &self.encryptor)?,
            token_type: get_attribute(item, "token_type")?,
            scope: get_attribute(item, "scope")?,
            authed_user_id: get_attribute(item, "authed_user_id")?,
            app_id: get_attribute(item, "app_id")?,
            bot_user_id: get_attribute(item, "bot_user_id")?,

            pager_duty_token: get_optional_encrypted_attribute(item, "pagerduty_token", &self.encryptor)?,
        })
    }
}

#[async_trait]
impl SlackInstallationRepository for SlackInstallationsDynamoDb {
    async fn save_slack_installation(&self, installation: &SlackInstallation) -> Result<(), AppError> {
        let now = Utc::now();

        let t = installation.clone();
        let encrypted_token = self.encryptor.encrypt(&t.access_token)?;
        let encrypted_token_json = serde_json::to_string(&encrypted_token)?;

        let builder = self
            .client
            .put_item()
            .item("id", AttributeValue::S(self.installation_id(&installation.team_id, &installation.enterprise_id)))
            .item("team_id", AttributeValue::S(t.team_id))
            .item("team_name", AttributeValue::S(t.team_name))
            .item("enterprise_id", AttributeValue::S(t.enterprise_id))
            .item("enterprise_name", AttributeValue::S(t.enterprise_name))
            .item("is_enterprise_install", AttributeValue::S(t.is_enterprise_install.to_string()))
            .item("access_token", AttributeValue::S(encrypted_token_json))
            .item("token_type", AttributeValue::S(t.token_type))
            .item("scope", AttributeValue::S(t.scope))
            .item("authed_user_id", AttributeValue::S(t.authed_user_id))
            .item("app_id", AttributeValue::S(t.app_id))
            .item("bot_user_id", AttributeValue::S(t.bot_user_id))
            .item("created_at", AttributeValue::S(now.to_rfc3339()))
            .item("last_updated_at", AttributeValue::S(now.to_rfc3339()));

        let request = builder.table_name(&self.table_name);

        tracing::info!(?request, "Save slack installation to DynamoDB");
        request.send().await?;

        Ok(())
    }

    async fn update_pagerduty_token(
        &self,
        slack_team_id: String,
        slack_enterprise_id: String,
        pagerduty_token: &str,
    ) -> Result<(), AppError> {
        let now = Utc::now();
        let installation_id = self.installation_id(&slack_team_id, &slack_enterprise_id);
        let encrypted_token = self.encryptor.encrypt(pagerduty_token)?;
        let encrypted_token_json = serde_json::to_string(&encrypted_token)?;

        let request = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("id", AttributeValue::S(installation_id.to_string()))
            .update_expression("SET pagerduty_token = :pagerduty_token, last_updated_at = :last_updated_at")
            .condition_expression("id = :id")
            .expression_attribute_values(":pagerduty_token", AttributeValue::S(encrypted_token_json))
            .expression_attribute_values(":last_updated_at", AttributeValue::S(now.to_rfc3339()))
            .expression_attribute_values(":id", AttributeValue::S(installation_id.to_string()));

        tracing::info!(slack_team_id, slack_enterprise_id, "Update pagerduty token for slack installation in DynamoDB");
        request.send().await?;

        Ok(())
    }

    async fn get_slack_installation(
        &self,
        slack_team_id: &str,
        slack_enterprise_id: &str,
    ) -> Result<SlackInstallation, AppError> {
        let installation_id = self.installation_id(slack_team_id, slack_enterprise_id);

        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("id", AttributeValue::S(installation_id.clone()))
            .send()
            .await?;

        let item = result.item.ok_or_else(|| {
            AppError::SlackInstallationNotFoundError(format!(
                "Slack installation not found for team: {}, enterprise: {}",
                slack_team_id, slack_enterprise_id
            ))
        })?;

        self.parse_installation(&item)
    }

    async fn list_installations(&self) -> Result<Vec<SlackInstallation>, AppError> {
        let all_items: Vec<_> = self
            .client
            .scan()
            .table_name(&self.table_name)
            .into_paginator()
            .items()
            .send()
            .collect::<Result<Vec<_>, _>>()
            .await?;

        tracing::debug!(count = all_items.len(), "Retrieved all Slack installation items from DynamoDB");

        let installations: Vec<SlackInstallation> = all_items
            .into_iter()
            .filter_map(|item| match self.parse_installation(&item) {
                Ok(installation) => Some(installation),
                Err(err) => {
                    tracing::error!(%err, item = ?item, "Failed to parse Slack installation, skipping");
                    None
                }
            })
            .collect();

        Ok(installations)
    }
}
