use async_trait::async_trait;
use aws_sdk_dynamodb::{Client, types::AttributeValue};
use chrono::Utc;
use futures::stream::{self, StreamExt};

use crate::config::Config;
use crate::db::slack_installation::{SlackInstallation, SlackInstallationRepository};
use crate::utils::dynamodb_client::{get_attribute, get_encrypted_attribute, get_optional_encrypted_attribute};
use crate::{encryptor::Encryptor, errors::AppError};
use crate::utils::logging::json_tracing;
use std::sync::Arc;

pub struct SlackInstallationsDynamoDb {
    pub(crate) client: Client,
    pub(crate) table_name: String,
    pub(crate) encryptor: Arc<dyn Encryptor + Send + Sync>,
}

impl SlackInstallationsDynamoDb {
    pub fn new(config: &Config, encryptor: Arc<dyn Encryptor + Send + Sync>) -> SlackInstallationsDynamoDb {
        SlackInstallationsDynamoDb {
            client: Client::new(&config.aws_config),
            table_name: config.installations_table_name.to_string(),
            encryptor,
        }
    }

    pub(crate) fn installation_id(&self, slack_team_id: &str, slack_enterprise_id: &str) -> String {
        format!("{}:{}", slack_team_id, slack_enterprise_id)
    }

    pub(crate) async fn parse_installation(
        &self,
        item: &std::collections::HashMap<String, AttributeValue>,
    ) -> Result<SlackInstallation, AppError> {
        Ok(SlackInstallation {
            team_id: get_attribute(item, "team_id")?,
            team_name: get_attribute(item, "team_name")?,
            enterprise_id: get_attribute(item, "enterprise_id")?,
            enterprise_name: get_attribute(item, "enterprise_name")?,
            is_enterprise_install: get_attribute(item, "is_enterprise_install")?.eq_ignore_ascii_case("true"),

            access_token: get_encrypted_attribute(item, "access_token", &self.encryptor).await?,
            token_type: get_attribute(item, "token_type")?,
            scope: get_attribute(item, "scope")?,
            authed_user_id: get_attribute(item, "authed_user_id")?,
            app_id: get_attribute(item, "app_id")?,
            bot_user_id: get_attribute(item, "bot_user_id")?,

            pager_duty_token: get_optional_encrypted_attribute(item, "pagerduty_token", &self.encryptor).await?,
        })
    }
}

#[async_trait]
impl SlackInstallationRepository for SlackInstallationsDynamoDb {
    async fn save_slack_installation(&self, installation: &SlackInstallation) -> Result<(), AppError> {
        let now = Utc::now();

        let t = installation.clone();
        let encrypted_token = self.encryptor.encrypt(&t.access_token).await?;

        let builder = self
            .client
            .put_item()
            .item("id", AttributeValue::S(self.installation_id(&installation.team_id, &installation.enterprise_id)))
            .item("team_id", AttributeValue::S(t.team_id))
            .item("team_name", AttributeValue::S(t.team_name))
            .item("enterprise_id", AttributeValue::S(t.enterprise_id))
            .item("enterprise_name", AttributeValue::S(t.enterprise_name))
            .item("is_enterprise_install", AttributeValue::S(t.is_enterprise_install.to_string()))
            .item("access_token", AttributeValue::S(encrypted_token))
            .item("token_type", AttributeValue::S(t.token_type))
            .item("scope", AttributeValue::S(t.scope))
            .item("authed_user_id", AttributeValue::S(t.authed_user_id))
            .item("app_id", AttributeValue::S(t.app_id))
            .item("bot_user_id", AttributeValue::S(t.bot_user_id))
            .item("created_at", AttributeValue::S(now.to_rfc3339()))
            .item("last_updated_at", AttributeValue::S(now.to_rfc3339()));

        let request = builder.table_name(&self.table_name);

        json_tracing::info!("Save slack installation to DynamoDB", request = &format!("{:?}", request));
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
        let encrypted_token = self.encryptor.encrypt(pagerduty_token).await?;

        let request = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("id", AttributeValue::S(installation_id.to_string()))
            .update_expression("SET pagerduty_token = :pagerduty_token, last_updated_at = :last_updated_at")
            .condition_expression("id = :id")
            .expression_attribute_values(":pagerduty_token", AttributeValue::S(encrypted_token))
            .expression_attribute_values(":last_updated_at", AttributeValue::S(now.to_rfc3339()))
            .expression_attribute_values(":id", AttributeValue::S(installation_id.to_string()));

        json_tracing::info!("Update pagerduty token for slack installation", slack_team_id, slack_enterprise_id);
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

        self.parse_installation(&item).await
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

        json_tracing::debug!("Retrieved all Slack installation items from DynamoDB", count = &all_items.len());

        let installations: Vec<SlackInstallation> = stream::iter(all_items)
            .filter_map(|item| async move {
                match self.parse_installation(&item).await {
                    Ok(installation) => Some(installation),
                    Err(err) => {
                        json_tracing::error!("Failed to parse Slack installation, skipping", err = &err.to_string(), item = &format!("{:?}", item));
                        None
                    }
                }
            })
            .collect()
            .await;

        Ok(installations)
    }
}
