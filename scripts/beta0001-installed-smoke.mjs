import { exactKeys, safeInteger, SHA256 } from "./ade0006a-c0b-common.mjs";
import {
  INSTALLED_SMOKE_ASSERTION_KEYS as assertionKeys,
  INSTALLED_SMOKE_FAILURE_CODES as failureCodes,
  INSTALLED_SMOKE_INSTALLER_KEYS as installerKeys,
  INSTALLED_SMOKE_PREREQUISITE_FAILURES as prerequisiteFailures,
  INSTALLED_SMOKE_PREREQUISITE_KEYS as prerequisiteKeys,
  INSTALLED_SMOKE_STEP_IDS,
  INSTALLED_SMOKE_STEP_TIMEOUT_MS as MAX_STEP_DURATION_MS,
  validateInstalledSmokeReceipt,
} from "./beta0001-installed-smoke-contract.mjs";

export { INSTALLED_SMOKE_STEP_IDS, validateInstalledSmokeReceipt };

export class InstalledSmokeOperationError extends Error {
  constructor(code) {
    if (!failureCodes.has(code))
      throw new Error("installed-smoke-failure-code-invalid");
    super(code);
    this.code = code;
  }
}

function boundedString(errors, value, label, maximumBytes, nullable = false) {
  if (nullable && value === null) return;
  if (typeof value !== "string") {
    errors.push(`${label} must be a string`);
    return;
  }
  if (Buffer.byteLength(value, "utf8") > maximumBytes) {
    errors.push(`${label} exceeds ${maximumBytes} UTF-8 bytes`);
  }
}

function validateRequest(request) {
  const errors = [];
  exactKeys(
    errors,
    request,
    ["campaignId", "packageReceiptSha256", "installer", "fixture"],
    "request",
  );
  exactKeys(errors, request?.installer, installerKeys, "request.installer");
  exactKeys(
    errors,
    request?.fixture,
    ["rootIdentitySha256", "rootLabel", "initialCommitSha256"],
    "request.fixture",
  );
  boundedString(errors, request?.campaignId, "request.campaignId", 64);
  boundedString(
    errors,
    request?.fixture?.rootLabel,
    "request.fixture.rootLabel",
    128,
  );
  safeInteger(
    errors,
    request?.installer?.bytes,
    "request.installer.bytes",
    1,
    512 * 1024 * 1024,
  );
  safeInteger(
    errors,
    request?.installer?.expectedInstalledExecutableBytes,
    "request.installer.expectedInstalledExecutableBytes",
    1,
    512 * 1024 * 1024,
  );
  if (
    request?.installer?.kind !== "nsis" ||
    !SHA256.test(request?.packageReceiptSha256 ?? "") ||
    !SHA256.test(request?.installer?.sha256 ?? "") ||
    !SHA256.test(request?.installer?.expectedInstalledExecutableSha256 ?? "") ||
    !/^[0-9a-f]{40}$/u.test(request?.installer?.signerThumbprint ?? "") ||
    !SHA256.test(request?.fixture?.rootIdentitySha256 ?? "") ||
    !SHA256.test(request?.fixture?.initialCommitSha256 ?? "")
  ) {
    errors.push("request identities are invalid");
  }
  if (errors.length > 0) throw new Error(errors.join(","));
}

function emptyAssertions() {
  return Object.fromEntries(assertionKeys.map((key) => [key, false]));
}

function skippedSteps() {
  return INSTALLED_SMOKE_STEP_IDS.map((id) => ({
    id,
    status: "skipped",
    durationMs: 0,
    evidenceSha256: null,
  }));
}

function failure(code, phase) {
  return { code, phase };
}

function baseReceipt(request, prerequisites, startedAtUtc) {
  return {
    schemaVersion: 1,
    workItem: "BETA-0001",
    campaignId: request.campaignId,
    platform: "windows",
    installer: { ...request.installer },
    packageReceiptSha256: request.packageReceiptSha256,
    fixture: { ...request.fixture },
    prerequisites: structuredClone(prerequisites),
    startedAtUtc,
    completedAtUtc: startedAtUtc,
    result: "failed",
    failure: null,
    steps: skippedSteps(),
    assertions: emptyAssertions(),
    limits: {
      stepCount: INSTALLED_SMOKE_STEP_IDS.length,
      stepTimeoutMs: MAX_STEP_DURATION_MS,
      retainedOutputBytes: 256 * 1024,
    },
  };
}

