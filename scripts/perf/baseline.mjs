/** Reproducible host/desktop performance baseline with explicit missing metrics. */
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

import { collectBuild, collectEnvironment } from "./lib/environment.mjs";
import {
  buildIdleMetrics,
  sampleProcessTree,
  sleep,
  stopProcessTree,
} from "./lib/ps.mjs";
import {
  available,
  samplesMetric,
  unavailable,
  validateReport,
  writeMarkdown,
  writeReport,
} from "./lib/report.mjs";
import { round } from "./lib/stats.mjs";

const USAGE = `Usage: node scripts/perf/baseline.mjs [options]

  --host-bin <path>        Host executable (default target/release/muxyard-host)
  --desktop-bin <path>     Desktop executable (default target/release/muxyard-desktop)
  --output <path>          JSON report (default target/perf/baseline.json)
  --markdown <path>        Optional Markdown summary
  --iterations <n>         Host launch samples (default 5)
  --idle-ms <ms>           Desktop stabilization before sampling (default 3000)
  --sample-ms <ms>         CPU sampling window (default 1000)
  --ready-timeout-ms <ms>  Desktop window timeout (default 20000)
  --skip-desktop           Skip desktop launch and GUI metrics
  --require-desktop        Fail when desktop readiness is unavailable
  --help                   Print this help

Unavailable metrics are written as NOT_AVAILABLE with a reason, never as zero.`;

const DEFAULTS = {
  hostBin: "target/release/muxyard-host",
  desktopBin: "target/release/muxyard-desktop",
  output: "target/perf/baseline.json",
  iterations: 5,
  idleMs: 3_000,
  sampleMs: 1_000,
  readyTimeoutMs: 20_000,
};

let activeChild = null;

export async function runBaseline(options) {
  const startedAt = Date.now();
  const warnings = [];
  const environment = collectEnvironment();
  const build = collectBuild(options.hostBin, options.desktopBin);

  const host = await measureHostLaunch(options.hostBin, options.iterations);
  const metrics = {
    host_launch: host.metric,
    host_ready: unavailable(
      "persistent host runtime is not implemented; only CLI startup and exit are measured",
    ),
    host_idle: unavailable("no persistent host process exists to sample"),
    desktop_ready: unavailable("desktop measurement did not run"),
    desktop_idle: unavailable("desktop measurement did not run"),
  };
  warnings.push(
    "host_ready and host_idle are NOT_AVAILABLE because the persistent host runtime does not exist yet",
  );
  if (host.failures.length > 0) {
    warnings.push(`${host.failures.length} host launch sample(s) failed`);
  }

  let cleanup = {
    desktop_exited: true,
    exit_code: null,
    signal: null,
    duration_ms: 0,
    orphans: [],
    orphan_check: "NOT_APPLICABLE",
  };

  if (options.skipDesktop) {
    metrics.desktop_ready = unavailable("desktop measurement skipped by flag");
    metrics.desktop_idle = unavailable("desktop measurement skipped by flag");
  } else {
    const desktop = await measureDesktop(options);
    metrics.desktop_ready = desktop.ready;
    metrics.desktop_idle = desktop.idle;
    cleanup = desktop.cleanup;
    if (desktop.ready.status !== "AVAILABLE") {
      warnings.push(`desktop readiness unavailable: ${desktop.ready.reason}`);
    }
    if (cleanup.orphans.length > 0) {
      warnings.push(
        `desktop cleanup left ${cleanup.orphans.length} orphan process(es)`,
      );
    }
    if (!cleanup.desktop_exited) {
      warnings.push(
        "desktop process did not report a clean exit after termination",
      );
    }
  }

  if (environment.platform !== "win32") {
    warnings.push(
      "working_set_bytes is NOT_AVAILABLE on this platform; rss_bytes is reported instead",
    );
  }

  const report = {
    schema_version: 1,
    generated_at: new Date(startedAt).toISOString(),
    run: {
      started_at: new Date(startedAt).toISOString(),
      duration_ms: Date.now() - startedAt,
      iterations: options.iterations,
      idle_ms: options.idleMs,
      sample_ms: options.sampleMs,
      ready_timeout_ms: options.readyTimeoutMs,
      skip_desktop: options.skipDesktop,
      require_desktop: options.requireDesktop,
    },
    environment,
    build,
    metrics,
    cleanup,
    warnings,
  };

  validateReport(report);
  const output = writeReport(options.output, report);
  if (options.markdown) writeMarkdown(options.markdown, report);

  let exitCode = 0;
  if (metrics.host_launch.status !== "AVAILABLE") exitCode = 1;
  if (options.requireDesktop && metrics.desktop_ready.status !== "AVAILABLE") {
    exitCode = 1;
  }
  if (cleanup.orphans.length > 0 || !cleanup.desktop_exited) exitCode = 1;

  return { report, output, exitCode };
}

