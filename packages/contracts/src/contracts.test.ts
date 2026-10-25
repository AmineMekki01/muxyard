import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, expectTypeOf, it } from "vitest";

import {
  MAX_CONTROL_BYTES,
  PROTOCOL_VERSION,
  type AgentStatus,
  type ApiError,
  type ControlEnvelope,
  type ErrorCode,
  type ExecutionEnvironment,
  type TaskState,
} from "./index";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const fixtures = join(root, "tests/fixtures/contracts");
let executable: string;

beforeAll(() => {
  execFileSync(
    "cargo",
    [
      "build",
      "--locked",
      "-p",
      "muxyard-contracts",
      "--example",
      "contract_roundtrip",
    ],
    { cwd: root, timeout: 120_000, stdio: "pipe" },
  );
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--offline", "--no-deps", "--format-version", "1"],
      {
        cwd: root,
        encoding: "utf8",
        timeout: 10_000,
      },
    ),
  );
  if (typeof metadata.target_directory !== "string") {
    throw new Error("Cargo did not return a target directory.");
  }
  executable = join(
    metadata.target_directory,
    "debug/examples/contract_roundtrip" +
      (process.platform === "win32" ? ".exe" : ""),
  );
}, 130_000);

function invoke(kind: string, input: string) {
  return spawnSync(executable, [kind], {
    input,
    encoding: "utf8",
    timeout: 10_000,
    maxBuffer: 2 * MAX_CONTROL_BYTES,
  });
}

function roundtrip(kind: string, value: unknown) {
  const result = invoke(kind, JSON.stringify(value));
  expect(result.error).toBeUndefined();
  expect(result.status, result.stderr).toBe(0);
  expect(JSON.parse(result.stdout)).toEqual(value);
  expect(result.stderr).toBe("");
}

describe("Rust-derived TypeScript wire contracts", () => {
  it("preserves shared fixture JSON from TypeScript through typed Rust", () => {
    for (const [name, kind] of [
      ["request", "environment"],
      ["response", "error"],
      ["event", "agent"],
      ["task", "task"],
    ] as const) {
      const fixture = JSON.parse(
        readFileSync(join(fixtures, `${name}.json`), "utf8"),
      );
      roundtrip(kind, fixture);
    }
  });

  it("compiles typed client messages and preserves their exact wire representation", () => {
    const request = {
      v: PROTOCOL_VERSION,
      id: "08ac1e24-30ea-46cc-bebf-4ba29a099452",
      type: "request",
      method: "workspace.environment",
      data: { kind: "wsl", distro: "Ubuntu-24.04" },
    } satisfies ControlEnvelope<ExecutionEnvironment>;
    const fixture = JSON.parse(
      readFileSync(join(fixtures, "request.json"), "utf8"),
    );
    expect(request).toEqual(fixture);
    roundtrip("environment", request);
    expectTypeOf<ControlEnvelope<TaskState>["v"]>().toEqualTypeOf<1>();
    expectTypeOf<ApiError["code"]>().toEqualTypeOf<ErrorCode>();
  });

  it("roundtrips every environment and lifecycle value from typed client code", () => {
    const environments: ExecutionEnvironment[] = [
      { kind: "native-mac" },
      { kind: "native-linux" },
      { kind: "native-windows" },
      { kind: "wsl", distro: "Ubuntu-24.04" },
    ];
    const tasks: TaskState[] = [
      "draft",
      "ready",
      "running",
      "blocked",
      "review",
      "approved",
      "integrated",
      "failed",
      "canceled",
    ];
    const agents: AgentStatus[] = [
      "starting",
      "working",
      "idle",
      "needs_input",
      "needs_approval",
      "turn_finished",
      "failed",
      "exited",
      "unknown",
    ];
    for (const [kind, values] of [
      ["environment", environments],
      ["task", tasks],
      ["agent", agents],
    ] as const) {
      for (const data of values) {
        roundtrip(kind, {
          v: 1,
          id: "fixture",
          type: "event",
          method: "fixture",
          data,
        });
      }
    }
  });

  it("roundtrips every stable error code", () => {
    const codes: ErrorCode[] = [
      "UNAUTHORIZED",
      "FORBIDDEN",
      "NOT_FOUND",
      "CONFLICT",
      "UNSUPPORTED_CAPABILITY",
      "STALE_REVISION",
      "HOST_OFFLINE",
      "RATE_LIMITED",
      "PROVIDER_ERROR",
      "INTERNAL",
      "UNSUPPORTED_VERSION",
      "INVALID_REQUEST",
      "PAYLOAD_TOO_LARGE",
    ];
    for (const code of codes) {
      const message = {
        v: 1,
        id: "fixture",
        type: "response",
        method: "fixture",
        data: { code, message: "Safe public diagnostic." },
      } satisfies ControlEnvelope<ApiError>;
      roundtrip("error", message);
    }
  });

  it("rejects shared malformed, unknown-version, and unknown-enum fixtures in Rust", () => {
    const invalid: { name: string; input: string; code: ErrorCode }[] =
      JSON.parse(readFileSync(join(fixtures, "invalid.json"), "utf8"));
    for (const fixture of invalid) {
      const result = invoke("task", fixture.input);
      expect(result.error).toBeUndefined();
      expect(result.status, fixture.name).toBe(1);
      expect(result.stdout).toBe("");
      const firstLine = result.stderr.split("\n")[0] ?? "";
      expect(firstLine).toBeTruthy();
      expect(JSON.parse(firstLine).code).toBe(fixture.code);
      expect(result.stderr).not.toContain("secret");
    }
  });

  it("enforces the generated byte limit and recovers after rejected messages", () => {
    const valid = readFileSync(join(fixtures, "task.json"), "utf8");
    const boundary = valid.padEnd(MAX_CONTROL_BYTES, " ");
    expect(invoke("task", boundary).status).toBe(0);
    const oversized = invoke("task", boundary + " ");
    expect(oversized.status).toBe(1);
    expect(oversized.stderr).toContain('"code":"PAYLOAD_TOO_LARGE"');
    const unicode = invoke("task", "\u00e9".repeat(MAX_CONTROL_BYTES / 2 + 1));
    expect(unicode.stderr).toContain('"code":"PAYLOAD_TOO_LARGE"');
    roundtrip("task", JSON.parse(valid));
  });
});
