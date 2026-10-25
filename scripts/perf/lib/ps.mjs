/** Process-tree sampling, classification, aggregation, and cleanup. */
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { basename } from "node:path";

import { available, partial, unavailable } from "./report.mjs";
import { round } from "./stats.mjs";

const CLASSES = ["desktop", "webview", "host", "children"];

export function parsePsOutput(output) {
  const processes = [];
  for (const line of output.split(/\r?\n/)) {
    const match = line.match(
      /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+([0-9:.-]+)\s+([0-9:.-]+)\s+(.+)$/,
    );
    if (!match) continue;
    const command = match[7].trim();
    processes.push({
      pid: Number(match[1]),
      ppid: Number(match[2]),
      rssBytes: Number(match[3]) * 1024,
      virtualBytes: Number(match[4]) * 1024,
      cpuSeconds: parseCpuSeconds(match[5]),
      elapsedSeconds: parseCpuSeconds(match[6]),
      command,
      name: basename(command),
    });
  }
  return processes;
}

export function parseCpuSeconds(value) {
  const text = String(value).trim();
  const [dayPart, ...timeParts] = text.split("-");
  let days = 0;
  let time = text;
  if (timeParts.length > 0) {
    days = Number(dayPart);
    time = timeParts.join("-");
  }
  const parts = time.split(":").map(Number);
  if (parts.some((part) => !Number.isFinite(part))) return null;
  if (parts.length === 2) {
    return days * 86_400 + parts[0] * 60 + parts[1];
  }
  if (parts.length === 3) {
    return days * 86_400 + parts[0] * 3_600 + parts[1] * 60 + parts[2];
  }
  return null;
}

export function classifyProcess(process, rootPid) {
  if (process.pid === rootPid) return "desktop";
  const name = String(process.name ?? "").toLowerCase();
  if (
    name.includes("webkit") ||
    name.includes("webcontent") ||
    name.includes("webview2") ||
    name.includes("msedgewebview")
  ) {
    return "webview";
  }
  if (name.includes("muxyard-host")) return "host";
  return "children";
}

export function collectDescendants(rootPid, processes) {
  const byPid = new Map(processes.map((process) => [process.pid, process]));
  const children = new Map();
  for (const process of processes) {
    if (!children.has(process.ppid)) children.set(process.ppid, []);
    children.get(process.ppid).push(process);
  }
  const result = [];
  const stack = [rootPid];
  const seen = new Set();
  while (stack.length > 0) {
    const pid = stack.pop();
    if (seen.has(pid)) continue;
    seen.add(pid);
    const process = byPid.get(pid);
    if (!process) continue;
    result.push(process);
    for (const child of children.get(pid) ?? []) stack.push(child.pid);
  }
  return result;
}

export function collectReparentedWebviews(rootPid, processes) {
  const root = processes.find((process) => process.pid === rootPid);
  if (!root || !Number.isFinite(root.elapsedSeconds)) return [];
  return processes.filter(
    (process) =>
      process.ppid === 1 &&
      process.pid !== rootPid &&
      classifyProcess(process, rootPid) === "webview" &&
      Number.isFinite(process.elapsedSeconds) &&
      process.elapsedSeconds <= root.elapsedSeconds + 2,
  );
}

export function sampleProcessTree(rootPid, options = {}) {
  const capturedAt = Date.now();
  const all =
    process.platform === "win32"
      ? collectWindowsProcesses()
      : collectUnixProcesses();
  const tree = collectDescendants(rootPid, all);
  if (tree.length === 0) {
    throw new Error(`process ${rootPid} is not running`);
  }
  const seen = new Set(tree.map((process) => process.pid));
  for (const webview of collectReparentedWebviews(rootPid, all)) {
    if (!seen.has(webview.pid)) {
      tree.push(webview);
      seen.add(webview.pid);
    }
  }
  const processes = tree.map((entry) => {
    const handles =
      options.includeHandles === false
        ? null
        : (entry.handles ?? countHandles(entry.pid));
    return {
      pid: entry.pid,
      ppid: entry.ppid,
      name: entry.name,
      command: entry.command,
      rssBytes: finiteOrNull(entry.rssBytes),
      workingSetBytes: finiteOrNull(entry.workingSetBytes),
      privateBytes: finiteOrNull(entry.privateBytes),
      virtualBytes: finiteOrNull(entry.virtualBytes),
      cpuSeconds: finiteOrNull(entry.cpuSeconds),
      handles: finiteOrNull(handles),
      class: classifyProcess(entry, rootPid),
    };
  });
  return { rootPid, capturedAt, processes };
}

