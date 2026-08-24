import { createHash } from "node:crypto";

import { BETA_CANDIDATE_CONTRACT } from "./beta0001-candidate-contract.mjs";
import {
  inspectBetaCandidateJson,
  readBoundedPlainEvidenceFile,
} from "./beta0001-candidate-bounds.mjs";
import { validateBetaCandidatePackageAdmission } from "./beta0001-candidate-package.mjs";

const campaignSidecarMaximumBytes = 65_536;
const campaignKind = "security-a11y-performance-recovery";
const campaignQuartet = Object.freeze([
  "renderer.a11y-reload-focus",
  "security.authority-secrets",
  "recovery.crash-cleanup",
  "performance.resource-storage",
]);

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

function validateJourneyEvidence(value) {
  if (!Array.isArray(value) || value.length !== 4) return false;
  return value.every(
    (item, index) =>
      exactKeys(item, ["id", "sha256"]) &&
      item.id === campaignQuartet[index] &&
      validHash(item.sha256),
  );
}

function validateReceipt(value) {
  if (
    !exactKeys(value, [
      "schemaVersion",
      "workItem",
      "sourceCommit",
      "packageReceiptSha256",
      "installerSha256",
      "campaignKind",
      "journeyEvidence",
    ]) ||
    value.schemaVersion !== 1 ||
    value.workItem !== "BETA-0001" ||
    !validCommit(value.sourceCommit) ||
    !validHash(value.packageReceiptSha256) ||
    !validHash(value.installerSha256) ||
    value.campaignKind !== campaignKind ||
    !validateJourneyEvidence(value.journeyEvidence)
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
      "campaignReceiptSha256",
      "installerSha256",
    ]) ||
    value.schemaVersion !== 1 ||
    value.workItem !== "BETA-0001" ||
    !validCommit(value.sourceCommit) ||
    !validHash(value.contractSha256) ||
    !validHash(value.packageReceiptSha256) ||
    !validHash(value.campaignReceiptSha256) ||
    !validHash(value.installerSha256)
  ) {
    reject("schema-invalid");
  }
}

async function readEvidenceFile(evidenceRoot, id, maximumBytes, onAfterOpen) {
  try {
    return await readBoundedPlainEvidenceFile({
      root: evidenceRoot,
      id,
      maximumBytes,
      onAfterOpen,
    });
  } catch (error) {
    if (error?.code === "ENOENT") reject("campaign-missing");
    throw error;
  }
}

function receiptSha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function bindJourneyBytes({
  evidenceRoot,
  journeyEvidence,
  onAfterJourneyOpen,
  onAfterFirstJourneyBind,
}) {
  const firstHashes = [];
  for (const item of journeyEvidence) {
    const bytes = await readEvidenceFile(
      evidenceRoot,
      item.id,
      BETA_CANDIDATE_CONTRACT.limits.evidenceBytesMaximum,
      onAfterJourneyOpen,
    );
    const digest = receiptSha256(bytes);
    if (digest !== item.sha256) reject("campaign-substituted");
    firstHashes.push(digest);
  }
  if (typeof onAfterFirstJourneyBind === "function")
    await onAfterFirstJourneyBind();
  for (const [index, item] of journeyEvidence.entries()) {
    const bytes = await readEvidenceFile(
      evidenceRoot,
      item.id,
      BETA_CANDIDATE_CONTRACT.limits.evidenceBytesMaximum,
      onAfterJourneyOpen,
    );
    if (receiptSha256(bytes) !== firstHashes[index])
      reject("evidence-identity-changed");
  }
}

export async function validateBetaCandidateCampaignAdmission({
  resultBytes,
  evidenceRoot,
  expectedSourceCommit,
  expectedPackageReceiptSha256,
  expectedInstallerSha256,
  authenticodeCatalog,
  expectedCampaignReceiptSha256,
  onAfterCampaignOpen,
  onAfterJourneyOpen,
  onAfterFirstJourneyBind,
}) {
  const matrix = await validateBetaCandidatePackageAdmission({
    resultBytes,
    evidenceRoot,
    expectedSourceCommit,
    expectedPackageReceiptSha256,
    expectedInstallerSha256,
    authenticodeCatalog,
  });
  if (!validHash(expectedCampaignReceiptSha256)) reject("schema-invalid");

  const admissionBytes = await readEvidenceFile(
    evidenceRoot,
    "campaign-admission",
    campaignSidecarMaximumBytes,
    onAfterCampaignOpen,
  );
  const receiptBytes = await readEvidenceFile(
    evidenceRoot,
    "campaign-receipt",
    campaignSidecarMaximumBytes,
    onAfterCampaignOpen,
  );
  const admission = parseSidecar(admissionBytes);
  const receipt = parseSidecar(receiptBytes);
  validateAdmission(admission);
  validateReceipt(receipt);

  if (
    admission.sourceCommit !== receipt.sourceCommit ||
    admission.sourceCommit !== expectedSourceCommit ||
    receipt.sourceCommit !== expectedSourceCommit ||
    admission.campaignReceiptSha256 !== expectedCampaignReceiptSha256 ||
    admission.packageReceiptSha256 !== expectedPackageReceiptSha256 ||
    receipt.packageReceiptSha256 !== expectedPackageReceiptSha256
  ) {
    reject("campaign-stale");
  }
  if (receiptSha256(receiptBytes) !== admission.campaignReceiptSha256)
    reject("campaign-substituted");
  if (
    admission.installerSha256 !== expectedInstallerSha256 ||
    admission.installerSha256 !== receipt.installerSha256 ||
    receipt.installerSha256 !== expectedInstallerSha256
  ) {
    reject("campaign-substituted");
  }
  if (
    admission.contractSha256 !==
    BETA_CANDIDATE_CONTRACT.thresholdAuthority.sha256
  ) {
    reject("contract-drift");
  }
  await bindJourneyBytes({
    evidenceRoot,
    journeyEvidence: receipt.journeyEvidence,
    onAfterJourneyOpen,
    onAfterFirstJourneyBind,
  });
  return matrix;
}
