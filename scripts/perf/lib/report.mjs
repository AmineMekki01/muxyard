/** Metric status helpers, report validation, and human-readable rendering. */
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { summarize } from "./stats.mjs";

const STATUSES = new Set(["AVAILABLE", "PARTIAL", "NOT_AVAILABLE"]);

export function available(value, unit, method) {
  return { status: "AVAILABLE", value, unit, method };
}

export function partial(value, unit, method, reason) {
  return { status: "PARTIAL", value, unit, method, reason };
}

export function unavailable(reason) {
  return { status: "NOT_AVAILABLE", reason };
}

export function samplesMetric(samples, unit, method) {
  if (!Array.isArray(samples) || samples.length === 0) {
    return unavailable("no successful samples");
  }
  return {
    status: "AVAILABLE",
    unit,
    method,
    samples,
    summary: summarize(samples),
  };
}

export function validateReport(report) {
  if (!report || typeof report !== "object")
    throw new Error("report must be an object");
  if (report.schema_version !== 1) throw new Error("schema_version must be 1");
  if (typeof report.generated_at !== "string") {
    throw new Error("generated_at must be an ISO timestamp");
  }
  if (!report.environment || typeof report.environment !== "object") {
    throw new Error("environment metadata is required");
  }
  if (!report.build || typeof report.build !== "object") {
    throw new Error("build metadata is required");
  }
  if (typeof report.build.git_sha !== "string" || !report.build.git_sha) {
    throw new Error("build.git_sha is required");
  }
  if (!report.run || typeof report.run !== "object") {
    throw new Error("run metadata is required");
  }
  if (!report.metrics || typeof report.metrics !== "object") {
    throw new Error("metrics are required");
  }
  for (const name of [
    "host_launch",
    "host_ready",
    "host_idle",
    "desktop_ready",
  ]) {
    assertMetric(name, report.metrics[name]);
  }
  const desktopIdle = report.metrics.desktop_idle;
  if (!desktopIdle || typeof desktopIdle !== "object") {
    throw new Error("desktop_idle metric is required");
  }
  if (!STATUSES.has(desktopIdle.status)) {
    throw new Error("desktop_idle has an invalid status");
  }
  if (desktopIdle.status === "NOT_AVAILABLE") {
    if (typeof desktopIdle.reason !== "string" || !desktopIdle.reason) {
      throw new Error("desktop_idle NOT_AVAILABLE requires a reason");
    }
    if ("value" in desktopIdle || "samples" in desktopIdle) {
      throw new Error("desktop_idle NOT_AVAILABLE must not carry a value");
    }
  } else {
    if (!desktopIdle.classes || typeof desktopIdle.classes !== "object") {
      throw new Error("desktop_idle.classes is required when sampled");
    }
    assertMetric(
      "desktop_idle.tree_aggregate.rss_bytes",
      desktopIdle.tree_aggregate?.rss_bytes,
    );
    assertMetric(
      "desktop_idle.tree_aggregate.cpu_percent",
      desktopIdle.tree_aggregate?.cpu_percent,
    );
  }
  if (!report.cleanup || typeof report.cleanup !== "object") {
    throw new Error("cleanup status is required");
  }
  if (!Array.isArray(report.cleanup.orphans)) {
    throw new Error("cleanup.orphans must be an array");
  }
  if (!Array.isArray(report.warnings)) {
    throw new Error("warnings must be an array");
  }
  return true;
}

function assertMetric(name, metric) {
  if (!metric || typeof metric !== "object") {
    throw new Error(`${name} metric is required`);
  }
  if (!STATUSES.has(metric.status)) {
    throw new Error(`${name} has an invalid status`);
  }
  if (metric.status === "NOT_AVAILABLE") {
    if (typeof metric.reason !== "string" || !metric.reason) {
      throw new Error(`${name} NOT_AVAILABLE requires a reason`);
    }
    if ("value" in metric || "samples" in metric) {
      throw new Error(`${name} NOT_AVAILABLE must not carry a value`);
    }
    return;
  }
  if (typeof metric.value !== "number" && !Array.isArray(metric.samples)) {
    throw new Error(
      `${name} ${metric.status} requires a numeric value or samples`,
    );
  }
}

export function writeReport(path, report) {
  validateReport(report);
  const output = resolve(path);
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
  return output;
}

export function writeMarkdown(path, report) {
  const output = resolve(path);
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, renderMarkdown(report));
  return output;
}

export function renderMarkdown(report) {
  const lines = [
    "# Performance Baseline",
    "",
    `- Generated: ${report.generated_at}`,
    `- Platform: ${report.environment.platform} / ${report.environment.arch}`,
    `- OS: ${report.environment.os?.name ?? "NOT_AVAILABLE"} ${report.environment.os?.version ?? ""}`.trim(),
    `- CPU: ${report.environment.cpu?.model ?? "NOT_AVAILABLE"}`,
    `- Git: ${report.build.git_sha ?? "NOT_AVAILABLE"}${report.build.dirty ? " (dirty)" : ""}`,
    `- Iterations: ${report.run.iterations}`,
    "",
    "## Metrics",
    "",
    "| Metric | Status | Result |",
    "| --- | --- | --- |",
    ...Object.entries(report.metrics).map(([name, metric]) => {
      if (metric.status === "NOT_AVAILABLE") {
        return `| ${name} | ${metric.status} | ${escapeCell(metric.reason)} |`;
      }
      if (name === "desktop_idle") {
        return `| ${name} | ${metric.status} | ${metric.process_count} processes |`;
      }
      if (Array.isArray(metric.samples)) {
        return `| ${name} | ${metric.status} | median ${metric.summary.median_ms} ms, p95 ${metric.summary.p95_ms} ms (${metric.summary.count} samples) |`;
      }
      return `| ${name} | ${metric.status} | ${metric.value} ${metric.unit} |`;
    }),
    "",
  ];

  const idle = report.metrics.desktop_idle;
  if (idle.status !== "NOT_AVAILABLE") {
    lines.push(
      "## Desktop Process Tree",
      "",
      `- Sample window: ${idle.sample_window_ms} ms`,
      `- Processes: ${idle.process_count}`,
      "",
      "| Class | Processes | RSS | CPU | Handles |",
      "| --- | ---: | --- | --- | --- |",
      ...Object.entries(idle.classes).map(
        ([name, group]) =>
          `| ${name} | ${group.processes.length} | ${formatMetric(group.aggregate.rss_bytes)} | ${formatMetric(group.aggregate.cpu_percent)} | ${formatMetric(group.aggregate.handles)} |`,
      ),
      "",
    );
  }

  if (report.warnings.length > 0) {
    lines.push(
      "## Warnings",
      "",
      ...report.warnings.map((warning) => `- ${warning}`),
      "",
    );
  }
  return `${lines.join("\n")}\n`;
}

function formatMetric(metric) {
  if (!metric || metric.status === "NOT_AVAILABLE") return "NOT_AVAILABLE";
  if (metric.unit === "bytes")
    return `${(metric.value / (1024 * 1024)).toFixed(1)} MiB`;
  if (metric.unit === "%") return `${metric.value.toFixed(2)}%`;
  return `${metric.value} ${metric.unit}`;
}

function escapeCell(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
}
