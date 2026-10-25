# muxyard

Your agents. One Terminal. One workspace

A fast, collaborative development terminal for coding with Claude Code, Codex, and more. Run agents in parallel, review changes, share context, and stay connected from anywhere.

## Implementation Status

FND-001 is the original application foundation, not a working terminal or agent
runtime. The desktop displays an empty workspace. `muxyard-host` reports its
version or help and exits without starting a daemon, socket, database, child
process, or HTTP service. The remote-web package is an unconnected placeholder.
See [current state](docs/CURRENT_STATE.md) and [handoff](docs/AGENT_HANDOFF.md)
for verified behavior and outstanding checks.

The root specification files are authoritative. `workspace-blueprint/` is the
unchanged local source archive, not an application directory.

## Prerequisites

- Node.js 22.12+, 24 LTS, or 26+, matching `package.json`'s engine range.
- pnpm 12.15.0, provisioned through Corepack. Use `corepack enable` in a writable
  tool directory, or run `corepack pnpm` instead of `pnpm`.
- Rust 1.99.1 stable via rustup, including rustfmt and Clippy. The exact toolchain
  is pinned in `rust-toolchain.toml`. Add `$HOME/.cargo/bin` to PATH on Unix
  (`. "$HOME/.cargo/env"` in the current shell).
- Native Tauri 2 prerequisites: Apple Command Line Tools on macOS; MSVC C++
  Build Tools and WebView2 on Windows; WebKitGTK 4.1 and the Tauri native packages
  on Ubuntu. See the [official prerequisite guide](https://v2.tauri.app/start/prerequisites/).

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
ports. No production server or public listener exists in this ticket.

```sh
cargo run --locked -p muxyard-host -- --version
cargo run --locked -p muxyard-host -- --help
```

Unsupported host flags return exit 2. No arguments print scaffold help and exit.

## Checks And Builds

```sh
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
pnpm build:host
pnpm build:desktop
```

`pnpm build` checks the contracts boundary and builds both Vite clients.
`pnpm build:desktop` produces a release native executable with embedded frontend
assets, without signed installers. Installers, signing, CI, daemon ownership,
IPC, terminals, and provider integration remain later tickets. Rust build/test
commands for the desktop need the platform's native Tauri libraries.

`pnpm format` and `cargo fmt` format application source. Imported locked
specifications are excluded from Prettier to preserve their content.
Rust/TypeScript lockfiles must be checked into the eventual authorized commit.
Never regenerate dependencies implicitly in CI.

## Layout

- `apps/host`: native CLI scaffold, no persistent runtime.
- `apps/desktop`: React 19/Vite/Tailwind 4 frontend and Tauri 2 Rust shell.
- `apps/remote-web`: static, unconnected React/Vite client scaffold.
- `crates/core`: UI-independent native foundation.
- `crates/contracts`: authoritative protocol metadata, DTOs deferred to FND-002.
- `packages/contracts`: TypeScript boundary, no duplicate handwritten wire DTOs.
- `scripts`: toolchain diagnostic and missing-dependency regression test.
- `docs` and `tasks`: approved specifications, evidence, continuity, and tickets.