export async function measureHostLaunch(hostBin, iterations) {
  if (!existsSync(hostBin)) {
    return {
      metric: unavailable(`host binary not found at ${hostBin}`),
      failures: [{ reason: "binary missing" }],
    };
  }
  const samples = [];
  const failures = [];
  for (let index = 0; index < iterations; index += 1) {
    const started = performance.now();
    const result = await runProcess(hostBin, ["--version"], 15_000);
    const duration = performance.now() - started;
    if (result.code === 0 && result.stdout.includes("muxyard-host")) {
      samples.push(round(duration));
    } else {
      failures.push({
        iteration: index,
        exit_code: result.code,
        signal: result.signal,
        stderr: truncate(result.stderr),
      });
    }
  }
  if (samples.length === 0) {
    return {
      metric: unavailable(
        `no successful host launch (${failures.length} failure(s))`,
      ),
      failures,
    };
  }
  const base = samplesMetric(
    samples,
    "ms",
    "spawn the host with --version and wait for process exit",
  );
  return {
    metric:
      failures.length === 0
        ? base
        : {
            ...base,
            status: "PARTIAL",
            reason: `${failures.length}/${iterations} launches failed`,
            failures,
          },
    failures,
  };
}

export async function measureDesktop(options) {
  const result = {
    ready: unavailable("desktop readiness was not measured"),
    idle: unavailable("desktop idle was not measured"),
    cleanup: {
      desktop_exited: true,
      exit_code: null,
      signal: null,
      duration_ms: 0,
      orphans: [],
      orphan_check: "NOT_APPLICABLE",
    },
  };
  if (!existsSync(options.desktopBin)) {
    result.ready = unavailable(
      `desktop binary not found at ${options.desktopBin}`,
    );
    return result;
  }

  let child = null;
  let stderr = "";
  let sampledPids = [];
  try {
    if (process.platform === "darwin") ensureWindowHelper();
    const started = performance.now();
    child = spawn(options.desktopBin, [], {
      detached: process.platform !== "win32",
      stdio: ["ignore", "ignore", "pipe"],
    });
    activeChild = child;
    child.stderr.on("data", (chunk) => {
      stderr = truncate(`${stderr}${chunk.toString()}`);
    });

    await waitForWindow(child, options.readyTimeoutMs, () => stderr);
    result.ready = available(
      round(performance.now() - started),
      "ms",
      "time until an on-screen window owned by the desktop PID appeared (macOS CGWindowList)",
    );

    await sleep(options.idleMs);
    const sample1 = sampleProcessTree(child.pid, { includeHandles: false });
    await sleep(options.sampleMs);
    const sample2 = sampleProcessTree(child.pid);
    sampledPids = sample2.processes.map((process) => process.pid);
    result.idle = buildIdleMetrics(sample1, sample2);
    result.cleanup = await stopProcessTree(child, sampledPids);
  } catch (error) {
    if (result.ready.status !== "AVAILABLE") {
      result.ready = unavailable(error.message);
    }
    result.idle = unavailable("desktop idle sampling did not complete");
    if (child) {
      result.cleanup = await stopProcessTree(child, sampledPids);
    }
  } finally {
    if (child && child.exitCode === null && child.signalCode === null) {
      result.cleanup = await stopProcessTree(child, sampledPids);
    }
    activeChild = null;
  }
  return result;
}

export async function waitForWindow(child, timeoutMs, getStderr) {
  if (process.platform !== "darwin") {
    throw new Error(
      `window readiness probe is not implemented on ${process.platform}`,
    );
  }
  const helper = ensureWindowHelper();
  const deadline = Date.now() + timeoutMs;
  let spawnError = null;
  child.once("error", (error) => {
    spawnError = error;
  });

  while (Date.now() < deadline) {
    if (spawnError) {
      throw new Error(`desktop failed to start: ${spawnError.message}`);
    }
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(
        `desktop exited before ready (code ${child.exitCode}, signal ${child.signalCode}): ${truncate(getStderr())}`,
      );
    }
    const probe = spawnSync(helper, [], {
      encoding: "utf8",
      timeout: 5_000,
    });
    if (!probe.error && probe.status === 0) {
      const pids = probe.stdout
        .split(/\r?\n/)
        .map((line) => Number(line.split("\t")[0]))
        .filter((pid) => Number.isFinite(pid));
      if (pids.includes(child.pid)) return;
    }
    await sleep(100);
  }
  throw new Error(
    `desktop window did not appear within ${timeoutMs} ms: ${truncate(getStderr())}`,
  );
}

