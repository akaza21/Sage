import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  parseBoundedJson,
  recordDigest,
  sameFileIdentity,
} from "./ade0006a-c0b-common.mjs";
import { cargoPackageFeatureErrors } from "./ade0006a-package-policy.mjs";
import {
  deriveInstalledExecutableBinding,
  selectPackagedObservationExecutable,
} from "./ade0006a-c0b-package-reference.mjs";
import { validateCharacterizationNativeReceipt } from "./ade0006a-c0b-observation.mjs";
import { authenticatePaintReceipt } from "./ade0006a-c0b-paint-receipt.mjs";
import {
  defaultWorkspaceRoot,
  loadFrozenContracts,
} from "./ade0006a-c0b-contracts.mjs";
import {
  finalizeResults,
  makeC0BFixture,
  refreshObservationRecord,
} from "./ade0006a-c0b-test-results-fixtures.mjs";
import { validateC0BPlan } from "./validate-ade0006a-c0b-plan.mjs";
import { validateC0BResults } from "./validate-ade0006a-c0b-results.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const frozen = loadFrozenContracts(root);

function resultErrors(fixture) {
  return validateC0BResults(
    fixture.bundle,
    fixture.results,
    Buffer.byteLength(JSON.stringify(fixture.results)),
    fixture.observations,
  );
}

function planErrors(fixture, expectedTrust = fixture.trustSha256) {
  return validateC0BPlan(
    fixture.plan,
    fixture.trust,
    fixture.trustSha256,
    expectedTrust,
    frozen,
    fixture.authenticatedPackage,
  );
}

test("frozen C0-B contracts derive the strong cadence and fixed sidecar inventory", () => {
  assert.equal(frozen.queueBoundaries.size, 14);
  assert.deepEqual(
    frozen.budgets.measurementContract.packagedFootprint
      .adr0005ProjectionOrdinals,
    [0, 4, 8, 12, 16],
  );
  assert.equal(
    frozen.budgets.measurementContract.packagedFootprint.snapshotCount,
    41,
  );
  assert.equal(
    frozen.budgets.measurementContract.characterization.cpuIntervalCount,
    40,
  );
  assert.equal(
    frozen.budgets.measurementContract.observationFiles.inventory.length,
    26,
  );
  assert.deepEqual(frozen.queueBoundaries.get("egressDataFrames"), {
    kind: "count",
    count: 56,
    bytes: 1_835_008,
  });
});

test("installed executable bindings include the authenticated Package receipt", () => {
  const binding = {
    packageReceiptSha256: "1".repeat(64),
    participant: "candidate",
    installerKind: "msi",
    memberManifestSha256: "2".repeat(64),
    installedExecutableSha256: "3".repeat(64),
    installedExecutableBytes: 4096,
  };
  assert.equal(
    deriveInstalledExecutableBinding(binding),
    recordDigest(binding),
  );
  assert.notEqual(
    deriveInstalledExecutableBinding(binding),
    deriveInstalledExecutableBinding({
      ...binding,
      packageReceiptSha256: "4".repeat(64),
    }),
  );
});

test("observation executable selection seals the authenticated handoff participant", () => {
  const fixture = makeC0BFixture(frozen);
  const { authenticatedPackage } = fixture;
  const artifacts = Object.fromEntries(
    ["candidate", "control"].map((participant) => {
      const expected = authenticatedPackage.participants[participant];
      return [
        participant,
        {
          installedExecutables: {
            [authenticatedPackage.installerKind]: {
              path: path.resolve(`${participant}-installed.exe`),
              sha256: expected.installedExecutableSha256,
              bytes: expected.installedExecutableBytes,
            },
          },
        },
      ];
    }),
  );
  assert.deepEqual(
    selectPackagedObservationExecutable({
      authenticatedPackage,
      artifacts,
      participant: "candidate",
    }),
    {
      ...artifacts.candidate.installedExecutables[
        authenticatedPackage.installerKind
      ],
      bindingSha256:
        authenticatedPackage.participants.candidate.executableBindingSha256,
    },
  );
  for (const changed of [
    artifacts.control.installedExecutables[authenticatedPackage.installerKind],
    {
      ...artifacts.candidate.installedExecutables[
        authenticatedPackage.installerKind
      ],
      sha256: "e".repeat(64),
    },
    {
      ...artifacts.candidate.installedExecutables[
        authenticatedPackage.installerKind
      ],
      bytes:
        authenticatedPackage.participants.candidate.installedExecutableBytes +
        1,
    },
  ]) {
    const invalid = structuredClone(artifacts);
    invalid.candidate.installedExecutables[authenticatedPackage.installerKind] =
      changed;
    assert.throws(
      () =>
        selectPackagedObservationExecutable({
          authenticatedPackage,
          artifacts: invalid,
          participant: "candidate",
        }),
      /packaged-observation-executable-mismatch/,
    );
  }
  const invalidPackage = structuredClone(authenticatedPackage);
  invalidPackage.participants.candidate.executableBindingSha256 = "f".repeat(
    64,
  );
  assert.throws(
    () =>
      selectPackagedObservationExecutable({
        authenticatedPackage: invalidPackage,
        artifacts,
        participant: "candidate",
      }),
    /packaged-observation-executable-mismatch/,
  );
});

