import {
  exactKeys,
  safeInteger,
  SHA256,
  validUtcTimestamp,
} from "./ade0006a-c0b-common.mjs";

export const INSTALLED_SMOKE_STEP_IDS = Object.freeze([
  "install-initial",
  "launch-initial",
  "drive-primary",
  "close-initial",
  "launch-restart",
  "verify-restart",
  "close-restart",
  "uninstall-initial",
  "install-reinstall",
  "launch-reinstall",
  "verify-reinstall",
  "close-reinstall",
  "uninstall-final",
]);
export const INSTALLED_SMOKE_ASSERTION_KEYS = Object.freeze([
  "projectSeeded",
  "fileOpened",
  "fileEditedAndSaved",
  "workspaceSearchMatched",
  "gitStaged",
  "gitCommitted",
  "chatCreated",
  "chatRenamed",
  "chatSwitched",
  "simulatorTurnCompleted",
  "restartPersisted",
  "reinstallVerified",
]);
export const INSTALLED_SMOKE_PREREQUISITE_KEYS = Object.freeze([
  "windows",
  "interactiveSession",
  "webView2",
  "administrator",
  "authenticode",
  "git",
  "osVersion",
  "webView2Version",
  "gitVersion",
  "signerThumbprint",
]);
export const INSTALLED_SMOKE_INSTALLER_KEYS = Object.freeze([
  "kind",
  "sha256",
  "bytes",
  "expectedInstalledExecutableSha256",
  "expectedInstalledExecutableBytes",
  "signerThumbprint",
]);
export const INSTALLED_SMOKE_PREREQUISITE_FAILURES = Object.freeze([
  ["windows", "windows-unavailable"],
  ["interactiveSession", "interactive-session-unavailable"],
  ["webView2", "webview2-unavailable"],
  ["administrator", "administrator-unavailable"],
  ["authenticode", "installer-signature-invalid"],
  ["git", "git-unavailable"],
]);
export const INSTALLED_SMOKE_STEP_TIMEOUT_MS = 10 * 60 * 1000;
export const INSTALLED_SMOKE_FAILURE_CODES = new Set([
  ...INSTALLED_SMOKE_PREREQUISITE_FAILURES.map(([, code]) => code),
  "prerequisite-inspection-failed",
  "step-failed",
  "step-timeout",
  "identity-lost",
  "cleanup-failed",
  "assertion-incomplete",
]);

function boundedString(errors, value, label, maximumBytes, nullable = false) {
  if (nullable && value === null) return;
  if (typeof value !== "string") {
    errors.push(`${label} must be a string`);
  } else if (Buffer.byteLength(value, "utf8") > maximumBytes) {
    errors.push(`${label} exceeds ${maximumBytes} UTF-8 bytes`);
  }
}

function validateIdentities(errors, receipt) {
  boundedString(errors, receipt?.campaignId, "receipt.campaignId", 64);
  boundedString(
    errors,
    receipt?.fixture?.rootLabel,
    "receipt.fixture.rootLabel",
    128,
  );
  safeInteger(
    errors,
    receipt?.installer?.bytes,
    "receipt.installer.bytes",
    1,
    512 * 1024 * 1024,
  );
  safeInteger(
    errors,
    receipt?.installer?.expectedInstalledExecutableBytes,
    "receipt.installer.expectedInstalledExecutableBytes",
    1,
    512 * 1024 * 1024,
  );
  if (
    receipt?.installer?.kind !== "nsis" ||
    !SHA256.test(receipt?.installer?.sha256 ?? "") ||
    !SHA256.test(receipt?.installer?.expectedInstalledExecutableSha256 ?? "") ||
    !/^[0-9a-f]{40}$/u.test(receipt?.installer?.signerThumbprint ?? "") ||
    !SHA256.test(receipt?.packageReceiptSha256 ?? "") ||
    !SHA256.test(receipt?.fixture?.rootIdentitySha256 ?? "") ||
    !SHA256.test(receipt?.fixture?.initialCommitSha256 ?? "")
  ) {
    errors.push("receipt identities are invalid");
  }
}

function validatePrerequisites(errors, receipt) {
  exactKeys(
    errors,
    receipt?.prerequisites,
    INSTALLED_SMOKE_PREREQUISITE_KEYS,
    "receipt.prerequisites",
  );
  for (const key of ["osVersion", "webView2Version", "gitVersion"]) {
    boundedString(
      errors,
      receipt?.prerequisites?.[key],
      `receipt.prerequisites.${key}`,
      128,
      true,
    );
  }
  for (const [key] of INSTALLED_SMOKE_PREREQUISITE_FAILURES) {
    if (typeof receipt?.prerequisites?.[key] !== "boolean") {
      errors.push(`receipt.prerequisites.${key} must be boolean`);
    }
  }
  if (
    receipt?.prerequisites?.authenticode &&
    (!/^[0-9a-f]{40}$/u.test(receipt.prerequisites.signerThumbprint ?? "") ||
      receipt.prerequisites.signerThumbprint !==
        receipt?.installer?.signerThumbprint)
  ) {
    errors.push("receipt signer thumbprint is invalid");
  }
}