export function ensureWindowHelper() {
  if (process.platform !== "darwin") {
    throw new Error("macOS window probe requested on another platform");
  }
  const source = fileURLToPath(
    new URL("./macos/window-owner.swift", import.meta.url),
  );
  const output = resolve("target/perf/macos-window-owner");
  if (
    existsSync(output) &&
    statSync(output).mtimeMs >= statSync(source).mtimeMs
  ) {
    return output;
  }
  mkdirSync(dirname(output), { recursive: true });
  const compiled = spawnSync("swiftc", ["-O", source, "-o", output], {
    encoding: "utf8",
    timeout: 120_000,
  });
  if (compiled.error || compiled.status !== 0) {
    throw new Error(
      `could not build macOS window probe: ${truncate(compiled.error?.message ?? compiled.stderr)}`,
    );
  }
  return output;
}

export function parseOptions(argv = process.argv.slice(2)) {
  let values;
  try {
    ({ values } = parseArgs({
      args: argv,
      options: {
        "host-bin": { type: "string", default: DEFAULTS.hostBin },
        "desktop-bin": { type: "string", default: DEFAULTS.desktopBin },
        output: { type: "string", default: DEFAULTS.output },
        markdown: { type: "string" },
        iterations: { type: "string", default: String(DEFAULTS.iterations) },
        "idle-ms": { type: "string", default: String(DEFAULTS.idleMs) },
        "sample-ms": { type: "string", default: String(DEFAULTS.sampleMs) },
        "ready-timeout-ms": {
          type: "string",
          default: String(DEFAULTS.readyTimeoutMs),
        },
        "skip-desktop": { type: "boolean", default: false },
        "require-desktop": { type: "boolean", default: false },
        help: { type: "boolean", default: false },
      },
      strict: true,
    }));
  } catch (error) {
    throw new Error(`${error.message}\n${USAGE}`, { cause: error });
  }
  if (values.help) return { help: true };
  return {
    hostBin: resolve(values["host-bin"]),
    desktopBin: resolve(values["desktop-bin"]),
    output: resolve(values.output),
    markdown: values.markdown ? resolve(values.markdown) : null,
    iterations: integerOption(values.iterations, "--iterations", 1),
    idleMs: integerOption(values["idle-ms"], "--idle-ms", 0),
    sampleMs: integerOption(values["sample-ms"], "--sample-ms", 1),
    readyTimeoutMs: integerOption(
      values["ready-timeout-ms"],
      "--ready-timeout-ms",
      1,
    ),
    skipDesktop: values["skip-desktop"],
    requireDesktop: values["require-desktop"],
  };
}

export async function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseOptions(argv);
  } catch (error) {
    console.error(`baseline: ${error.message}`);
    return 1;
  }
  if (options.help) {
    console.log(USAGE);
    return 0;
  }

  process.once("SIGINT", () => {
    const child = activeChild;
    if (child) {
      stopProcessTree(child).finally(() => process.exit(130));
    } else {
      process.exit(130);
    }
  });

  try {
    const { report, output, exitCode } = await runBaseline(options);
    console.log(`Wrote baseline report to ${output}.`);
    console.log(
      `Host launch: ${formatMetric(report.metrics.host_launch)}; ` +
        `desktop ready: ${formatMetric(report.metrics.desktop_ready)}; ` +
        `desktop RSS: ${formatMetric(report.metrics.desktop_idle.tree_aggregate?.rss_bytes)}.`,
    );
    if (options.markdown) {
      console.log(`Wrote Markdown summary to ${options.markdown}.`);
    }
    return exitCode;
  } catch (error) {
    console.error(`baseline: ${error.message}`);
    return 1;
  }
}

function integerOption(value, name, minimum) {
  const number = Number(value);
  if (!Number.isInteger(number) || number < minimum) {
    throw new Error(`${name} must be an integer >= ${minimum}`);
  }
  return number;
}

function runProcess(command, args, timeoutMs) {
  return new Promise((resolve) => {
    let stdout = "";
    let stderr = "";
    let settled = false;
    const child = spawn(command, args, { stdio: ["ignore", "pipe", "pipe"] });
    const timer = setTimeout(() => {
      child.kill("SIGTERM");
      finish({
        code: null,
        signal: "TIMEOUT",
        error: `process exceeded ${timeoutMs} ms`,
      });
    }, timeoutMs);
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({
        ...result,
        stdout: truncate(stdout),
        stderr: truncate(stderr),
      });
    };
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.once("error", (error) => {
      finish({ code: null, signal: null, error: error.message });
    });
    child.once("close", (code, signal) => {
      finish({ code, signal });
    });
  });
}

function formatMetric(metric) {
  if (!metric) return "NOT_AVAILABLE";
  if (metric.status === "NOT_AVAILABLE") return "NOT_AVAILABLE";
  if (Array.isArray(metric.samples)) {
    return `${metric.summary.median_ms} ms median`;
  }
  return `${metric.value} ${metric.unit}`;
}

function truncate(value) {
  const text = String(value ?? "");
  return text.length > 2_000 ? text.slice(-2_000) : text;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  main()
    .then((code) => {
      process.exitCode = code;
    })
    .catch((error) => {
      console.error(`baseline: ${error.message}`);
      process.exitCode = 1;
    });
}

export { USAGE };
