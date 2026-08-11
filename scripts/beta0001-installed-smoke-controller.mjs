import {
  INSTALLED_SMOKE_LOCKED_BOOTSTRAP_MAXIMUM_BYTES,
  INSTALLED_SMOKE_LOCKED_LOADER_ENCODED_COMMAND,
  lockedPowerShellArguments,
  serializeLockedPowerShellRequest,
} from "./beta0001-windows-locked-loader.mjs";

const campaignJobTimeoutMs = 22 * 60 * 1000;

export function installedSmokeFailure(code, message) {
  const error = new Error(message);
  error.failureCode = code;
  return error;
}

export function installedSmokeProcessFailureCode(error) {
  if (typeof error?.failureCode === "string") return error.failureCode;
  const message = String(error?.message ?? error);
  if (message.includes("timeout")) return "orchestrator-timeout";
  if (message.includes("oversize")) return "orchestrator-output-oversize";
  if (message.includes("exit-")) return "orchestrator-exit-nonzero";
  return "orchestrator-process-failed";
}

export function parseInstalledSmokeControllerProof(stdout, campaignId) {
  let value;
  try {
    value = JSON.parse(stdout?.toString("utf8") ?? "");
  } catch {
    throw installedSmokeFailure(
      "orchestrator-job-proof-invalid",
      "installed-smoke-controller-proof-invalid",
    );
  }
  const keys = [
    "schemaVersion",
    "campaignId",
    "status",
    "jobConverged",
    "childExitCode",
    "forced",
    "orphanProcessesAfterClose",
    "receiptSha256",
    "receiptAuthenticationSha256",
  ];
  if (
    !value ||
    Object.keys(value).sort().join("\0") !== keys.sort().join("\0") ||
    value.schemaVersion !== 1 ||
    value.campaignId !== campaignId ||
    !["completed", "child-exit", "timeout", "child-failed"].includes(
      value.status,
    ) ||
    value.jobConverged !== true ||
    !Number.isInteger(value.orphanProcessesAfterClose) ||
    value.orphanProcessesAfterClose !== 0 ||
    typeof value.forced !== "boolean" ||
    (value.childExitCode !== null && !Number.isInteger(value.childExitCode)) ||
    !(
      (value.receiptSha256 === null &&
        value.receiptAuthenticationSha256 === null) ||
      (/^[0-9a-f]{64}$/u.test(value.receiptSha256 ?? "") &&
        /^[0-9a-f]{64}$/u.test(value.receiptAuthenticationSha256 ?? ""))
    )
  ) {
    throw installedSmokeFailure(
      "orchestrator-job-proof-invalid",
      "installed-smoke-controller-proof-invalid",
    );
  }
  return value;
}

export function createInstalledSmokeControllerInvocation({
  campaignId,
  sourceRoot,
  runnerIdentities,
  powerShellIdentity,
  mainParameters,
}) {
  const campaignBootstrap = serializeLockedPowerShellRequest({
    sourceRoot,
    entryRelativePath: "scripts/measure-windows-installed-smoke.ps1",
    identities: runnerIdentities,
    parameters: mainParameters,
  });
  const stdin = serializeLockedPowerShellRequest({
    sourceRoot,
    entryRelativePath: "scripts/control-windows-installed-smoke.ps1",
    identities: runnerIdentities,
    parameters: {
      CampaignId: campaignId,
      AuthenticatedSourceRoot: sourceRoot,
      PowerShellPath: powerShellIdentity.path,
      PowerShellSha256: powerShellIdentity.sha256,
      PowerShellBytes: powerShellIdentity.bytes,
      CampaignBootstrapBase64: campaignBootstrap.toString("base64"),
      LockedLoaderEncodedCommand: INSTALLED_SMOKE_LOCKED_LOADER_ENCODED_COMMAND,
      OuterJobTimeoutMilliseconds: campaignJobTimeoutMs,
    },
  });
  return {
    args: lockedPowerShellArguments(),
    stdin,
    stdinMaximum: INSTALLED_SMOKE_LOCKED_BOOTSTRAP_MAXIMUM_BYTES,
  };
}

export const INSTALLED_SMOKE_CAMPAIGN_JOB_TIMEOUT_MS = campaignJobTimeoutMs;
