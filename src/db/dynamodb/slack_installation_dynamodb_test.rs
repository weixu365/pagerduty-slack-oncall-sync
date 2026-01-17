use crate::{
    db::slack_installation::{SlackInstallation, SlackInstallationRepository},
    encryptor::Encryptor,
    errors::AppError,
};

use super::slack_installation_dynamodb::SlackInstallationsDynamoDb;
use aws_sdk_dynamodb::{types::AttributeValue, Client};
use aws_smithy_mocks::{mock, mock_client, RuleMode};
use std::collections::HashMap;

fn create_test_encryptor() -> Encryptor {
    let key = "0123456789abcdef0123456789abcdef";
    Encryptor::from_key(key).unwrap()
}

fn create_test_installation() -> SlackInstallation {
    SlackInstallation {
        team_id: "T123456".to_string(),
        team_name: "Test Team".to_string(),
        enterprise_id: "E123456".to_string(),
        enterprise_name: "Test Enterprise".to_string(),
        is_enterprise_install: false,

        access_token: "xoxb-test-token-123".to_string(),
        token_type: "bot".to_string(),
        scope: "chat:write,users:read".to_string(),

        authed_user_id: "U123456".to_string(),
        app_id: "A123456".to_string(),
        bot_user_id: "B123456".to_string(),

        pager_duty_token: Some("pd_token_123".to_string()),
    }
}