test("production Cargo defaults cannot admit the characterization fixture", () => {
  const valid =
    '[features]\ndefault = []\nacp-fixture = []\ncustom-protocol = ["tauri/custom-protocol"]\ndiagnostics = []\n';
  assert.deepEqual(cargoPackageFeatureErrors(valid, frozen.package), []);
  assert.notDeepEqual(
    cargoPackageFeatureErrors(
      valid.replace("default = []", 'default = ["acp-fixture"]'),
      frozen.package,
    ),
    [],
  );
});

test("native characterization receipt authenticates process and monotonic anchors", () => {
  const executableSha256 = "a".repeat(64);
  const receipt = {
    schemaVersion: 2,
    case: "cold-cycle",
    runtimeEpoch: 1,
    fixtureProcess: {
      pid: 42,
      creationTimeFiletime100ns: "116444736012340007",
      executableSha256,
      executableBytes: 369664,
    },
    monotonicClock: {
      frequencyHz: 10_000_000,
      spawnStartedTick: 1_000_000,
      processReadyTick: 1_010_001,
      initializeWriteStartedTick: 1_020_000,
      readyCommittedTick: 1_040_001,
      promptWriteCompletedTick: 1_050_000,
      firstNormalizedItemAcceptedTick: 1_060_001,
      cancelReceivedTick: 1_070_000,
      cancelWriteCompletedTick: 1_070_101,
      cancelTerminalCommittedTick: 1_080_001,
      closeReceivedTick: 2_000_000,
      jobZeroTick: 2_020_001,
      closeCommittedTick: 2_020_101,
      walConvergedTick: 2_020_201,
    },
    walConvergence: {
      walBytes: 1_000_000,
      checkpointTargetBytes: 16_777_216,
      checkpointed: false,
    },
    latencyUs: {
      spawnToProcessReady: 1001,
      initialize: 2001,
      firstNormalizedOutput: 1001,
      cancelDispatch: 11,
      cancelTerminal: 1001,
      nativeProcessConvergence: 2001,
    },
  };
  const expected = {
    case: "cold-cycle",
    runtimeEpoch: 1,
    executableSha256,
    executableBytes: 369664,
    frequencyHz: 10_000_000,
    walCheckpointTargetBytes: 16_777_216,
  };
  const errors = [];
  const normalized = validateCharacterizationNativeReceipt(
    errors,
    receipt,
    expected,
  );
  assert.deepEqual(errors, []);
  assert.equal(
    normalized.fixtureProcess.creationTimeUtc,
    "1970-01-01T00:00:01.234Z",
  );
  assert.equal(
    normalized.fixtureProcess.creationTimeFiletime100ns,
    "116444736012340007",
  );
  assert.equal(normalized.spawnToProcessReadyUs, 1001);
  assert.equal(normalized.initializeUs, 2001);
  assert.equal(normalized.cancelDispatchUs, 11);

  const adjacentFiletime = structuredClone(receipt);
  adjacentFiletime.fixtureProcess.creationTimeFiletime100ns =
    "116444736012340008";
  const adjacentErrors = [];
  const adjacent = validateCharacterizationNativeReceipt(
    adjacentErrors,
    adjacentFiletime,
    expected,
  );
  assert.deepEqual(adjacentErrors, []);
  assert.equal(
    adjacent.fixtureProcess.creationTimeUtc,
    normalized.fixtureProcess.creationTimeUtc,
  );
  assert.notEqual(
    adjacent.fixtureProcess.processIdentitySha256,
    normalized.fixtureProcess.processIdentitySha256,
  );

  for (const mutate of [
    (value) => (value.unknown = true),
    (value) => (value.fixtureProcess.executableSha256 = "b".repeat(64)),
    (value) => (value.latencyUs.spawnToProcessReady = 1000),
    (value) => (value.monotonicClock.jobZeroTick = 1),
    (value) => (value.monotonicClock.cancelReceivedTick = null),
    (value) => (value.walConvergence.walBytes = 16_777_216),
  ]) {
    const changed = structuredClone(receipt);
    mutate(changed);
    const changedErrors = [];
    assert.equal(
      validateCharacterizationNativeReceipt(changedErrors, changed, expected),
      null,
    );
    assert.notEqual(changedErrors.length, 0);
  }
});

