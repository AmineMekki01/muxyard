# muxyard

Your agents. One Terminal. One workspace

A fast, collaborative development terminal for coding with Claude Code, Codex, and more. Run agents in parallel, review changes, share context, and stay connected from anywhere.

## Implementation Status

The current build provides the application foundation, shared wire contracts, a
keyboard-first desktop shell, a single-instance user-owned host, a protected
same-user local control endpoint, and a Tauri Rust host bridge. It is not yet
an agent runtime. When the host is ready the desktop opens a local terminal;
otherwise it shows an honest empty workspace with live host connection status.
`muxyard-host start` launches a detached headless host; `run` stays in the
foreground; `stop` and `status`
manage the current user's host. The host opens private per-user configuration,
logs, and a migrated SQLite database, and serves typed `host.status` requests
over a Unix socket or Windows named pipe. The desktop bridge auto-launches the
host when absent, checks protocol compatibility, and watches status without
exposing the raw endpoint to the webview. Native PTY lifecycle, binary terminal
stream framing, bounded per-client backpressure, and an xterm.js desktop
terminal are implemented. It does not yet expose an agent or an HTTP service.
The remote-web package is an unconnected placeholder.

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
`127.0.0.1:1420`. Build a host binary first so the desktop bridge can find
`muxyard-host` beside the desktop executable:

```sh
cargo build --locked -p muxyard-host   # development desktop
pnpm build:host                        # release desktop smoke
pnpm tauri dev
```

Alternatively set `MUXYARD_HOST_BIN` to an explicit host executable path. Close
the native window or interrupt with Ctrl+C to stop development. Restart with the
same command. `pnpm dev` starts only the desktop browser frontend, not the native
app. `pnpm dev:remote` explicitly starts the placeholder remote frontend on
`127.0.0.1:1421`, not a remote host gateway. Both servers require their exact
ports and fail rather than silently changing ports. No production server or
public listener is started.

```sh
cargo run --locked -p muxyard-host -- --version
cargo run --locked -p muxyard-host -- --help
cargo run --locked -p muxyard-host -- start
cargo run --locked -p muxyard-host -- status --json
cargo run --locked -p muxyard-host -- stop
cargo run --locked -p muxyard-host -- run
cargo run --locked -p muxyard-host -- --init
```

Unsupported host flags return exit 2. No arguments print help and exit.
`--init` is a one-shot storage bootstrap. `start` is detached and waits for the
host to become ready; `run` is the foreground form; `stop` is graceful and
idempotent; `status` exits 0 only while a live host is present. Starting twice
reports the existing host instead of opening a second writer.

## Host Lifecycle

The host is a normal user process, never a root service. It stores a
`host.lock` containing PID, OS process start identity, instance ID, host version,
protocol, and start time under the private runtime directory. A live lock
requires both PID and start identity to match; dead PIDs, PID reuse, and corrupt
locks are never treated as live. `host.ready` is written only after configuration
and migrations are ready, and `host.stop` is an instance-scoped graceful stop
request.

`start` places the host in a detached OS process group/session with null terminal
IO, so exiting the launcher or desktop UI does not terminate it. The host stops
on explicit `stop` or Ctrl+C/SIGTERM. OS logout/reboot may stop it, but that
behavior is not physically tested here. The host does not resume terminals after
the host process exits. Unix runtime directories are 0700 and runtime files are 0600. The desktop bridge connects to an existing host or launches one, and a
second `start` still reports the existing host rather than opening a second
writer.

## Local IPC

The host exposes typed `host.status` and, when terminal streaming is enabled,
`terminal.attach` on a same-user OS-local transport. On Unix the socket is
`runtime/muxyard.sock` with mode 0600 inside a 0700 runtime directory, and the
host checks the peer's effective UID. On Windows the named pipe is derived from
the current user SID and its DACL grants full control only to that SID; the
transport also rejects remote clients. Windows SID lookup, ACL behavior, and the
named-pipe path are not verified on this macOS machine.

