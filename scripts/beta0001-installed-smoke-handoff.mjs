import { lstat, realpath, stat } from "node:fs/promises";
import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import path from "node:path";

import {
  readBoundedFile,
  sha256,
  validateJsonShape,
} from "./ade0006a-c0b-common.mjs";
import { runBounded } from "./ade0006a-package-build.mjs";
import { hashBoundedFile } from "./ade0006a-package-members.mjs";
import { validateInstalledSmokeReceipt } from "./beta0001-installed-smoke.mjs";
import {
  INSTALLED_SMOKE_SETTLEMENT_TIMEOUT_MS,
  settleInstalledSmokeWatchdog,
} from "./beta0001-installed-smoke-settlement.mjs";
import { INSTALLED_SMOKE_RUNNER_RELATIVE_PATHS } from "./beta0001-installed-smoke-source-closure.mjs";
import {
  acquireInstalledSmokeOutputAuthority,
  assertInstalledSmokeOutputAuthority,
  releaseInstalledSmokeOutputAuthority,
  writeInstalledSmokeOutputFile,
} from "./beta0001-installed-smoke-output-authority.mjs";
import {
  createInstalledSmokeControllerInvocation,
  INSTALLED_SMOKE_CAMPAIGN_JOB_TIMEOUT_MS,
  installedSmokeFailure,
  installedSmokeProcessFailureCode,
  parseInstalledSmokeControllerProof,
} from "./beta0001-installed-smoke-controller.mjs";

const receiptMaximumBytes = 256 * 1024;
const smokeTimeoutMs = 30 * 60 * 1000;
const campaignDeadlineMs = 20 * 60 * 1000;
const executableMaximumBytes = 512 * 1024 * 1024;
const runnerMaximumBytes = 2 * 1024 * 1024;
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

async function existingItem(value) {
  try {
    return await lstat(value);
  } catch (error) {
    if (error?.code === "ENOENT") return null;
    throw error;
  }
}

