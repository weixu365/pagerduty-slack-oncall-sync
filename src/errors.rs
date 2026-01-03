use std::{env::VarError, num::ParseIntError};

use aws_sdk_cloudformation::operation::describe_stacks::DescribeStacksError;
use aws_sdk_dynamodb::{
    error::SdkError,
    operation::{delete_item::DeleteItemError, get_item::GetItemError, put_item::PutItemError, scan::ScanError, update_item::UpdateItemError},
};
use aws_sdk_scheduler::operation::list_schedules::ListSchedulesError;
use aws_sdk_scheduler::operation::{create_schedule::CreateScheduleError, delete_schedule::DeleteScheduleError};
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError;
use lambda_runtime::Diagnostic;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Failed to decode base64, `{0:?}`")]
    Base64DecodeError(#[from] base64::DecodeError),

    #[error("IO error")]
    IOError(#[from] std::io::Error),

    #[error("Invalid UTF-8 sequence")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),

    #[error("Failed to get header from request: `{0:?}`")]
    ToStrError(#[from] reqwest::header::ToStrError),

    #[error("Slack error: `{0:?}`")]
    SlackError(String),

    #[error("Failed to send request to PagerDuty, error: `{0:?}`")]
    PagerDutyError(String),

    #[error("Failed to parse int, error: `{0:?}`")]
    ParseIntError(ParseIntError),

    #[error("Reqwest error")]
    ReqwestError(#[from] reqwest::Error),

    #[error("Invalid Slack request: `{0:?}`")]
    InvalidSlackRequest(String),

    #[error("Slack App not installed, error: `{0:?}`")]
    SlackInstallationNotFoundError(String),

    #[error("Failed to update user group in Slack, error: `{0:?}`")]
    SlackUpdateUserGroupError(String),

    #[error("User group not found in Slack: `{0:?}`")]
    SlackUserGroupNotFoundError(String),

    #[error("Failed to describe cloudformation stack: `{0:?}`")]
    DescribeStacksError(#[from] SdkError<DescribeStacksError>),

    #[error("Failed to put item to DynamoDB: `{0:?}`")]
    GetSecretValueError(#[from] SdkError<GetSecretValueError>),

    #[error("Failed to put item to DynamoDB: `{0:?}`")]
    DynamoDBPutItemError(#[from] SdkError<PutItemError>),

    #[error("Failed to update item to DynamoDB: `{0:?}`")]
    DynamoDBUpdateItemError(#[from] SdkError<UpdateItemError>),

    #[error("Failed to delete item from DynamoDB: `{0:?}`")]
    DynamoDBDeleteItemError(#[from] SdkError<DeleteItemError>),

    #[error("Failed to get item from DynamoDB: `{0:?}`")]
    DynamoDBGetItemError(#[from] SdkError<GetItemError>),

    #[error("Failed to scan DynamoDB table: `{0:?}`")]
    DynamoDBScanError(#[from] SdkError<ScanError>),

    #[error("Failed to create schedule in AWS Scheduler: `{0:?}`")]
    CreateScheduleError(#[from] SdkError<CreateScheduleError>),

    #[error("Failed to list current schedules in AWS Scheduler: `{0:?}`")]
    ListScheduleError(#[from] SdkError<ListSchedulesError>),

    #[error("Failed to delete schedule in AWS Scheduler: `{0:?}`")]
    DeleteScheduleError(#[from] SdkError<DeleteScheduleError>),

    #[error("Failed to serialize/deserialize json: `{0:?}`")]
    JsonError(#[from] serde_json::Error),

    #[error("Invalid key length: expected 32 bytes, got {0} bytes.")]
    InvalidKeyLength(usize),

    #[error("Invalid data: {0:?}")]
    InvalidData(String),

    #[error("Invalid secret: {0:?}")]
    InvalidSecret(String),

    #[error("Failed to encrypt/decrypt: `{0:?}`")]
    Chacha20poly1305Error(#[from] chacha20poly1305::Error),

    #[error("{0:?}")]
    HttpError(String),

    #[error("Invalid regex: `{0:?}`")]
    RegexError(#[from] regex::Error),

    #[error("Invalid timezone: `{0:?}`")]
    TimeZoneError(#[from] chrono_tz::ParseError),

    #[error("Failed to load enviroment variable: `{0:?}`")]
    VarError(#[from] VarError),

    #[error("Unexpected error: `{0:?}`")]
    UnexpectedError(String),
}

// required by Lambda Runtime crate
impl From<AppError> for Diagnostic {
    fn from(error: AppError) -> Diagnostic {
        Diagnostic {
            error_type: format!("{:?}", error),
            error_message: error.to_string(),
        }
    }
}
