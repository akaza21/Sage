import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  exactKeys,
  inspectSanitized,
  parseBoundedJson,
  recordDigest,
  safeSlug,
  sameValue,
} from "./ade0006a-c0b-common.mjs";
import {
  defaultWorkspaceRoot,
  loadFrozenContracts,
} from "./ade0006a-c0b-contracts.mjs";
import { validateTrustManifest } from "./ade0006a-c0b-trust.mjs";

const scriptPath = fileURLToPath(import.meta.url);
export function buildPairSchedule(count) {
  return Array.from({ length: count }, (_, index) => ({
    pairOrdinal: index + 1,
    order:
      index % 2 === 0 ? ["control", "candidate"] : ["candidate", "control"],
  }));
}

function expectedCollection(frozen) {
  const samples = frozen.budgets.sampleContract;
  const idle = frozen.budgets.measurementContract.readyIdle;
  return {
    validPairs: samples.cleanResourceLaunches,
    excludedWarmups:
      frozen.budgets.measurementContract.runOrder.warmupCyclesExcluded,
    percentileMethod: samples.percentileMethod,
    runOrder: frozen.budgets.measurementContract.runOrder.order,
    launchSamplesPerParticipant: samples.cleanResourceLaunches,
    receiptToPaintSamplesPerParticipant: samples.receiptToPaintSamples,
    inputToPaintSamplesPerParticipant: samples.inputToPaintSamples,
    tabSwitchSamplesPerParticipant: samples.inputToPaintSamples,
    cancelDispatchSamplesPerParticipant: samples.inputToPaintSamples,
    readyIdleSamplesPerParticipant:
      samples.cleanResourceLaunches * idle.samplesPerLaunch,
    saturationCyclesPerBoundary: samples.saturationCyclesPerBoundary,
    fairnessCycles: samples.saturationCyclesPerBoundary,
    coalescingSamplesPerPolicy: samples.receiptToPaintSamples,
    coalescingPolicies: ["16ms", "33ms", "50ms", "4kib"],
  };
}

function validateCommandTemplates(errors, plan, trust, frozen) {
  const policy = frozen.evidence.commandTemplateContract;
  if (
    !Array.isArray(plan.commandTemplates) ||
    plan.commandTemplates.length < 1 ||
    plan.commandTemplates.length > policy.maximumEntries
  ) {
    errors.push("commandTemplates count is invalid");
    return;
  }
  const tools = new Map(trust.tools.map((tool) => [tool.name, tool.sha256]));
  plan.commandTemplates.forEach((template, index) => {
    const label = `commandTemplates[${index}]`;
    exactKeys(errors, template, policy.requiredFields, label);
    safeSlug(errors, template?.tool, `${label}.tool`);
    safeSlug(errors, template?.subcommand, `${label}.subcommand`);
    if (template?.toolSha256 !== tools.get(template?.tool)) {
      errors.push(
        `${label}.toolSha256 is not authenticated by the trust manifest`,
      );
    }
    for (const key of ["fixedFlags", "placeholderNames"]) {
      if (!Array.isArray(template?.[key]) || template[key].length > 32) {
        errors.push(`${label}.${key} is invalid`);
      }
    }
    for (const flag of template?.fixedFlags ?? []) {
      if (!/^(?:--?[a-z0-9][a-z0-9-]*|[a-z0-9][a-z0-9._-]*)$/.test(flag)) {
        errors.push(`${label}.fixedFlags may contain option names only`);
      }
    }
    for (const placeholder of template?.placeholderNames ?? []) {
      if (!/^<[a-z][a-z0-9-]{0,62}>$/.test(placeholder)) {
        errors.push(`${label}.placeholderNames is invalid`);
      }
    }
  });
}

export function validateC0BPlan(
  plan,
  trust,
  trustSha256,
  expectedTrustSha256,
  frozen,
) {
  const errors = validateTrustManifest(
    trust,
    trustSha256,
    expectedTrustSha256,
    frozen,
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
      "fixtureManifestSha256",
      "limitsSha256",
      "candidateBindingSha256",
      "controlBindingSha256",
      "provenance",
      "collection",
      "warmupPlan",
      "anchors",
      "processMembership",
      "isolation",
      "commandTemplates",
    ],
    "plan",
  );
  if (
    plan?.schemaVersion !== 1 ||
    plan?.mode !== "dry-run" ||
    plan?.workItem !== "ADE-0006A" ||
    plan?.phase !== "C0-B"
  )
    errors.push("plan header is invalid");
  if (plan?.trustManifestSha256 !== trustSha256)
    errors.push("plan trust hash is stale");
  if (
    plan?.contractManifestSha256 !== frozen.contractManifestSha256 ||
    plan?.fixtureManifestSha256 !== frozen.fixtureManifestSha256 ||
    plan?.limitsSha256 !== frozen.limitsSha256
  )
    errors.push("plan frozen contract binding is stale");
  if (
    plan?.candidateBindingSha256 !== recordDigest(trust.candidate) ||
    plan?.controlBindingSha256 !== recordDigest(trust.control)
  )
    errors.push("plan participant binding is stale");
  validateProvenance(errors, plan, trust);
  const collection = expectedCollection(frozen);
  exactKeys(errors, plan?.collection, Object.keys(collection), "collection");
  sameValue(errors, plan?.collection, collection, "collection");
  const warmups = buildPairSchedule(collection.excludedWarmups).map(
    ({ pairOrdinal, order }) => ({
      warmupOrdinal: pairOrdinal,
      order,
      excluded: true,
    }),
  );
  sameValue(errors, plan?.warmupPlan, warmups, "warmupPlan");
  validateAnchors(errors, plan, frozen);
  sameValue(
    errors,
    plan?.processMembership,
    frozen.budgets.measurementContract.processMembership,
    "processMembership",
  );
  validateIsolation(errors, plan, frozen);
  validateCommandTemplates(errors, plan, trust, frozen);
  inspectSanitized(plan, errors, "plan", ["contractManifestSha256"]);
  return errors;
}

