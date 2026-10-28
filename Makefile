SHELL := /bin/sh

.DEFAULT_GOAL := help

CARGO ?= cargo
PNPM ?= pnpm
BRANCH ?= $(shell git branch --show-current)
HOST_BIN ?= $(CURDIR)/target/debug/muxyard-host
ISOLATED_ROOT ?= $(CURDIR)/target/dev-smoke

.PHONY: help run run-isolated stop-isolated clean-isolated app build-host-dev verify verify-full ci ci-dispatch ci-watch ci-branch check test test-rust test-js test-terminal test-ipc \
	rust-fmt rust-clippy contracts-check format-check lint typecheck \
	build build-js build-rust build-host build-desktop diff-check doctor platform-report

help:
	@printf '%s\n' \
		'Muxyard verification targets:' \
		'  make run             Build the debug host and start the desktop app' \
		'  make run-isolated    Start the app with target/dev-smoke as its data root' \
		'  make stop-isolated   Stop the host from the isolated smoke root' \
		'  make clean-isolated  Remove the isolated smoke root' \
		'  make verify          Run checks, tests, JS build, Rust build, and diff check' \
		'  make verify-full     Also build the release host and desktop app' \
		'  make ci              Alias for verify (current-OS local CI equivalent)' \
		'  make ci-dispatch     Dispatch hosted macOS/Linux/Windows CI for current branch' \
		'  make ci-watch        Watch the latest hosted CI run for current branch' \
		'  make ci-branch       Dispatch then watch hosted CI for current branch' \
		'  make check           fmt, Clippy, contracts, format, lint, typecheck, diff' \
		'  make test            Rust and JavaScript tests' \
		'  make test-rust       Rust workspace tests' \
		'  make test-js         Node and Vitest tests' \
		'  make test-terminal   Real PTY lifecycle tests' \
		'  make test-ipc        IPC and terminal stream tests' \
		'  make build           JS, Rust workspace, release host, release desktop' \
		'  make doctor          Check local toolchain prerequisites' \
		'  make platform-report Write target/ci/platform-report.json'

run: build-host-dev
	MUXYARD_HOST_BIN="$(HOST_BIN)" $(PNPM) tauri dev

run-isolated: build-host-dev
	@printf 'Isolated root: %s\n' "$(ISOLATED_ROOT)"
	MUXYARD_DATA_DIR="$(ISOLATED_ROOT)/data" \
	MUXYARD_CONFIG_DIR="$(ISOLATED_ROOT)/config" \
	MUXYARD_RUNTIME_DIR="$(ISOLATED_ROOT)/runtime" \
	MUXYARD_HOST_BIN="$(HOST_BIN)" \
	$(PNPM) tauri dev

stop-isolated: build-host-dev
	MUXYARD_DATA_DIR="$(ISOLATED_ROOT)/data" \
	MUXYARD_CONFIG_DIR="$(ISOLATED_ROOT)/config" \
	MUXYARD_RUNTIME_DIR="$(ISOLATED_ROOT)/runtime" \
	"$(HOST_BIN)" stop

clean-isolated:
	rm -rf "$(ISOLATED_ROOT)"

app: run

build-host-dev:
	$(CARGO) build --locked -p muxyard-host

verify:
	$(MAKE) check
	$(MAKE) test
	$(MAKE) build-js
	$(MAKE) build-rust
	$(MAKE) diff-check
	@printf '%s\n' 'Verification passed.'

verify-full:
	$(MAKE) verify
	$(MAKE) build-host
	$(MAKE) build-desktop
	@printf '%s\n' 'Full verification passed.'

ci: verify

ci-dispatch:
	@test -n "$(BRANCH)" || { echo "No current Git branch; set BRANCH=<name>."; exit 1; }
	@git ls-remote --exit-code --heads origin "refs/heads/$(BRANCH)" >/dev/null 2>&1 || { \
		echo "Branch $(BRANCH) is not pushed to origin. Commit and push it first."; \
		exit 1; \
	}
	gh workflow run CI --ref "$(BRANCH)"

ci-watch:
	@test -n "$(BRANCH)" || { echo "No current Git branch; set BRANCH=<name>."; exit 1; }
	@run=$$(gh run list --workflow CI --branch "$(BRANCH)" --limit 1 --json databaseId --jq '.[0].databaseId'); \
	if [ -z "$$run" ]; then echo "No CI run found for $(BRANCH)."; exit 1; fi; \
	gh run watch "$$run" --exit-status

ci-branch: ci-dispatch
	@sleep 3
	$(MAKE) ci-watch

check:
	$(MAKE) rust-fmt
	$(MAKE) rust-clippy
	$(MAKE) contracts-check
	$(MAKE) format-check
	$(MAKE) lint
	$(MAKE) typecheck
	$(MAKE) diff-check

test: test-rust test-js

test-rust:
	$(CARGO) test --locked --workspace

test-js:
	$(PNPM) test

test-terminal:
	$(CARGO) test --locked -p muxyard-terminal --test pty_lifecycle

test-ipc:
	$(CARGO) test --locked -p muxyard-ipc --tests

rust-fmt:
	$(CARGO) fmt --all -- --check

rust-clippy:
	$(CARGO) clippy --locked --workspace --all-targets --all-features -- -D warnings

contracts-check:
	$(PNPM) contracts:check

format-check:
	$(PNPM) format:check

lint:
	$(PNPM) lint

typecheck:
	$(PNPM) typecheck

build:
	$(MAKE) build-js
	$(MAKE) build-rust
	$(MAKE) build-host
	$(MAKE) build-desktop

build-js:
	$(PNPM) build

build-rust:
	$(CARGO) build --locked --workspace

build-host:
	$(PNPM) build:host

build-desktop:
	$(PNPM) build:desktop

diff-check:
	git diff --check

doctor:
	$(PNPM) doctor

platform-report:
	mkdir -p target/ci
	node scripts/ci/platform-report.mjs --output target/ci/platform-report.json
