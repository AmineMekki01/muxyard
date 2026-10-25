/** Hardware, OS, toolchain, and build metadata collection. */
import { spawnSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";
import os from "node:os";
import { relative, resolve } from "node:path";

export function runCommand(command, args, timeout = 15_000) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    timeout,
  });
  if (result.error || result.status !== 0) {
    return {
      available: false,
      reason:
        result.error?.message ??
        result.stderr?.trim() ??
        `exit ${result.status}`,
    };
  }
  return { available: true, output: result.stdout.trim() };
}

export function collectEnvironment() {
  const platform = process.platform;
  const cpus = os.cpus();
  return {
    platform,
    arch: process.arch,
    os: osInfo(platform),
    cpu: {
      model: cpus[0]?.model?.trim() ?? null,
      logical_cores: cpus.length || null,
      physical_cores: physicalCores(platform),
    },
    memory_bytes: os.totalmem(),
    node: process.version,
    rustc: toolVersion("rustc"),
    cargo: toolVersion("cargo"),
    ci: process.env.CI === "true",
  };
}

export function collectBuild(hostBin, desktopBin) {
  const sha = runCommand("git", ["rev-parse", "HEAD"]);
  const status = runCommand("git", [
    "status",
    "--porcelain",
    "--untracked-files=normal",
  ]);
  return {
    git_sha: sha.available ? sha.output : null,
    dirty: status.available ? status.output.length > 0 : null,
    profile: /[/\\]debug[/\\]/.test(String(hostBin)) ? "debug" : "release",
    binaries: {
      host: binaryInfo(hostBin),
      desktop: binaryInfo(desktopBin),
    },
  };
}

function osInfo(platform) {
  if (platform === "darwin") {
    const version = runCommand("sw_vers", ["-productVersion"]);
    const build = runCommand("sw_vers", ["-buildVersion"]);
    const kernel = runCommand("uname", ["-r"]);
    return {
      name: "macOS",
      version: version.available ? version.output : null,
      build: build.available ? build.output : null,
      kernel: kernel.available ? kernel.output : null,
    };
  }
  if (platform === "linux") {
    const release = readOsRelease();
    const kernel = runCommand("uname", ["-r"]);
    return {
      name: release.PRETTY_NAME ?? "Linux",
      version: release.VERSION_ID ?? null,
      build: null,
      kernel: kernel.available ? kernel.output : null,
    };
  }
  if (platform === "win32") {
    return {
      name: "Windows",
      version: os.version(),
      build: os.release(),
      kernel: os.release(),
    };
  }
  return { name: platform, version: null, build: null, kernel: null };
}

function readOsRelease() {
  try {
    const content = readFileSync("/etc/os-release", "utf8");
    return Object.fromEntries(
      content
        .split(/\r?\n/)
        .filter((line) => line.includes("="))
        .map((line) => {
          const [key, ...rest] = line.split("=");
          return [key, rest.join("=").replace(/^"|"$/g, "")];
        }),
    );
  } catch {
    return {};
  }
}

function physicalCores(platform) {
  if (platform === "darwin") {
    const result = runCommand("sysctl", ["-n", "hw.physicalcpu"]);
    return result.available ? Number(result.output) : null;
  }
  if (platform === "linux") {
    try {
      const content = readFileSync("/proc/cpuinfo", "utf8");
      const pairs = new Set();
      let physicalId = "0";
      for (const line of content.split(/\r?\n/)) {
        if (line.startsWith("physical id"))
          physicalId = line.split(":")[1].trim();
        if (line.startsWith("core id")) {
          pairs.add(`${physicalId}:${line.split(":")[1].trim()}`);
        }
      }
      return pairs.size || null;
    } catch {
      return null;
    }
  }
  if (platform === "win32") {
    const result = runCommand("powershell", [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      "(Get-CimInstance Win32_Processor | Measure-Object -Property NumberOfCores -Sum).Sum",
    ]);
    return result.available ? Number(result.output) : null;
  }
  return null;
}

function toolVersion(command) {
  const result = runCommand(command, ["--version"]);
  return result.available
    ? { available: true, output: result.output }
    : { available: false, reason: result.reason };
}

function binaryInfo(binaryPath) {
  const absolute = resolve(binaryPath);
  const displayPath = relative(process.cwd(), absolute);
  const path =
    displayPath && !displayPath.startsWith("..") ? displayPath : absolute;
  try {
    const stats = statSync(absolute);
    return {
      path,
      exists: true,
      size_bytes: stats.size,
      modified_ms: stats.mtimeMs,
    };
  } catch {
    return { path, exists: false, size_bytes: null, modified_ms: null };
  }
}
