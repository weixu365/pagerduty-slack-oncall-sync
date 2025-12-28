
build:
	cargo build

release:
	cargo build --release
	cargo build --release --bin user-group-updater-lambda

release-linux:
	cargo build --release --target x86_64-unknown-linux-musl
	cargo build --release --bin user-group-updater-lambda --target x86_64-unknown-linux-musl

lambda:
	cargo lambda build --release --output-format zip
	cp target/lambda/slack_request_handler_lambda/bootstrap.zip target/lambda/slack_request_handler_lambda.zip
	cp target/lambda/update_user_group_mk_lambda/bootstrap.zip target/lambda/update_user_group_mk_lambda.zip
	cp target/lambda/update_user_groups_lambda/bootstrap.zip target/lambda/update_user_groups_lambda.zip

run:
	cargo run

update:
	cargo run --bin update_user_group

threads:
	cargo run --bin test

docker-build:
	docker run -it --rm -v `pwd`:/work -w /work messense/rust-musl-cross:x86_64-musl bash