test("authenticated plan and complete sharded evidence derive a passing result", () => {
  const fixture = makeC0BFixture(frozen);
  assert.deepEqual(fixture.bundle.errors, []);
  assert.deepEqual(resultErrors(fixture), []);
  assert.equal(fixture.results.result, "passed");
  assert.equal(fixture.observations.size, 26);
  assert.equal(fixture.results.gateEvaluation.gates.length, 157);
  const epochs = [...fixture.observations.values()].reduce(
    (count, record) => count + (record.value.epochs?.length ?? 0),
    0,
  );
  assert.equal(epochs, 505);
});

test("cold latency evidence is derived from one native receipt per epoch", () => {
  const mutations = [
    [
      "characterization-cold.json",
      (cold) =>
        (cold.epochs[0].nativeReceipt.monotonicClock.processReadyTick += 1),
    ],
    [
      "characterization-cold.json",
      (cold) => {
        const samples = cold.latencySamples.filter(
          ({ metric }) => metric === "spawnToProcessReady",
        );
        samples[1].epochId = samples[0].epochId;
      },
    ],
    [
      "characterization-cold.json",
      (cold) => {
        const samples = cold.latencySamples.filter(
          ({ metric }) => metric === "initialize",
        );
        samples[1].epochId = samples[0].epochId;
      },
    ],
    [
      "characterization-cold.json",
      (cold) =>
        (cold.latencySamples.find(
          ({ metric }) => metric === "spawnToProcessReady",
        ).durationUs = 7),
    ],
    [
      "characterization-cold.json",
      (cold) => {
        [cold.epochs[0].nativeReceipt, cold.epochs[1].nativeReceipt] = [
          cold.epochs[1].nativeReceipt,
          cold.epochs[0].nativeReceipt,
        ];
      },
    ],
    [
      "characterization-cold.json",
      (cold) => delete cold.epochs[0].nativeReceipt,
    ],
    [
      "characterization-cold.json",
      (cold) =>
        (cold.epochs[0].readyVisibleTick =
          cold.epochs[0].nativeReceipt.monotonicClock.readyCommittedTick - 1),
    ],
    [
      "characterization-cold.json",
      (cold) => delete cold.epochs[0].readyVisibleTick,
    ],
    [
      "characterization-core.json",
      (core) => (core.epochs[0].nativeReceipt = {}),
    ],
  ];
  for (const [file, mutate] of mutations) {
    const fixture = makeC0BFixture(frozen);
    mutate(fixture.observations.get(file).value);
    refreshObservationRecord(fixture, file);
    assert.notEqual(resultErrors(fixture).length, 0);
  }
});

test("every native-bound cold metric rejects a detached sample", () => {
  for (const metric of [
    "spawnToProcessReady",
    "initialize",
    "firstNormalizedOutput",
    "cancelDispatch",
    "cancelTerminal",
    "coldActivation",
    "explicitClose",
  ]) {
    const fixture = makeC0BFixture(frozen);
    const file = "characterization-cold.json";
    const cold = fixture.observations.get(file).value;
    cold.latencySamples.find((sample) => sample.metric === metric).durationUs +=
      1;
    refreshObservationRecord(fixture, file);
    assert.match(resultErrors(fixture).join("\n"), /native receipt/);
  }
});

