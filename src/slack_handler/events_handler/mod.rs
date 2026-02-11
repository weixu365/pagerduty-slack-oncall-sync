use std::{env, sync::Arc};

use crate::slack_handler::morphism_patches::push_event::{SlackEventCallbackBody, SlackPushEvent};
use aws_lambda_events::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};

use crate::utils::logging::json_tracing;
use crate::{
    aws::event_bridge_scheduler::EventBridgeScheduler,
    config::Config,
    db::dynamodb::{ScheduledTasksDynamodb, SlackInstallationsDynamoDb},
    errors::AppError,
    slack_handler::{
        events_handler::{app_home_opened_handler::AppHomeOpenedEvent, slack_request::parse_slack_request},
        utils::{request_utils::validate_request, slack_response::response},
    },
};

pub mod app_home_opened_handler;
pub mod slack_request;

use app_home_opened_handler::app_home_opened;

pub async fn handle_slack_events(
    config: &Arc<Config>,
    event: ApiGatewayProxyRequest,
) -> Result<ApiGatewayProxyResponse, AppError> {
    json_tracing::info!("Processing slack events", event = &event);

    let request_body = event.body.as_deref().unwrap_or("");

    let secrets = config.secrets().await?;
    validate_request(event.headers, request_body, &secrets.slack_signing_secret)?;

    let slack_request = parse_slack_request(request_body)?;

    let encryptor = config.build_encryptor().await?;
    let scheduled_tasks_db = ScheduledTasksDynamodb::new(&config, encryptor);
    let slack_installations_db = SlackInstallationsDynamoDb::new(&config, config.build_encryptor().await?);

    let lambda_arn = env::var("UPDATE_USER_GROUP_LAMBDA")?;
    let lambda_role = env::var("UPDATE_USER_GROUP_LAMBDA_ROLE")?;
    let scheduler = EventBridgeScheduler::new(&config, lambda_arn, lambda_role);

    match slack_request {
        SlackPushEvent::UrlVerification(url_verification) => {
            tracing::info!(?url_verification, "Received URL verification event");

            let challenge_response = serde_json::json!({
                "challenge": url_verification.challenge
            })
            .to_string();

            return response(200, challenge_response);
        }

        SlackPushEvent::EventCallback(callback_event) => match &callback_event.event {
            SlackEventCallbackBody::AppHomeOpened(app_home_opened_event) => {
                json_tracing::info!(
                    "Received AppHomeOpened event",
                    event_type = &"AppHomeOpened",
                    user_id = &app_home_opened_event.user.0,
                    team_id = &callback_event.team_id.0,
                    enterprise_id = &callback_event
                        .enterprise_id
                        .as_ref()
                        .map(|e| e.0.as_str())
                        .unwrap_or(""),
                );

                let home_opened_event = AppHomeOpenedEvent {
                    user_id: app_home_opened_event.user.0.clone(),
                    team_id: callback_event.team_id.0.clone(),
                    enterprise_id: callback_event
                        .enterprise_id
                        .as_ref()
                        .map(|e| e.0.clone())
                        .unwrap_or_default(),
                };

                app_home_opened(&home_opened_event, &scheduled_tasks_db, &slack_installations_db, &scheduler, 5)
                    .await?;
            }
            _ => {
                tracing::warn!("Received unsupported event callback type");
            }
        },
        SlackPushEvent::AppRateLimited(event) => {
            json_tracing::info!("Received app rate limited event", event = &event);
        }
    }

    response(200, r#"{"status": "completed"}"#.to_string())
}