function validateSteps(errors, receipt) {
  const steps = Array.isArray(receipt?.steps) ? receipt.steps : [];
  const exactSteps =
    Array.isArray(receipt?.steps) &&
    steps.map((step) => step?.id).join("\0") ===
      INSTALLED_SMOKE_STEP_IDS.join("\0");
  if (!exactSteps) {
    errors.push(
      "receipt.steps must contain the exact installed-smoke lifecycle",
    );
    return false;
  }
  steps.forEach((step, index) => {
    exactKeys(
      errors,
      step,
      ["id", "status", "durationMs", "evidenceSha256"],
      `receipt.steps[${index}]`,
    );
    safeInteger(
      errors,
      step?.durationMs,
      `receipt.steps[${index}].durationMs`,
      0,
      INSTALLED_SMOKE_STEP_TIMEOUT_MS,
    );
    if (!["passed", "failed", "skipped"].includes(step?.status)) {
      errors.push(`receipt.steps[${index}].status is invalid`);
    }
    if (
      (step?.status === "passed") !==
      SHA256.test(step?.evidenceSha256 ?? "")
    ) {
      errors.push(`receipt.steps[${index}].evidenceSha256 is invalid`);
    }
  });
  return true;
}

export function validateInstalledSmokeReceipt(receipt) {
  const errors = [];
  exactKeys(
    errors,
    receipt,
    [
      "schemaVersion",
      "workItem",
      "campaignId",
      "platform",
      "installer",
      "packageReceiptSha256",
      "fixture",
      "prerequisites",
      "startedAtUtc",
      "completedAtUtc",
      "result",
      "failure",
      "steps",
      "assertions",
      "limits",
    ],
    "receipt",
  );
  exactKeys(
    errors,
    receipt?.installer,
    INSTALLED_SMOKE_INSTALLER_KEYS,
    "receipt.installer",
  );
  exactKeys(
    errors,
    receipt?.fixture,
    ["rootIdentitySha256", "rootLabel", "initialCommitSha256"],
    "receipt.fixture",
  );
  exactKeys(
    errors,
    receipt?.limits,
    ["stepCount", "stepTimeoutMs", "retainedOutputBytes"],
    "receipt.limits",
  );
  validateIdentities(errors, receipt);
  validatePrerequisites(errors, receipt);
  const exactSteps = validateSteps(errors, receipt);
  if (
    receipt?.schemaVersion !== 1 ||
    receipt?.workItem !== "BETA-0001" ||
    receipt?.platform !== "windows"
  ) {
    errors.push("receipt header is invalid");
  }
  if (!["passed", "failed", "blocked"].includes(receipt?.result)) {
    errors.push("receipt result is invalid");
  }
  if (
    !validUtcTimestamp(receipt?.startedAtUtc) ||
    !validUtcTimestamp(receipt?.completedAtUtc)
  ) {
    errors.push("receipt timestamps are invalid");
  }
  if (receipt?.failure !== null) {
    exactKeys(errors, receipt?.failure, ["code", "phase"], "receipt.failure");
    if (!INSTALLED_SMOKE_FAILURE_CODES.has(receipt?.failure?.code)) {
      errors.push("receipt failure code is invalid");
    }
    boundedString(errors, receipt?.failure?.phase, "receipt.failure.phase", 64);
  }
  exactKeys(
    errors,
    receipt?.assertions,
    INSTALLED_SMOKE_ASSERTION_KEYS,
    "receipt.assertions",
  );
  if (
    INSTALLED_SMOKE_ASSERTION_KEYS.some(
      (key) => typeof receipt?.assertions?.[key] !== "boolean",
    )
  ) {
    errors.push("receipt assertions must be boolean");
  }
  if (
    receipt?.result === "passed" &&
    INSTALLED_SMOKE_ASSERTION_KEYS.some(
      (key) => receipt?.assertions?.[key] !== true,
    )
  ) {
    errors.push("passing receipt has incomplete assertions");
  }
  if (
    receipt?.result === "passed" &&
    (INSTALLED_SMOKE_PREREQUISITE_FAILURES.some(
      ([key]) => receipt?.prerequisites?.[key] !== true,
    ) ||
      receipt?.prerequisites?.signerThumbprint !==
        receipt?.installer?.signerThumbprint)
  ) {
    errors.push("passing receipt has unmet prerequisites");
  }
  if (
    receipt?.result === "passed" &&
    (receipt.failure !== null ||
      (exactSteps && receipt.steps.some((step) => step?.status !== "passed")))
  ) {
    errors.push("passing receipt contains failed or skipped work");
  }
  if (
    receipt?.result === "blocked" &&
    (receipt.failure?.phase !== "prerequisites" ||
      (exactSteps &&
        receipt.steps.some((step) => step?.status !== "skipped")) ||
      INSTALLED_SMOKE_ASSERTION_KEYS.some((key) => receipt.assertions?.[key]))
  ) {
    errors.push("blocked receipt contains mutation evidence");
  }
  if (receipt?.result !== "passed" && receipt?.failure === null) {
    errors.push("non-passing receipt lacks typed failure evidence");
  }
  if (
    receipt?.limits?.stepCount !== INSTALLED_SMOKE_STEP_IDS.length ||
    receipt?.limits?.stepTimeoutMs !== INSTALLED_SMOKE_STEP_TIMEOUT_MS ||
    receipt?.limits?.retainedOutputBytes !== 256 * 1024
  ) {
    errors.push("receipt limits are invalid");
  }
  if (
    validUtcTimestamp(receipt?.startedAtUtc) &&
    validUtcTimestamp(receipt?.completedAtUtc) &&
    Date.parse(receipt.completedAtUtc) < Date.parse(receipt.startedAtUtc)
  ) {
    errors.push("receipt completion precedes start");
  }
  return errors;
}
