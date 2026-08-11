import { spawn, spawnSync } from "node:child_process";
import { mkdir, readdir, realpath, stat } from "node:fs/promises";
import path from "node:path";

import { parseBoundedJson } from "./ade0006a-c0b-common.mjs";
import { hashBoundedFile } from "./ade0006a-package-members.mjs";

const buildTimeoutMs = 20 * 60 * 1000;

async function requiredAbsoluteFile(value, label) {
  if (!value || !path.isAbsolute(value)) {
    throw new Error(`${label}-path-invalid`);
  }
  const executionPath = path.resolve(value);
  const identityPath = await realpath(executionPath);
  if (!(await stat(identityPath)).isFile()) {
    throw new Error(`${label}-path-invalid`);
  }
  return { executionPath, identityPath };
}

function terminateTree(child) {
  const taskkill = path.join(
    process.env.SystemRoot ?? "C:\\Windows",
    "System32",
    "taskkill.exe",
  );
  const killed = spawnSync(taskkill, ["/PID", String(child.pid), "/T", "/F"], {
    windowsHide: true,
    stdio: "ignore",
    timeout: 5000,
  });
  if (killed.error || killed.status !== 0) child.kill();
}

export function runBounded(executable, args, options) {
  const stdinMaximum = options.stdinMaximum ?? 4096;
  if (
    options.stdin !== undefined &&
    (!Number.isSafeInteger(stdinMaximum) ||
      stdinMaximum < 1 ||
      stdinMaximum > 2 * 1024 * 1024 ||
      !Buffer.isBuffer(options.stdin) ||
      options.stdin.byteLength > stdinMaximum)
  ) {
    throw new Error(`${options.label}-stdin-invalid`);
  }
  return new Promise((resolve, reject) => {
    let stdoutBytes = 0;
    let stderrBytes = 0;
    const stdout = [];
    let terminalReason;
    const child = spawn(executable, args, {
      cwd: options.cwd,
      env: options.env,
      windowsHide: true,
      stdio: [options.stdin ? "pipe" : "ignore", "pipe", "pipe"],
    });
    if (options.stdin) {
      child.stdin.on("error", (error) => {
        terminalReason ??= `stdin-${error.code ?? "failed"}`;
      });
      child.stdin.end(options.stdin);
    }
    function capture(chunk, stream) {
      if (terminalReason) return;
      const maximum =
        stream === "stdout" ? options.stdoutMaximum : options.stderrMaximum;
      const next =
        (stream === "stdout" ? stdoutBytes : stderrBytes) + chunk.length;
      if (next > maximum) {
        terminalReason = `${stream}-oversize`;
        terminateTree(child);
      } else if (stream === "stdout") {
        stdoutBytes = next;
        if (options.captureStdout) stdout.push(Buffer.from(chunk));
      } else {
        stderrBytes = next;
      }
    }
    child.stdout.on("data", (chunk) => capture(chunk, "stdout"));
    child.stderr.on("data", (chunk) => capture(chunk, "stderr"));
    child.on("error", (error) => {
      terminalReason ??= `spawn-${error.code ?? "failed"}`;
    });
    const timer = setTimeout(() => {
      terminalReason = "timeout";
      terminateTree(child);
    }, options.timeoutMs);
    child.on("close", (exitCode) => {
      clearTimeout(timer);
      if (terminalReason) {
        reject(new Error(`${options.label}-process-${terminalReason}`));
      } else if (exitCode !== 0) {
        reject(new Error(`${options.label}-process-exit-${exitCode}`));
      } else {
        resolve({
          exitCode,
          stdoutBytes,
          stderrBytes,
          stdout: options.captureStdout ? Buffer.concat(stdout) : undefined,
        });
      }
    });
  });
}

async function toolRecord(name, tool, maximumBytes) {
  const value = await hashBoundedFile(tool.identityPath, maximumBytes);
  return { name, path: tool.executionPath, sha256: value.sha256 };
}

