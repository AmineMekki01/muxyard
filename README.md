# muxyard

Your agents. One Terminal. One workspace

A fast, collaborative development terminal for coding with Claude Code, Codex, and more. Run agents in parallel, review changes, share context, and stay connected from anywhere.

## Implementation Status

The current build provides the application foundation, shared wire contracts, and
a one-shot local storage initializer, not a working terminal or agent runtime. The
desktop displays an empty workspace. `muxyard-host --version` and `--help` exit
without starting a daemon, socket, database, child process, or HTTP service.
`muxyard-host --init` creates private per-user configuration, logs, and a migrated
SQLite database, then exits. The remote-web package is an unconnected placeholder.

## Prerequisites

- Node.js 22.12+, 24 LTS, or 26+, matching `package.json`'s engine range.
- pnpm 12.15.0, provisioned through Corepack. Use `corepack enable` in a writable
  tool directory, or run `corepack pnpm` instead of `pnpm`.
- Rust 1.99.1 stable via rustup, including rustfmt and Clippy. The exact toolchain
  is pinned in `rust-toolchain.toml`. Add `$HOME/.cargo/bin` to PATH on Unix
  (`. "$HOME/.cargo/env"` in the current shell).
- Native Tauri 2 prerequisites: Apple Command Line Tools on macOS; MSVC C++
  Build Tools and WebView2 on Windows; WebKitGTK 4.1 and the Tauri native packages
  on Ubuntu. The host's bundled SQLite also needs a working C compiler. See the
  [official prerequisite guide](https://v2.tauri.app/start/prerequisites/).

Run `node scripts/doctor.mjs` to diagnose missing tools without requiring pnpm.
No installer, elevation, or network service runs automatically.

## Install And Run

```sh
pnpm install --frozen-lockfile
pnpm run doctor
pnpm tauri dev
```

`pnpm tauri dev` opens the native desktop and starts Vite only on
`127.0.0.1:1420`. Close the native window or interrupt with Ctrl+C to stop
development. Restart with the same command. `pnpm dev` starts only the desktop
browser frontend, not the native app. `pnpm dev:remote` explicitly starts the
placeholder remote frontend on `127.0.0.1:1421`, not a remote host gateway.
Both servers require their exact ports and fail rather than silently changing
ports. No production server or public listener is started.

```sh
cargo run --locked -p muxyard-host -- --version
cargo run --locked -p muxyard-host -- --help
cargo run --locked -p muxyard-host -- --init
```

Unsupported host flags return exit 2. No arguments print scaffold help and exit.
`--init` is a one-shot bootstrap, not the persistent host runtime.

## Local Storage Foundation

`--init` resolves OS-standard per-user locations, creates them with owner-only
permissions on Unix, writes `config.json` only when it is absent, and opens
`muxyard.sqlite3` with foreign keys enabled, WAL journaling, a busy timeout, and
ordered transactional migrations. A checksum mismatch, malformed configuration,
or newer database schema blocks initialization instead of overwriting data. A
failed migration rolls back its transaction and leaves earlier schema versions
usable.

The default configuration selects JSON logs and bounded resource defaults. Logs
are append-only under the data directory and use an explicit `Sensitive` wrapper
for values that must be redacted. No configuration contents, room text, terminal
output, or credentials are logged.

Absolute `MUXYARD_DATA_DIR`, `MUXYARD_CONFIG_DIR`, and `MUXYARD_RUNTIME_DIR`
environment overrides select isolated locations for development and tests. On
macOS the default data directory is
`~/Library/Application Support/Muxyard`; Linux follows the XDG data, config, and
runtime conventions; Windows uses the local and roaming application-data
directories. Windows file ACL hardening is not implemented, and native path
behavior is not verified on this machine.

## Checks And Builds

```sh
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm contracts:check
pnpm build
pnpm build:host
pnpm build:desktop
```

`pnpm build` checks the contracts boundary and builds both Vite clients.
`pnpm build:desktop` produces a release native executable with embedded frontend
assets, without signed installers. Persistent host services, terminal sessions,
and provider integration are not available yet. Rust build/test
commands for the desktop need the platform's native Tauri libraries.

`pnpm format` and `cargo fmt` format application source.
Rust/TypeScript lockfiles must be checked into the eventual authorized commit.
Never regenerate dependencies implicitly in CI.

## Shared Contracts

Rust in `crates/contracts` is the source of truth. Change DTOs there, then run:

```sh
pnpm contracts:generate
pnpm contracts:check
pnpm typecheck
pnpm test
```

Commit the generated `packages/contracts/src/generated.ts` alongside Rust
changes. `contracts:check` compares freshly derived, formatted declarations
without rewriting files and exits nonzero on drift. Generation requires Cargo
and Prettier; failures or cancellation leave existing declarations unchanged.
The MIT-licensed `ts-rs` 12.0.1 generator is an optional development-only Rust
dependency and is absent from default host/desktop dependency trees.

The JSON control envelope is `{ v: 1, id, type, method, data }`, where `type` is
`request`, `response`, or `event`. A response echoes the request's `id` for
correlation. `data` is method-specific. Methods in test fixtures are examples,
not implemented endpoints. Control events here are not the durable event
envelope with stream cursors and sequence numbers.

Native receivers use `decode_control<T>` to enforce the 1 MiB JSON byte limit,
version 1, and the expected DTO shape. Unknown versions return
`UNSUPPORTED_VERSION`; malformed DTOs return `INVALID_REQUEST`; oversized
messages return `PAYLOAD_TOO_LARGE`. Parser diagnostics never echo input.
The generated TypeScript types are compile-time declarations, not runtime
validation of arbitrary JSON. IDs, methods, WSL distribution availability,
state transitions, authentication, and authorization still require host/domain
validation. No transport, state mutation, or services are started by contracts.

Contract tests invoke the real Rust fixture tool from Vitest, so `pnpm test`
also requires the pinned Rust toolchain. Shared JSON fixtures live in
`tests/fixtures/contracts`.

## Layout

- `apps/host`: native CLI scaffold, no persistent runtime.
- `apps/desktop`: React 19/Vite/Tailwind 4 frontend and Tauri 2 Rust shell.
- `apps/remote-web`: static, unconnected React/Vite client scaffold.
- `crates/core`: UI-independent native foundation.
- `crates/contracts`: authoritative DTOs, bounded JSON decoder, and code generator.
- `crates/platform`: per-user paths, validated configuration, and secret-safe logging.
- `crates/storage`: SQLite connection policy, checksummed migrations, and host records.
- `packages/contracts`: generated TypeScript declarations and roundtrip tests.
- `scripts`: toolchain diagnostics and contract generation/check commands.
- `tests/fixtures/contracts`: shared wire compatibility and failure fixtures.
