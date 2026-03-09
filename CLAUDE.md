# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                    # build
cargo test                     # run all tests
cargo test <test_name>         # run a single test by name
cargo test service::schedule   # run all tests in a module
make test                      # run tests with coverage (cargo-llvm-cov)
make lambda                    # build Lambda zips for deployment (requires cargo-lambda)
make deploy                    # deploy to AWS via Serverless Framework
```

## Architecture

This is a Rust serverless app (AWS Lambda) that syncs PagerDuty on-call rotations to Slack user groups. Two Lambda functions:

- **`slack_request_handler_lambda`** — Handles all Slack interactions: slash commands, OAuth, events, interactive components (modals, buttons), and external suggestion dropdowns
- **`update_user_groups_lambda`** — Periodic Lambda (triggered by EventBridge Scheduler) that fetches who's on-call from PagerDuty and updates the corresponding Slack user group

### Infrastructure

- **DynamoDB** — two tables (prefix configurable via `TABLE_NAME_PREFIX` env var):
  - `*-schedules-{env}` — `ScheduledTask` records (PK: `team`, SK: `task_id`)
  - `*-installations-{env}` — `SlackInstallation` records per workspace
- **EventBridge Scheduler** — one schedule entry per `ScheduledTask`, fires the update lambda at the next cron-computed time
- **Secrets Manager** — stores `encryption_key`, `slack_client_id`, `slack_client_secret`, `slack_signing_secret`, `kms_key_id`
- All tokens in DynamoDB are encrypted at rest (XChaCha20-Poly1305 or KMS, via `Encryptor` trait)

### Code Structure

```
src/
├── service/
│   ├── pager_duty.rs      # PagerDuty API client
│   ├── slack.rs           # Slack API client + OAuth helpers
│   └── schedule.rs        # Schedule creation, validation, CreateScheduleRequest/Response
├── db/
│   ├── scheduled_task.rs  # ScheduledTask model + ScheduledTaskRepository trait
│   ├── slack_installation.rs  # SlackInstallation model + SlackInstallationRepository trait
│   └── dynamodb/          # DynamoDB implementations of repository traits
├── slack_handler/
│   ├── command_handler/   # Slash command routing (/schedule, /setup-pagerduty)
│   ├── interactive_handler/  # Button clicks, modal submissions (new_schedule_modal/, schedule_list/)
│   ├── events_handler/    # app_home_opened
│   ├── external_selection_handler/  # Dynamic dropdown options (PagerDuty schedules, timezones, user groups)
│   ├── oauth_handler/     # OAuth install flow
│   ├── views/             # Slack UI builders (schedule_list.rs, modals)
│   └── morphism_patches/  # Extensions to slack-morphism types (blocks_kit.rs, interaction_event.rs)
├── aws/
│   ├── event_bridge_scheduler.rs  # Create/update/delete EventBridge schedules
│   └── secrets_client.rs
├── user_group_updater/    # Core sync logic: fetch on-call → find Slack user → update group
├── config.rs              # Config loaded from Secrets Manager, cached via OnceCell
└── encryptor/             # Encryptor trait, XChaCha20 + KMS implementations
```

### Key Patterns

**Async Slack handling:** Slack requires a response within 3 seconds. The `slack_request_handler_lambda` detects a header flag (`x-invoke-source: async`) to distinguish first-call (acknowledge immediately) vs second-call (do actual work). The first invocation re-invokes itself asynchronously via `aws-sdk-lambda`.

**Repository trait pattern:** Database operations are behind `SlackInstallationRepository` and `ScheduledTaskRepository` traits. Tests use mock implementations directly; production uses DynamoDB structs in `db/dynamodb/`.

**HTTP mocking in tests:** `PagerDuty` and `Slack` service structs accept a `base_url` override (`#[cfg(test)] new_with_base_url(...)`) so tests can point at a `wiremock::MockServer`. See `service/schedule.rs` tests for examples.

**`CreateScheduleResponse`:** `create_new_schedule` returns `Result<CreateScheduleResponse, AppError>`. Validation errors (wrong user group, missing PagerDuty user, etc.) are captured in `response.errors: HashMap<String, String>` (keyed by Slack modal field action_id) rather than propagating as `AppError`. Infrastructure errors (DynamoDB, scheduler) still return `Err(AppError)`.

**Modal field → error key mapping:**

| Validation failure | `errors` key |
|---|---|
| PagerDuty API error / no token | `pagerduty_schedule_suggestion` |
| User group >2 members / Slack API error | `user_group_suggestion` |
| Slack user not in PagerDuty schedule | `user_group_suggestion` |
| Invalid timezone | `timezone_suggestion` |
