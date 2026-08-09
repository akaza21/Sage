import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  exactKeys,
  inspectSanitized,
  parseBoundedJson,
} from "./ade0006a-c0b-common.mjs";
import {
  defaultWorkspaceRoot,
  loadFrozenContracts,
} from "./ade0006a-c0b-contracts.mjs";
import { authenticatePackageReceipt } from "./ade0006a-c0b-package-reference.mjs";
import { validateTrustManifest } from "./ade0006a-c0b-trust.mjs";

const scriptPath = fileURLToPath(import.meta.url);

export function buildPairSchedule(count) {
  return Array.from({ length: count }, (_, index) => ({
    pairOrdinal: index + 1,
    order:
      index % 2 === 0 ? ["control", "candidate"] : ["candidate", "control"],
  }));
}

export function deriveC0BContract(frozen) {
  const samples = frozen.budgets.sampleContract;
  const measurement = frozen.budgets.measurementContract;
  return {
    packagedFootprint: {
      states: structuredClone(samples.packagedStates),
      validPairsPerState: samples.validPairsPerPackagedState,
      pairSchedule: buildPairSchedule(samples.validPairsPerPackagedState),
      cadence: {
        settleAfterSemanticReadyMs:
          measurement.packagedFootprint.settleAfterSemanticReadyMs,
        snapshotIntervalMs: measurement.packagedFootprint.snapshotIntervalMs,
        snapshotCount: measurement.packagedFootprint.snapshotCount,
        cpuIntervalCount: measurement.packagedFootprint.cpuIntervalCount,
        minimumSamplingWindowMs:
          measurement.packagedFootprint.minimumSamplingWindowMs,
        adr0005ProjectionOrdinals: structuredClone(
          measurement.packagedFootprint.adr0005ProjectionOrdinals,
        ),
      },
    },
    characterization: {
      coreEpochs: samples.characterizationCoreEpochs,
      coldFixtureCycles: samples.coldFixtureCycles,
      receiptToPaintSamples: samples.receiptToPaintSamples,
      inputToPaintSamples: samples.inputToPaintSamples,
      saturationCyclesPerBoundary: samples.saturationCyclesPerBoundary,
      fairnessCycles: samples.fairnessCycles,
      coalescingEpochsPerPolicy: samples.coalescingEpochsPerPolicy,
      coalescingPolicies: structuredClone(samples.coalescingPolicies),
      cadence: {
        settleAfterSemanticReadyMs:
          measurement.characterization.settleAfterSemanticReadyMs,
        snapshotIntervalMs: measurement.characterization.snapshotIntervalMs,
        snapshotCount: measurement.characterization.snapshotCount,
        cpuIntervalCount: measurement.characterization.cpuIntervalCount,
        minimumSamplingWindowMs:
          measurement.characterization.minimumSamplingWindowMs,
      },
    },
  };
}

export function validateC0BPlan(
  plan,
  trust,
  trustSha256,
  expectedTrustSha256,
  frozen,
  authenticatedPackage,
) {
  const errors = validateTrustManifest(
    trust,
    trustSha256,
    expectedTrustSha256,
    frozen,
    authenticatedPackage,
  );
  exactKeys(
    errors,
    plan,
    [
      "schemaVersion",
      "mode",
      "workItem",
      "phase",
      "trustManifestSha256",
      "contractManifestSha256",
      "packagedFootprint",
      "characterization",
    ],
    "plan",
  );
  exactKeys(
    errors,
    plan?.packagedFootprint,
    ["installerKind"],
    "plan.packagedFootprint",
  );
  exactKeys(
    errors,
    plan?.characterization,
    ["participant"],
    "plan.characterization",
  );
  if (
    plan?.schemaVersion !== 1 ||
    plan?.mode !== "acceptance" ||
    plan?.workItem !== "ADE-0006A" ||
    plan?.phase !== "C0-B" ||
    plan?.trustManifestSha256 !== trustSha256 ||
    plan?.contractManifestSha256 !== frozen.contractManifestSha256 ||
    plan?.packagedFootprint?.installerKind !==
      authenticatedPackage.installerKind ||
    plan?.characterization?.participant !== "candidate"
  ) {
    errors.push("plan header or derived participant selection is invalid");
  }
  inspectSanitized(plan, errors, "plan", ["contractManifestSha256"]);
  return errors;
}

export async function loadPlanInputs({
  planPath,
  trustPath,
  expectedTrustSha256,
  packageReceiptPath,
  packageRawRoot,
  root = defaultWorkspaceRoot,
}) {
  const frozen = loadFrozenContracts(root);
  const trust = parseBoundedJson(
    trustPath,
    1024 * 1024,
    frozen.limits.wire,
    "C0-B trust manifest",
  );
  const plan = parseBoundedJson(
    planPath,
    1024 * 1024,
    frozen.limits.wire,
    "C0-B plan",
  );
  const authenticatedPackage = await authenticatePackageReceipt({
    receiptPath: packageReceiptPath,
    rawRoot: packageRawRoot,
    expectedReceiptSha256: trust.value.packageReceiptSha256,
    installerKind: plan.value.packagedFootprint?.installerKind,
    frozen,
  });
  const errors = validateC0BPlan(
    plan.value,
    trust.value,
    trust.sha256,
    expectedTrustSha256,
    frozen,
    authenticatedPackage,
  );
  return { frozen, trust, plan, authenticatedPackage, errors };
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
  if (
    !process.argv.includes("--dry-run") ||
    Object.values(options).some((value) => !value)
  ) {
    console.error(
      "usage: node scripts/validate-ade0006a-c0b-plan.mjs --dry-run --plan <plan.json> --trust <trusted.json> --expected-trust-sha256 <sha256> --package-receipt <receipt.json> --package-raw-root <raw-root>",
    );
    process.exitCode = 1;
  } else {
    try {
      const bundle = await loadPlanInputs(options);
      if (bundle.errors.length > 0) throw new Error(bundle.errors.join("\n"));
      const contract = deriveC0BContract(bundle.frozen);
      console.log(
        JSON.stringify({
          status: "passed",
          mode: "dry-run",
          planSha256: bundle.plan.sha256,
          trustManifestSha256: bundle.trust.sha256,
          packageReceiptSha256: bundle.authenticatedPackage.receiptSha256,
          packagedStates: contract.packagedFootprint.states.length,
          validPairsPerState: contract.packagedFootprint.validPairsPerState,
          queueBoundaries: bundle.frozen.queueBoundaries.size,
        }),
      );
    } catch (error) {
      console.error(`c0b-plan: ${error.message}`);
      process.exitCode = 1;
    }
  }
}
