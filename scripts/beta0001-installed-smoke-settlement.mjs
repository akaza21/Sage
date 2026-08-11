import { createHmac, timingSafeEqual } from "node:crypto";
import { lstat, realpath } from "node:fs/promises";
import path from "node:path";

import { exactKeys, parseBoundedJson } from "./ade0006a-c0b-common.mjs";
import { runBounded } from "./ade0006a-package-build.mjs";
import { hashBoundedFile } from "./ade0006a-package-members.mjs";
import { INSTALLED_SMOKE_SETTLEMENT_RELATIVE_PATHS } from "./beta0001-installed-smoke-source-closure.mjs";
import { assertInstalledSmokeOutputAuthority } from "./beta0001-installed-smoke-output-authority.mjs";
import {
  INSTALLED_SMOKE_LOCKED_BOOTSTRAP_MAXIMUM_BYTES,
  lockedPowerShellArguments,
  serializeLockedPowerShellRequest,
} from "./beta0001-windows-locked-loader.mjs";

const executableMaximumBytes = 512 * 1024 * 1024;
const runnerMaximumBytes = 2 * 1024 * 1024;
const authorityMaximumBytes = 16 * 1024;
const settlementTimeoutMs = 2 * 60 * 1000;
const jsonLimits = Object.freeze({
  jsonDepth: 5,
  jsonNodes: 64,
  arrayEntries: 8,
  objectMembers: 8,
  decodedKeyBytes: 64,
});

async function existingItem(value) {
  try {
    return await lstat(value);
  } catch (error) {
    if (error?.code === "ENOENT") return null;
    throw error;
  }
}

async function assertPlainDirectoryChain(value, label) {
  const resolved = path.resolve(value);
  const volumeRoot = path.parse(resolved).root;
  if (!volumeRoot || resolved === volumeRoot) {
    throw new Error(`${label}-root-ambiguous`);
  }
  let current = volumeRoot;
  for (const segment of path
    .relative(volumeRoot, resolved)
    .split(path.sep)
    .filter(Boolean)) {
    current = path.join(current, segment);
    const item = await lstat(current);
    if (!item.isDirectory() || item.isSymbolicLink()) {
      throw new Error(`${label}-path-invalid`);
    }
  }
  return resolved;
}

async function exactIdentity(filePath, expected, maximumBytes, label) {
  if (!path.isAbsolute(filePath ?? ""))
    throw new Error(`${label}-path-invalid`);
  await assertPlainDirectoryChain(path.dirname(filePath), label);
  const observed = await hashBoundedFile(filePath, maximumBytes);
  if (
    observed.sha256 !== expected?.sha256 ||
    observed.bytes !== expected?.bytes
  ) {
    throw new Error(`${label}-identity-changed`);
  }
  return { path: await realpath(filePath), ...observed };
}

async function authenticateSettlementClosure(sourceRoot, sourceManifest) {
  const identities = [];
  for (const relativePath of INSTALLED_SMOKE_SETTLEMENT_RELATIVE_PATHS) {
    const expected = sourceManifest?.files?.find(
      (entry) => entry?.path === relativePath,
    );
    if (!expected)
      throw new Error("watchdog-settlement-manifest-entry-missing");
    const observed = await exactIdentity(
      path.join(sourceRoot, ...relativePath.split("/")),
      expected,
      runnerMaximumBytes,
      "watchdog-settlement-runner",
    );
    identities.push({
      relativePath,
      sha256: observed.sha256,
      bytes: observed.bytes,
    });
  }
  return identities;
}

function authorityAuthentication(value, cleanupKey) {
  const uninstaller = value?.uninstaller;
  const canonical = [
    `schemaVersion=${value?.schemaVersion}`,
    `campaignId=${value?.campaignId}`,
    `state=${value?.state}`,
    `installRootLabel=${value?.installRootLabel}`,
    `relativePath=${uninstaller?.relativePath ?? ""}`,
    `sha256=${uninstaller?.sha256 ?? ""}`,
    `bytes=${uninstaller?.bytes ?? ""}`,
    "",
  ].join("\n");
  return createHmac("sha256", cleanupKey).update(canonical).digest();
}