Control frames are a 4-byte big-endian length plus UTF-8 JSON, from 1 byte to
1 MiB. The versioned control envelope is `{ v: 1, id, type, method, data }`.
Responses use `{ status: "ok", data }` or `{ status: "error", error }` with stable
`ApiError` codes. The server applies a five-second timeout and keeps malformed,
oversized, truncated, disconnected, cancelled, or idle connections from stopping
the listener. The Rust `IpcClient` is the supported client; the React frontend
does not connect to the raw endpoint.

Terminal output uses a typed `terminal.attach` request followed by binary frames
on that connection. Frames carry a stream UUID, monotonic sequence, kind, and up
to 64 KiB of payload. Each attached client has a bounded 1–4 MiB write queue; a
slow subscriber is detached without blocking other subscribers or the control
listener. The codec defines input, resize, end, flow-control, and ACK kinds.
The host exposes typed `terminal.spawn`, `terminal.input`, `terminal.resize`,
and `terminal.close` control methods. The Tauri Rust bridge forwards them to the
host and streams binary output through a raw Tauri Channel.

## Desktop Terminal

The workspace mounts `@xterm/xterm` once the host is ready. PTY bytes go directly
from the Tauri Channel into xterm and never through React state. Input, resize,
and close commands are typed Tauri commands; WebGL is attempted with a DOM
fallback on construction or context loss. Fit and Search addons are loaded, and
the pane exposes selection copying, search-next, and a retry action. The output
stream stays attached through idle periods instead of expiring with the control
request timeout. Closing or unmounting the pane disposes the renderer and closes
the host session.

The terminal starts in the host process working directory because project
selection is not implemented yet. Scrollback is renderer-local and bounded;
no terminal output is written to SQLite. Full-screen TUI/Unicode and native
copy/selection smoke remain manual verification items.

The Tauri Rust bridge exposes typed `host_connect`, `host_status`,
`host_watch`, `host_unwatch`, `terminal_spawn`, `terminal_input`,
`terminal_resize`, `terminal_close`, and `terminal_attach` commands. It checks
protocol version 1, shows
host starting/ready/reconnecting/offline/incompatible states, and retries status
a bounded number of times before reporting `HOST_OFFLINE`. The status watcher
uses a Tauri Channel; the webview receives no socket or TCP credentials. Closing
the desktop window stops the watcher but never stops the host. Durable domain
events and snapshot cursors are not implemented yet; the current subscription is
host status only. No installer or sidecar bundle is produced yet, so the host
executable must be beside the desktop executable, on `PATH`, or named by
`MUXYARD_HOST_BIN`.

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
behavior is not verified on this machine. The local IPC named pipe has its own
current-user DACL, but that path is also unverified on macOS.

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

## Continuous Integration

`.github/workflows/ci.yml` runs on pushes to `main`, pull requests, and manual
dispatch. A macOS/Linux/Windows matrix installs from the frozen pnpm lockfile and
runs Rust formatting, Clippy, tests, TypeScript formatting/lint/typecheck/tests,
contract generation checks, and both client and Rust workspace builds. Matrix
failures do not cancel other platforms.

A separate job produces dependency license artifacts from `cargo metadata` and
`pnpm licenses list`. First-party unpublished workspace packages are listed
separately; unknown third-party licenses fail the job. The workflow also runs a
static safety check that rejects persisted checkout credentials, repository
secret or token references, write permissions, silently tolerated failures,
shell tracing, downloaded-content piping, unpinned remote actions, and workflows
that omit any of the three platforms. Actions are pinned to immutable commits, caches are keyed by the
lockfiles, and reports are uploaded as short-lived artifacts. `.gitattributes`
normalizes text to LF so Windows formatting checks match the other platforms.

Local equivalents:

```sh
mkdir -p target/ci/licenses
cargo metadata --locked --format-version 1 > target/ci/cargo-metadata.json
pnpm licenses list --json > target/ci/pnpm-licenses.json
node scripts/ci/license-report.mjs \
  --cargo target/ci/cargo-metadata.json \
  --npm target/ci/pnpm-licenses.json \
  --output target/ci/licenses \
  --fail-on-unknown
node scripts/ci/workflow-safety.mjs --workflows .github/workflows
node scripts/ci/platform-report.mjs --output target/ci/platform-report.json
```

