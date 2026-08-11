import { lstat, mkdir, realpath, stat, writeFile } from "node:fs/promises";
import path from "node:path";

import { parseBoundedJson, sha256 } from "./ade0006a-c0b-common.mjs";
import { runBounded } from "./ade0006a-package-build.mjs";
import { hashBoundedFile } from "./ade0006a-package-members.mjs";
import { validateInstalledSmokeReceipt } from "./beta0001-installed-smoke.mjs";

const receiptMaximumBytes = 256 * 1024;
const smokeTimeoutMs = 30 * 60 * 1000;
const executableMaximumBytes = 512 * 1024 * 1024;
const runnerMaximumBytes = 2 * 1024 * 1024;
const runnerRelativePaths = Object.freeze([
  "scripts/measure-windows-installed-smoke.ps1",
  "scripts/footprint/InstalledSmokeIdentity.psm1",
  "scripts/footprint/InstalledSmokePackageLifecycle.psm1",
  "scripts/footprint/InstalledSmokeSemantic.psm1",
  "scripts/footprint/DisposableProfile.psm1",
  "scripts/footprint/ScenarioEvidence.psm1",
  "scripts/footprint/WindowsFootprint.psm1",
  "scripts/footprint/WindowsJob.psm1",
  "scripts/footprint/WindowsJobCompletion.psm1",
  "scripts/footprint/ProcessIdentity.psm1",
  "scripts/footprint/bounded-node-runner.mjs",
  "scripts/footprint/drive-installed-smoke.mjs",
]);
const jsonLimits = Object.freeze({
  jsonDepth: 8,
  jsonNodes: 512,
  arrayEntries: 32,
  objectMembers: 32,
  decodedKeyBytes: 128,
});

async function requiredFile(value, label) {
  if (!path.isAbsolute(value ?? "")) throw new Error(`${label}-path-invalid`);
  const resolved = await realpath(value);
  if (!(await stat(resolved)).isFile())
    throw new Error(`${label}-path-invalid`);
  return resolved;
}

async function reserveOutput(value) {
  if (!path.isAbsolute(value ?? "")) {
    throw new Error("installed-smoke-output-path-invalid");
  }
  const resolved = path.resolve(value);
  const parent = await realpath(path.dirname(resolved));
  const item = await lstat(parent);
  if (!item.isDirectory() || item.isSymbolicLink()) {
    throw new Error("installed-smoke-output-parent-invalid");
  }
  await mkdir(resolved, { recursive: false });
  return resolved;
}

function argument(name, value) {
  return [name, value];
}

async function observedIdentity(filePath, label, maximumBytes, expected) {
  const resolved = await requiredFile(filePath, label);
  const identity = await hashBoundedFile(resolved, maximumBytes);
  if (
    expected &&
    (identity.sha256 !== expected.sha256 || identity.bytes !== expected.bytes)
  ) {
    throw new Error(`${label}-identity-changed`);
  }
  return { path: resolved, ...identity };
}