export async function resolvePackageBuildTools(options, frozen) {
  const maximum = frozen.package.observerBounds.executableArtifactBytes;
  if (
    !path.isAbsolute(options.rustToolchainRoot ?? "") ||
    path.basename(path.resolve(options.rustToolchainRoot)) !==
      frozen.package.authenticatedBuild.rust.toolchainDirectoryName
  ) {
    throw new Error("rust-toolchain-root-invalid");
  }
  const rustRoot = await realpath(options.rustToolchainRoot);
  if (!(await stat(rustRoot)).isDirectory()) {
    throw new Error("rust-toolchain-root-invalid");
  }
  const cargo = await requiredAbsoluteFile(
    path.join(rustRoot, "bin", "cargo.exe"),
    "cargo",
  );
  const rustc = await requiredAbsoluteFile(
    path.join(rustRoot, "bin", "rustc.exe"),
    "rustc",
  );
  const node = await requiredAbsoluteFile(options.node, "node");
  const git = await requiredAbsoluteFile(options.git, "git");
  const tauriJs = await requiredAbsoluteFile(
    path.join(
      options.sourceRoot,
      "node_modules",
      "@tauri-apps",
      "cli",
      "tauri.js",
    ),
    "tauri-cli-js",
  );
  const tauriNative = await requiredAbsoluteFile(
    path.join(
      options.sourceRoot,
      "node_modules",
      "@tauri-apps",
      "cli-win32-x64-msvc",
      "cli.win32-x64-msvc.node",
    ),
    "tauri-cli-native",
  );
  const records = await Promise.all([
    toolRecord("cargo", cargo, maximum),
    toolRecord("rustc", rustc, maximum),
    toolRecord("node", node, maximum),
    toolRecord("git", git, maximum),
    toolRecord("tauri-cli-js", tauriJs, maximum),
    toolRecord("tauri-cli-native", tauriNative, maximum),
  ]);
  return Object.fromEntries(records.map((record) => [record.name, record]));
}

function buildEnvironment(tools, targetDirectory) {
  if (typeof tools.createCargoEnvironment !== "function") {
    throw new Error("authenticated-package-environment-missing");
  }
  return tools.createCargoEnvironment(targetDirectory);
}

export async function buildDiagnosticFixture({
  sourceRoot,
  targetDirectory,
  tools,
  frozen,
}) {
  const spec = frozen.package.fixtureExclusion;
  await mkdir(targetDirectory, { recursive: false });
  await runBounded(
    tools.cargo.path,
    [
      "build",
      "--quiet",
      "--locked",
      "--offline",
      "--manifest-path",
      path.join(sourceRoot, spec.cargoManifest),
      "--release",
      "--features",
      spec.canonicalFeature,
      "--bin",
      spec.binaryName,
      "--target-dir",
      targetDirectory,
    ],
    {
      cwd: sourceRoot,
      env: buildEnvironment(tools, targetDirectory),
      timeoutMs: buildTimeoutMs,
      label: "fixture-build",
      stdoutMaximum: frozen.package.observerBounds.toolStdoutBytes,
      stderrMaximum: frozen.package.observerBounds.toolStderrBytes,
    },
  );
  return path.join(targetDirectory, spec.executableRelativePath);
}

export async function buildCandidateCharacterization({
  sourceRoot,
  targetDirectory,
  tools,
  frozen,
}) {
  const spec = frozen.package.characterizationBuild;
  const profile = frozen.package.participants[spec.participant];
  await mkdir(targetDirectory, { recursive: false });
  await runBounded(
    tools.cargo.path,
    [
      "build",
      "--quiet",
      "--locked",
      "--offline",
      "--manifest-path",
      path.join(sourceRoot, profile.cargoManifest),
      "--release",
      "--features",
      spec.enabledFeatures.join(","),
      "--bin",
      spec.binaryName,
      "--target-dir",
      targetDirectory,
    ],
    {
      cwd: sourceRoot,
      env: buildEnvironment(tools, targetDirectory),
      timeoutMs: buildTimeoutMs,
      label: "candidate-characterization-build",
      stdoutMaximum: frozen.package.observerBounds.toolStdoutBytes,
      stderrMaximum: frozen.package.observerBounds.toolStderrBytes,
    },
  );
  return targetDirectory;
}

