/** Diagnose missing development tools before starting the native desktop. */
import { spawnSync } from "node:child_process";

const tools = [
  [
    "rustc",
    "Install the pinned Rust toolchain with rustup (rust-toolchain.toml).",
  ],
  ["cargo", "Install Rust via rustup and add its bin directory to PATH."],
  ["pnpm", "Provision pnpm 12.15.0 with Corepack and add it to PATH."],
];

let failed = false;
for (const [command, guidance] of tools) {
  const result = spawnSync(command, ["--version"], {
    encoding: "utf8",
    timeout: 10_000,
    // Windows Corepack exposes a .cmd shim; all command/argument values are fixed.
    shell: process.platform === "win32",
  });
  if (result.error || result.status !== 0) {
    console.error(
      `Missing or unusable development dependency: ${command}. ${guidance}`,
    );
    const detail = result.error?.message ?? result.stderr?.trim();
    if (detail) console.error(detail);
    failed = true;
  } else {
    console.log(result.stdout.trim());
  }
}
process.exitCode = failed ? 1 : 0;