export function buildIdleMetrics(sample1, sample2) {
  const elapsedMs = sample2.capturedAt - sample1.capturedAt;
  const classes = Object.fromEntries(
    CLASSES.map((name) => {
      const processes = sample2.processes
        .filter((process) => process.class === name)
        .map((process) => processView(process, sample1, elapsedMs));
      return [
        name,
        {
          processes,
          aggregate: aggregateProcesses(processes, sample1, elapsedMs),
        },
      ];
    }),
  );

  const treeProcesses = sample2.processes.map((process) =>
    processView(process, sample1, elapsedMs),
  );
  return {
    status: "AVAILABLE",
    sample_window_ms: elapsedMs,
    captured_at: new Date(sample2.capturedAt).toISOString(),
    process_count: treeProcesses.length,
    classes,
    webview_attribution:
      "descendants plus WebKit XPC processes reparented to launchd and started within 2s of the desktop root",
    tree_aggregate: aggregateProcesses(treeProcesses, sample1, elapsedMs),
    excluded: [
      ...(classes.host.processes.length === 0
        ? ["persistent host process: none observed"]
        : []),
      ...(classes.children.processes.length === 0
        ? ["agent child processes: none observed"]
        : []),
    ],
  };
}

function processView(process, sample1, elapsedMs) {
  const previous = sample1.processes.find((entry) => entry.pid === process.pid);
  const cpuDelta =
    previous &&
    Number.isFinite(previous.cpuSeconds) &&
    Number.isFinite(process.cpuSeconds)
      ? process.cpuSeconds - previous.cpuSeconds
      : null;
  return {
    pid: process.pid,
    ppid: process.ppid,
    name: process.name,
    command: process.command,
    rss_bytes: process.rssBytes,
    working_set_bytes: process.workingSetBytes,
    private_bytes: process.privateBytes,
    virtual_bytes: process.virtualBytes,
    cpu_seconds: process.cpuSeconds,
    cpu_percent:
      cpuDelta !== null && elapsedMs > 0
        ? round((cpuDelta / (elapsedMs / 1000)) * 100)
        : null,
    handles: process.handles,
  };
}

function aggregateProcesses(processes, sample1, elapsedMs) {
  return {
    process_count: processes.length,
    rss_bytes: aggregateMetric(
      processes,
      "rss_bytes",
      "bytes",
      "sum of resident set sizes from ps",
    ),
    working_set_bytes: aggregateMetric(
      processes,
      "working_set_bytes",
      "bytes",
      "sum of Windows working-set sizes",
    ),
    private_bytes: aggregateMetric(
      processes,
      "private_bytes",
      "bytes",
      "sum of Windows private bytes",
    ),
    handles: aggregateMetric(
      processes,
      "handles",
      "count",
      "sum of open file descriptors/handles",
    ),
    cpu_percent: aggregateCpu(processes, sample1, elapsedMs),
  };
}

function aggregateMetric(processes, key, unit, method) {
  const values = processes
    .map((process) => process[key])
    .filter((value) => Number.isFinite(value));
  if (values.length === 0) {
    return unavailable(`no sampled process exposed ${key}`);
  }
  const value = round(values.reduce((total, current) => total + current, 0));
  if (values.length < processes.length) {
    return partial(
      value,
      unit,
      method,
      `${values.length}/${processes.length} processes exposed ${key}`,
    );
  }
  return available(value, unit, method);
}

function aggregateCpu(processes, sample1, elapsedMs) {
  const deltas = processes
    .map((process) => {
      const previous = sample1.processes.find(
        (entry) => entry.pid === process.pid,
      );
      if (!previous) return null;
      if (
        !Number.isFinite(previous.cpuSeconds) ||
        !Number.isFinite(process.cpu_seconds)
      ) {
        return null;
      }
      return process.cpu_seconds - previous.cpuSeconds;
    })
    .filter((value) => value !== null);
  if (deltas.length === 0 || elapsedMs <= 0) {
    return unavailable("CPU time delta could not be sampled");
  }
  const value = round(
    (deltas.reduce((total, current) => total + current, 0) /
      (elapsedMs / 1000)) *
      100,
  );
  if (deltas.length < processes.length) {
    return partial(
      value,
      "%",
      "sum of CPU-time deltas over the sample window",
      `${deltas.length}/${processes.length} processes matched the first sample`,
    );
  }
  return available(value, "%", "sum of CPU-time deltas over the sample window");
}

