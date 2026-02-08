use std::sync::Arc;

use super::{
    options::{OptionItem, OptionsResponse, TextObject},
    slack_request::ExternalSelectRequest,
};
use crate::{db::SlackInstallationRepository, errors::AppError, service_provider::slack::Slack};

pub async fn handle_user_group_options(
    request: &ExternalSelectRequest,
    slack_installations_db: &dyn SlackInstallationRepository,
    http_client: Arc<reqwest::Client>,
) -> Result<OptionsResponse, AppError> {
    tracing::info!(action_id = %request.action_id, "Fetching user group options");

    let enterprise_id = request.enterprise.as_ref().map(|e| e.id.clone()).unwrap_or_default();

    let installation = slack_installations_db
        .get_slack_installation(&request.team.id, &enterprise_id)
        .await?;

    let slack_client = Slack::new(http_client, installation.access_token);
    let user_groups = slack_client.list_user_groups().await?;

    tracing::info!(action_id = %request.action_id, count = user_groups.len(), "Fetched user group options");

    // Filter user groups based on search value if provided
    let filtered_groups = if let Some(search_value) = request.value.as_deref() {
        let search_lower = search_value.to_lowercase();
        user_groups
            .into_iter()
            .filter(|user_group| {
                user_group.name.to_lowercase().contains(&search_lower)
                    || user_group.handle.to_lowercase().contains(&search_lower)
            })
            .collect()
    } else {
        user_groups
    };

    let options: Vec<OptionItem> = filtered_groups
        .iter()
        .map(|ug| OptionItem {
            text: TextObject {
                text_type: "plain_text".to_string(),
                text: format!("@{} ({})", ug.handle, ug.name),
            },
            value: format!("<!subteam^{}|@{}>", ug.id, ug.handle),
        })
        .collect();

    Ok(OptionsResponse { options })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_item_serialization() {
        let option = OptionItem {
            text: TextObject {
                text_type: "plain_text".to_string(),
                text: "@oncall (On-Call Team)".to_string(),
            },
            value: "<!subteam^S123|@oncall>".to_string(),
        };

        let json = serde_json::to_string(&option).unwrap();
        assert!(json.contains("plain_text"));
        assert!(json.contains("@oncall"));
    }

    #[test]
    fn test_options_response_serialization() {
        let response = OptionsResponse {
            options: vec![
                OptionItem {
                    text: TextObject {
                        text_type: "plain_text".to_string(),
                        text: "@oncall".to_string(),
                    },
                    value: "<!subteam^S123|@oncall>".to_string(),
                },
                OptionItem {
                    text: TextObject {
                        text_type: "plain_text".to_string(),
                        text: "@engineering".to_string(),
                    },
                    value: "<!subteam^S456|@engineering>".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("options"));
        assert!(json.contains("@oncall"));
        assert!(json.contains("@engineering"));
    }
}