test("explicit close includes WAL convergence after descendant zero", () => {
  const fixture = makeC0BFixture(frozen);
  const file = "characterization-cold.json";
  const cold = fixture.observations.get(file).value;
  const epoch = cold.epochs[0];
  epoch.nativeReceipt.monotonicClock.walConvergedTick =
    epoch.zeroDescendantsTick + 120;
  cold.latencySamples.find(
    (sample) =>
      sample.metric === "explicitClose" && sample.epochId === epoch.epochId,
  ).durationUs = 620_000;
  refreshObservationRecord(fixture, file);
  assert.deepEqual(resultErrors(fixture), []);
});

test("core latency samples are bound to unique per-epoch QPC spans", () => {
  const mutations = [
    (core) => {
      const epochId = core.epochs[0].epochId;
      core.latencySamples.forEach((sample) => (sample.epochId = epochId));
    },
    (core) => core.latencySamples[0].durationUs++,
    (core) => {
      const samples = core.latencySamples.filter(
        ({ metric, epochId }) =>
          metric === "nativeReceiptToPaint" &&
          epochId === core.epochs[0].epochId,
      );
      samples[1].startTick = samples[0].startTick;
      samples[1].endTick = samples[0].endTick;
      samples[1].durationUs = samples[0].durationUs;
    },
    (core) => {
      const forced = core.latencySamples.find(
        ({ metric }) => metric === "forcedTeardown",
      );
      forced.startTick--;
      forced.durationUs++;
    },
  ];
  for (const mutate of mutations) {
    const fixture = makeC0BFixture(frozen);
    const file = "characterization-core.json";
    mutate(fixture.observations.get(file).value);
    refreshObservationRecord(fixture, file);
    assert.notEqual(resultErrors(fixture).length, 0);
  }
});

test("a structurally valid over-budget observation is valid failed evidence", () => {
  const fixture = makeC0BFixture(frozen);
  const cold = fixture.observations.get("characterization-cold.json").value;
  for (const sample of cold.latencySamples.filter(
    ({ metric }) => metric === "coldActivation",
  )) {
    const epoch = cold.epochs.find(({ epochId }) => epochId === sample.epochId);
    const clock = epoch.nativeReceipt.monotonicClock;
    epoch.readyVisibleTick = epoch.startedTick + 2_000;
    clock.promptWriteCompletedTick = epoch.readyVisibleTick + 10;
    clock.firstNormalizedItemAcceptedTick = epoch.readyVisibleTick + 110;
    clock.cancelReceivedTick = epoch.readyVisibleTick + 130;
    clock.cancelWriteCompletedTick = epoch.readyVisibleTick + 150;
    clock.cancelTerminalCommittedTick = epoch.readyVisibleTick + 630;
    sample.durationUs = 2_000_000;
  }
  refreshObservationRecord(fixture, "characterization-cold.json");
  finalizeResults(fixture);
  assert.equal(fixture.results.result, "failed");
  assert.deepEqual(resultErrors(fixture), []);
  fixture.results.result = "passed";
  assert.match(
    resultErrors(fixture).join("\n"),
    /results\.result must be failed/,
  );
});

test("plan rejects stale receipt, trust, collector, platform, and duplicate facts", () => {
  const mutations = [
    [(fixture) => fixture, "0".repeat(64)],
    [(fixture) => (fixture.trust.packageReceiptSha256 = "0".repeat(64))],
    [(fixture) => fixture.trust.collector.modules.pop()],
    [(fixture) => (fixture.trust.collector.tools[0].sha256 = "bad")],
    [(fixture) => (fixture.trust.environment.platform = "linux")],
    [(fixture) => (fixture.plan.packageReference = {})],
  ];
  for (const [mutate, expected] of mutations) {
    const fixture = makeC0BFixture(frozen);
    mutate(fixture);
    assert.notEqual(
      planErrors(fixture, expected ?? fixture.trustSha256).length,
      0,
    );
  }
});

