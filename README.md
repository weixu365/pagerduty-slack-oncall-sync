# On-Call Support

A Rust serverless app that syncs PagerDuty on-call rotations to Slack user groups. When someone goes on-call in PagerDuty, the corresponding Slack user group is automatically updated so you can `@mention` the right people.

## Deploy to AWS

Sample SAM templates are in `deploy/sam/`. Prerequisites: [AWS SAM CLI](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html), Lambda zip artifacts.

1. Create your own repo and copy deploy/sam/* to the root of your repo

2. Download binaries from Github:
   ```bash
   make download-lambda
   ```

3. Deploy:
   ```bash
   make deploy-dev
   ```

4. Before deploy: ensure Secrets Manager has `on-call-support/secrets` with `encryption_key`, `slack_client_id`, `slack_client_secret`, `slack_signing_secret`. Update `ADMIN_USER_SLACK_IDS` in the template if needed.

## Install Slack App

Use `deploy/slack/slack_app_manifest.yml` as a reference when creating your Slack app. **Update all URLs** with the `ApiGatewayApiEndpoint` output from your SAM stack (e.g. `https://abc123.execute-api.ap-southeast-2.amazonaws.com/`).

Replace `aws_gw_api_id.execute-api.ap-southeast-2.amazonaws.com` in the manifest with your actual API Gateway base URL. The paths are:

| Setting | Path |
|---------|------|
| Slash command URL | `{endpoint}{Stage}/slack/command` |
| OAuth redirect | `{endpoint}{Stage}/slack/oauth` |
| Event subscriptions | `{endpoint}{Stage}/slack/events` |
| Interactivity | `{endpoint}{Stage}/slack/interactive` |
| Message menu (external select) | `{endpoint}{Stage}/slack/external_select` |

`{Stage}` matches your deployment (e.g. `dev` or `prod`). Create the app from the manifest in [Slack API](https://api.slack.com/apps) → Create New App → From an app manifest.

## Usage

> **Note:** This doc assumes the slash command is `/on-call-support`. If you use a different command, adjust the examples accordingly.

All commands are scoped to the channel where they are run.

### GUI-style

Run without arguments to open the interactive wizard for listing and creating schedules:

```
/on-call-support
```

The wizard lets you
- View all schedules
- Create new schedules via a modal (select PagerDuty schedule, user group, cron, timezone)
- Delete schedules (Owner users)
- Manually sync all PagerDuty schedules

### Setup Schedule (CLI-style)

Create a schedule that syncs a PagerDuty schedule to a Slack user group on a cron:

```
/on-call-support schedule --user-group @user-group --pagerduty-schedule <schedule-id> --cron "0 9 ? * MON-FRI *" --timezone "Australia/Melbourne"
```

| Option | Description |
|--------|-------------|
| `--user-group` | Slack user group handle (e.g. `@oncall-support`) |
| `--pagerduty-schedule` | PagerDuty schedule ID |
| `--cron` | Cron expression (e.g. `"0 9 ? * MON-FRI *"` for weekdays at 9am) |
| `--timezone` | IANA timezone (e.g. `Australia/Melbourne`) |


## DB Layer

### DynamoDB Tables

| Table | Key Schema | Purpose |
|-------|------------|---------|
| `{prefix}schedules-{env}` | PK: `team`, SK: `task_id` | Scheduled sync tasks (PagerDuty schedule → Slack user group), One record per task |
| `{prefix}installations-{env}` | PK: `id` (`team_id:enterprise_id`) | Slack workspace installations. One record per installation (Slack workspace) |

Default prefix: `on-call-support-` (configurable via `TABLE_NAME_PREFIX`).

### Encryption

Sensitive tokens are encrypted at rest before being stored in DynamoDB:

| Field | Table | Algorithm |
|-------|-------|-----------|
| `access_token` | installations | XChaCha20-Poly1305 or AWS KMS |
| `pager_duty_token` | installations | XChaCha20-Poly1305 or AWS KMS |
| `pager_duty_token` | schedules | XChaCha20-Poly1305 or AWS KMS |

Encryption is selected at runtime:
- **KMS**: when `KMS_KEY_ID` is set
- **XChaCha20**: when `AWS_SECRET_ID` points to a 32-byte key, or from Secrets Manager `encryption_key`

### Data Saved

**ScheduledTask** (schedules table): team, task_id, channel, user_group, PagerDuty schedule ID, cron, timezone, next_update_timestamp_utc, created_by, etc.

**SlackInstallation** (installations table): team_id, access_token, bot_user_id, pager_duty_token (optional), etc.

---

## Local Development

### Build Locally

```bash
cargo build                    # debug build
cargo build --release          # release build
make lambda                    # build Lambda zips (requires cargo-lambda)
make test                      # run tests with coverage
```

For cross-compilation to Linux (e.g. Lambda):

```bash
make release                   # static binary for x86_64-linux-musl (requires cargo-zigbuild)
```

### API Processing Diagram

Slack requires a response within 3 seconds. For `/slack/command`, `/slack/interactive`, and `/slack/events`, the Lambda uses a two-phase flow:

```
Slack Request
     │
     ▼
┌─────────────────────────────────────────────────────────┐
│  API Gateway  →  slack_request_handler_lambda           │
└─────────────────────────────────────────────────────────┘
     │
     ├── /slack/oauth ──────────────► handle_slack_oauth (sync)
     │
     ├── /slack/external_select ────► handle_slack_external_select (sync, block_suggestion)
     │
     ├── /slack/command ────────────────────────────────────────────────┐
     ├── /slack/interactive ────────────────────────────────────────────┤
     └── /slack/events ─────────────────────────────────────────────────┤
                    │                                                    │
                    │  First call (no x-slack-handler-async header)      │
                    ▼                                                    │
              Return 200 immediately                                     │
                    │                                                    │
                    │  Invoke self asynchronously (Event)                │
                    │  with x-slack-handler-async: true                   │
                    └────────────────────────────────────────────────────┘
                                         │
                                         │  Second call (async invocation)
                                         ▼
                              Do actual work (command/interactive/events)
```

### Scheduled Task Flow

The update Lambda is triggered by EventBridge Scheduler. One schedule entry fires at the next cron-computed time across all tasks:

```
EventBridge Scheduler (at next cron time)
     │
     ▼
┌─────────────────────────────────────────────────────────┐
│  update_user_groups_lambda                              │
└─────────────────────────────────────────────────────────┘
     │
     ├── Load ScheduledTasks from DynamoDB
     ├── Load SlackInstallations (for tokens)
     │
     ├── For each task where next_update_timestamp_utc <= now:
     │      ├── Fetch on-call users from PagerDuty
     │      ├── Map to Slack user IDs
     │      ├── Update Slack user group
     │      ├── Update next_update_timestamp_utc in DynamoDB
     │      └── Track earliest next trigger
     │
     └── Update EventBridge Scheduler with earliest next trigger
         (so the Lambda runs again at the next task's cron time)
```