function collectUnixProcesses() {
  const result = spawnSync(
    "ps",
    ["-axo", "pid=,ppid=,rss=,vsz=,time=,etime=,comm="],
    {
      encoding: "utf8",
      timeout: 15_000,
    },
  );
  if (result.error || result.status !== 0) {
    throw new Error(
      `ps failed: ${result.error?.message ?? result.stderr?.trim() ?? result.status}`,
    );
  }
  return parsePsOutput(result.stdout).map((process) => ({
    ...process,
    workingSetBytes: null,
    privateBytes: null,
    handles: null,
  }));
}

function collectWindowsProcesses() {
  const script =
    "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,WorkingSetSize,PrivatePageCount,KernelModeTime,UserModeTime,HandleCount | ConvertTo-Json -Compress";
  const result = spawnSync(
    "powershell",
    ["-NoProfile", "-NonInteractive", "-Command", script],
    { encoding: "utf8", timeout: 20_000 },
  );
  if (result.error || result.status !== 0) {
    throw new Error(
      `process query failed: ${result.error?.message ?? result.stderr?.trim() ?? result.status}`,
    );
  }
  const parsed = JSON.parse(result.stdout);
  const rows = Array.isArray(parsed) ? parsed : [parsed];
  return rows.map((row) => {
    const kernel = Number(row.KernelModeTime);
    const user = Number(row.UserModeTime);
    const cpuSeconds =
      Number.isFinite(kernel) && Number.isFinite(user)
        ? (kernel + user) / 10_000_000
        : null;
    return {
      pid: Number(row.ProcessId),
      ppid: Number(row.ParentProcessId),
      name: String(row.Name ?? ""),
      command: String(row.Name ?? ""),
      rssBytes: Number(row.WorkingSetSize),
      workingSetBytes: Number(row.WorkingSetSize),
      privateBytes: Number(row.PrivatePageCount),
      virtualBytes: null,
      cpuSeconds,
      handles: Number(row.HandleCount),
    };
  });
}

function countHandles(pid) {
  if (process.platform === "linux") {
    try {
      return readdirSync(`/proc/${pid}/fd`).length;
    } catch {
      return null;
    }
  }
  if (process.platform === "darwin") {
    const result = spawnSync("lsof", ["-nP", "-p", String(pid)], {
      encoding: "utf8",
      timeout: 5_000,
    });
    if (result.error || result.status !== 0) return null;
    const lines = result.stdout.trim().split(/\r?\n/).filter(Boolean);
    return lines.length > 0 ? lines.length - 1 : 0;
  }
  return null;
}

export async function stopProcessTree(child, pids = []) {
  const start = Date.now();
  if (
    !child ||
    !child.pid ||
    child.exitCode !== null ||
    child.signalCode !== null
  ) {
    return {
      desktop_exited: true,
      exit_code: child?.exitCode ?? null,
      signal: child?.signalCode ?? null,
      duration_ms: 0,
      orphans: [],
      orphan_check: "AVAILABLE",
    };
  }

  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
      encoding: "utf8",
      timeout: 10_000,
    });
  } else {
    try {
      process.kill(-child.pid, "SIGTERM");
    } catch {
      child.kill("SIGTERM");
    }
  }

  let exited = await waitForExit(child, 5_000);
  if (!exited) {
    if (process.platform === "win32") {
      spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
        encoding: "utf8",
        timeout: 10_000,
      });
    } else {
      try {
        process.kill(-child.pid, "SIGKILL");
      } catch {
        child.kill("SIGKILL");
      }
    }
    await waitForExit(child, 2_000);
  }

  const orphanCheck =
    process.platform === "win32" ? "NOT_AVAILABLE" : "AVAILABLE";
  const candidates = pids.filter((pid) => pid !== child.pid);
  const orphans =
    orphanCheck === "AVAILABLE" ? await waitForOrphans(candidates, 3_000) : [];
  return {
    desktop_exited: child.exitCode !== null || child.signalCode !== null,
    exit_code: child.exitCode,
    signal: child.signalCode,
    duration_ms: Date.now() - start,
    orphans,
    orphan_check: orphanCheck,
    orphan_grace_ms: 3_000,
  };
}

export function waitForExit(child, timeoutMs) {
  if (!child || child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve(true);
  }
  return new Promise((resolve) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.removeListener("exit", onExit);
      resolve(false);
    }, timeoutMs);
    const onExit = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(true);
    };
    child.once("exit", onExit);
  });
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForOrphans(pids, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (pids.every((pid) => !isAlive(pid))) return [];
    await sleep(100);
  }
  return pids.filter(isAlive);
}

function finiteOrNull(value) {
  return Number.isFinite(value) ? value : null;
}

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === "EPERM";
  }
}

export { CLASSES };
