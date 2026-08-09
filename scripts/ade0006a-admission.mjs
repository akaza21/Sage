import { mkdir, rmdir } from "node:fs/promises";
import path from "node:path";

import { exactKeys } from "./ade0006a-c0b-common.mjs";

const phases = ["Package", "C0-B"];

function admissionPolicy(frozen, phase) {
  const policy = frozen?.evidence?.adeCampaignAdmission;
  const packagePolicy = frozen?.evidence?.packageCampaignRetention;
  const errors = [];
  exactKeys(
    errors,
    policy,
    ["root", "admissionLock", "serializedPhases", "rule"],
    "adeCampaignAdmission",
  );
  if (
    errors.length > 0 ||
    policy?.root !== "local-evidence/ade0006a" ||
    policy?.admissionLock !== `${policy?.root}/.admission-lock` ||
    JSON.stringify(policy?.serializedPhases) !== JSON.stringify(phases) ||
    packagePolicy?.admissionLock !== policy?.admissionLock ||
    typeof policy?.rule !== "string" ||
    policy.rule.length < 1 ||
    !phases.includes(phase)
  ) {
    throw new Error("admission-policy-invalid");
  }
  return policy;
}

export async function withAdeCampaignAdmission({
  sourceRoot,
  frozen,
  phase,
  action,
}) {
  const policy = admissionPolicy(frozen, phase);
  if (typeof action !== "function") {
    throw new Error("admission-policy-invalid");
  }
  const campaignRoot = path.resolve(sourceRoot, ...policy.root.split("/"));
  const lockPath = path.resolve(sourceRoot, ...policy.admissionLock.split("/"));
  if (path.dirname(lockPath) !== campaignRoot) {
    throw new Error("admission-policy-invalid");
  }
  await mkdir(campaignRoot, { recursive: true });
  try {
    await mkdir(lockPath);
  } catch (error) {
    if (error.code === "EEXIST") {
      throw new Error("capacity-rejected", { cause: error });
    }
    throw error;
  }
  try {
    return await action(campaignRoot);
  } finally {
    await rmdir(lockPath);
  }
}
