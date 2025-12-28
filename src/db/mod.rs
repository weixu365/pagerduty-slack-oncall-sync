pub mod dynamodb_client;
mod slack_installation;
mod slack_installation_dynamodb;

pub use slack_installation::SlackInstallation;
pub use slack_installation_dynamodb::SlackInstallationsDynamoDb;
