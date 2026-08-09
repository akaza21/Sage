import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  exactKeys,
  inspectSanitized,
  parseBoundedJson,
  sameValue,
} from "./ade0006a-c0b-common.mjs";
import { validateCharacterizationInvalidAttempts } from "./ade0006a-c0b-characterization-epochs.mjs";
import { validateCoreAndCold } from "./ade0006a-c0b-characterization-evidence.mjs";
import { validateCharacterizationLoad } from "./ade0006a-c0b-characterization-load.mjs";
import {
  validateGestures,
  validatePersistence,
} from "./ade0006a-c0b-characterization-artifacts.mjs";
import {
  validateDerivedDisposition,
  validateEnvelope,
} from "./ade0006a-c0b-envelope.mjs";
import { validatePackagedFootprint } from "./ade0006a-c0b-footprint-evidence.mjs";
import {
  loadObservationRecords,
  validateObservationRecords,
} from "./ade0006a-c0b-observation-files.mjs";
import {
  deriveC0BContract,
  loadPlanInputs,
} from "./validate-ade0006a-c0b-plan.mjs";

const scriptPath = fileURLToPath(import.meta.url);

function validateCharacterizationRoot(errors, results) {
  exactKeys(
    errors,
    results?.characterization,
    ["participant", "invalidAttempts"],
    "characterization",
  );
  if (results?.characterization?.participant !== "candidate") {
    errors.push("characterization must be candidate-only");
  }
}

function validateGateEvaluation(errors, results, gates) {
  exactKeys(
    errors,
    results?.gateEvaluation,
    ["method", "gates"],
    "results.gateEvaluation",
  );
  if (results?.gateEvaluation?.method !== "derived-v1") {
    errors.push("results.gateEvaluation.method is invalid");
  }
  const ids = new Set();
  for (const [index, gate] of gates.entries()) {
    exactKeys(
      errors,
      gate,
      ["id", "actual", "ceiling", "passed"],
      `derivedGate[${index}]`,
    );
    if (
      typeof gate.id !== "string" ||
      !/^[a-z0-9][a-zA-Z0-9-]{0,127}$/.test(gate.id) ||
      !Number.isFinite(gate.actual) ||
      !Number.isFinite(gate.ceiling) ||
      gate.passed !== gate.actual <= gate.ceiling ||
      ids.has(gate.id)
    ) {
      errors.push(`derivedGate[${index}] is invalid or duplicated`);
    }
    ids.add(gate.id);
  }
  sameValue(
    errors,
    results?.gateEvaluation?.gates,
    gates,
    "results.gateEvaluation.gates",
  );
}

export function evaluateC0BResults(
  bundle,
  results,
  serializedBytes,
  observationRecords,
) {
  const errors = [...bundle.errors];
  const gates = [];
  bundle.contract ??= deriveC0BContract(bundle.frozen);
  validateEnvelope(errors, bundle, results, serializedBytes);
  const records = validateObservationRecords(
    errors,
    results,
    bundle.frozen,
    observationRecords,
  );
  validateCharacterizationRoot(errors, results);
  const identities = validatePackagedFootprint(
    errors,
    gates,
    bundle,
    results,
    records,
  ) ?? {
    processes: new Set(),
    profiles: new Set(),
  };
  const epochMap = new Map();
  validateCoreAndCold(
    errors,
    gates,
    bundle,
    results,
    records,
    identities,
    epochMap,
  );
  validateCharacterizationLoad(
    errors,
    gates,
    bundle,
    results,
    records,
    identities,
    epochMap,
  );
  validateCharacterizationInvalidAttempts(
    errors,
    results?.characterization?.invalidAttempts,
    epochMap,
    bundle.frozen,
  );
  validatePersistence(
    errors,
    gates,
    bundle,
    results,
    records.get("characterization-persistence.json"),
    identities,
  );
  validateGestures(
    errors,
    gates,
    bundle,
    results,
    records.get("characterization-gestures.json"),
    identities,
  );
  for (const record of records.values()) {
    inspectSanitized(record.value, errors, "observation");
  }
  inspectSanitized(results, errors, "results", [
    "contractManifestSha256",
    "ceiling",
  ]);
  validateGateEvaluation(errors, results, gates);
  validateDerivedDisposition(
    errors,
    results,
    gates.filter(({ passed }) => !passed),
  );
  return { errors, gates };
}

export function validateC0BResults(
  bundle,
  results,
  serializedBytes,
  observationRecords,
) {
  return evaluateC0BResults(
    bundle,
    results,
    serializedBytes,
    observationRecords,
  ).errors;
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  const options = {
    planPath: argument("--plan"),
    trustPath: argument("--trust"),
    expectedTrustSha256: argument("--expected-trust-sha256"),
    packageReceiptPath: argument("--package-receipt"),
    packageRawRoot: argument("--package-raw-root"),
  };
  const resultsPath = argument("--results");
  const observationRoot = argument("--observation-root");
  if (
    !process.argv.includes("--dry-run") ||
    Object.values(options).some((value) => !value) ||
    !resultsPath ||
    !observationRoot
  ) {
    console.error(
      "usage: node scripts/validate-ade0006a-c0b-results.mjs --dry-run --plan <plan.json> --trust <trusted.json> --expected-trust-sha256 <sha256> --package-receipt <receipt.json> --package-raw-root <raw-root> --results <results.json> --observation-root <raw-root>",
    );
    process.exitCode = 1;
  } else {
    try {
      const bundle = await loadPlanInputs(options);
      if (bundle.errors.length > 0) throw new Error(bundle.errors.join("\n"));
      const resultRecord = parseBoundedJson(
        resultsPath,
        bundle.frozen.evidence.campaignBounds.curatedBytes,
        bundle.frozen.limits.wire,
        "C0-B curated results",
      );
      const records = loadObservationRecords(
        observationRoot,
        resultRecord.value.observationFiles,
        bundle.frozen,
      );
      const errors = validateC0BResults(
        bundle,
        resultRecord.value,
        resultRecord.bytes,
        records,
      );
      if (errors.length > 0) throw new Error(errors.join("\n"));
      console.log(
        JSON.stringify({
          status: "passed",
          mode: "dry-run",
          result: resultRecord.value.result,
          planSha256: bundle.plan.sha256,
          trustManifestSha256: bundle.trust.sha256,
          packageReceiptSha256: bundle.authenticatedPackage.receiptSha256,
          observationFiles: records.size,
          gates: resultRecord.value.gateEvaluation.gates.length,
        }),
      );
    } catch (error) {
      console.error(`c0b-results: ${error.message}`);
      process.exitCode = 1;
    }
  }
}