export async function buildProductionPackage({
  sourceRoot,
  participant,
  targetDirectory,
  tools,
  frozen,
}) {
  const profile = frozen.package.participants[participant];
  const applicationRoot = path.join(sourceRoot, profile.applicationRoot);
  await mkdir(targetDirectory, { recursive: false });
  await tools.prepareBundlers(targetDirectory);
  const args = [
    tools["tauri-cli-js"].path,
    "build",
    "--ci",
    "--no-sign",
    "--runner",
    tools.cargo.path,
    "--bundles",
    frozen.package.productionBuild.bundleTargets.join(","),
    "--config",
    path.join(sourceRoot, profile.configuration.platformOverlay),
    "--config",
    JSON.stringify({
      build: { beforeBuildCommand: null, beforeBundleCommand: null },
      bundle: { useLocalToolsDir: true },
    }),
  ];
  if (frozen.package.productionBuild.enabledFeatures.length > 0) {
    args.push(
      "--features",
      frozen.package.productionBuild.enabledFeatures.join(","),
    );
  }
  args.push("--", "--locked", "--offline");
  await runBounded(tools.node.path, args, {
    cwd: applicationRoot,
    env: buildEnvironment(tools, targetDirectory),
    timeoutMs: buildTimeoutMs,
    label: `${participant}-production-build`,
    stdoutMaximum: frozen.package.observerBounds.toolStdoutBytes,
    stderrMaximum: frozen.package.observerBounds.toolStderrBytes,
  });
  await tools.verifyBundlers(targetDirectory);
  return targetDirectory;
}

async function observeBuildFingerprint({
  participant,
  targetDirectory,
  frozen,
  expected,
  binaryName,
  label,
}) {
  const profile = frozen.package.participants[participant];
  const fingerprintRoot = path.join(targetDirectory, "release", ".fingerprint");
  const directories = await readdir(fingerprintRoot, { withFileTypes: true });
  if (directories.length > frozen.package.observerBounds.membersPerInstaller) {
    throw new Error("cargo-fingerprint-directory-count-oversize");
  }
  const matches = [];
  for (const directory of directories) {
    if (
      !directory.isDirectory() ||
      !directory.name.startsWith(`${profile.cargoPackageName}-`)
    ) {
      continue;
    }
    const directoryPath = path.join(fingerprintRoot, directory.name);
    const entries = await readdir(directoryPath, { withFileTypes: true });
    if (entries.length > 32) {
      throw new Error("cargo-fingerprint-entry-count-oversize");
    }
    const filename = `bin-${binaryName}.json`;
    if (entries.some((entry) => entry.isFile() && entry.name === filename)) {
      matches.push(path.join(directoryPath, filename));
    }
  }
  if (matches.length !== 1) {
    throw new Error(`cargo-${label}-fingerprint-not-unique`);
  }
  const record = parseBoundedJson(
    matches[0],
    1024 * 1024,
    frozen.limits.wire,
    `${participant} Cargo ${label} fingerprint`,
  );
  if (typeof record.value.features !== "string") {
    throw new Error(`cargo-${label}-fingerprint-features-invalid`);
  }
  const rawFeatures = JSON.parse(record.value.features);
  const defaultIndex = rawFeatures.indexOf("default");
  const defaultFeaturesEnabled = defaultIndex >= 0;
  const enabledFeatures = rawFeatures.filter(
    (feature) => feature !== "default",
  );
  if (
    defaultFeaturesEnabled !== expected.defaultFeaturesEnabled ||
    JSON.stringify(enabledFeatures) !== JSON.stringify(expected.enabledFeatures)
  ) {
    throw new Error(`cargo-${label}-fingerprint-features-mismatch`);
  }
  return {
    profile: expected.profile,
    defaultFeaturesEnabled,
    enabledFeatures,
    fingerprintSha256: record.sha256,
  };
}

export async function observeProductionBuild({
  participant,
  targetDirectory,
  frozen,
}) {
  return observeBuildFingerprint({
    participant,
    targetDirectory,
    frozen,
    expected: frozen.package.productionBuild,
    binaryName: frozen.package.participants[participant].cargoBinaryName,
    label: "production",
  });
}

export async function observeCandidateCharacterization({
  targetDirectory,
  frozen,
}) {
  const spec = frozen.package.characterizationBuild;
  const build = await observeBuildFingerprint({
    participant: spec.participant,
    targetDirectory,
    frozen,
    expected: spec,
    binaryName: spec.binaryName,
    label: "characterization",
  });
  const executable = await hashBoundedFile(
    path.join(targetDirectory, spec.executableRelativePath),
    frozen.package.observerBounds.executableArtifactBytes,
  );
  return {
    executablePath: path.join(targetDirectory, spec.executableRelativePath),
    summary: {
      executableSha256: executable.sha256,
      executableBytes: executable.bytes,
      featureSet: build.enabledFeatures,
      defaultFeaturesEnabled: build.defaultFeaturesEnabled,
      fingerprintSha256: build.fingerprintSha256,
    },
  };
}