The workflow is configured for hosted macOS, Linux, and Windows runners. Hosted
runner results are available only after the workflow is pushed; local macOS
checks and workflow syntax validation do not substitute for those reports.

## Performance Baseline

`scripts/perf/baseline.mjs` measures the current host CLI launch, desktop window
readiness, and the desktop process tree after an idle stabilization period. It
records OS, CPU, memory, toolchain, Git SHA, dirty state, binary sizes, durations,
RSS, CPU, handles, and cleanup status in machine-readable JSON. It can also write
a Markdown summary.

```sh
pnpm build:host
pnpm build:desktop
node scripts/perf/baseline.mjs \
  --iterations 5 \
  --idle-ms 3000 \
  --output target/perf/baseline.json \
  --markdown target/perf/baseline.md
```

The report separates the desktop root, WebKit/WebView helpers, a persistent host
process if one is observed, and other children. Missing measurements are written
as `NOT_AVAILABLE` with a reason, never as zero. The persistent host runtime does
not exist yet, so host ready/idle metrics are unavailable while host CLI startup
is measured. Desktop readiness uses an on-screen window probe on macOS and is
unavailable on other platforms. RSS is available on Unix and Windows;
working-set and private-byte values are Windows-specific. WebKit XPC processes
reparented to `launchd` are attributed on a best-effort basis by start time.

The harness terminates the launched desktop process tree and reports orphan
processes. Baseline numbers are descriptive, not target-compliance claims.

## Keyboard And Focus

The desktop shell supports local navigation without a host connection:

- `Cmd/Ctrl+Shift+P` opens the command palette.
- `?` opens the keyboard reference.
- `Escape` closes an open overlay and restores the previous focus.
- Sidebar and inspector toggle buttons expose `aria-expanded`; both regions are
  programmatically focusable.
- `Tab`/`Shift+Tab` stay inside an open overlay. Focus returns to the trigger
  after the overlay closes.

The global shortcut listener ignores focused inputs, textareas, selects,
`contenteditable` regions, `.xterm`, `[role="terminal"]`, and
`[data-terminal-focus="true"]`. It never handles `Ctrl+C`, `Ctrl+D`, `Ctrl+Z`,
`Escape`, mouse reporting, or Alt sequences on behalf of a terminal. The command
palette contains only local layout, focus, and help actions; unknown queries do
nothing privileged.

The shell uses in-memory layout state and does not persist preferences yet. The
responsive breakpoints stack the main workspace, sidebar, and inspector on
narrow windows; native resize verification remains a manual smoke test.

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

- `apps/host`: native host lifecycle, runtime, and local IPC listener.
- `apps/desktop`: React 19/Vite/Tailwind 4 frontend, xterm.js terminal, and Tauri 2 Rust host bridge.
- `apps/remote-web`: static, unconnected React/Vite client scaffold.
- `crates/core`: UI-independent native foundation.
- `crates/contracts`: authoritative DTOs, bounded JSON decoder, and code generator.
- `crates/ipc`: protected local socket/named-pipe transport, typed `host.status` client, and framing.
- `crates/terminal`: native PTY sessions, terminal binary frame codec, bounded per-client queues, and slow-subscriber detachment.
- `crates/platform`: per-user paths, validated configuration, and secret-safe logging.
- `crates/storage`: SQLite connection policy, checksummed migrations, and host records.
- `packages/contracts`: generated TypeScript declarations and roundtrip tests.
- `scripts`: toolchain diagnostics and contract generation/check commands.
- `scripts/ci`: license, platform, and workflow safety checks used by CI.
- `scripts/perf`: host/desktop performance baseline harness.
- `.github/workflows`: the three-platform CI matrix and report jobs.
- `tests/fixtures/contracts`: shared wire compatibility and failure fixtures.
- `tests/benchmarks`: deterministic process-table fixture and harness notes.
