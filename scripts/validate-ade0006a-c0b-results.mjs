import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  exactKeys,
  inspectSanitized,
  parseBoundedJson,
} from "./ade0006a-c0b-common.mjs";
import {
  validateEvidenceBounds,
  validateGestures,
  validatePackage,
  validatePersistence,
  validTimestampRange,
} from "./ade0006a-c0b-artifact-evidence.mjs";
import {
  validateCoalescing,
  validateLatencyMetrics,
  validateResourceMetrics,
} from "./ade0006a-c0b-metrics.mjs";
import {
  validateFairness,
  validatePairs,
  validateSaturation,
} from "./ade0006a-c0b-runtime-evidence.mjs";
import { loadPlanInputs } from "./validate-ade0006a-c0b-plan.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const rootKeys = [
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
  "fixtureManifestSha256",
  "limitsSha256",
  "candidateBindingSha256",
  "controlBindingSha256",
  "warmupExclusions",
  "validPairs",
  "invalidPairs",
  "latencyMs",
  "resources",
  "queueSaturation",
  "fairness",
  "coalescing",
  "persistence",
  "package",
  "gestures",
  "evidence",
  "redaction",
];

export function validateC0BResults(bundle, results, serializedBytes) {
  const errors = [...bundle.errors];
  const { plan, trust, frozen } = bundle;
  exactKeys(errors, results, rootKeys, "results");
  if (
    results?.schemaVersion !== 1 ||
    results?.workItem !== "ADE-0006A" ||
    results?.phase !== "C0-B" ||
    results?.sanitized !== true ||
    results?.result !== "passed"
  ) {
    errors.push("results header is invalid");
  }
  if (
    results?.planSha256 !== plan.sha256 ||
    results?.trustManifestSha256 !== trust.sha256 ||
    results?.contractManifestSha256 !== frozen.contractManifestSha256 ||
    results?.fixtureManifestSha256 !== frozen.fixtureManifestSha256 ||
    results?.limitsSha256 !== frozen.limitsSha256 ||
    results?.candidateBindingSha256 !== plan.value.candidateBindingSha256 ||
    results?.controlBindingSha256 !== plan.value.controlBindingSha256
  ) {
    errors.push("results identity binding is stale");
  }
  validTimestampRange(errors, results);
  inspectSanitized(results, errors, "results", ["contractManifestSha256"]);
  const pairs = validatePairs(errors, results, plan.value, trust.value);
  if (pairs.size === plan.value.collection.validPairs) {
    validateLatencyMetrics(errors, results, plan.value, frozen, pairs);
    validateResourceMetrics(errors, results, frozen, pairs);
    validateSaturation(errors, results, plan.value, frozen, pairs);
    validateFairness(errors, results, plan.value, frozen, pairs);
    validateCoalescing(errors, results, plan.value, frozen, pairs);
  }
  validatePersistence(errors, results, frozen);
  validatePackage(errors, results, trust.value, frozen);
  validateGestures(errors, results, trust.value, frozen);
  validateEvidenceBounds(errors, results, frozen, serializedBytes);
  return errors;
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  const planPath = argument("--plan");
  const trustPath = argument("--trust");
  const expectedTrust = argument("--expected-trust-sha256");
  const resultsPath = argument("--results");
  if (
    !process.argv.includes("--dry-run") ||
    !planPath ||
    !trustPath ||
    !expectedTrust ||
    !resultsPath
  ) {
    console.error(
      "usage: node scripts/validate-ade0006a-c0b-results.mjs --dry-run --plan <plan.json> --trust <trusted.json> --expected-trust-sha256 <sha256> --results <results.json>",
    );
    process.exitCode = 1;
  } else {
    try {
      const bundle = loadPlanInputs(planPath, trustPath, expectedTrust);
      const resultRecord = parseBoundedJson(
        resultsPath,
        bundle.frozen.evidence.campaignBounds.curatedBytes,
        bundle.frozen.limits.wire,
        "C0-B curated results",
      );
      const errors = validateC0BResults(
        bundle,
        resultRecord.value,
        resultRecord.bytes,
      );
      if (errors.length > 0) throw new Error(errors.join("\n"));
      console.log(
        JSON.stringify({
          status: "passed",
          mode: "dry-run",
          planSha256: bundle.plan.sha256,
          trustManifestSha256: bundle.trust.sha256,
          validPairs: resultRecord.value.validPairs.length,
          invalidPairs: resultRecord.value.invalidPairs.length,
          queueBoundaries: resultRecord.value.queueSaturation.length,
        }),
      );
    } catch (error) {
      console.error(`c0b-results: ${error.message}`);
      process.exitCode = 1;
    }
  }
}