#[tokio::test]
async fn test_save_slack_installation() -> Result<(), AppError> {
    let installation = create_test_installation();
    let encryptor = create_test_encryptor();

    let rule = mock!(Client::put_item)
        .then_output(|| {
            aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    db.save_slack_installation(&installation).await?;

    Ok(())
}

#[tokio::test]
async fn test_save_slack_installation_validates_request() -> Result<(), AppError> {
    let installation = create_test_installation();
    let encryptor = create_test_encryptor();

    let expected_team_id = installation.team_id.clone();
    let expected_team_name = installation.team_name.clone();

    let rule = mock!(Client::put_item)
        .match_requests(move |req| {
            let table = req.table_name().unwrap();
            assert_eq!(table, "test-installations");

            let items = req.item().unwrap();
            assert_eq!(items.get("id").unwrap(), &AttributeValue::S("T123456:E123456".to_string()));
            assert_eq!(items.get("team_id").unwrap(), &AttributeValue::S(expected_team_id.clone()));
            assert_eq!(items.get("team_name").unwrap(), &AttributeValue::S(expected_team_name.clone()));
            assert!(items.contains_key("access_token"));
            assert!(items.contains_key("created_at"));
            assert!(items.contains_key("last_updated_at"));
            true
        })
        .then_output(|| {
            aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    db.save_slack_installation(&installation).await?;

    Ok(())
}

#[tokio::test]
async fn test_update_pagerduty_token() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let rule = mock!(Client::update_item)
        .match_requests(|req| {
            let keys = req.key().unwrap();
            assert_eq!(keys.get("id").unwrap(), &AttributeValue::S("T123456:E123456".to_string()));

            let update_expr = req.update_expression().unwrap();
            assert!(update_expr.contains("pagerduty_token"));
            assert!(update_expr.contains("last_updated_at"));

            let condition_expr = req.condition_expression().unwrap();
            assert!(condition_expr.contains("id = :id"));

            let table = req.table_name().unwrap();
            assert_eq!(table, "test-installations");
            true
        })
        .then_output(|| {
            aws_sdk_dynamodb::operation::update_item::UpdateItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    db.update_pagerduty_token(
        "T123456".to_string(),
        "E123456".to_string(),
        "new_pd_token_456",
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn test_get_slack_installation_success() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();
    let installation = create_test_installation();

    let encrypted_token = encryptor.encrypt(&installation.access_token)?;
    let encrypted_token_json = serde_json::to_string(&encrypted_token)?;

    let encrypted_pd_token = encryptor.encrypt("pd_token_123")?;
    let encrypted_pd_token_json = serde_json::to_string(&encrypted_pd_token)?;

    let mut item = HashMap::new();
    item.insert("id".to_string(), AttributeValue::S("T123456:E123456".to_string()));
    item.insert("team_id".to_string(), AttributeValue::S(installation.team_id.clone()));
    item.insert("team_name".to_string(), AttributeValue::S(installation.team_name.clone()));
    item.insert("enterprise_id".to_string(), AttributeValue::S(installation.enterprise_id.clone()));
    item.insert("enterprise_name".to_string(), AttributeValue::S(installation.enterprise_name.clone()));
    item.insert("is_enterprise_install".to_string(), AttributeValue::S(installation.is_enterprise_install.to_string()));
    item.insert("access_token".to_string(), AttributeValue::S(encrypted_token_json));
    item.insert("token_type".to_string(), AttributeValue::S(installation.token_type.clone()));
    item.insert("scope".to_string(), AttributeValue::S(installation.scope.clone()));
    item.insert("authed_user_id".to_string(), AttributeValue::S(installation.authed_user_id.clone()));
    item.insert("app_id".to_string(), AttributeValue::S(installation.app_id.clone()));
    item.insert("bot_user_id".to_string(), AttributeValue::S(installation.bot_user_id.clone()));
    item.insert("pagerduty_token".to_string(), AttributeValue::S(encrypted_pd_token_json));

    let rule = mock!(Client::get_item)
        .match_requests(|req| {
            let keys = req.key().unwrap();
            assert_eq!(keys.get("id").unwrap(), &AttributeValue::S("T123456:E123456".to_string()));

            let table = req.table_name().unwrap();
            assert_eq!(table, "test-installations");
            true
        })
        .then_output(move || {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder()
                .set_item(Some(item.clone()))
                .build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    let result = db.get_slack_installation("T123456", "E123456").await?;
    assert_eq!(result.team_id, "T123456");
    assert_eq!(result.team_name, "Test Team");
    assert_eq!(result.access_token, "xoxb-test-token-123");
    assert_eq!(result.pager_duty_token, Some("pd_token_123".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_get_slack_installation_not_found() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let rule = mock!(Client::get_item)
        .then_output(|| {
            aws_sdk_dynamodb::operation::get_item::GetItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    let result = db.get_slack_installation("T999999", "E999999").await;
    assert!(result.is_err());
    match result {
        Err(AppError::SlackInstallationNotFoundError(msg)) => {
            assert!(msg.contains("Slack installation not found"));
            assert!(msg.contains("T999999"));
            assert!(msg.contains("E999999"));
        }
        _ => panic!("Expected SlackInstallationNotFoundError"),
    }

    Ok(())
}

#[tokio::test]
async fn test_list_installations_empty() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();

    let rule = mock!(Client::scan)
        .then_output(|| {
            aws_sdk_dynamodb::operation::scan::ScanOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    let installations = db.list_installations().await?;
    assert_eq!(installations.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_list_installations_with_items() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();
    let installation = create_test_installation();

    let encrypted_token = encryptor.encrypt(&installation.access_token)?;
    let encrypted_token_json = serde_json::to_string(&encrypted_token)?;

    let encrypted_pd_token = encryptor.encrypt("pd_token_123")?;
    let encrypted_pd_token_json = serde_json::to_string(&encrypted_pd_token)?;

    let mut item = HashMap::new();
    item.insert("id".to_string(), AttributeValue::S("T123456:E123456".to_string()));
    item.insert("team_id".to_string(), AttributeValue::S(installation.team_id.clone()));
    item.insert("team_name".to_string(), AttributeValue::S(installation.team_name.clone()));
    item.insert("enterprise_id".to_string(), AttributeValue::S(installation.enterprise_id.clone()));
    item.insert("enterprise_name".to_string(), AttributeValue::S(installation.enterprise_name.clone()));
    item.insert("is_enterprise_install".to_string(), AttributeValue::S(installation.is_enterprise_install.to_string()));
    item.insert("access_token".to_string(), AttributeValue::S(encrypted_token_json));
    item.insert("token_type".to_string(), AttributeValue::S(installation.token_type.clone()));
    item.insert("scope".to_string(), AttributeValue::S(installation.scope.clone()));
    item.insert("authed_user_id".to_string(), AttributeValue::S(installation.authed_user_id.clone()));
    item.insert("app_id".to_string(), AttributeValue::S(installation.app_id.clone()));
    item.insert("bot_user_id".to_string(), AttributeValue::S(installation.bot_user_id.clone()));
    item.insert("pagerduty_token".to_string(), AttributeValue::S(encrypted_pd_token_json));

    let rule = mock!(Client::scan)
        .then_output(move || {
            aws_sdk_dynamodb::operation::scan::ScanOutput::builder()
                .set_items(Some(vec![item.clone()]))
                .build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    let installations = db.list_installations().await?;
    assert_eq!(installations.len(), 1);
    assert_eq!(installations[0].team_id, "T123456");
    assert_eq!(installations[0].team_name, "Test Team");
    assert_eq!(installations[0].access_token, "xoxb-test-token-123");
    assert_eq!(installations[0].pager_duty_token, Some("pd_token_123".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_installation_id_formatting() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();
    let rule = mock!(Client::put_item)
        .then_output(|| {
            aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    let id = db.installation_id("T123456", "E789012");
    assert_eq!(id, "T123456:E789012");

    Ok(())
}

#[tokio::test]
async fn test_parse_installation_with_valid_data() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();
    let rule = mock!(Client::put_item)
        .then_output(|| {
            aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor: encryptor.clone(),
    };

    let installation = create_test_installation();
    let encrypted_token = encryptor.encrypt(&installation.access_token)?;
    let encrypted_token_json = serde_json::to_string(&encrypted_token)?;

    let encrypted_pd_token = encryptor.encrypt("pd_token_123")?;
    let encrypted_pd_token_json = serde_json::to_string(&encrypted_pd_token)?;

    let mut item = HashMap::new();
    item.insert("team_id".to_string(), AttributeValue::S(installation.team_id.clone()));
    item.insert("team_name".to_string(), AttributeValue::S(installation.team_name.clone()));
    item.insert("enterprise_id".to_string(), AttributeValue::S(installation.enterprise_id.clone()));
    item.insert("enterprise_name".to_string(), AttributeValue::S(installation.enterprise_name.clone()));
    item.insert("is_enterprise_install".to_string(), AttributeValue::S(installation.is_enterprise_install.to_string()));
    item.insert("access_token".to_string(), AttributeValue::S(encrypted_token_json));
    item.insert("token_type".to_string(), AttributeValue::S(installation.token_type.clone()));
    item.insert("scope".to_string(), AttributeValue::S(installation.scope.clone()));
    item.insert("authed_user_id".to_string(), AttributeValue::S(installation.authed_user_id.clone()));
    item.insert("app_id".to_string(), AttributeValue::S(installation.app_id.clone()));
    item.insert("bot_user_id".to_string(), AttributeValue::S(installation.bot_user_id.clone()));
    item.insert("pagerduty_token".to_string(), AttributeValue::S(encrypted_pd_token_json));

    let result = db.parse_installation(&item)?;
    assert_eq!(result.team_id, "T123456");
    assert_eq!(result.team_name, "Test Team");
    assert_eq!(result.access_token, "xoxb-test-token-123");
    assert_eq!(result.pager_duty_token, Some("pd_token_123".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_parse_installation_without_pagerduty_token() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();
    let rule = mock!(Client::put_item)
        .then_output(|| {
            aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor: encryptor.clone(),
    };

    let installation = create_test_installation();
    let encrypted_token = encryptor.encrypt(&installation.access_token)?;
    let encrypted_token_json = serde_json::to_string(&encrypted_token)?;

    let mut item = HashMap::new();
    item.insert("team_id".to_string(), AttributeValue::S(installation.team_id.clone()));
    item.insert("team_name".to_string(), AttributeValue::S(installation.team_name.clone()));
    item.insert("enterprise_id".to_string(), AttributeValue::S(installation.enterprise_id.clone()));
    item.insert("enterprise_name".to_string(), AttributeValue::S(installation.enterprise_name.clone()));
    item.insert("is_enterprise_install".to_string(), AttributeValue::S(installation.is_enterprise_install.to_string()));
    item.insert("access_token".to_string(), AttributeValue::S(encrypted_token_json));
    item.insert("token_type".to_string(), AttributeValue::S(installation.token_type.clone()));
    item.insert("scope".to_string(), AttributeValue::S(installation.scope.clone()));
    item.insert("authed_user_id".to_string(), AttributeValue::S(installation.authed_user_id.clone()));
    item.insert("app_id".to_string(), AttributeValue::S(installation.app_id.clone()));
    item.insert("bot_user_id".to_string(), AttributeValue::S(installation.bot_user_id.clone()));

    let result = db.parse_installation(&item)?;
    assert_eq!(result.team_id, "T123456");
    assert_eq!(result.pager_duty_token, None);

    Ok(())
}

#[tokio::test]
async fn test_parse_installation_missing_required_field() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();
    let rule = mock!(Client::put_item)
        .then_output(|| {
            aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    let mut item = HashMap::new();
    item.insert("team_id".to_string(), AttributeValue::S("T123456".to_string()));

    let result = db.parse_installation(&item);
    assert!(result.is_err());
    match result {
        Err(AppError::UnexpectedError(msg)) => {
            assert!(msg.contains("Missing or invalid field"));
        }
        _ => panic!("Expected UnexpectedError"),
    }

    Ok(())
}

#[tokio::test]
async fn test_parse_installation_invalid_encrypted_token() -> Result<(), AppError> {
    let encryptor = create_test_encryptor();
    let rule = mock!(Client::put_item)
        .then_output(|| {
            aws_sdk_dynamodb::operation::put_item::PutItemOutput::builder().build()
        })
        ;

    let client = mock_client!(aws_sdk_dynamodb, RuleMode::Sequential, &[&rule]);

    let db = SlackInstallationsDynamoDb {
        client,
        table_name: "test-installations".to_string(),
        encryptor,
    };

    let mut item = HashMap::new();
    item.insert("team_id".to_string(), AttributeValue::S("T123456".to_string()));
    item.insert("team_name".to_string(), AttributeValue::S("Test Team".to_string()));
    item.insert("enterprise_id".to_string(), AttributeValue::S("E123456".to_string()));
    item.insert("enterprise_name".to_string(), AttributeValue::S("Test Enterprise".to_string()));
    item.insert("is_enterprise_install".to_string(), AttributeValue::S("false".to_string()));
    item.insert("access_token".to_string(), AttributeValue::S("invalid_json".to_string()));
    item.insert("token_type".to_string(), AttributeValue::S("bot".to_string()));
    item.insert("scope".to_string(), AttributeValue::S("chat:write".to_string()));
    item.insert("authed_user_id".to_string(), AttributeValue::S("U123456".to_string()));
    item.insert("app_id".to_string(), AttributeValue::S("A123456".to_string()));
    item.insert("bot_user_id".to_string(), AttributeValue::S("B123456".to_string()));

    let result = db.parse_installation(&item);
    assert!(result.is_err());
    match result {
        Err(AppError::InvalidData(msg)) => {
            assert!(msg.contains("invalid json field"));
            assert!(msg.contains("access_token"));
        }
        _ => panic!("Expected InvalidData error"),
    }

    Ok(())
}
