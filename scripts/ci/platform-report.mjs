/** Write a small, secret-free report describing the current CI toolchain. */
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const USAGE = "Usage: node scripts/ci/platform-report.mjs [--output <file>]";

const TOOLS = [
  ["rustc", ["--version"]],
  ["cargo", ["--version"]],
  ["node", ["--version"]],
  ["pnpm", ["--version"]],
  ["git", ["--version"]],
];

export function collectReport(run = runVersion) {
  const tools = {};
  for (const [command, args] of TOOLS) {
    tools[command] = run(command, args);
  }
  return {
    platform: process.platform,
    arch: process.arch,
    node: process.version,
    ci: process.env.CI === "true",
    tools,
  };
}

export function main(argv = process.argv.slice(2)) {
  let values;
  try {
    ({ values } = parseArgs({
      args: argv,
      options: {
        output: { type: "string", default: "target/ci/platform-report.json" },
      },
      strict: true,
    }));
  } catch (error) {
    console.error(`platform-report: ${error.message}\n${USAGE}`);
    return 1;
  }

  const report = collectReport();
  const output = resolve(values.output);
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`Wrote platform report to ${output}.`);

  const missing = Object.entries(report.tools)
    .filter(([, result]) => !result.available)
    .map(([name]) => name);
  if (missing.length > 0) {
    console.error(`platform-report: missing tools: ${missing.join(", ")}`);
    return 1;
  }
  return 0;
}

function runVersion(command, args) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    timeout: 10_000,
    // Windows exposes pnpm through a .cmd shim; all command/argument values are fixed.
    shell: process.platform === "win32",
  });
  if (result.error || result.status !== 0) {
    return {
      available: false,
      error: result.error?.message ?? result.stderr?.trim() ?? "unavailable",
    };
  }
  return { available: true, output: result.stdout.trim() };
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  process.exitCode = main();
}

export { USAGE };