function validateAuthority(value, campaignId, cleanupKey) {
  const errors = [];
  exactKeys(
    errors,
    value,
    [
      "schemaVersion",
      "campaignId",
      "state",
      "installRootLabel",
      "uninstaller",
      "authenticationSha256",
    ],
    "cleanup authority",
  );
  exactKeys(
    errors,
    value?.uninstaller,
    ["relativePath", "sha256", "bytes"],
    "cleanup authority uninstaller",
  );
  if (
    value?.schemaVersion !== 1 ||
    value?.campaignId !== campaignId ||
    value?.state !== "installed" ||
    value?.installRootLabel !== "installed" ||
    value?.uninstaller?.relativePath !== "installed/uninstall.exe" ||
    !/^[0-9a-f]{64}$/u.test(value?.uninstaller?.sha256 ?? "") ||
    !Number.isSafeInteger(value?.uninstaller?.bytes) ||
    value.uninstaller.bytes < 1 ||
    value.uninstaller.bytes > executableMaximumBytes ||
    !/^[0-9a-f]{64}$/u.test(value?.authenticationSha256 ?? "")
  ) {
    errors.push("cleanup authority value is invalid");
  }
  if (errors.length === 0) {
    const observed = Buffer.from(value.authenticationSha256, "hex");
    const expected = authorityAuthentication(value, cleanupKey);
    if (!timingSafeEqual(observed, expected)) {
      errors.push("cleanup authority authentication is invalid");
    }
  }
  if (errors.length > 0) throw new Error(errors.join(","));
}

async function inspectResidue(outputAuthority) {
  const outputDirectory = await assertInstalledSmokeOutputAuthority(
    outputAuthority,
    "watchdog-quarantine-authority",
  );
  const installRoot = path.join(outputDirectory, "installed");
  const installItem = await existingItem(installRoot);
  if (installItem) {
    if (!installItem.isDirectory() || installItem.isSymbolicLink()) {
      throw new Error("watchdog-installed-residue-invalid");
    }
    return "installed";
  }
  const authorityPath = path.join(outputDirectory, "cleanup-authority.json");
  const authorityItem = await existingItem(authorityPath);
  if (authorityItem) {
    if (!authorityItem.isFile() || authorityItem.isSymbolicLink()) {
      throw new Error("watchdog-authority-residue-invalid");
    }
    return "cleanup-authority.json";
  }
  const quarantineRoot = path.join(outputDirectory, "quarantine-installed");
  const quarantineItem = await existingItem(quarantineRoot);
  if (quarantineItem) {
    if (!quarantineItem.isDirectory() || quarantineItem.isSymbolicLink()) {
      throw new Error("watchdog-quarantine-residue-invalid");
    }
    return "quarantine-installed";
  }
  return null;
}

function residueStatus(relativePath) {
  if (!relativePath) return "absent";
  return relativePath === "quarantine-installed" ? "quarantined" : "retained";
}

