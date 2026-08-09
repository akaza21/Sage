import {
  exactKeys,
  safeInteger,
  validUtcTimestamp,
} from "./ade0006a-c0b-common.mjs";

export const resultRootKeys = [
  "schemaVersion",
  "workItem",
  "phase",
  "sanitized",
  "result",
  "timestamps",
  "limitations",
  "planSha256",
  "trustManifestSha256",
  "contractManifestSha256",
  "packageReceiptSha256",
  "monotonicClock",
  "packagedFootprint",
  "characterization",
  "observationFiles",
  "gateEvaluation",
  "evidence",
  "redaction",
];

export function validateTimestampRange(errors, results) {
  exactKeys(
    errors,
    results?.timestamps,
    ["startedAtUtc", "completedAtUtc"],
    "results.timestamps",
  );
  if (
    !validUtcTimestamp(results?.timestamps?.startedAtUtc) ||
    !validUtcTimestamp(results?.timestamps?.completedAtUtc) ||
    Date.parse(results.timestamps.completedAtUtc) <
      Date.parse(results.timestamps.startedAtUtc)
  ) {
    errors.push("results.timestamps are invalid");
  }
  if (
    !Array.isArray(results?.limitations) ||
    results.limitations.length > 16 ||
    results.limitations.some(
      (value) => !/^[a-z0-9][a-z0-9._-]{0,127}$/.test(value),
    )
  ) {
    errors.push("results.limitations must contain 0..16 sanitized codes");
  }
}

export function validateEvidenceBounds(
  errors,
  results,
  frozen,
  serializedBytes,
) {
  const bounds = frozen.evidence.campaignBounds;
  const mapping = {
    rawFiles: "rawFiles",
    rawBytes: "rawBytes",
    campaignsForCandidateIdentity: "campaignsPerCandidateIdentity",
    rawBytesForCandidateIdentity: "rawBytesPerCandidateIdentity",
    retainedCandidateIdentities: "retainedCandidateIdentities",
    globalFiles: "globalFiles",
    globalBytes: "globalBytes",
    curatedBytes: "curatedBytes",
    compactedCuratedFiles: "compactedCuratedFiles",
    compactedCuratedBytes: "compactedCuratedBytes",
  };
  exactKeys(
    errors,
    results?.evidence,
    Object.keys(mapping),
    "results.evidence",
  );
  for (const [key, bound] of Object.entries(mapping)) {
    safeInteger(
      errors,
      results?.evidence?.[key],
      `results.evidence.${key}`,
      0,
      bounds[bound],
    );
  }
  if (results?.evidence?.curatedBytes !== serializedBytes) {
    errors.push("results.evidence.curatedBytes does not equal retained bytes");
  }
  exactKeys(
    errors,
    results?.redaction,
    [
      "scanPerformed",
      "violations",
      "rawProtocolFramesRetained",
      "secretsRetained",
      "commandLinesRetained",
      "absolutePathsRetained",
    ],
    "results.redaction",
  );
  if (
    results?.redaction?.scanPerformed !== true ||
    !Array.isArray(results?.redaction?.violations) ||
    results.redaction.violations.length !== 0
  ) {
    errors.push("results.redaction scan is absent or failed");
  }
  for (const key of [
    "rawProtocolFramesRetained",
    "secretsRetained",
    "commandLinesRetained",
    "absolutePathsRetained",
  ]) {
    if (results?.redaction?.[key] !== 0) {
      errors.push(`results.redaction.${key} must be zero`);
    }
  }
}

export function validateEnvelope(errors, bundle, results, serializedBytes) {
  const { plan, trust, frozen, authenticatedPackage } = bundle;
  exactKeys(errors, results, resultRootKeys, "results");
  if (
    results?.schemaVersion !== 1 ||
    results?.workItem !== "ADE-0006A" ||
    results?.phase !== "C0-B" ||
    results?.sanitized !== true ||
    !["passed", "failed"].includes(results?.result)
  ) {
    errors.push("results header is invalid");
  }
  if (
    results?.planSha256 !== plan.sha256 ||
    results?.trustManifestSha256 !== trust.sha256 ||
    results?.contractManifestSha256 !== frozen.contractManifestSha256 ||
    results?.packageReceiptSha256 !== authenticatedPackage.receiptSha256
  ) {
    errors.push("results identity binding is stale");
  }
  exactKeys(
    errors,
    results?.monotonicClock,
    ["kind", "frequencyHz"],
    "results.monotonicClock",
  );
  if (results?.monotonicClock?.kind !== "query-performance-counter") {
    errors.push("results.monotonicClock.kind is not admitted");
  }
  safeInteger(
    errors,
    results?.monotonicClock?.frequencyHz,
    "results.monotonicClock.frequencyHz",
    1,
    Number.MAX_SAFE_INTEGER,
  );
  validateTimestampRange(errors, results);
  validateEvidenceBounds(errors, results, frozen, serializedBytes);
}

export function validateDerivedDisposition(errors, results, gateFailures) {
  const expected = gateFailures.length === 0 ? "passed" : "failed";
  if (results?.result !== expected) {
    errors.push(`results.result must be ${expected} for the observed gates`);
  }
}