export async function runAuthenticatedInstalledSmoke({
  handoff,
  campaignId,
  outputDirectory,
  powershell,
  expectedSignerThumbprint,
  runProcess = runBounded,
}) {
  const installer = handoff?.artifacts?.candidate?.installerArtifacts?.nsis;
  const installedExecutable =
    handoff?.artifacts?.candidate?.installedExecutables?.nsis;
  const expectedInstalledExecutable =
    handoff?.receipt?.participants?.candidate?.installers?.find(
      ({ kind }) => kind === "nsis",
    )?.installedExecutable;
  const git = handoff?.tools?.git;
  const node = handoff?.tools?.node;
  if (
    !installer ||
    typeof installer.path !== "string" ||
    typeof installer.sha256 !== "string" ||
    !Number.isSafeInteger(installer.bytes) ||
    !installedExecutable ||
    installedExecutable.sha256 !== expectedInstalledExecutable?.sha256 ||
    installedExecutable.bytes !== expectedInstalledExecutable?.bytes ||
    !git ||
    typeof git.path !== "string" ||
    typeof git.sha256 !== "string" ||
    !Number.isSafeInteger(git.bytes) ||
    !node ||
    typeof node.path !== "string" ||
    typeof node.sha256 !== "string" ||
    !Number.isSafeInteger(node.bytes) ||
    typeof handoff?.sourceRoot !== "string" ||
    !/^[0-9a-f]{40}$/u.test(expectedSignerThumbprint ?? "")
  ) {
    throw new Error("installed-smoke-authenticated-nsis-binding-invalid");
  }
  const sourceRoot = await realpath(handoff.sourceRoot);
  const [powerShellIdentity, nodeIdentity, gitIdentity] = await Promise.all([
    observedIdentity(
      powershell,
      "installed-smoke-powershell",
      executableMaximumBytes,
    ),
    observedIdentity(
      node.path,
      "installed-smoke-node",
      executableMaximumBytes,
      node,
    ),
    observedIdentity(
      git.path,
      "installed-smoke-git",
      executableMaximumBytes,
      git,
    ),
  ]);
  const runnerIdentities = await Promise.all(
    runnerRelativePaths.map(async (relativePath) => ({
      relativePath,
      ...(await observedIdentity(
        path.join(sourceRoot, ...relativePath.split("/")),
        "installed-smoke-runner",
        runnerMaximumBytes,
      )),
    })),
  );
  const script = runnerIdentities[0].path;
  const encodedRunnerIdentities = Buffer.from(
    JSON.stringify(
      runnerIdentities.map(({ relativePath, sha256: hash, bytes }) => ({
        relativePath,
        sha256: hash,
        bytes,
      })),
    ),
  ).toString("base64");
  const output = await reserveOutput(outputDirectory);
  const receiptPath = path.join(output, "receipt.json");
  const packageReceiptSha256 = sha256(handoff.receiptBytes);
  const args = [
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    script,
    ...argument("-InstallerPath", installer.path),
    ...argument("-InstallerSha256", installer.sha256),
    ...argument("-InstallerBytes", String(installer.bytes)),
    ...argument(
      "-ExpectedInstalledExecutableSha256",
      installedExecutable.sha256,
    ),
    ...argument(
      "-ExpectedInstalledExecutableBytes",
      String(installedExecutable.bytes),
    ),
    ...argument("-ExpectedSignerThumbprint", expectedSignerThumbprint),
    ...argument("-PackageReceiptSha256", packageReceiptSha256),
    ...argument("-CampaignId", campaignId),
    ...argument("-OutputDirectory", output),
    ...argument("-PowerShellSha256", powerShellIdentity.sha256),
    ...argument("-PowerShellBytes", String(powerShellIdentity.bytes)),
    ...argument("-NodePath", nodeIdentity.path),
    ...argument("-NodeSha256", nodeIdentity.sha256),
    ...argument("-NodeBytes", String(nodeIdentity.bytes)),
    ...argument("-GitPath", gitIdentity.path),
    ...argument("-GitSha256", gitIdentity.sha256),
    ...argument("-GitBytes", String(gitIdentity.bytes)),
    ...argument("-RunnerFileIdentitiesBase64", encodedRunnerIdentities),
  ];
  try {
    await runProcess(powerShellIdentity.path, args, {
      cwd: sourceRoot,
      env: process.env,
      timeoutMs: smokeTimeoutMs,
      label: "installed-smoke",
      stdoutMaximum: receiptMaximumBytes,
      stderrMaximum: 64 * 1024,
      captureStdout: false,
    });
  } catch (error) {
    const message = String(error?.message ?? error);
    const code = message.includes("timeout")
      ? "orchestrator-timeout"
      : message.includes("oversize")
        ? "orchestrator-output-oversize"
        : message.includes("exit-")
          ? "orchestrator-exit-nonzero"
          : "orchestrator-process-failed";
    await writeFile(
      path.join(output, "orchestrator-failure.json"),
      `${JSON.stringify({
        schemaVersion: 1,
        campaignId,
        result: "failed",
        failure: { code, phase: "orchestrator" },
      })}\n`,
      { flag: "wx" },
    );
    throw new Error(`installed-smoke-${code}`, { cause: error });
  }
  const { value: receipt } = parseBoundedJson(
    receiptPath,
    receiptMaximumBytes,
    jsonLimits,
    "installed smoke receipt",
  );
  const errors = validateInstalledSmokeReceipt(receipt);
  if (
    receipt.campaignId !== campaignId ||
    receipt.packageReceiptSha256 !== packageReceiptSha256 ||
    receipt.installer?.kind !== "nsis" ||
    receipt.installer?.sha256 !== installer.sha256 ||
    receipt.installer?.bytes !== installer.bytes ||
    receipt.installer?.expectedInstalledExecutableSha256 !==
      installedExecutable.sha256 ||
    receipt.installer?.expectedInstalledExecutableBytes !==
      installedExecutable.bytes ||
    receipt.installer?.signerThumbprint !== expectedSignerThumbprint
  ) {
    errors.push("installed smoke receipt is not bound to its package handoff");
  }
  if (errors.length > 0) {
    throw new Error(`installed-smoke-receipt-invalid:${errors.join(",")}`);
  }
  return receipt;
}

export const INSTALLED_SMOKE_RECEIPT_MAXIMUM_BYTES = receiptMaximumBytes;
export const INSTALLED_SMOKE_TIMEOUT_MS = smokeTimeoutMs;
