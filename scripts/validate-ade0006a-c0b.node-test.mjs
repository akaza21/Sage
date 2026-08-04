import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  parseBoundedJson,
  sameFileIdentity,
  sha256,
} from "./ade0006a-c0b-common.mjs";
import {
  defaultWorkspaceRoot,
  loadFrozenContracts,
} from "./ade0006a-c0b-contracts.mjs";
import { serializedFixture } from "./ade0006a-c0b-test-fixtures.mjs";
import { validateC0BPlan } from "./validate-ade0006a-c0b-plan.mjs";
import { validateC0BResults } from "./validate-ade0006a-c0b-results.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const frozen = loadFrozenContracts(root);

function fixture() {
  const value = serializedFixture(frozen);
  value.bundle = {
    frozen,
    trust: {
      value: value.trust,
      sha256: value.trustSha256,
      bytes: Buffer.byteLength(value.trustText),
    },
    plan: {
      value: value.plan,
      sha256: value.planSha256,
      bytes: Buffer.byteLength(value.planText),
    },
    errors: validateC0BPlan(
      value.plan,
      value.trust,
      value.trustSha256,
      value.trustSha256,
      frozen,
    ),
  };
  return value;
}

function resultErrors(value) {
  return validateC0BResults(
    value.bundle,
    value.results,
    Buffer.byteLength(JSON.stringify(value.results)),
  );
}

test("frozen contracts authenticate every loaded member and derive queue limits", () => {
  assert.equal(frozen.contractManifestSha256.length, 64);
  assert.equal(
    frozen.fixtureManifestSha256,
    frozen.manifest.files.find(
      ({ relativePath }) => relativePath === "fixture-vectors-v1.json",
    ).sha256,
  );
  assert.equal(frozen.queueBoundaries.size, 14);
  assert.deepEqual(frozen.queueBoundaries.get("egressDataFrames"), {
    kind: "count",
    count: 56,
    bytes: 1_835_008,
  });
});

test("valid independently anchored plan and integrated result pass", () => {
  const value = fixture();
  assert.deepEqual(value.bundle.errors, []);
  assert.deepEqual(resultErrors(value), []);
});

