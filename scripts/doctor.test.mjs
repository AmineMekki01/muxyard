/** Exercise missing-tool behavior without modifying the user's installations. */
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath, URL } from "node:url";

test("missing toolchain fails clearly before any development server starts", () => {
  const result = spawnSync(
    process.execPath,
    [fileURLToPath(new URL("./doctor.mjs", import.meta.url))],
    {
      env: { ...process.env, PATH: "" },
      encoding: "utf8",
      timeout: 10_000,
    },
  );
  assert.equal(result.error, undefined);
  assert.equal(result.status, 1);
  for (const command of ["rustc", "cargo", "pnpm"]) {
    assert.match(
      result.stderr,
      new RegExp(`Missing or unusable development dependency: ${command}`),
    );
  }
  assert.equal(result.stdout, "");
});
