/** Verify report status rules and artifact rendering. */
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  available,
  renderMarkdown,
  samplesMetric,
  unavailable,
  validateReport,
  writeReport,
} from "./report.mjs";

function validReport() {
  return {
    schema_version: 1,
    generated_at: "2026-10-25T00:00:00.000Z",
    run: { iterations: 1 },
    environment: {
      platform: "darwin",
      arch: "arm64",
      os: { name: "macOS", version: "27.0" },
      cpu: { model: "Apple M5" },
    },
    build: { git_sha: "abc123", dirty: false },
    metrics: {
      host_launch: samplesMetric([10, 12], "ms", "spawn and exit"),
      host_ready: unavailable("persistent host runtime is not implemented"),
      host_idle: unavailable("no persistent host process exists"),
      desktop_ready: available(500, "ms", "window probe"),
      desktop_idle: {
        status: "AVAILABLE",
        sample_window_ms: 1_000,
        process_count: 1,
        classes: {
          desktop: {
            processes: [],
            aggregate: {
              rss_bytes: available(1024, "bytes", "ps"),
              cpu_percent: available(0, "%", "cpu delta"),
              handles: available(2, "count", "lsof"),
            },
          },
        },
        tree_aggregate: {
          rss_bytes: available(1024, "bytes", "ps"),
          cpu_percent: available(0, "%", "cpu delta"),
          handles: available(2, "count", "lsof"),
        },
      },
    },
    cleanup: {
      desktop_exited: true,
      exit_code: 0,
      signal: null,
      duration_ms: 10,
      orphans: [],
      orphan_check: "AVAILABLE",
    },
    warnings: ["host_ready and host_idle are NOT_AVAILABLE"],
  };
}

test("accepts a complete report and rejects missing metrics", () => {
  assert.equal(validateReport(validReport()), true);
  const report = validReport();
  delete report.metrics.host_idle;
  assert.throws(() => validateReport(report), /host_idle/);
});

test("NOT_AVAILABLE never carries a numeric value", () => {
  const report = validReport();
  report.metrics.host_ready = { status: "NOT_AVAILABLE", value: 0 };
  assert.throws(() => validateReport(report), /requires a reason/);
  report.metrics.host_ready = {
    status: "NOT_AVAILABLE",
    reason: "missing",
    value: 0,
  };
  assert.throws(() => validateReport(report), /must not carry a value/);
});

test("renders missing metrics as NOT_AVAILABLE and writes JSON", () => {
  const report = validReport();
  const markdown = renderMarkdown(report);
  assert.match(markdown, /host_ready \| NOT_AVAILABLE/);
  assert.match(markdown, /desktop_idle/);
  const root = mkdtempSync(join(tmpdir(), "muxyard-report-"));
  try {
    const output = writeReport(join(root, "nested", "report.json"), report);
    const parsed = JSON.parse(readFileSync(output, "utf8"));
    assert.equal(parsed.metrics.host_launch.summary.median_ms, 11);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
