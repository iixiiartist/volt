.PHONY: db up down init check test fmt run package

up:
	docker compose up -d postgres

down:
	docker compose down

init:
	cargo run -- init-db

check:
	cargo check

test:
	cargo test

fmt:
	cargo fmt

run:
	cargo run -- list-tools

package:
	mkdir -p dist
	zip -r dist/volt-project.zip . -x "target/*" "dist/*" ".env"