import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const BETA_CANDIDATE_CONTRACT_SHA256 =
  "a8c2606707852c333394af5e9cb9ca3b8d504c1ecdfb2a0e964b8721b762660f";

const scriptsDirectory = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.dirname(scriptsDirectory);
const contractPath = path.join(
  repositoryRoot,
  "docs",
  "work-items",
  "BETA-0001-primary-beta-path",
  "contracts",
  "beta-candidate-evidence-v1.json",
);

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function assert(condition, message) {
  if (!condition)
    throw new Error(`BETA-0001 candidate matrix invalid: ${message}`);
}

function sameKeys(value, keys) {
  return (
    value &&
    !Array.isArray(value) &&
    typeof value === "object" &&
    JSON.stringify(Object.keys(value).sort()) ===
      JSON.stringify([...keys].sort())
  );
}

function metricSource(threshold, entry) {
  if (entry.id === "performance.resource-storage")
    return threshold.codexBudgets;
  if (entry.id === "security.authority-secrets") {
    return { ...threshold.codexJourney, ...threshold.codexBudgets };
  }
  return threshold.codexJourney;
}

function validateContract(contract, threshold) {
  assert(
    sameKeys(contract, [
      "schemaVersion",
      "status",
      "workItem",
      "thresholdAuthority",
      "limits",
      "requiredEvidence",
    ]),
    "top-level keys",
  );
  assert(
    contract.schemaVersion === 1 &&
      contract.status === "frozen-verdict-matrix" &&
      contract.workItem === "BETA-0001",
    "header",
  );
  assert(
    sameKeys(contract.limits, [
      "resultBytesMaximum",
      "evidenceBytesMaximum",
      "evidenceAggregateBytesMaximum",
      "jsonDepthMaximum",
      "jsonNodesMaximum",
      "stringBytesMaximum",
      "relativePathBytesMaximum",
      "evidenceCount",
    ]),
    "limits keys",
  );
  const expectedLimits = [262144, 1048576, 4194304, 32, 16384, 4096, 512, 20];
  assert(
    Object.values(contract.limits).every(Number.isSafeInteger) &&
      JSON.stringify(Object.values(contract.limits)) ===
        JSON.stringify(expectedLimits),
    "limits",
  );
  assert(
    Array.isArray(contract.requiredEvidence) &&
      contract.requiredEvidence.length === 20,
    "evidence count",
  );
  const ids = new Set();
  for (const entry of contract.requiredEvidence) {
    assert(
      sameKeys(entry, ["id", "metrics"]) &&
        /^[a-z0-9]+(?:[.-][a-z0-9]+)*$/u.test(entry.id) &&
        !ids.has(entry.id),
      "evidence descriptor",
    );
    ids.add(entry.id);
    assert(Array.isArray(entry.metrics), "metrics array");
    const names = new Set();
    for (const metric of entry.metrics) {
      assert(
        sameKeys(metric, ["name", "operator", "thresholdField"]) &&
          typeof metric.name === "string" &&
          !names.has(metric.name) &&
          ["minimum", "maximum"].includes(metric.operator),
        "metric descriptor",
      );
      names.add(metric.name);
      const value = metricSource(threshold, entry)[metric.thresholdField];
      assert(Number.isFinite(value) && value >= 0, "metric threshold field");
    }
  }
}

async function loadContract() {
  const bytes = await readFile(contractPath);
  assert(sha256(bytes) === BETA_CANDIDATE_CONTRACT_SHA256, "matrix bytes");
  const contract = JSON.parse(bytes.toString("utf8"));
  assert(
    sameKeys(contract.thresholdAuthority, ["relativePath", "bytes", "sha256"]),
    "threshold authority keys",
  );
  const authority = contract.thresholdAuthority;
  assert(
    authority.relativePath === "beta-acceptance-v1.json" &&
      Number.isSafeInteger(authority.bytes) &&
      /^[0-9a-f]{64}$/u.test(authority.sha256),
    "threshold authority",
  );
  const thresholdPath = path.resolve(
    path.dirname(contractPath),
    authority.relativePath,
  );
  assert(
    path.dirname(thresholdPath) === path.dirname(contractPath),
    "threshold authority path",
  );
  const thresholdBytes = await readFile(thresholdPath);
  assert(
    thresholdBytes.length === authority.bytes &&
      sha256(thresholdBytes) === authority.sha256,
    "threshold authority bytes",
  );
  const threshold = JSON.parse(thresholdBytes.toString("utf8"));
  validateContract(contract, threshold);
  return Object.freeze({
    contract: Object.freeze(contract),
    threshold: Object.freeze(threshold),
  });
}

const loaded = await loadContract();
export const BETA_CANDIDATE_CONTRACT = loaded.contract;
export const BETA_ACCEPTANCE_THRESHOLD = loaded.threshold;
export const BETA_CANDIDATE_REQUIRED_IDS = Object.freeze(
  BETA_CANDIDATE_CONTRACT.requiredEvidence.map(({ id }) => id),
);

export function betaCandidateMetricRules(id) {
  const entry = BETA_CANDIDATE_CONTRACT.requiredEvidence.find(
    (item) => item.id === id,
  );
  const source = metricSource(BETA_ACCEPTANCE_THRESHOLD, entry);
  return entry.metrics.map((metric) => ({
    ...metric,
    value: source[metric.thresholdField],
  }));
}