test("bounded reader rejects size before read and frozen JSON structural limits", () => {
  const directory = mkdtempSync(path.join(tmpdir(), "sage-c0b-bounds-"));
  try {
    assert.equal(
      sameFileIdentity({ dev: 1, ino: 2 }, { dev: 1, ino: 3 }),
      false,
    );
    assert.throws(
      () => parseBoundedJson(directory, 1024, frozen.limits.wire, "fixture"),
      /regular non-symlink file/,
    );
    const file = path.join(directory, "input.json");
    writeFileSync(file, " ".repeat(1025));
    assert.throws(
      () => parseBoundedJson(file, 1024, frozen.limits.wire, "fixture"),
      /must contain 1\.\.1024 bytes/,
    );
    for (const value of [
      Array.from({ length: 513 }, () => 0),
      Object.fromEntries(
        Array.from({ length: 257 }, (_, index) => [`k${index}`, 0]),
      ),
    ]) {
      writeFileSync(file, JSON.stringify(value));
      assert.throws(
        () => parseBoundedJson(file, 100_000, frozen.limits.wire, "fixture"),
        /exceeds/,
      );
    }
    writeFileSync(file, `${"[".repeat(33)}0${"]".repeat(33)}`);
    assert.throws(
      () => parseBoundedJson(file, 100_000, frozen.limits.wire, "fixture"),
      /depth/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("plan rejects stale trust, source, contract, fixture, tool, and platform identity", () => {
  const mutations = [
    (value) => (value.expectedTrust = "0".repeat(64)),
    (value) => (value.trust.candidate.sourceManifestSha256 = "0".repeat(64)),
    (value) => (value.trust.candidate.contractManifestSha256 = "0".repeat(64)),
    (value) => (value.plan.fixtureManifestSha256 = "0".repeat(64)),
    (value) => (value.plan.limitsSha256 = "0".repeat(64)),
    (value) => (value.plan.commandTemplates[0].toolSha256 = "0".repeat(64)),
    (value) => (value.trust.control.platform = "linux"),
  ];
  for (const mutate of mutations) {
    const value = fixture();
    value.expectedTrust = value.trustSha256;
    mutate(value);
    const errors = validateC0BPlan(
      value.plan,
      value.trust,
      value.trustSha256,
      value.expectedTrust,
      frozen,
    );
    assert.notEqual(errors.length, 0);
  }
});

test("results reject warmup, pair, process, and sample provenance gaps", () => {
  const mutations = [
    (value) => value.results.warmupExclusions.pop(),
    (value) => (value.results.validPairs[0].provenanceSha256 = "0".repeat(64)),
    (value) =>
      (value.results.validPairs[0].participants.candidate.creationIdentitySha256 =
        "0".repeat(64)),
    (value) => (value.results.latencyMs.initialize.samples[0].pairOrdinal = 5),
    (value) => value.results.latencyMs.spawnToProcessReady.samples.pop(),
    (value) =>
      (value.results.resources.readyIdleCpuSingleCorePercent.anchor = "wrong"),
    (value) =>
      (value.results.resources.fixtureTreePrivateBytes.anchor = "wrong"),
    (value) =>
      (value.results.resources.sageNativeAndRendererIncrementalPrivateBytes.anchor =
        "wrong"),
    (value) => {
      value.results.invalidPairs = [
        {
          attemptOrdinal: 1,
          order: ["candidate", "control"],
          reason: "launch-failure",
          replacementValidPairOrdinal: 1,
          participantsRecorded: false,
        },
        {
          attemptOrdinal: 1,
          order: ["control", "candidate"],
          reason: "launch-failure",
          replacementValidPairOrdinal: 1,
          participantsRecorded: false,
        },
      ];
    },
  ];
  for (const mutate of mutations) {
    const value = fixture();
    mutate(value);
    assert.notEqual(resultErrors(value).length, 0);
  }
});

test("results require complete queue, fairness, control-lane, and coalescing evidence", () => {
  const mutations = [
    (value) => value.results.queueSaturation.pop(),
    (value) =>
      value.results.queueSaturation.push(
        structuredClone(value.results.queueSaturation[0]),
      ),
    (value) =>
      (value.results.queueSaturation[0].cycles[0].exact.retainedBytes = 0),
    (value) =>
      (value.results.queueSaturation[0].cycles[0].controlLaneDispatchMs = 101),
    (value) => (value.results.fairness.observations[0].zeroStarved = false),
    (value) => value.results.coalescing.policies.pop(),
    (value) =>
      (value.results.coalescing.policies[0].sqliteWritesPerTextDelta = 1),
    (value) => (value.results.coalescing.selectedPolicy = "50ms"),
  ];
  for (const mutate of mutations) {
    const value = fixture();
    mutate(value);
    assert.notEqual(resultErrors(value).length, 0);
  }
});

test("results require WAL convergence and authenticated source/build/package deltas", () => {
  const mutations = [
    (value) =>
      (value.results.persistence.markers[0].candidate.walBytesAfterCheckpoint = 16_777_217),
    (value) =>
      (value.results.persistence.markers[0].candidate.walConverged = false),
    (value) =>
      (value.results.package.candidate.dependencyGraphSha256 = "0".repeat(64)),
    (value) => (value.results.package.deltas.sourceBytes += 1),
    (value) => (value.results.package.runtimeDownloadBytes = 1),
  ];
  for (const mutate of mutations) {
    const value = fixture();
    mutate(value);
    assert.notEqual(resultErrors(value).length, 0);
  }
});

test("results require exact semantic gesture oracles, timestamps, and bounded evidence", () => {
  const mutations = [
    (value) => value.results.gestures.cases.pop(),
    (value) => value.results.gestures.cases[0].observedOracle.pop(),
    (value) => (value.results.gestures.cases[0].semanticAutomation = false),
    (value) =>
      (value.results.gestures.cases[0].startedAtUtc =
        "2026-07-29T23:59:59.999Z"),
    (value) =>
      (value.results.gestures.cases[0].startedAtUtc =
        "2026-02-31T00:00:00.000Z"),
    (value) => (value.results.timestamps.completedAtUtc = "bad"),
    (value) =>
      (value.results.timestamps.startedAtUtc = "2026-02-31T00:00:00.000Z"),
    (value) => (value.results.evidence.rawFiles = 65),
    (value) => (value.results.redaction.absolutePathsRetained = 1),
    (value) => (value.results.resources.orphanProcessesAfterClose = 1),
  ];
  for (const mutate of mutations) {
    const value = fixture();
    mutate(value);
    assert.notEqual(resultErrors(value).length, 0);
  }
});

test("sanitization rejects embedded drive, NT, root-relative paths and command lines", () => {
  for (const limitation of [
    "host=C:\\Users\\person\\sage.db",
    "host=\\\\?\\C:\\secret",
    "host=\\??\\C:\\secret",
    "host=\\Users\\person\\sage.db",
    "host=/tmp",
    "host=/project/repository/sage.db",
    "host=file:///C:/Users/person/sage.db",
    "ran node --token=value",
    "ran node private-suite.mjs",
    "ran python private-suite.py",
    "ran git status",
  ]) {
    const value = fixture();
    value.results.limitations = [limitation];
    assert.notEqual(resultErrors(value).length, 0);
  }
});

test("public dry-run CLIs validate bound fixtures and reject an untrusted hash", () => {
  const directory = mkdtempSync(path.join(tmpdir(), "sage-c0b-cli-"));
  try {
    const value = fixture();
    const planPath = path.join(directory, "plan.json");
    const trustPath = path.join(directory, "trust.json");
    const resultsPath = path.join(directory, "results.json");
    writeFileSync(planPath, value.planText);
    writeFileSync(trustPath, value.trustText);
    writeFileSync(resultsPath, JSON.stringify(value.results));
    const common = [
      "--dry-run",
      "--plan",
      planPath,
      "--trust",
      trustPath,
      "--expected-trust-sha256",
      value.trustSha256,
    ];
    const planRun = spawnSync(
      process.execPath,
      [path.join(root, "scripts/validate-ade0006a-c0b-plan.mjs"), ...common],
      { encoding: "utf8" },
    );
    assert.equal(planRun.status, 0, planRun.stderr);
    const resultRun = spawnSync(
      process.execPath,
      [
        path.join(root, "scripts/validate-ade0006a-c0b-results.mjs"),
        ...common,
        "--results",
        resultsPath,
      ],
      { encoding: "utf8" },
    );
    assert.equal(resultRun.status, 0, resultRun.stderr);
    const staleRun = spawnSync(
      process.execPath,
      [
        path.join(root, "scripts/validate-ade0006a-c0b-plan.mjs"),
        ...common.slice(0, -1),
        sha256("untrusted"),
      ],
      { encoding: "utf8" },
    );
    assert.notEqual(staleRun.status, 0);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

assert.equal(defaultWorkspaceRoot, root);
