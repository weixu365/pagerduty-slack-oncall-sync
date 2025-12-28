use aws_config::SdkConfig;
use aws_sdk_dynamodb::{Client, types::AttributeValue};
use chrono::Utc;

use crate::{encryptor::Encryptor, errors::AppError};
use super::dynamodb_client::{get_attribute, get_optional_attribute};

use super::SlackInstallation;

pub struct SlackInstallationsDynamoDb {
    client: Client,
    table_name: String,
    encryptor: Encryptor,
}

impl SlackInstallationsDynamoDb {
    pub fn new(config: &SdkConfig, table_name: String, encryptor: Encryptor) -> SlackInstallationsDynamoDb {
        SlackInstallationsDynamoDb{ client: Client::new(&config), table_name, encryptor }
    }
   
    pub fn installation_id(&self, slack_team_id: &str, slack_enterprise_id: &str) -> String {
        format!("{}:{}", slack_team_id, slack_enterprise_id)
    }

    pub async fn save_slack_installation(&self, installation: &SlackInstallation) -> Result<(), AppError> {
        let now = Utc::now();

        let t = installation.clone();
        let encrypted_token = self.encryptor.encrypt(&t.access_token)?;
        let encrypted_token_json = serde_json::to_string(&encrypted_token).unwrap();

        let builder = self.client
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
            .item("last_updated_at", AttributeValue::S(now.to_rfc3339()))
        ;

        let request = builder.table_name(&self.table_name);

        println!("Save slack installation to DynamoDB [{request:?}]");
        request.send().await?;
        
        Ok(())
    }

    pub async fn update_pagerduty_token(&self, slack_team_id: String, slack_enterprise_id: String, pagerduty_token: &str) -> Result<(), AppError> {
        let now = Utc::now();
        let installation_id = self.installation_id(&slack_team_id, &slack_enterprise_id);
        let encrypted_token = self.encryptor.encrypt(pagerduty_token)?;
        let encrypted_token_json = serde_json::to_string(&encrypted_token).unwrap();

        let request = self.client
            .update_item()
            .table_name(&self.table_name)
            .key("id", AttributeValue::S(installation_id.to_string()))
            .update_expression("SET pagerduty_token = :pagerduty_token, last_updated_at = :last_updated_at")
            .condition_expression("id = :id")
            .expression_attribute_values(":pagerduty_token", AttributeValue::S(encrypted_token_json))
            .expression_attribute_values(":last_updated_at", AttributeValue::S(now.to_rfc3339()))
            .expression_attribute_values(":id", AttributeValue::S(installation_id.to_string()))
        ;

        println!("Update pagerduty token for slack installation in DynamoDB, team_id: {}, enterprise_id: {}", slack_team_id, slack_enterprise_id);
        request.send().await?;
        
        Ok(())
    }

    pub async fn list_installations(&self) -> Result<Vec<SlackInstallation>, AppError> {
        let scan_output = self.client
            .scan()
            .table_name(&self.table_name)
            .send()
            .await?;
        
        let items: Vec<SlackInstallation> = scan_output.items.unwrap_or_else(Vec::new)
            .into_iter()
            .map(|item| {
                let team_id = get_attribute(&item, "team_id");
                let encrypted_token_json = get_attribute(&item, "access_token");
                let encrypted_token = serde_json::from_str(&encrypted_token_json).unwrap();
                let access_token = self.encryptor.decrypt(&encrypted_token)
                    .expect(format!("Couldn't decrypt slack token for installation {}", team_id).as_str());

                let pagerduty_token = get_optional_attribute(&item, "pagerduty_token")
                    .map(|json| serde_json::from_str(&json).unwrap())
                    .map(|encrypted| self.encryptor.decrypt(&encrypted))
                    .map(|token| token.expect(format!("Couldn't decrypt pagerduty token for installation {}", team_id).as_str()))
                ;

                SlackInstallation {
                    team_id,
                    team_name: get_attribute(&item, "team_name"),
                    enterprise_id: get_attribute(&item, "enterprise_id"),
                    enterprise_name: get_attribute(&item, "enterprise_name"),
                    is_enterprise_install: get_attribute(&item, "is_enterprise_install").eq_ignore_ascii_case("true"),
                    
                    access_token,
                    token_type: get_attribute(&item, "token_type"),
                    scope: get_attribute(&item, "scope"),
                    authed_user_id: get_attribute(&item, "authed_user_id"),
                    app_id: get_attribute(&item, "app_id"),
                    bot_user_id: get_attribute(&item, "bot_user_id"),

                    pager_duty_token: pagerduty_token,
                }
            })
            .collect();

        Ok(items)
    }
}
