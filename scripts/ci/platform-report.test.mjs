/** Verify platform reports contain only tool versions and platform facts. */
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { collectReport, main } from "./platform-report.mjs";

test("collects tool versions with an injected runner", () => {
  const report = collectReport((command) => ({
    available: true,
    output: `${command} version`,
  }));
  assert.equal(report.platform, process.platform);
  assert.equal(report.arch, process.arch);
  assert.equal(report.node, process.version);
  assert.equal(report.tools.rustc.output, "rustc version");
  assert.equal(report.tools.pnpm.output, "pnpm version");
  assert.equal("env" in report, false);
});

test("writes a report with real local tools", () => {
  const root = mkdtempSync(join(tmpdir(), "muxyard-platform-"));
  const output = join(root, "nested", "platform-report.json");
  try {
    assert.equal(main(["--output", output]), 0);
    const report = JSON.parse(readFileSync(output, "utf8"));
    assert.equal(report.tools.rustc.available, true);
    assert.match(report.tools.rustc.output, /rustc/);
    assert.equal(report.tools.node.available, true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("missing tools write a report and fail", () => {
  const root = mkdtempSync(join(tmpdir(), "muxyard-platform-missing-"));
  const output = join(root, "platform-report.json");
  const script = fileURLToPath(
    new URL("./platform-report.mjs", import.meta.url),
  );
  try {
    const result = spawnSync(process.execPath, [script, "--output", output], {
      env: { ...process.env, PATH: "" },
      encoding: "utf8",
      timeout: 30_000,
    });
    assert.equal(result.status, 1);
    const report = JSON.parse(readFileSync(output, "utf8"));
    assert.equal(report.tools.rustc.available, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