function validatePrerequisites(value) {
  const errors = [];
  exactKeys(errors, value, prerequisiteKeys, "prerequisites");
  for (const [key] of prerequisiteFailures) {
    if (typeof value?.[key] !== "boolean") {
      errors.push(`prerequisites.${key} must be boolean`);
    }
  }
  for (const key of ["osVersion", "webView2Version", "gitVersion"]) {
    boundedString(errors, value?.[key], `prerequisites.${key}`, 128, true);
  }
  boundedString(
    errors,
    value?.signerThumbprint,
    "prerequisites.signerThumbprint",
    64,
    true,
  );
  if (errors.length > 0) throw new Error(errors.join(","));
}

function mergeAssertions(target, patch) {
  for (const [key, value] of Object.entries(patch ?? {})) {
    if (!assertionKeys.includes(key) || typeof value !== "boolean") {
      throw new Error("installed-smoke-assertion-invalid");
    }
    target[key] = value;
  }
}

function boundedOperationResult(value, label) {
  if (value && typeof value.then === "function") {
    throw new Error(`${label}-must-be-authoritatively-bounded`);
  }
  return value;
}

export function runInstalledNsisSmoke({
  request,
  operations,
  clock = () => new Date(),
}) {
  validateRequest(request);
  if (
    !operations ||
    typeof operations.inspectPrerequisites !== "function" ||
    typeof operations.runStep !== "function"
  ) {
    throw new Error("installed-smoke-operations-invalid");
  }
  const prerequisites = boundedOperationResult(
    operations.inspectPrerequisites(request),
    "installed-smoke-prerequisites",
  );
  validatePrerequisites(prerequisites);
  const receipt = baseReceipt(request, prerequisites, clock().toISOString());
  const missing = prerequisiteFailures.find(([key]) => !prerequisites[key]);
  if (missing) {
    receipt.result = "blocked";
    receipt.failure = failure(missing[1], "prerequisites");
    receipt.completedAtUtc = clock().toISOString();
    return receipt;
  }

  let failed = false;
  let mayBeInstalled = false;
  for (let index = 0; index < INSTALLED_SMOKE_STEP_IDS.length; index += 1) {
    const id = INSTALLED_SMOKE_STEP_IDS[index];
    const cleanup = id === "uninstall-final" && mayBeInstalled;
    if (failed && !cleanup) continue;
    if (id === "install-initial" || id === "install-reinstall") {
      mayBeInstalled = true;
    }
    try {
      const observed = boundedOperationResult(
        operations.runStep(id, {
          request: structuredClone(request),
          assertions: structuredClone(receipt.assertions),
        }),
        `installed-smoke-${id}`,
      );
      safeStepResult(observed);
      receipt.steps[index] = {
        id,
        status: "passed",
        durationMs: observed.durationMs,
        evidenceSha256: observed.evidenceSha256,
      };
      mergeAssertions(receipt.assertions, observed.assertions);
      if (id === "uninstall-initial" || id === "uninstall-final") {
        mayBeInstalled = false;
      }
    } catch (error) {
      receipt.steps[index] = {
        id,
        status: "failed",
        durationMs: 0,
        evidenceSha256: null,
      };
      receipt.failure ??= failure(
        error instanceof InstalledSmokeOperationError
          ? error.code
          : "step-failed",
        id,
      );
      failed = true;
    }
  }
  const assertionsPassed = Object.values(receipt.assertions).every(Boolean);
  receipt.result = !failed && assertionsPassed ? "passed" : "failed";
  receipt.failure ??= assertionsPassed
    ? null
    : failure("assertion-incomplete", "receipt");
  receipt.completedAtUtc = clock().toISOString();
  return receipt;
}

function safeStepResult(value) {
  const errors = [];
  exactKeys(
    errors,
    value,
    ["durationMs", "evidenceSha256", "assertions"],
    "step result",
  );
  safeInteger(
    errors,
    value?.durationMs,
    "step result durationMs",
    0,
    MAX_STEP_DURATION_MS,
  );
  if (!SHA256.test(value?.evidenceSha256 ?? "")) {
    errors.push("step result evidenceSha256 is invalid");
  }
  if (
    !value?.assertions ||
    typeof value.assertions !== "object" ||
    Array.isArray(value.assertions)
  ) {
    errors.push("step result assertions are invalid");
  }
  if (errors.length > 0) throw new Error(errors.join(","));
}