test("packaged evidence rejects identity reuse, cadence gaps, and malformed snapshots without throwing", () => {
  const mutations = [
    (fixture, shell) =>
      shell.validPairs[0].participants.candidate.snapshots.pop(),
    (fixture, shell) =>
      (shell.validPairs[0].participants.candidate.snapshots[1].capturedTick =
        shell.validPairs[0].participants.candidate.snapshots[2].dueTick),
    (fixture, shell) =>
      (shell.validPairs[0].participants.candidate.processIdentitySha256 =
        "0".repeat(64)),
    (fixture, shell) =>
      (shell.validPairs[1].participants.candidate.disposableProfileIdentitySha256 =
        shell.validPairs[0].participants.candidate.disposableProfileIdentitySha256),
    (fixture, shell) =>
      delete shell.validPairs[0].participants.candidate.snapshots,
    (fixture, shell) =>
      shell.validPairs[0].participants.candidate.spawnToWindowReadyUs++,
    (fixture, shell) =>
      shell.validPairs[0].participants.candidate.inputToPaintUs++,
    (fixture, shell) =>
      shell.validPairs[0].participants.candidate.tabSwitchUs++,
    (fixture, shell) => {
      const run = shell.validPairs[0].participants.candidate;
      run.inputPaintedTick = run.inputReceivedTick - 1;
    },
    (fixture, shell) => {
      const run = shell.validPairs[0].participants.candidate;
      run.tabPaintedTick = run.closeRequestedTick + 1;
    },
    (fixture, shell) => {
      const run = shell.validPairs[0].participants.candidate;
      run.inputPaintReceipt = {
        ...run.inputPaintReceipt,
        sha256: "a".repeat(64),
      };
    },
    (fixture, shell) => {
      const run = shell.validPairs[0].participants.candidate;
      run.tabPaintReceipt = run.inputPaintReceipt;
    },
  ];
  for (const mutate of mutations) {
    const fixture = makeC0BFixture(frozen);
    const shell = fixture.observations.get("footprint-shell.json").value;
    mutate(fixture, shell);
    refreshObservationRecord(fixture, "footprint-shell.json");
    assert.doesNotThrow(() => resultErrors(fixture));
    assert.notEqual(resultErrors(fixture).length, 0);
  }
});

test("packaged close isolation requires a five-second empty-tree observation", () => {
  assert.deepEqual(resultErrors(makeC0BFixture(frozen)), []);

  const shortWindow = makeC0BFixture(frozen);
  const shortRun = shortWindow.observations.get("footprint-shell.json").value
    .validPairs[0].participants.candidate;
  shortRun.orphanCheckCompletedTick =
    shortRun.closeRequestedTick +
    5 * shortWindow.results.monotonicClock.frequencyHz -
    1;
  refreshObservationRecord(shortWindow, "footprint-shell.json");
  assert.match(
    resultErrors(shortWindow).join("\n"),
    /did not close in isolation/,
  );

  const omitted = makeC0BFixture(frozen);
  delete omitted.observations.get("footprint-shell.json").value.validPairs[0]
    .participants.candidate.orphanCheckCompletedTick;
  refreshObservationRecord(omitted, "footprint-shell.json");
  assert.match(
    resultErrors(omitted).join("\n"),
    /has missing or unknown fields/,
  );
});

test("packaged runs reject a swapped or cross-wired post-paint receipt", () => {
  assert.deepEqual(resultErrors(makeC0BFixture(frozen)), []);

  const swapped = makeC0BFixture(frozen);
  const swappedRun = swapped.observations.get("footprint-shell.json").value
    .validPairs[0].participants.candidate;
  swappedRun.inputPaintReceipt = {
    ...swappedRun.inputPaintReceipt,
    sha256: "a".repeat(64),
  };
  refreshObservationRecord(swapped, "footprint-shell.json");
  assert.match(
    resultErrors(swapped).join("\n"),
    /inputPaintReceipt is invalid/,
  );

  const omitted = makeC0BFixture(frozen);
  delete omitted.observations.get("footprint-shell.json").value.validPairs[0]
    .participants.candidate.inputPaintReceipt;
  refreshObservationRecord(omitted, "footprint-shell.json");
  assert.match(
    resultErrors(omitted).join("\n"),
    /has missing or unknown fields/,
  );
});

