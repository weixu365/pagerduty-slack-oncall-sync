
build:
	cargo build

release:
	cargo zigbuild --release --target x86_64-unknown-linux-musl

release-linux:
	cargo zigbuild --release --target x86_64-unknown-linux-musl
	cargo zigbuild --release --bin user-group-updater-lambda --target x86_64-unknown-linux-musl

lambda:
	cargo lambda build --release --output-format zip --target x86_64-unknown-linux-musl
	cp target/lambda/slack_request_handler_lambda/bootstrap.zip target/lambda/slack_request_handler_lambda.zip
	cp target/lambda/update_user_groups_lambda/bootstrap.zip target/lambda/update_user_groups_lambda.zip

run:
	cargo run

update:
	cargo run --bin update_user_group

threads:
	cargo run --bin test

upgrade-deps:
	cargo upgrade
	cargo update

docker-build:
	docker run -it --rm -v `pwd`:/work -w /work messense/rust-musl-cross:x86_64-musl bash

test:
	cargo install cargo-llvm-cov --quiet 2>&1 | tail -5 && cargo llvm-cov
