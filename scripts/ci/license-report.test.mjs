/** Verify deterministic license artifacts and unknown-license gating. */
import assert from "node:assert/strict";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildReport, main } from "./license-report.mjs";

const CARGO = {
  packages: [
    {
      name: "muxyard-host",
      version: "0.1.0",
      source: null,
      license: null,
      license_file: null,
    },
    {
      name: "serde",
      version: "1.0.0",
      source: "registry+https://github.com/rust-lang/crates.io-index",
      license: "MIT OR Apache-2.0",
      repository: "https://github.com/serde-rs/serde",
    },
    {
      name: "mystery-rust",
      version: "2.0.0",
      source: "registry+https://github.com/rust-lang/crates.io-index",
      license: null,
      license_file: null,
    },
  ],
};

const NPM = {
  MIT: [
    {
      name: "left-pad",
      versions: ["1.0.0"],
      license: "MIT",
      homepage: "https://example.invalid/left-pad",
    },
  ],
  Unknown: [{ name: "mystery-js", versions: ["2.0.0"] }],
};

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "muxyard-license-"));
  const cargo = join(root, "cargo.json");
  const npm = join(root, "npm.json");
  const output = join(root, "licenses");
  writeFileSync(cargo, JSON.stringify(CARGO));
  writeFileSync(npm, JSON.stringify(NPM));
  return { root, cargo, npm, output };
}

test("builds a sorted summary with unknown third-party licenses", () => {
  const report = buildReport(CARGO, NPM);
  assert.equal(report.summary.rust.totalPackages, 3);
  assert.equal(report.summary.rust.firstParty, 1);
  assert.equal(report.summary.rust.thirdParty, 2);
  assert.equal(report.summary.rust.unknown, 1);
  assert.equal(report.summary.javascript.totalPackages, 2);
  assert.equal(report.summary.javascript.unknown, 1);
  assert.equal(report.rust.packages[0].name, "mystery-rust");
  assert.equal(report.rust.packages[1].name, "serde");
});

test("writes JSON and Markdown artifacts deterministically", () => {
  const { root, cargo, npm, output } = fixture();
  try {
    assert.equal(main(["--cargo", cargo, "--npm", npm, "--output", output]), 0);
    for (const name of [
      "summary.json",
      "rust-licenses.json",
      "rust-licenses.md",
      "javascript-licenses.json",
      "javascript-licenses.md",
    ]) {
      assert.ok(existsSync(join(output, name)), name);
    }
    const first = readFileSync(join(output, "summary.json"), "utf8");
    assert.equal(main(["--cargo", cargo, "--npm", npm, "--output", output]), 0);
    assert.equal(readFileSync(join(output, "summary.json"), "utf8"), first);
    assert.match(
      readFileSync(join(output, "rust-licenses.md"), "utf8"),
      /mystery-rust/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("unknown licenses fail only when gating is requested", () => {
  const { root, cargo, npm, output } = fixture();
  try {
    assert.equal(
      main([
        "--cargo",
        cargo,
        "--npm",
        npm,
        "--output",
        output,
        "--fail-on-unknown",
      ]),
      1,
    );
    assert.ok(existsSync(join(output, "summary.json")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("missing or malformed inputs fail with a diagnostic", () => {
  const root = mkdtempSync(join(tmpdir(), "muxyard-license-bad-"));
  try {
    const bad = join(root, "bad.json");
    writeFileSync(bad, "{not json");
    assert.equal(main(["--cargo", bad, "--npm", bad]), 1);
    assert.equal(main([]), 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
