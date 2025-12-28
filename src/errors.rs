use std::{num::ParseIntError, env::VarError};

use aws_sdk_cloudformation::operation::describe_stacks::DescribeStacksError;
use aws_sdk_dynamodb::{operation::{put_item::PutItemError, delete_item::DeleteItemError, scan::ScanError, update_item::UpdateItemError}, error::SdkError};
use aws_sdk_scheduler::operation::{create_schedule::CreateScheduleError, delete_schedule::DeleteScheduleError};
use aws_sdk_scheduler::operation::list_schedules::ListSchedulesError;
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError;
use lambda_runtime::Diagnostic;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Failed to decode base64, `{0:?}`")]
    Base64DecodeError(#[from] base64::DecodeError),

    #[error("IO error")]
    IOError(#[from] std::io::Error),

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

    #[error("Failed to scan DynamoDB table: `{0:?}`")]
    DynamoDBScanError(#[from] SdkError<ScanError>),

    #[error("Failed to create schedule in AWS Scheduler: `{0:?}`")]
    CreateScheduleError(#[from] SdkError<CreateScheduleError>),

    #[error("Failed to list current schedules in AWS Scheduler: `{0:?}`")]
    ListScheduleError(#[from] SdkError<ListSchedulesError>),

    #[error("Failed to delete schedule in AWS Scheduler: `{0:?}`")]
    DeleteScheduleError(#[from] SdkError<DeleteScheduleError>),

    #[error("Failed to encrypt/decrypt: `{0:?}`")]
    Chacha20poly1305Error(#[from] chacha20poly1305::Error),

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
