/** Verify process-table parsing, tree classification, and idle aggregation. */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  buildIdleMetrics,
  classifyProcess,
  collectDescendants,
  collectReparentedWebviews,
  parseCpuSeconds,
  parsePsOutput,
  stopProcessTree,
} from "./ps.mjs";

const fixture = readFileSync(
  fileURLToPath(
    new URL("../../../tests/benchmarks/fixtures/ps-macos.txt", import.meta.url),
  ),
  "utf8",
);

test("parses macOS process rows into byte and CPU values", () => {
  const processes = parsePsOutput(fixture);
  assert.equal(processes.length, 5);
  assert.deepEqual(processes[0], {
    pid: 100,
    ppid: 1,
    rssBytes: 1024 * 1024,
    virtualBytes: 2048 * 1024,
    cpuSeconds: 1,
    elapsedSeconds: 5,
    command: "/Applications/Muxyard.app/Contents/MacOS/muxyard-desktop",
    name: "muxyard-desktop",
  });
  assert.equal(parseCpuSeconds("1:02.50"), 62.5);
  assert.equal(parseCpuSeconds("1:02:03.50"), 3723.5);
  assert.equal(parseCpuSeconds("1-02:03:04"), 93784);
  assert.equal(parseCpuSeconds("not-a-time"), null);
});

test("collects descendants and separates webview and child processes", () => {
  const processes = parsePsOutput(fixture);
  const tree = collectDescendants(100, processes);
  assert.deepEqual(
    tree.map((process) => process.pid).sort((left, right) => left - right),
    [100, 101, 102, 103, 104],
  );
  const byPid = new Map(tree.map((process) => [process.pid, process]));
  assert.equal(classifyProcess(byPid.get(100), 100), "desktop");
  assert.equal(classifyProcess(byPid.get(101), 100), "webview");
  assert.equal(classifyProcess(byPid.get(103), 100), "children");
  assert.equal(
    classifyProcess({ pid: 200, name: "muxyard-host" }, 100),
    "host",
  );
});

test("includes recent launchd-reparented WebKit XPC services", () => {
  const processes =
    parsePsOutput(`  100     1  1024  2048 0:01.00 00:05 /Applications/Muxyard.app/Contents/MacOS/muxyard-desktop
  105     1   512  1024 0:00.20 00:04 /System/Library/Frameworks/WebKit.framework/com.apple.WebKit.GPU
  106     1   512  1024 0:00.20 00:20 /System/Library/Frameworks/WebKit.framework/com.apple.WebKit.WebContent
`);
  assert.deepEqual(
    collectReparentedWebviews(100, processes).map((process) => process.pid),
    [105],
  );
});

test("aggregates RSS, handles, and CPU over the sample window", () => {
  const first = parsePsOutput(fixture).map((process) => ({
    ...process,
    workingSetBytes: null,
    privateBytes: null,
    handles: 1,
    class: classifyProcess(process, 100),
  }));
  const second = first.map((process) => ({
    ...process,
    cpuSeconds: process.cpuSeconds + 0.1,
  }));
  const idle = buildIdleMetrics(
    { capturedAt: 1_000, processes: first },
    { capturedAt: 2_000, processes: second },
  );
  assert.equal(idle.status, "AVAILABLE");
  assert.equal(idle.process_count, 5);
  assert.equal(idle.classes.desktop.processes.length, 1);
  assert.equal(idle.classes.webview.processes.length, 2);
  assert.equal(idle.classes.children.processes.length, 2);
  assert.equal(idle.tree_aggregate.rss_bytes.status, "AVAILABLE");
  assert.equal(idle.tree_aggregate.rss_bytes.value, 3968 * 1024);
  assert.equal(idle.tree_aggregate.cpu_percent.status, "AVAILABLE");
  assert.equal(idle.tree_aggregate.cpu_percent.value, 50);
  assert.equal(idle.tree_aggregate.handles.value, 5);
  assert.equal(idle.tree_aggregate.working_set_bytes.status, "NOT_AVAILABLE");
});

test("stops a spawned process tree without leaving the root alive", async () => {
  const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    detached: process.platform !== "win32",
    stdio: "ignore",
  });
  await new Promise((resolve, reject) => {
    child.once("spawn", resolve);
    child.once("error", reject);
  });
  const cleanup = await stopProcessTree(child, [child.pid]);
  assert.equal(cleanup.desktop_exited, true);
  assert.deepEqual(cleanup.orphans, []);
});
