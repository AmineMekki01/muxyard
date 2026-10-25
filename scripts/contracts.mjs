/** Generate or check TypeScript declarations without changing files on failure. */
import { execFile } from "node:child_process";
import {
  existsSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { format, resolveConfig } from "prettier";

const root = fileURLToPath(new URL("../", import.meta.url));
const output = fileURLToPath(
  new URL("../packages/contracts/src/generated.ts", import.meta.url),
);
const args = process.argv.slice(2);
const check = args.length === 1 && args[0] === "--check";
const abort = new AbortController();
const cancel = () => abort.abort();
const temporary = `${output}.${process.pid}.tmp`;

try {
  if (args.length && !check) {
    throw new Error("Usage: node scripts/contracts.mjs [--check]");
  }
  process.once("SIGINT", cancel);
  process.once("SIGTERM", cancel);
  const generation = promisify(execFile)(
    "cargo",
    [
      "run",
      "--locked",
      "--quiet",
      "-p",
      "muxyard-contracts",
      "--features",
      "typescript",
      "--example",
      "export_types",
    ],
    {
      cwd: root,
      encoding: "utf8",
      signal: abort.signal,
      timeout: 120_000,
      maxBuffer: 2 * 1024 * 1024,
    },
  );
  console.error("Generating TypeScript contracts from Rust...");
  const { stdout, stderr } = await generation;
  if (stderr.trim()) console.error(stderr.trim());
  const generated = await format(stdout, {
    ...(await resolveConfig(output)),
    filepath: output,
  });
  if (abort.signal.aborted) throw new Error("Contract generation canceled.");
  if (check) {
    if (!existsSync(output) || readFileSync(output, "utf8") !== generated) {
      throw new Error(
        "Generated contracts are stale. Run pnpm contracts:generate.",
      );
    }
    console.log("Generated contracts are current.");
  } else {
    writeFileSync(temporary, generated);
    renameSync(temporary, output);
    console.log("Generated packages/contracts/src/generated.ts");
  }
} catch (error) {
  if (abort.signal.aborted) {
    console.error(
      "Contract generation canceled; generated contracts unchanged.",
    );
    process.exitCode = 130;
  } else {
    console.error(
      error.code === "ENOENT"
        ? "Contract generation requires Cargo on PATH. Install Rust via rustup."
        : error.message,
    );
    process.exitCode = 1;
  }
} finally {
  process.removeListener("SIGINT", cancel);
  process.removeListener("SIGTERM", cancel);
  if (existsSync(temporary)) unlinkSync(temporary);
}
