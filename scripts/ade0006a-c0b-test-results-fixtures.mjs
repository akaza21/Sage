import { sha256 } from "./ade0006a-c0b-common.mjs";
import { makeObservationRecords } from "./ade0006a-c0b-test-observations.mjs";
import {
  makePackageReference,
  makePlan,
  makeTrust,
} from "./ade0006a-c0b-test-plan-fixtures.mjs";
import {
  deriveC0BContract,
  validateC0BPlan,
} from "./validate-ade0006a-c0b-plan.mjs";
import { evaluateC0BResults } from "./validate-ade0006a-c0b-results.mjs";

export function makeC0BFixture(frozen) {
  const frequencyHz = 1000;
  const authenticatedPackage = makePackageReference(frozen);
  const trust = makeTrust(frozen, authenticatedPackage);
  const trustText = JSON.stringify(trust);
  const trustSha256 = sha256(trustText);
  const plan = makePlan(trustSha256, frozen, authenticatedPackage);
  const planText = JSON.stringify(plan);
  const planSha256 = sha256(planText);
  const bundle = {
    frozen,
    contract: deriveC0BContract(frozen),
    authenticatedPackage,
    trust: {
      value: trust,
      sha256: trustSha256,
      bytes: Buffer.byteLength(trustText),
    },
    plan: {
      value: plan,
      sha256: planSha256,
      bytes: Buffer.byteLength(planText),
    },
    errors: validateC0BPlan(
      plan,
      trust,
      trustSha256,
      trustSha256,
      frozen,
      authenticatedPackage,
    ),
  };
  const observations = makeObservationRecords(
    frozen,
    trust,
    authenticatedPackage,
    frequencyHz,
  );
  const observationFiles = [...observations].map(
    ([relativePath, { bytes, sha256: digest }]) => ({
      relativePath,
      bytes,
      sha256: digest,
    }),
  );
  const rawBytes = observationFiles.reduce((sum, { bytes }) => sum + bytes, 0);
  const results = {
    schemaVersion: 1,
    workItem: "ADE-0006A",
    phase: "C0-B",
    sanitized: true,
    result: "passed",
    timestamps: {
      startedAtUtc: "2026-08-07T00:00:00.000Z",
      completedAtUtc: "2026-08-07T23:59:59.999Z",
    },
    limitations: [],
    planSha256,
    trustManifestSha256: trustSha256,
    contractManifestSha256: frozen.contractManifestSha256,
    packageReceiptSha256: authenticatedPackage.receiptSha256,
    monotonicClock: {
      kind: "query-performance-counter",
      frequencyHz,
    },
    packagedFootprint: {
      installerKind: authenticatedPackage.installerKind,
      states: frozen.budgets.sampleContract.packagedStates.map((id) => ({
        id,
        observationFile: `footprint-${id}.json`,
      })),
      invalidAttempts: [],
    },
    characterization: {
      participant: "candidate",
      invalidAttempts: [],
    },
    observationFiles,
    gateEvaluation: {
      method: "derived-v1",
      gates: [],
    },
    evidence: {
      rawFiles: observationFiles.length,
      rawBytes,
      campaignsForCandidateIdentity: 1,
      rawBytesForCandidateIdentity: rawBytes,
      retainedCandidateIdentities: 2,
      globalFiles: observationFiles.length,
      globalBytes: rawBytes,
      curatedBytes: 0,
      compactedCuratedFiles: 1,
      compactedCuratedBytes: 10_000,
    },
    redaction: {
      scanPerformed: true,
      violations: [],
      rawProtocolFramesRetained: 0,
      secretsRetained: 0,
      commandLinesRetained: 0,
      absolutePathsRetained: 0,
    },
  };
  results.gateEvaluation.gates = evaluateC0BResults(
    bundle,
    results,
    Buffer.byteLength(JSON.stringify(results)),
    observations,
  ).gates;
  results.result = results.gateEvaluation.gates.every(({ passed }) => passed)
    ? "passed"
    : "failed";
  let previous = -1;
  while (results.evidence.curatedBytes !== previous) {
    previous = results.evidence.curatedBytes;
    results.evidence.curatedBytes = Buffer.byteLength(JSON.stringify(results));
  }
  return {
    authenticatedPackage,
    trust,
    trustText,
    trustSha256,
    plan,
    planText,
    planSha256,
    bundle,
    observations,
    results,
  };
}

export function refreshObservationRecord(fixture, relativePath) {
  const record = fixture.observations.get(relativePath);
  record.buffer = Buffer.from(JSON.stringify(record.value));
  record.bytes = record.buffer.byteLength;
  record.sha256 = sha256(record.buffer);
  const descriptor = fixture.results.observationFiles.find(
    (entry) => entry.relativePath === relativePath,
  );
  descriptor.bytes = record.bytes;
  descriptor.sha256 = record.sha256;
  fixture.results.evidence.rawBytes = [...fixture.observations.values()].reduce(
    (sum, item) => sum + item.bytes,
    0,
  );
  fixture.results.evidence.rawBytesForCandidateIdentity =
    fixture.results.evidence.rawBytes;
  fixture.results.evidence.globalBytes = fixture.results.evidence.rawBytes;
}

export function finalizeResults(fixture) {
  const evaluated = evaluateC0BResults(
    fixture.bundle,
    fixture.results,
    Buffer.byteLength(JSON.stringify(fixture.results)),
    fixture.observations,
  );
  fixture.results.gateEvaluation.gates = evaluated.gates;
  fixture.results.result = evaluated.gates.every(({ passed }) => passed)
    ? "passed"
    : "failed";
  let previous = -1;
  while (fixture.results.evidence.curatedBytes !== previous) {
    previous = fixture.results.evidence.curatedBytes;
    fixture.results.evidence.curatedBytes = Buffer.byteLength(
      JSON.stringify(fixture.results),
    );
  }
}