function validateProvenance(errors, plan, trust) {
  const keys = [
    "testOwner",
    "reviewRole",
    "hostClass",
    "hardware",
    "osVersion",
    "platform",
    "architecture",
    "powerPolicy",
    "webViewRuntime",
    "toolchain",
    "collector",
  ];
  exactKeys(errors, plan?.provenance, keys, "provenance");
  sameValue(errors, plan?.provenance, trust.provenance, "provenance");
  for (const key of keys.slice(0, 9))
    safeSlug(errors, plan?.provenance?.[key], `provenance.${key}`);
  if (
    plan?.provenance?.platform !== trust.candidate.platform ||
    plan?.provenance?.architecture !== trust.candidate.architecture
  )
    errors.push("provenance platform/architecture does not match participants");
  exactKeys(
    errors,
    plan?.provenance?.toolchain,
    ["node", "rustc", "cargo", "tauri"],
    "provenance.toolchain",
  );
  for (const value of Object.values(plan?.provenance?.toolchain ?? {}))
    if (!/^[a-z0-9][a-z0-9.+_-]{0,63}$/i.test(value))
      errors.push("provenance.toolchain contains an invalid version");
  exactKeys(
    errors,
    plan?.provenance?.collector,
    ["name", "sha256"],
    "provenance.collector",
  );
  const trustedTool = trust.tools.find(
    (tool) => tool.name === plan?.provenance?.collector?.name,
  );
  if (
    !trustedTool ||
    trustedTool.sha256 !== plan?.provenance?.collector?.sha256
  )
    errors.push("collector is not authenticated by the trust manifest");
}

function validateAnchors(errors, plan, frozen) {
  const contract = frozen.budgets.measurementContract.latencyAnchors;
  const expected = {
    spawnToProcessReady: contract.spawnToProcessReady,
    initialize: contract.initialize,
    firstNormalizedOutput: contract.firstNormalizedOutput,
    nativeReceiptToPaint: contract.nativeReceiptToPaint,
    inputToPaint: contract.inputToPaint,
    tabSwitch:
      "trusted tab activation through browser performance.now mark after resulting paint",
    cancelDispatch: contract.cancelDispatch,
    cancelTerminal: contract.cancelTerminal,
    coldActivation: "activation receipt through durable ready projection",
    explicitClose: contract.explicitClose,
    forcedTeardown: "forced termination start through zero owned descendants",
  };
  exactKeys(errors, plan?.anchors, Object.keys(expected), "anchors");
  sameValue(errors, plan?.anchors, expected, "anchors");
}

function validateIsolation(errors, plan, frozen) {
  const expected = {
    isolatedData: true,
    normalSageDataAllowed: frozen.evidence.rootPolicy.normalSageDataAllowed,
    defaultPackageArtifactsMutable:
      frozen.evidence.rootPolicy.defaultPackageArtifactsMutable,
    systemOrNetworkControlsAllowed:
      frozen.evidence.rootPolicy.systemOrNetworkControlsAllowed,
    installRequired: frozen.gestures.interactionPolicy.installRequired,
  };
  exactKeys(errors, plan?.isolation, Object.keys(expected), "isolation");
  sameValue(errors, plan?.isolation, expected, "isolation");
}

export function loadPlanInputs(
  planPath,
  trustPath,
  expectedTrustSha256,
  root = defaultWorkspaceRoot,
) {
  const frozen = loadFrozenContracts(root);
  const trust = parseBoundedJson(
    trustPath,
    1024 * 1024,
    frozen.limits.wire,
    "C0-B trust manifest",
  );
  const plan = parseBoundedJson(
    planPath,
    2 * 1024 * 1024,
    frozen.limits.wire,
    "C0-B plan",
  );
  const errors = validateC0BPlan(
    plan.value,
    trust.value,
    trust.sha256,
    expectedTrustSha256,
    frozen,
  );
  return { frozen, trust, plan, errors };
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  const planPath = argument("--plan");
  const trustPath = argument("--trust");
  const expectedTrust = argument("--expected-trust-sha256");
  if (
    !process.argv.includes("--dry-run") ||
    !planPath ||
    !trustPath ||
    !expectedTrust
  ) {
    console.error(
      "usage: node scripts/validate-ade0006a-c0b-plan.mjs --dry-run --plan <plan.json> --trust <trusted.json> --expected-trust-sha256 <sha256>",
    );
    process.exitCode = 1;
  } else {
    try {
      const bundle = loadPlanInputs(planPath, trustPath, expectedTrust);
      if (bundle.errors.length > 0) throw new Error(bundle.errors.join("\n"));
      console.log(
        JSON.stringify({
          status: "passed",
          mode: "dry-run",
          planSha256: bundle.plan.sha256,
          trustManifestSha256: bundle.trust.sha256,
          contractManifestSha256: bundle.frozen.contractManifestSha256,
          fixtureManifestSha256: bundle.frozen.fixtureManifestSha256,
          queueBoundaries: [...bundle.frozen.queueBoundaries.keys()],
          schedule: buildPairSchedule(bundle.plan.value.collection.validPairs),
        }),
      );
    } catch (error) {
      console.error(`c0b-plan: ${error.message}`);
      process.exitCode = 1;
    }
  }
}
