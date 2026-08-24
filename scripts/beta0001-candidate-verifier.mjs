import {
  BETA_CANDIDATE_CONTRACT,
  BETA_CANDIDATE_REQUIRED_IDS,
  betaCandidateMetricRules,
} from "./beta0001-candidate-contract.mjs";
import {
  inspectBetaCandidateJson,
  readBoundedPlainEvidenceFile,
} from "./beta0001-candidate-bounds.mjs";

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

function parseJson(bytes) {
  let value;
  try {
    value = JSON.parse(Buffer.from(bytes).toString("utf8"));
  } catch {
    reject("malformed-json");
  }
  inspectBetaCandidateJson(value, BETA_CANDIDATE_CONTRACT.limits);
  return value;
}

function validCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40}$/u.test(value);
}

function validCandidate(value) {
  return (
    exactKeys(value, ["platform", "architecture", "sourceCommit"]) &&
    value.platform === "windows" &&
    value.architecture === "x86_64" &&
    validCommit(value.sourceCommit)
  );
}

function validateAggregate(result, expectedSourceCommit) {
  if (
    !exactKeys(result, [
      "schemaVersion",
      "workItem",
      "contractSha256",
      "candidate",
      "evidence",
      "verdict",
    ]) ||
    result.schemaVersion !== 1 ||
    result.workItem !== "BETA-0001" ||
    !validCandidate(result.candidate) ||
    !["passed", "failed", "incomplete"].includes(result.verdict)
  ) {
    reject("schema-invalid");
  }
  if (result.candidate.sourceCommit !== expectedSourceCommit)
    reject("candidate-stale");
  if (
    result.contractSha256 !== BETA_CANDIDATE_CONTRACT.thresholdAuthority.sha256
  )
    reject("contract-drift");
  if (!Array.isArray(result.evidence)) reject("schema-invalid");
  if (result.evidence.length !== BETA_CANDIDATE_CONTRACT.limits.evidenceCount)
    reject("evidence-count");
  const seen = new Set();
  for (const [index, descriptor] of result.evidence.entries()) {
    if (
      !exactKeys(descriptor, ["id", "status"]) ||
      typeof descriptor.id !== "string" ||
      !["passed", "failed", "missing"].includes(descriptor.status)
    )
      reject("schema-invalid");
    if (seen.has(descriptor.id)) reject("duplicate-evidence");
    seen.add(descriptor.id);
    if (!BETA_CANDIDATE_REQUIRED_IDS.includes(descriptor.id))
      reject("unknown-evidence");
    if (descriptor.id !== BETA_CANDIDATE_REQUIRED_IDS[index])
      reject("evidence-order");
  }
}

function validateDocument(document, descriptor, aggregate) {
  if (
    !exactKeys(document, [
      "schemaVersion",
      "evidenceId",
      "contractSha256",
      "candidate",
      "status",
      "metrics",
    ]) ||
    document.schemaVersion !== 1 ||
    document.evidenceId !== descriptor.id ||
    !validCandidate(document.candidate) ||
    !["passed", "failed"].includes(document.status) ||
    !document.metrics ||
    Array.isArray(document.metrics) ||
    typeof document.metrics !== "object"
  ) {
    reject("schema-invalid");
  }
  if (document.contractSha256 !== aggregate.contractSha256)
    reject("contract-drift");
  if (document.candidate.sourceCommit !== aggregate.candidate.sourceCommit)
    reject("candidate-drift");
  if (document.status !== descriptor.status) reject("status-drift");
}

function metricsFail(document, id) {
  const rules = betaCandidateMetricRules(id);
  if (
    !exactKeys(
      document.metrics,
      rules.map(({ name }) => name),
    )
  )
    reject("schema-invalid");
  let failed = false;
  for (const rule of rules) {
    const value = document.metrics[rule.name];
    const allowsFraction = rule.name === "readyIdleCpuSingleCorePercentP95";
    if (
      !Number.isFinite(value) ||
      value < 0 ||
      (!allowsFraction && !Number.isSafeInteger(value))
    ) {
      reject("metric-invalid");
    }
    if (
      (rule.operator === "minimum" && value < rule.value) ||
      (rule.operator === "maximum" && value > rule.value)
    )
      failed = true;
  }
  return failed;
}

async function optionalEvidenceBytes(root, id, maximumBytes) {
  try {
    return await readBoundedPlainEvidenceFile({ root, id, maximumBytes });
  } catch (error) {
    if (error?.code === "ENOENT") return undefined;
    throw error;
  }
}

export async function validateBetaCandidateEvidence({
  resultBytes,
  evidenceRoot,
  expectedSourceCommit,
}) {
  if (!validCommit(expectedSourceCommit)) reject("schema-invalid");
  if (!(Buffer.isBuffer(resultBytes) || resultBytes instanceof Uint8Array))
    reject("schema-invalid");
  if (
    resultBytes.byteLength > BETA_CANDIDATE_CONTRACT.limits.resultBytesMaximum
  )
    reject("result-oversize");
  const result = parseJson(resultBytes);
  validateAggregate(result, expectedSourceCommit);

  let incomplete = false;
  let failed = false;
  let aggregateBytes = 0;
  for (const descriptor of result.evidence) {
    const bytes = await optionalEvidenceBytes(
      evidenceRoot,
      descriptor.id,
      BETA_CANDIDATE_CONTRACT.limits.evidenceBytesMaximum,
    );
    if (bytes === undefined) {
      if (descriptor.status !== "missing") reject("status-drift");
      incomplete = true;
      continue;
    }
    if (descriptor.status === "missing") reject("status-drift");
    aggregateBytes += bytes.byteLength;
    if (
      aggregateBytes >
      BETA_CANDIDATE_CONTRACT.limits.evidenceAggregateBytesMaximum
    ) {
      reject("evidence-aggregate-oversize");
    }
    const document = parseJson(bytes);
    validateDocument(document, descriptor, result);
    const metricFailure = metricsFail(document, descriptor.id);
    failed ||= document.status === "failed" || metricFailure;
  }
  const verdict = incomplete ? "incomplete" : failed ? "failed" : "passed";
  if (result.verdict !== verdict) reject("status-drift");
  return { ...result, verdict };
}
