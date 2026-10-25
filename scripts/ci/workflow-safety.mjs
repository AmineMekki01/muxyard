/** Reject workflow patterns that hide platform failures or expose credentials. */
import { readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const USAGE =
  "Usage: node scripts/ci/workflow-safety.mjs [--workflows <directory>]";

const FORBIDDEN = [
  [/pull_request_target/, "uses pull_request_target"],
  [/\bsecrets\./, "references repository secrets"],
  [/\bgithub\.token\b/, "references the repository token"],
  [/persist-credentials:\s*true/, "persists checkout credentials"],
  [/continue-on-error:\s*true/, "silently tolerates failed steps"],
  [/permissions:\s*write-all/, "grants write-all permissions"],
  [/^\s*[a-z-]+:\s*write\s*$/m, "grants a write permission"],
  [/\bACTIONS_(?:STEP|RUNNER)_DEBUG\b/, "enables runner debug logging"],
  [/\bset\s+-x\b/, "enables shell tracing"],
  [/echo[^\n]*\$\{\{/i, "interpolates workflow expressions into echo"],
  [
    /\b(?:curl|wget)\b[^\n|]*\|\s*(?:bash|sh)\b/,
    "pipes downloaded content into a shell",
  ],
];

const REQUIRED = [
  [/^\s*permissions:\s*$/m, "missing top-level permissions block"],
  [/^\s*contents:\s*read\s*$/m, "missing contents: read permission"],
  [
    /^\s*persist-credentials:\s*false\s*$/m,
    "checkout credentials are not disabled",
  ],
  [/^\s*matrix:\s*$/m, "missing matrix definition"],
  [/^\s*fail-fast:\s*false\s*$/m, "matrix must not cancel other platforms"],
  [/^\s*push:\s*$/m, "missing push trigger"],
  [/^\s*pull_request:\s*$/m, "missing pull_request trigger"],
  [/actions\/checkout@/m, "missing checkout action"],
  [/actions\/upload-artifact@/m, "missing artifact upload"],
];

const REQUIRED_OS = ["macos-latest", "ubuntu-latest", "windows-latest"];

export function inspectWorkflowText(name, text) {
  const violations = [];
  for (const [pattern, message] of FORBIDDEN) {
    if (pattern.test(text)) violations.push(`${name}: ${message}`);
  }
  for (const [pattern, message] of REQUIRED) {
    if (!pattern.test(text)) violations.push(`${name}: ${message}`);
  }
  for (const os of REQUIRED_OS) {
    if (!text.includes(os))
      violations.push(`${name}: missing ${os} matrix entry`);
  }
  for (const match of text.matchAll(/^\s*(?:-\s*)?uses:\s*([^\s#]+)/gm)) {
    const action = match[1];
    if (action.startsWith("./")) continue;
    const reference = action.split("@").at(-1);
    if (!/^[0-9a-f]{40}$/i.test(reference)) {
      violations.push(`${name}: action ${action} is not pinned to a commit`);
    }
  }
  return violations;
}

export function inspectWorkflowDirectory(directory) {
  const resolved = resolve(directory);
  let entries;
  try {
    entries = readdirSync(resolved, { withFileTypes: true });
  } catch (error) {
    return {
      files: [],
      violations: [`${resolved}: ${error.message}`],
    };
  }
  const files = entries
    .filter((entry) => entry.isFile() && /\.ya?ml$/i.test(entry.name))
    .map((entry) => entry.name)
    .sort();
  if (files.length === 0) {
    return {
      files,
      violations: [`${resolved}: no workflow files found`],
    };
  }
  const violations = [];
  for (const file of files) {
    const path = join(resolved, file);
    try {
      violations.push(...inspectWorkflowText(file, readFileSync(path, "utf8")));
    } catch (error) {
      violations.push(`${file}: ${error.message}`);
    }
  }
  return { files, violations };
}

export function main(argv = process.argv.slice(2)) {
  let values;
  try {
    ({ values } = parseArgs({
      args: argv,
      options: {
        workflows: { type: "string", default: ".github/workflows" },
      },
      strict: true,
    }));
  } catch (error) {
    console.error(`workflow-safety: ${error.message}\n${USAGE}`);
    return 1;
  }

  const result = inspectWorkflowDirectory(values.workflows);
  if (result.violations.length > 0) {
    for (const violation of result.violations) {
      console.error(`workflow-safety: ${violation}`);
    }
    return 1;
  }
  console.log(
    `Workflow safety checks passed for ${result.files.length} file(s).`,
  );
  return 0;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  process.exitCode = main();
}

export { USAGE };
