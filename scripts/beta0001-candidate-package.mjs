import { createHash } from "node:crypto";

import { BETA_CANDIDATE_CONTRACT } from "./beta0001-candidate-contract.mjs";
import {
  inspectBetaCandidateJson,
  readBoundedPlainEvidenceFile,
} from "./beta0001-candidate-bounds.mjs";
import { validateBetaCandidateEvidence } from "./beta0001-candidate-verifier.mjs";

const packageMaximumBytes = 65_536;

function reject(code) {
  const error = new Error(code);
  error.code = code;
  throw error;
}

function exactKeys(value, keys) {
  return (
    value &&
    !Array.isArray(value) &&
    typeof value === "object" &&
    JSON.stringify(Object.keys(value).sort()) ===
      JSON.stringify([...keys].sort())
  );
}

function validCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40}$/u.test(value);
}

function validHash(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function validThumbprint(value) {
  return typeof value === "string" && /^[0-9a-f]{40}$/u.test(value);
}

function parseSidecar(bytes) {
  let value;
  try {
    value = JSON.parse(Buffer.from(bytes).toString("utf8"));
  } catch {
    reject("malformed-json");
  }
  inspectBetaCandidateJson(value, BETA_CANDIDATE_CONTRACT.limits);
  return value;
}

function validateReceipt(value) {
  if (
    !exactKeys(value, [
      "schemaVersion",
      "workItem",
      "sourceCommit",
      "installerSha256",
      "installedExecutableSha256",
      "signerThumbprint",
      "authenticode",
    ]) ||
    value.schemaVersion !== 1 ||
    value.workItem !== "BETA-0001" ||
    !validCommit(value.sourceCommit) ||
    !validHash(value.installerSha256) ||
    !validHash(value.installedExecutableSha256) ||
    !validThumbprint(value.signerThumbprint) ||
    typeof value.authenticode !== "boolean"
  ) {
    reject("schema-invalid");
  }
}

function validateAdmission(value) {
  if (
    !exactKeys(value, [
      "schemaVersion",
      "workItem",
      "sourceCommit",
      "contractSha256",
      "packageReceiptSha256",
      "installerSha256",
      "installedExecutableSha256",
    ]) ||
    value.schemaVersion !== 1 ||
    value.workItem !== "BETA-0001" ||
    !validCommit(value.sourceCommit) ||
    !validHash(value.contractSha256) ||
    !validHash(value.packageReceiptSha256) ||
    !validHash(value.installerSha256) ||
    !validHash(value.installedExecutableSha256)
  ) {
    reject("schema-invalid");
  }
}

async function readSidecar(evidenceRoot, id) {
  try {
    return await readBoundedPlainEvidenceFile({
      root: evidenceRoot,
      id,
      maximumBytes: packageMaximumBytes,
    });
  } catch (error) {
    if (error?.code === "ENOENT") reject("package-missing");
    throw error;
  }
}

function receiptSha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function authenticates(catalog, thumbprint) {
  if (!catalog || typeof catalog.verify !== "function") return false;
  try {
    return catalog.verify(thumbprint) === true;
  } catch {
    return false;
  }
}

export async function validateBetaCandidatePackageAdmission({
  resultBytes,
  evidenceRoot,
  expectedSourceCommit,
  expectedPackageReceiptSha256,
  expectedInstallerSha256,
  authenticodeCatalog,
}) {
  const matrix = await validateBetaCandidateEvidence({
    resultBytes,
    evidenceRoot,
    expectedSourceCommit,
  });
  if (
    !validHash(expectedPackageReceiptSha256) ||
    !validHash(expectedInstallerSha256)
  )
    reject("schema-invalid");

  const admissionBytes = await readSidecar(evidenceRoot, "package-admission");
  const receiptBytes = await readSidecar(evidenceRoot, "package-receipt");
  const admission = parseSidecar(admissionBytes);
  const receipt = parseSidecar(receiptBytes);
  validateAdmission(admission);
  validateReceipt(receipt);

  if (
    receipt.authenticode !== true ||
    !authenticates(authenticodeCatalog, receipt.signerThumbprint)
  )
    reject("package-unsigned");
  if (
    admission.sourceCommit !== receipt.sourceCommit ||
    admission.sourceCommit !== expectedSourceCommit ||
    receipt.sourceCommit !== expectedSourceCommit ||
    admission.packageReceiptSha256 !== expectedPackageReceiptSha256
  ) {
    reject("package-stale");
  }
  if (receiptSha256(receiptBytes) !== admission.packageReceiptSha256)
    reject("package-substituted");
  if (
    admission.installerSha256 !== expectedInstallerSha256 ||
    admission.installerSha256 !== receipt.installerSha256 ||
    admission.installedExecutableSha256 !== receipt.installedExecutableSha256
  ) {
    reject("package-substituted");
  }
  if (
    admission.contractSha256 !==
    BETA_CANDIDATE_CONTRACT.thresholdAuthority.sha256
  ) {
    reject("contract-drift");
  }
  return matrix;
}