export async function settleInstalledSmokeWatchdog({
  campaignId,
  outputDirectory,
  sourceRoot,
  sourceManifest,
  powershell,
  cleanupKey,
  outerJobConverged,
  outputAuthority,
  runProcess = runBounded,
}) {
  let authoritySha256 = null;
  let outputRoot = null;
  try {
    if (
      !/^[a-z0-9][a-z0-9-]{0,63}$/u.test(campaignId ?? "") ||
      !path.isAbsolute(outputDirectory ?? "") ||
      !path.isAbsolute(sourceRoot ?? "") ||
      !Buffer.isBuffer(cleanupKey) ||
      cleanupKey.byteLength !== 32 ||
      typeof outerJobConverged !== "boolean"
    ) {
      throw new Error("watchdog-settlement-request-invalid");
    }
    const sealedOutputRoot = await assertInstalledSmokeOutputAuthority(
      outputAuthority,
      "watchdog-settlement-output-authority",
    );
    if (
      sealedOutputRoot.toLowerCase() !==
      path.resolve(outputDirectory).toLowerCase()
    ) {
      throw new Error("watchdog-settlement-output-authority-mismatch");
    }
    const outputItem = await lstat(sealedOutputRoot);
    const authenticatedOutputRoot = await realpath(
      await assertPlainDirectoryChain(sealedOutputRoot, "watchdog-settlement"),
    );
    if (
      !outputItem.isDirectory() ||
      outputItem.isSymbolicLink() ||
      authenticatedOutputRoot.toLowerCase() !==
        path.resolve(outputDirectory).toLowerCase()
    ) {
      throw new Error("watchdog-settlement-output-invalid");
    }
    outputRoot = authenticatedOutputRoot;
    if (outerJobConverged !== true) {
      const residueRelativePath = await inspectResidue(outputAuthority);
      return {
        status: residueStatus(residueRelativePath),
        authoritySha256,
        residueRelativePath,
        outerJobConverged: outerJobConverged === true,
        fallbackJobConverged: false,
      };
    }
    const authorityPath = path.join(outputRoot, "cleanup-authority.json");
    await assertInstalledSmokeOutputAuthority(
      outputAuthority,
      "watchdog-authority-read",
    );
    if (!(await existingItem(authorityPath))) {
      const residueRelativePath = await inspectResidue(outputAuthority);
      return {
        status: residueStatus(residueRelativePath),
        authoritySha256,
        residueRelativePath,
        outerJobConverged: true,
        fallbackJobConverged: false,
      };
    }
    const authorityIdentity = await hashBoundedFile(
      authorityPath,
      authorityMaximumBytes,
    );
    authoritySha256 = authorityIdentity.sha256;
    const { value: authority } = parseBoundedJson(
      authorityPath,
      authorityMaximumBytes,
      jsonLimits,
      "installed smoke cleanup authority",
    );
    validateAuthority(authority, campaignId, cleanupKey);
    await assertInstalledSmokeOutputAuthority(
      outputAuthority,
      "watchdog-uninstaller-read",
    );
    await exactIdentity(
      path.join(outputRoot, "installed", "uninstall.exe"),
      authority.uninstaller,
      executableMaximumBytes,
      "watchdog-uninstaller",
    );
    const [powerShellIdentity, runnerIdentities] = await Promise.all([
      exactIdentity(
        powershell?.path,
        powershell,
        executableMaximumBytes,
        "watchdog-powershell",
      ),
      authenticateSettlementClosure(await realpath(sourceRoot), sourceManifest),
    ]);
    const resultPath = path.join(outputRoot, "fallback-cleanup.json");
    const runnerBase64 = Buffer.from(JSON.stringify(runnerIdentities)).toString(
      "base64",
    );
    const fallbackBootstrap = serializeLockedPowerShellRequest({
      sourceRoot,
      entryRelativePath: "scripts/settle-windows-installed-smoke.ps1",
      identities: runnerIdentities,
      parameters: {
        CampaignId: campaignId,
        OutputDirectory: outputRoot,
        AuthenticatedSourceRoot: sourceRoot,
        CleanupKeyBase64: cleanupKey.toString("base64"),
        AuthoritySha256: authoritySha256,
        PowerShellSha256: powerShellIdentity.sha256,
        PowerShellBytes: powerShellIdentity.bytes,
        RunnerFileIdentitiesBase64: runnerBase64,
      },
    });
    await assertInstalledSmokeOutputAuthority(
      outputAuthority,
      "watchdog-fallback-launch",
    );
    await runProcess(powerShellIdentity.path, lockedPowerShellArguments(), {
      cwd: sourceRoot,
      env: process.env,
      timeoutMs: settlementTimeoutMs,
      label: "installed-smoke-watchdog-settlement",
      stdoutMaximum: 16 * 1024,
      stderrMaximum: 16 * 1024,
      captureStdout: false,
      stdin: fallbackBootstrap,
      stdinMaximum: INSTALLED_SMOKE_LOCKED_BOOTSTRAP_MAXIMUM_BYTES,
    });
    await assertInstalledSmokeOutputAuthority(
      outputAuthority,
      "watchdog-fallback-result-read",
    );
    const { value: result } = parseBoundedJson(
      resultPath,
      authorityMaximumBytes,
      jsonLimits,
      "installed smoke fallback result",
    );
    const resultErrors = [];
    exactKeys(
      resultErrors,
      result,
      [
        "schemaVersion",
        "campaignId",
        "status",
        "authoritySha256",
        "jobConverged",
      ],
      "fallback cleanup result",
    );
    if (
      result?.schemaVersion !== 1 ||
      result?.campaignId !== campaignId ||
      result?.status !== "uninstalled" ||
      result?.authoritySha256 !== authoritySha256 ||
      result?.jobConverged !== true ||
      (await existingItem(path.join(outputRoot, "installed"))) ||
      (await existingItem(authorityPath))
    ) {
      resultErrors.push("fallback cleanup result is invalid");
    }
    if (resultErrors.length > 0) throw new Error(resultErrors.join(","));
    return {
      status: "uninstalled",
      authoritySha256,
      residueRelativePath: null,
      outerJobConverged: true,
      fallbackJobConverged: true,
    };
  } catch {
    if (!outputRoot) {
      return {
        status: "unresolved",
        authoritySha256,
        residueRelativePath: null,
        outerJobConverged: outerJobConverged === true,
        fallbackJobConverged: false,
      };
    }
    let residueRelativePath;
    try {
      residueRelativePath = await inspectResidue(outputAuthority);
    } catch {
      return {
        status: "unresolved",
        authoritySha256,
        residueRelativePath: null,
        outerJobConverged: outerJobConverged === true,
        fallbackJobConverged: false,
      };
    }
    return {
      status: residueStatus(residueRelativePath),
      authoritySha256,
      residueRelativePath,
      outerJobConverged: outerJobConverged === true,
      fallbackJobConverged: false,
    };
  }
}

export const INSTALLED_SMOKE_SETTLEMENT_TIMEOUT_MS = settlementTimeoutMs;
