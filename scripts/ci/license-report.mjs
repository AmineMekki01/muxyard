/** Generate deterministic dependency license artifacts from Cargo and pnpm metadata. */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const USAGE =
  "Usage: node scripts/ci/license-report.mjs --cargo <metadata.json> --npm <licenses.json> --output <dir> [--fail-on-unknown]";

export function buildReport(cargoMetadata, npmLicenses) {
  if (!cargoMetadata || !Array.isArray(cargoMetadata.packages)) {
    throw new Error("Cargo metadata must contain a packages array");
  }
  if (
    !npmLicenses ||
    typeof npmLicenses !== "object" ||
    Array.isArray(npmLicenses)
  ) {
    throw new Error("pnpm license data must be an object keyed by license");
  }

  const firstParty = [];
  const rustPackages = [];
  for (const pkg of cargoMetadata.packages) {
    if (
      !pkg ||
      typeof pkg.name !== "string" ||
      typeof pkg.version !== "string"
    ) {
      throw new Error("Cargo metadata contains an invalid package entry");
    }
    if (pkg.source === null || pkg.source === undefined) {
      firstParty.push({ name: pkg.name, version: pkg.version });
      continue;
    }
    rustPackages.push({
      name: pkg.name,
      version: pkg.version,
      license: packageLicense(pkg),
      repository: pkg.repository ?? null,
    });
  }
  rustPackages.sort(comparePackages);
  firstParty.sort(comparePackages);

  const javascriptPackages = [];
  for (const [licenseKey, packages] of Object.entries(npmLicenses)) {
    if (!Array.isArray(packages)) {
      throw new Error(`pnpm license group ${licenseKey} must be an array`);
    }
    for (const pkg of packages) {
      if (!pkg || typeof pkg.name !== "string") {
        throw new Error("pnpm license data contains an invalid package entry");
      }
      const versions = Array.isArray(pkg.versions)
        ? pkg.versions.filter((version) => typeof version === "string")
        : [];
      javascriptPackages.push({
        name: pkg.name,
        versions,
        license: normalizedLicense(pkg.license ?? licenseKey),
        homepage: pkg.homepage ?? null,
      });
    }
  }
  javascriptPackages.sort((left, right) => left.name.localeCompare(right.name));

  const rustUnknown = rustPackages.filter((pkg) => pkg.license === "UNKNOWN");
  const javascriptUnknown = javascriptPackages.filter(
    (pkg) => pkg.license === "UNKNOWN",
  );

  const summary = {
    rust: {
      totalPackages: cargoMetadata.packages.length,
      firstParty: firstParty.length,
      thirdParty: rustPackages.length,
      unknown: rustUnknown.length,
      byLicense: countLicenses(rustPackages),
    },
    javascript: {
      totalPackages: javascriptPackages.length,
      unknown: javascriptUnknown.length,
      byLicense: countLicenses(javascriptPackages),
    },
  };

  return {
    summary,
    rust: { firstParty, packages: rustPackages },
    javascript: { packages: javascriptPackages },
  };
}

export function writeReport(report, outputDirectory) {
  const directory = resolve(outputDirectory);
  mkdirSync(directory, { recursive: true });
  writeJson(resolve(directory, "summary.json"), report.summary);
  writeJson(resolve(directory, "rust-licenses.json"), report.rust);
  writeJson(resolve(directory, "javascript-licenses.json"), report.javascript);
  writeFileSync(
    resolve(directory, "rust-licenses.md"),
    renderRustMarkdown(report),
  );
  writeFileSync(
    resolve(directory, "javascript-licenses.md"),
    renderJavaScriptMarkdown(report),
  );
  return directory;
}

export function main(argv = process.argv.slice(2)) {
  let values;
  try {
    ({ values } = parseArgs({
      args: argv,
      options: {
        cargo: { type: "string" },
        npm: { type: "string" },
        output: { type: "string", default: "target/ci/licenses" },
        "fail-on-unknown": { type: "boolean", default: false },
      },
      strict: true,
    }));
  } catch (error) {
    console.error(`license-report: ${error.message}\n${USAGE}`);
    return 1;
  }
  if (!values.cargo || !values.npm) {
    console.error(`license-report: --cargo and --npm are required\n${USAGE}`);
    return 1;
  }

  try {
    const cargoMetadata = readJson(values.cargo, "Cargo metadata");
    const npmLicenses = readJson(values.npm, "pnpm licenses");
    const report = buildReport(cargoMetadata, npmLicenses);
    const directory = writeReport(report, values.output);
    console.log(
      `Rust: ${report.summary.rust.thirdParty} third-party, ${report.summary.rust.unknown} unknown; ` +
        `JavaScript: ${report.summary.javascript.totalPackages}, ${report.summary.javascript.unknown} unknown.`,
    );
    console.log(`Wrote license artifacts to ${directory}.`);

    const unknown =
      report.summary.rust.unknown + report.summary.javascript.unknown;
    if (values["fail-on-unknown"] && unknown > 0) {
      console.error(`license-report: ${unknown} dependency license(s) unknown`);
      return 1;
    }
    return 0;
  } catch (error) {
    console.error(`license-report: ${error.message}`);
    return 1;
  }
}

function readJson(path, label) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(
      `${label} could not be read from ${path}: ${error.message}`,
      { cause: error },
    );
  }
}

function packageLicense(pkg) {
  if (typeof pkg.license === "string" && pkg.license.trim()) {
    return pkg.license.trim();
  }
  if (typeof pkg.license_file === "string" && pkg.license_file.trim()) {
    return `SEE LICENSE IN ${pkg.license_file.trim()}`;
  }
  return "UNKNOWN";
}

function normalizedLicense(value) {
  if (typeof value !== "string" || !value.trim()) return "UNKNOWN";
  const license = value.trim();
  return license.toLowerCase() === "unknown" ? "UNKNOWN" : license;
}

function comparePackages(left, right) {
  return (
    left.name.localeCompare(right.name) ||
    left.version.localeCompare(right.version)
  );
}

function countLicenses(packages) {
  const counts = {};
  for (const pkg of packages) {
    counts[pkg.license] = (counts[pkg.license] ?? 0) + 1;
  }
  return Object.fromEntries(
    Object.entries(counts).sort(([left], [right]) => left.localeCompare(right)),
  );
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function renderRustMarkdown(report) {
  const lines = [
    "# Rust Dependency Licenses",
    "",
    `First-party packages: ${report.rust.firstParty.length}`,
    "",
    "| Package | Version | License |",
    "| --- | --- | --- |",
    ...report.rust.packages.map(
      (pkg) =>
        `| ${cell(pkg.name)} | ${cell(pkg.version)} | ${cell(pkg.license)} |`,
    ),
    "",
  ];
  return lines.join("\n");
}

function renderJavaScriptMarkdown(report) {
  const lines = [
    "# JavaScript Dependency Licenses",
    "",
    `Packages: ${report.javascript.packages.length}`,
    "",
    "| Package | Versions | License |",
    "| --- | --- | --- |",
    ...report.javascript.packages.map(
      (pkg) =>
        `| ${cell(pkg.name)} | ${cell(pkg.versions.join(", "))} | ${cell(pkg.license)} |`,
    ),
    "",
  ];
  return lines.join("\n");
}

function cell(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  process.exitCode = main();
}

export { USAGE };