test("packaged evidence verifies actual alternating chronology", () => {
  assert.deepEqual(resultErrors(makeC0BFixture(frozen)), []);
  function rebindPaintReceipts(run, frequencyHz) {
    run.inputPaintReceipt = authenticatePaintReceipt({
      operation: run.inputPaintReceipt.operation,
      startedTick: run.inputReceivedTick,
      completedTick: run.inputPaintedTick,
      durationUs: run.inputToPaintUs,
      frequencyHz,
    });
    run.tabPaintReceipt = authenticatePaintReceipt({
      operation: run.tabPaintReceipt.operation,
      startedTick: run.tabActivatedTick,
      completedTick: run.tabPaintedTick,
      durationUs: run.tabSwitchUs,
      frequencyHz,
    });
  }
  function shift(run, offset, frequencyHz) {
    for (const key of [
      "spawnStartedTick",
      "windowReadyTick",
      "stateReadyTick",
      "samplingStartedTick",
      "inputReceivedTick",
      "inputPaintedTick",
      "tabActivatedTick",
      "tabPaintedTick",
      "closeRequestedTick",
      "zeroDescendantsTick",
      "orphanCheckCompletedTick",
    ]) {
      run[key] += offset;
    }
    for (const snapshot of run.snapshots) {
      snapshot.dueTick += offset;
      snapshot.capturedTick += offset;
    }
    rebindPaintReceipts(run, frequencyHz);
  }
  function overlap(first, second, frequencyHz) {
    shift(
      second,
      first.zeroDescendantsTick - second.spawnStartedTick - 1,
      frequencyHz,
    );
  }
  const mutations = [
    (_fixture, shell, frequencyHz) => {
      const pair = shell.validPairs[0];
      overlap(
        pair.participants[pair.order[0]],
        pair.participants[pair.order[1]],
        frequencyHz,
      );
    },
    (_fixture, shell, frequencyHz) => {
      const previous = shell.validPairs[0];
      const current = shell.validPairs[1];
      overlap(
        previous.participants[previous.order[1]],
        current.participants[current.order[0]],
        frequencyHz,
      );
    },
  ];
  for (const mutate of mutations) {
    const fixture = makeC0BFixture(frozen);
    const shell = fixture.observations.get("footprint-shell.json").value;
    mutate(fixture, shell, fixture.results.monotonicClock.frequencyHz);
    refreshObservationRecord(fixture, "footprint-shell.json");
    assert.notEqual(resultErrors(fixture).length, 0);
  }

  const stateLocal = makeC0BFixture(frozen);
  const chat = stateLocal.observations.get("footprint-chat.json").value;
  const frequencyHz = stateLocal.results.monotonicClock.frequencyHz;
  for (const pair of chat.validPairs) {
    for (const participant of pair.order) {
      shift(pair.participants[participant], -450_000, frequencyHz);
    }
  }
  refreshObservationRecord(stateLocal, "footprint-chat.json");
  assert.deepEqual(resultErrors(stateLocal), []);
});

test("multiple typed invalid attempts may bind one fresh replacement", () => {
  const fixture = makeC0BFixture(frozen);
  fixture.results.packagedFootprint.invalidAttempts = [1, 2].map(
    (attemptOrdinal) => ({
      attemptOrdinal,
      stateId: "shell",
      plannedPairOrdinal: 1,
      order: ["control", "candidate"],
      failedParticipant: attemptOrdinal === 1 ? "candidate" : "pair",
      reason: attemptOrdinal === 1 ? "launch-failure" : "cadence-miss",
      replacementPairOrdinal: 1,
      samplesRetained: false,
    }),
  );
  finalizeResults(fixture);
  assert.deepEqual(resultErrors(fixture), []);
});

test("characterization rejects control fields, cross-purpose reuse, and incomplete load evidence", () => {
  const mutations = [
    ["characterization-core.json", (value) => (value.control = {})],
    [
      "saturation-driverDataMessages.json",
      (value) => (value.cycles[1].epochId = value.cycles[0].epochId),
    ],
    [
      "characterization-coalescing.json",
      (value) => value.policies[0].receiptToPaintSamples.pop(),
    ],
    ["characterization-persistence.json", (value) => (value.control = {})],
    [
      "characterization-gestures.json",
      (value) => (value.runs[0].screenCoordinatesUsed = true),
    ],
    [
      "characterization-core.json",
      (value, fixture) => {
        const { sageProcess, fixtureProcess } = value.epochs[0];
        fixtureProcess.pid = sageProcess.pid;
        fixtureProcess.creationTimeUtc = sageProcess.creationTimeUtc;
        fixtureProcess.creationTimeFiletime100ns =
          sageProcess.creationTimeFiletime100ns;
        fixtureProcess.processIdentitySha256 = recordDigest({
          pid: sageProcess.pid,
          creationTimeFiletime100ns: sageProcess.creationTimeFiletime100ns,
          executableIdentitySha256:
            fixture.trust.characterization.fixtureExecutableSha256,
        });
      },
    ],
  ];
  for (const [file, mutate] of mutations) {
    const fixture = makeC0BFixture(frozen);
    mutate(fixture.observations.get(file).value, fixture);
    refreshObservationRecord(fixture, file);
    assert.notEqual(resultErrors(fixture).length, 0);
  }
});

