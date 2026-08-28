import path from "node:path";
import { fileURLToPath } from "node:url";

import { withAdeCampaignAdmission } from "./ade0006a-admission.mjs";
import { parseBoundedJson, sha256 } from "./ade0006a-c0b-common.mjs";
import {
  defaultWorkspaceRoot,
  loadFrozenContracts,
} from "./ade0006a-c0b-contracts.mjs";
import { selectPackagedObservationExecutable } from "./ade0006a-c0b-package-reference.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const bindJsonLimits = {
  jsonDepth: 32,
  jsonNodes: 16384,
  objectMembers: 256,
  arrayEntries: 512,
  decodedKeyBytes: 256,
};
const STATE_PLAN_BYTES = 65_536;

function digestCampaignStatePlan(statePlan) {
  if (statePlan == null || typeof statePlan !== "object") {
    throw new Error("campaign-state-plan-invalid");
  }
  return sha256(JSON.stringify(statePlan));
}

function unadmitted() {
  throw new Error("campaign-state-plan-unadmitted");
}

function resolveAdmittedStatePlanFile(sourceRoot, stateId) {
  if (typeof sourceRoot !== "string" || !path.isAbsolute(sourceRoot)) {
    unadmitted();
  }
  if (
    typeof stateId !== "string" ||
    stateId.includes("/") ||
    stateId.includes("\\") ||
    stateId.includes("..")
  ) {
    unadmitted();
  }
  const relativePath = `scripts/c0b-state-plans/${stateId}.json`;
  const absolute = path.resolve(sourceRoot, ...relativePath.split("/"));
  const relative = path.relative(sourceRoot, absolute);
  if (
    relative === "" ||
    relative.startsWith("..") ||
    path.isAbsolute(relative)
  ) {
    unadmitted();
  }
  return { absolute, relativePath };
}

export function authenticateAdmittedCampaignStatePlan({
  sourceRoot,
  frozen,
  stateId,
}) {
  const states = frozen?.budgets?.sampleContract?.packagedStates;
  if (!Array.isArray(states) || !states.includes(stateId)) unadmitted();
  try {
    const { absolute, relativePath } = resolveAdmittedStatePlanFile(
      sourceRoot,
      stateId,
    );
    const parsed = parseBoundedJson(
      absolute,
      STATE_PLAN_BYTES,
      bindJsonLimits,
      stateId,
    );
    return Object.freeze({
      stateId,
      path: relativePath,
      statePlanSha256: digestCampaignStatePlan(parsed.value),
    });
  } catch (error) {
    if (
      error instanceof Error &&
      error.message === "campaign-state-plan-unadmitted"
    ) {
      throw error;
    }
    unadmitted();
  }
}

export function readAdmittedCampaignStatePlan({ sourceRoot, frozen, stateId }) {
  const admitted = authenticateAdmittedCampaignStatePlan({
    sourceRoot,
    frozen,
    stateId,
  });
  const parsed = parseBoundedJson(
    path.resolve(sourceRoot, ...admitted.path.split("/")),
    STATE_PLAN_BYTES,
    bindJsonLimits,
    stateId,
  );
  if (digestCampaignStatePlan(parsed.value) !== admitted.statePlanSha256) {
    unadmitted();
  }
  return Object.freeze({
    ...admitted,
    statePlan: parsed.value,
  });
}

export async function composeC0BCampaignObservationLaunch({
  sourceRoot,
  frozen,
  authenticatedPackage,
  artifacts,
  participant,
  stateId,
}) {
  const admitted = authenticateAdmittedCampaignStatePlan({
    sourceRoot,
    frozen,
    stateId,
  });
  return withAdeCampaignAdmission({
    sourceRoot,
    frozen,
    phase: "C0-B",
    action: async () =>
      Object.freeze({
        participant,
        executable: selectPackagedObservationExecutable({
          authenticatedPackage,
          artifacts,
          participant,
        }),
        stateId: admitted.stateId,
        statePlanSha256: admitted.statePlanSha256,
      }),
  });
}

export function bindC0BCampaignObservationRequest({ request, launch }) {
  const requestExecutable = request?.executable;
  const launchExecutable = launch?.executable;
  if (
    request?.participant !== launch?.participant ||
    requestExecutable?.path !== launchExecutable?.path ||
    requestExecutable?.sha256 !== launchExecutable?.sha256 ||
    requestExecutable?.bytes !== launchExecutable?.bytes ||
    requestExecutable?.bindingSha256 !== launchExecutable?.bindingSha256
  ) {
    throw new Error("packaged-observation-executable-mismatch");
  }
  if (request?.stateId !== launch?.stateId) {
    throw new Error("campaign-state-plan-mismatch");
  }
  if (digestCampaignStatePlan(request?.statePlan) !== launch?.statePlanSha256) {
    throw new Error("campaign-state-plan-mismatch");
  }
  if (requestExecutable && typeof requestExecutable === "object") {
    Object.freeze(requestExecutable);
  }
  return Object.freeze(request);
}

function bindCampaignObservationFiles(requestPath, launchPath) {
  if (
    typeof requestPath !== "string" ||
    typeof launchPath !== "string" ||
    !path.isAbsolute(requestPath) ||
    !path.isAbsolute(launchPath)
  ) {
    throw new Error("campaign observation bind paths must be absolute");
  }
  return bindC0BCampaignObservationRequest({
    request: parseBoundedJson(requestPath, 1_048_576, bindJsonLimits, "request")
      .value,
    launch: parseBoundedJson(launchPath, 1_048_576, bindJsonLimits, "launch")
      .value,
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  try {
    const [command, first, second] = process.argv.slice(2);
    if (command === "plan" && process.argv.length === 4) {
      const frozen = loadFrozenContracts(defaultWorkspaceRoot);
      const admitted = readAdmittedCampaignStatePlan({
        sourceRoot: defaultWorkspaceRoot,
        frozen,
        stateId: first,
      });
      process.stdout.write(`${JSON.stringify(admitted.statePlan)}\n`);
    } else if (command === "bind" && process.argv.length === 5) {
      process.stdout.write(
        `${JSON.stringify(bindCampaignObservationFiles(first, second))}\n`,
      );
    } else {
      throw new Error(
        "usage: bind <request-json> <launch-json> | plan <stateId>",
      );
    }
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : error}\n`);
    process.exitCode = 1;
  }
}
