VERSION=v0.5.2
GH_RELEASE_URL=https://github.com/weixu365/pagerduty-slack-oncall-sync/releases/download/pagerduty-slack-oncall-sync-$(VERSION)

download-lambda:
	mkdir -p dist
	wget -P dist $(GH_RELEASE_URL)/slack_request_handler_lambda.zip
	wget -P dist $(GH_RELEASE_URL)/update_user_groups_lambda.zip

deploy-dev:
	sam deploy --parameter-overrides Stage=dev --stack-name on-call-support-dev

deploy-preview:
	sam deploy --parameter-overrides Stage=dev --stack-name on-call-support-dev --no-execute-changeset

deploy-prod:
	sam deploy --parameter-overrides Stage=prod --stack-name on-call-support-prod