test("persistence evidence derives ordered, in-launch WAL convergence", () => {
  const mutations = [
    (value) => value.markers[0].checkpointDurationUs++,
    (value) =>
      (value.markers[0].checkpointCompletedTick =
        value.markers[0].checkpointStartedTick - 1),
    (value) =>
      (value.markers[0].checkpointCompletedTick =
        value.launches[0].zeroDescendantsTick + 1),
    (value) =>
      (value.launches[1].startedTick =
        value.launches[0].zeroDescendantsTick - 1),
    (value) =>
      (value.markers[0].walBytesAfterCheckpoint =
        frozen.budgets.persistence.walCheckpointTargetBytes),
    (value) =>
      (value.markers[0].walBytesAfterCheckpoint =
        value.markers[0].walBytesBeforeCheckpoint + 1),
  ];
  for (const mutate of mutations) {
    const fixture = makeC0BFixture(frozen);
    const file = "characterization-persistence.json";
    mutate(fixture.observations.get(file).value);
    refreshObservationRecord(fixture, file);
    assert.notEqual(resultErrors(fixture).length, 0);
  }
});

test("queue or fairness contract failures produce failed evidence, not malformed evidence", () => {
  const fixture = makeC0BFixture(frozen);
  const saturation = fixture.observations.get(
    "saturation-driverDataMessages.json",
  ).value;
  saturation.cycles[0].zeroStarved = false;
  refreshObservationRecord(fixture, "saturation-driverDataMessages.json");
  finalizeResults(fixture);
  assert.equal(fixture.results.result, "failed");
  assert.deepEqual(resultErrors(fixture), []);
  assert.equal(
    fixture.results.gateEvaluation.gates.find(
      ({ id }) => id === "saturation-driverDataMessages-contract",
    ).passed,
    false,
  );
});

test("observation descriptors and retained byte accounting authenticate exact files", () => {
  const mutations = [
    (fixture) => fixture.results.observationFiles.pop(),
    (fixture) => (fixture.results.observationFiles[0].sha256 = "0".repeat(64)),
    (fixture) => (fixture.results.evidence.rawFiles = 25),
    (fixture) =>
      (fixture.observations.get("footprint-shell.json").value.stateId = "chat"),
  ];
  for (const mutate of mutations) {
    const fixture = makeC0BFixture(frozen);
    mutate(fixture);
    assert.notEqual(resultErrors(fixture).length, 0);
  }
});

test("bounded JSON reader rejects files and structures before unbounded validation", () => {
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
    writeFileSync(file, JSON.stringify(Array.from({ length: 513 }, () => 0)));
    assert.throws(
      () => parseBoundedJson(file, 100_000, frozen.limits.wire, "fixture"),
      /exceeds/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("sanitization rejects paths and command lines in curated results", () => {
  for (const limitation of [
    "host=C:\\Users\\person\\sage.db",
    "host=/project/repository/sage.db",
    "ran node private-suite.mjs",
    "ran git status",
  ]) {
    const fixture = makeC0BFixture(frozen);
    fixture.results.limitations = [limitation];
    assert.notEqual(resultErrors(fixture).length, 0);
  }
});

test("public validators fail closed when package or observation authority is omitted", () => {
  for (const script of [
    "validate-ade0006a-c0b-plan.mjs",
    "validate-ade0006a-c0b-results.mjs",
  ]) {
    const run = spawnSync(
      process.execPath,
      [path.join(root, "scripts", script)],
      {
        encoding: "utf8",
      },
    );
    assert.notEqual(run.status, 0);
    assert.match(run.stderr, /usage:/);
  }
});

assert.equal(defaultWorkspaceRoot, root);
