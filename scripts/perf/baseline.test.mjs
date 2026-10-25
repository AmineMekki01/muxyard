/** Verify baseline options, missing binaries, and report shape. */
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { measureHostLaunch, parseOptions, runBaseline } from "./baseline.mjs";

test("parses defaults and rejects invalid iteration counts", () => {
  const defaults = parseOptions([]);
  assert.equal(defaults.iterations, 5);
  assert.equal(defaults.skipDesktop, false);
  assert.throws(() => parseOptions(["--iterations", "0"]), /iterations/);
  assert.throws(() => parseOptions(["--unknown"]), /Usage/);
  assert.deepEqual(parseOptions(["--help"]), { help: true });
});

test("missing host binary is NOT_AVAILABLE, not zero", async () => {
  const root = mkdtempSync(join(tmpdir(), "muxyard-host-missing-"));
  try {
    const result = await measureHostLaunch(join(root, "missing-host"), 1);
    assert.equal(result.metric.status, "NOT_AVAILABLE");
    assert.match(result.metric.reason, /not found/);
    assert.equal("value" in result.metric, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("missing host still writes a machine-readable report and exits 1", async () => {
  const root = mkdtempSync(join(tmpdir(), "muxyard-baseline-"));
  const output = join(root, "report.json");
  try {
    const result = await runBaseline({
      hostBin: join(root, "missing-host"),
      desktopBin: join(root, "missing-desktop"),
      output,
      markdown: null,
      iterations: 1,
      idleMs: 0,
      sampleMs: 1,
      readyTimeoutMs: 1,
      skipDesktop: true,
      requireDesktop: false,
    });
    assert.equal(result.exitCode, 1);
    const report = JSON.parse(readFileSync(output, "utf8"));
    assert.equal(report.schema_version, 1);
    assert.equal(report.metrics.host_launch.status, "NOT_AVAILABLE");
    assert.equal(report.metrics.desktop_ready.status, "NOT_AVAILABLE");
    assert.equal(report.metrics.host_ready.status, "NOT_AVAILABLE");
    assert.equal(report.metrics.host_idle.status, "NOT_AVAILABLE");
    assert.equal("value" in report.metrics.host_launch, false);
    assert.ok(report.warnings.length > 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
