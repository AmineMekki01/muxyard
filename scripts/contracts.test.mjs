/** Verify generator failures and cancellation never overwrite declarations. */
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./contracts.mjs", import.meta.url));
const output = new URL(
  "../packages/contracts/src/generated.ts",
  import.meta.url,
);

test("missing Cargo fails clearly without changing generated files", () => {
  const before = readFileSync(output, "utf8");
  for (const args of [[], ["--check"]]) {
    const result = spawnSync(process.execPath, [script, ...args], {
      env: { ...process.env, PATH: "" },
      encoding: "utf8",
      timeout: 10_000,
    });
    assert.equal(result.error, undefined);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /requires Cargo on PATH/);
    assert.equal(readFileSync(output, "utf8"), before);
  }
});

test("unsupported generator arguments fail instead of overwriting declarations", () => {
  const before = readFileSync(output, "utf8");
  const result = spawnSync(process.execPath, [script, "--invalid"], {
    encoding: "utf8",
    timeout: 10_000,
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Usage:/);
  assert.equal(readFileSync(output, "utf8"), before);
});

test(
  "canceling generation preserves output and leaves no temporary files",
  {
    timeout: 20_000,
    skip: process.platform === "win32" ? "POSIX signal delivery test" : false,
  },
  async () => {
    const before = readFileSync(output, "utf8");
    const child = spawn(process.execPath, [script], { stdio: "pipe" });
    let stderr = "";
    let canceled = false;
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
      if (
        !canceled &&
        stderr.includes("Generating TypeScript contracts from Rust...")
      ) {
        canceled = true;
        child.kill("SIGTERM");
      }
    });
    const exit = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", resolve);
    });
    assert.equal(exit, 130, stderr);
    assert.match(stderr, /canceled/);
    assert.equal(readFileSync(output, "utf8"), before);
    assert.ok(
      readdirSync(new URL("../packages/contracts/src/", import.meta.url)).every(
        (name) => !name.endsWith(".tmp"),
      ),
    );
  },
);
