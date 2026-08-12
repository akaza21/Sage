import { existsSync, realpathSync, statSync } from "node:fs";
import path from "node:path";

import { executableName } from "./check-process.mjs";

export const VALIDATION_DEADLINES_MS = Object.freeze({
  devLane: 15 * 60_000,
  integrationFrontendLane: 30 * 60_000,
  integrationRustLane: 60 * 60_000,
  releaseLane: 150 * 60_000,
});

export const PROCESS_LIMITS = Object.freeze({
  terminationGraceMs: 3_000,
  settlementTimeoutMs: 10_000,
  maximumLineBytes: 16 * 1_024,
  maximumOutputBytes: 16 * 1_024 * 1_024,
});

export const COMMAND_DEADLINES_MS = Object.freeze({
  format: 5 * 60_000,
  lint: 15 * 60_000,
  frontendTests: 30 * 60_000,
  budgets: 30 * 60_000,
  build: 20 * 60_000,
  policy: 15 * 60_000,
  rustFormat: 10 * 60_000,
  rustClippy: 45 * 60_000,
  rustTests: 45 * 60_000,
  focusedTest: 10 * 60_000,
  frontendCheck: 45 * 60_000,
  packageCampaign: 90 * 60_000,
});

export function boundedCommand(command, timeoutMs) {
  return { ...command, timeoutMs, ...PROCESS_LIMITS };
}

export function resolveNpmInvocation({
  platform = process.platform,
  nodeExecutable = process.execPath,
} = {}) {
  if (platform !== "win32") {
    return { command: executableName("npm", platform), argsPrefix: [] };
  }
  const nodePath = realpathSync(nodeExecutable);
  const installationRoot = realpathSync(path.dirname(nodePath));
  const cliCandidate = path.join(
    installationRoot,
    "node_modules",
    "npm",
    "bin",
    "npm-cli.js",
  );
  if (!existsSync(cliCandidate) || !statSync(cliCandidate).isFile()) {
    throw new Error("validated Windows npm-cli.js was not found beside Node");
  }
  const npmCli = realpathSync(cliCandidate);
  const relative = path.relative(installationRoot, npmCli);
  if (
    relative.startsWith("..") ||
    path.isAbsolute(relative) ||
    path.basename(npmCli).toLowerCase() !== "npm-cli.js"
  ) {
    throw new Error("Windows npm-cli.js escaped the Node installation");
  }
  return { command: nodePath, argsPrefix: [npmCli] };
}

export function npmCommand({
  label,
  npmArguments,
  repositoryRoot,
  timeoutMs,
  npmInvocation = resolveNpmInvocation(),
}) {
  return boundedCommand(
    {
      label,
      command: npmInvocation.command,
      args: [...npmInvocation.argsPrefix, ...npmArguments],
      cwd: repositoryRoot,
    },
    timeoutMs,
  );
}

export function createRustValidationCommands({
  labelPrefix,
  platform = process.platform,
  repositoryRoot,
  rustEnvironment,
}) {
  const cargo = executableName("cargo", platform);
  return [
    boundedCommand(
      {
        label: `${labelPrefix}:rust-format`,
        command: cargo,
        args: [
          "fmt",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "--",
          "--check",
        ],
        cwd: repositoryRoot,
        env: rustEnvironment,
      },
      COMMAND_DEADLINES_MS.rustFormat,
    ),
    boundedCommand(
      {
        label: `${labelPrefix}:rust-clippy`,
        command: cargo,
        args: [
          "clippy",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "--all-targets",
          "--all-features",
          "--",
          "-D",
          "warnings",
        ],
        cwd: repositoryRoot,
        env: rustEnvironment,
      },
      COMMAND_DEADLINES_MS.rustClippy,
    ),
    boundedCommand(
      {
        label: `${labelPrefix}:rust-tests`,
        command: cargo,
        args: [
          "test",
          "--manifest-path",
          "src-tauri/Cargo.toml",
          "--all-features",
          "--",
          "--test-threads=1",
        ],
        cwd: repositoryRoot,
        env: rustEnvironment,
      },
      COMMAND_DEADLINES_MS.rustTests,
    ),
  ];
}