export async function runAuthenticatedInstalledSmoke({
  handoff,
  campaignId,
  outputDirectory,
  expectedSignerThumbprint,
  runProcess = runBounded,
  runFallbackProcess = runBounded,
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
  const powershell = handoff?.tools?.powershell;
  const sourceManifest = handoff?.sourceManifest;
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
    !powershell ||
    typeof powershell.path !== "string" ||
    typeof powershell.sha256 !== "string" ||
    !Number.isSafeInteger(powershell.bytes) ||
    powershell.sha256 !==
      handoff?.receipt?.buildIdentity?.powershell?.executableSha256 ||
    typeof handoff?.sourceRoot !== "string" ||
    !Array.isArray(sourceManifest?.files) ||
    !/^[0-9a-f]{40}$/u.test(expectedSignerThumbprint ?? "")
  ) {
    throw new Error("installed-smoke-authenticated-nsis-binding-invalid");
  }
  const sourceRoot = await realpath(handoff.sourceRoot);
  const [powerShellIdentity, nodeIdentity, gitIdentity] = await Promise.all([
    observedIdentity(
      powershell.path,
      "installed-smoke-powershell",
      executableMaximumBytes,
      powershell,
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
    INSTALLED_SMOKE_RUNNER_RELATIVE_PATHS.map(async (relativePath) => {
      const expected = sourceManifest.files.find(
        (entry) => entry?.path === relativePath,
      );
      if (
        !expected ||
        typeof expected.sha256 !== "string" ||
        !Number.isSafeInteger(expected.bytes)
      ) {
        throw new Error("installed-smoke-runner-manifest-entry-missing");
      }
      return {
        relativePath,
        ...(await observedIdentity(
          path.join(sourceRoot, ...relativePath.split("/")),
          "installed-smoke-runner",
          runnerMaximumBytes,
          expected,
        )),
      };
    }),
  );
  const encodedRunnerIdentities = Buffer.from(
    JSON.stringify(
      runnerIdentities.map(({ relativePath, sha256: hash, bytes }) => ({
        relativePath,
        sha256: hash,
        bytes,
      })),
    ),
  ).toString("base64");
  const outputAuthority = await acquireInstalledSmokeOutputAuthority({
    outputDirectory,
    powerShell: powerShellIdentity,
    create: true,
  });
  const output = outputAuthority.root;
  try {
    const cleanupKey = randomBytes(32);
    const receiptPath = path.join(output, "receipt.json");
    const packageReceiptSha256 = sha256(handoff.receiptBytes);
    const mainParameters = {
      InstallerPath: installer.path,
      InstallerSha256: installer.sha256,
      InstallerBytes: installer.bytes,
      ExpectedInstalledExecutableSha256: installedExecutable.sha256,
      ExpectedInstalledExecutableBytes: installedExecutable.bytes,
      ExpectedSignerThumbprint: expectedSignerThumbprint,
      PackageReceiptSha256: packageReceiptSha256,
      CampaignId: campaignId,
      OutputDirectory: output,
      AuthenticatedSourceRoot: sourceRoot,
      CleanupKeyBase64: cleanupKey.toString("base64"),
      PowerShellSha256: powerShellIdentity.sha256,
      PowerShellBytes: powerShellIdentity.bytes,
      CampaignDeadlineMilliseconds: campaignDeadlineMs,
      NodePath: nodeIdentity.path,
      NodeSha256: nodeIdentity.sha256,
      NodeBytes: nodeIdentity.bytes,
      GitPath: gitIdentity.path,
      GitSha256: gitIdentity.sha256,
      GitBytes: gitIdentity.bytes,
      RunnerFileIdentitiesBase64: encodedRunnerIdentities,
    };
    const controller = createInstalledSmokeControllerInvocation({
      campaignId,
      sourceRoot,
      runnerIdentities,
      powerShellIdentity,
      mainParameters,
    });
    let outerJob = null;
    try {
      await assertInstalledSmokeOutputAuthority(
        outputAuthority,
        "installed-smoke-launch-output-authority",
      );
      const processResult = await runProcess(
        powerShellIdentity.path,
        controller.args,
        {
          cwd: sourceRoot,
          env: process.env,
          timeoutMs: smokeTimeoutMs,
          label: "installed-smoke",
          stdoutMaximum: 16 * 1024,
          stderrMaximum: 64 * 1024,
          captureStdout: true,
          stdin: controller.stdin,
          stdinMaximum: controller.stdinMaximum,
        },
      );
      outerJob = parseInstalledSmokeControllerProof(
        processResult.stdout,
        campaignId,
      );
      if (outerJob.status !== "completed" || outerJob.childExitCode !== 0) {
        throw installedSmokeFailure(
          outerJob.status === "timeout"
            ? "orchestrator-timeout"
            : "orchestrator-exit-nonzero",
          `installed-smoke-controller-${outerJob.status}`,
        );
      }
      let receipt;
      try {
        await assertInstalledSmokeOutputAuthority(
          outputAuthority,
          "installed-smoke-receipt-output-authority",
        );
        const receiptBuffer = readBoundedFile(
          receiptPath,
          receiptMaximumBytes,
          "installed smoke receipt",
        );
        const observedSha256 = sha256(receiptBuffer);
        const observedAuthentication = createHmac("sha256", cleanupKey)
          .update(receiptBuffer)
          .digest();
        const expectedAuthentication = Buffer.from(
          outerJob.receiptAuthenticationSha256 ?? "",
          "hex",
        );
        if (
          observedSha256 !== outerJob.receiptSha256 ||
          expectedAuthentication.byteLength !==
            observedAuthentication.byteLength ||
          !timingSafeEqual(expectedAuthentication, observedAuthentication)
        ) {
          throw new Error("controller receipt authentication changed");
        }
        receipt = JSON.parse(receiptBuffer);
        validateJsonShape(receipt, jsonLimits, "installed smoke receipt");
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
          errors.push(
            "installed smoke receipt is not bound to its package handoff",
          );
        }
        if (errors.length > 0) throw new Error(errors.join(","));
      } catch (error) {
        throw installedSmokeFailure(
          "receipt-invalid",
          `installed-smoke-receipt-invalid:${error?.message ?? error}`,
        );
      }
      if (
        (await existingItem(path.join(output, "installed"))) ||
        (await existingItem(path.join(output, "cleanup-authority.json")))
      ) {
        throw installedSmokeFailure(
          "receipt-cleanup-unproven",
          "installed-smoke-receipt-cleanup-unproven",
        );
      }
      return receipt;
    } catch (error) {
      const code = installedSmokeProcessFailureCode(error);
      const cleanup = await settleInstalledSmokeWatchdog({
        campaignId,
        outputDirectory: output,
        sourceRoot,
        sourceManifest,
        powershell: powerShellIdentity,
        cleanupKey,
        outerJobConverged: outerJob?.jobConverged === true,
        outputAuthority,
        runProcess: runFallbackProcess,
      });
      const cleanupFailed = !["absent", "uninstalled"].includes(cleanup.status);
      const settlement = {
        schemaVersion: 1,
        workItem: "BETA-0001",
        campaignId,
        result: "failed",
        failure: {
          code: cleanupFailed ? "cleanup-failed" : code,
          phase: cleanupFailed ? "fallback-cleanup" : "orchestrator-watchdog",
        },
        cleanup,
        outerJob,
      };
      if (cleanup.status !== "unresolved") {
        await writeInstalledSmokeOutputFile(
          outputAuthority,
          "orchestrator-failure.json",
          `${JSON.stringify(settlement)}\n`,
          { flag: "wx" },
        );
      }
      return settlement;
    }
  } finally {
    await releaseInstalledSmokeOutputAuthority(outputAuthority);
  }
}

export const INSTALLED_SMOKE_RECEIPT_MAXIMUM_BYTES = receiptMaximumBytes;
export const INSTALLED_SMOKE_TIMEOUT_MS = smokeTimeoutMs;
export const INSTALLED_SMOKE_CAMPAIGN_DEADLINE_MS = campaignDeadlineMs;
export { INSTALLED_SMOKE_CAMPAIGN_JOB_TIMEOUT_MS };
export const INSTALLED_SMOKE_SETTLEMENT_RESERVE_MS =
  smokeTimeoutMs - campaignDeadlineMs;
if (
  INSTALLED_SMOKE_SETTLEMENT_RESERVE_MS <= INSTALLED_SMOKE_SETTLEMENT_TIMEOUT_MS
) {
  throw new Error("installed-smoke-settlement-reserve-invalid");
}
