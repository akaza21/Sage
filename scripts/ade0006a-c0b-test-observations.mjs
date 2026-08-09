import { recordDigest, sha256 } from "./ade0006a-c0b-common.mjs";
import { buildPairSchedule } from "./validate-ade0006a-c0b-plan.mjs";

const startedAt = Date.parse("2026-08-07T00:00:00.000Z");

function iso(ordinal) {
  return new Date(startedAt + ordinal * 1000).toISOString();
}

function digest(label) {
  return sha256(label);
}

function ticksForUs(durationUs, frequencyHz) {
  const ticks = (durationUs * frequencyHz) / 1_000_000;
  if (!Number.isSafeInteger(ticks)) throw new Error("fixture tick conversion");
  return ticks;
}

function processIdentity(pid, executableIdentitySha256) {
  const creationTimeUtc = iso(pid);
  const creationTimeFiletime100ns = (
    BigInt(filetime100ns(creationTimeUtc)) + BigInt(pid % 10)
  ).toString();
  return {
    pid,
    creationTimeUtc,
    creationTimeFiletime100ns,
    processIdentitySha256: recordDigest({
      pid,
      creationTimeFiletime100ns,
      executableIdentitySha256,
    }),
  };
}

function closeFields(base) {
  return {
    closeRequestedTick: base + 20_000,
    zeroDescendantsTick: base + 20_500,
    normalSageDataMutations: 0,
    orphanProcessesAfterClose: 0,
  };
}

function filetime100ns(creationTimeUtc) {
  return (
    BigInt(Date.parse(creationTimeUtc)) * 10_000n +
    116_444_736_000_000_000n
  ).toString();
}

function nativeReceipt(
  fixtureProcess,
  base,
  trust,
  frequencyHz,
  walCheckpointTargetBytes,
) {
  return {
    schemaVersion: 2,
    case: "cold-cycle",
    runtimeEpoch: 1,
    fixtureProcess: {
      pid: fixtureProcess.pid,
      creationTimeFiletime100ns: fixtureProcess.creationTimeFiletime100ns,
      executableSha256: trust.characterization.fixtureExecutableSha256,
      executableBytes: trust.characterization.fixtureExecutableBytes,
    },
    monotonicClock: {
      frequencyHz,
      spawnStartedTick: base,
      processReadyTick: base + 200,
      initializeWriteStartedTick: base + 210,
      readyCommittedTick: base + 410,
      promptWriteCompletedTick: base + 420,
      firstNormalizedItemAcceptedTick: base + 520,
      cancelReceivedTick: base + 540,
      cancelWriteCompletedTick: base + 560,
      cancelTerminalCommittedTick: base + 1040,
      closeReceivedTick: base + 20_000,
      jobZeroTick: base + 20_400,
      closeCommittedTick: base + 20_410,
      walConvergedTick: base + 20_420,
    },
    walConvergence: {
      walBytes: 1_000_000,
      checkpointTargetBytes: walCheckpointTargetBytes,
      checkpointed: false,
    },
    latencyUs: {
      spawnToProcessReady: 200_000,
      initialize: 200_000,
      firstNormalizedOutput: 100_000,
      cancelDispatch: 20_000,
      cancelTerminal: 500_000,
      nativeProcessConvergence: 400_000,
    },
  };
}

function footprintRun(context, participant, launchOrdinal, orderIndex) {
  const executable = context.authenticatedPackage.participants[participant];
  const executableBindingSha256 = executable.executableBindingSha256;
  const pid = context.nextPid++;
  const base = launchOrdinal * 100_000 + orderIndex * 30_000;
  const spawnToWindowReadyUs =
    participant === "candidate" ? 1_000_000 : 900_000;
  const inputToPaintUs = participant === "candidate" ? 20_000 : 15_000;
  const tabSwitchUs = participant === "candidate" ? 50_000 : 40_000;
  const spawnStartedTick = base;
  const windowReadyTick =
    spawnStartedTick + ticksForUs(spawnToWindowReadyUs, context.frequencyHz);
  const stateReadyTick = windowReadyTick + 100;
  const inputReceivedTick = stateReadyTick + 50;
  const inputPaintedTick =
    inputReceivedTick + ticksForUs(inputToPaintUs, context.frequencyHz);
  const tabActivatedTick = inputPaintedTick + 50;
  const tabPaintedTick =
    tabActivatedTick + ticksForUs(tabSwitchUs, context.frequencyHz);
  const samplingStartedTick = stateReadyTick + 8_000;
  const cpuIncrement = participant === "candidate" ? 5_000 : 3_750;
  const privateBytes = participant === "candidate" ? 50_000_000 : 40_000_000;
  return {
    launchOrdinal,
    ...processIdentity(pid, executableBindingSha256),
    executableBindingSha256,
    disposableProfileIdentitySha256: digest(`profile-${context.nextProfile++}`),
    spawnStartedTick,
    windowReadyTick,
    stateReadyTick,
    samplingStartedTick,
    stateVerified: true,
    inputReceivedTick,
    inputPaintedTick,
    tabActivatedTick,
    tabPaintedTick,
    spawnToWindowReadyUs,
    inputToPaintUs,
    tabSwitchUs,
    snapshots: Array.from({ length: 41 }, (_, ordinal) => ({
      ordinal,
      dueTick: samplingStartedTick + ordinal * 250,
      capturedTick: samplingStartedTick + ordinal * 250 + 1,
      membershipSha256: digest(`${participant}-${launchOrdinal}-membership`),
      coreProcessCount: participant === "candidate" ? 4 : 3,
      unknownProcessCount: 0,
      coreCpuTime100ns: ordinal * cpuIncrement,
      corePrivateBytes: privateBytes,
      coreWorkingSetBytes: privateBytes + 5_000_000,
    })),
    ...closeFields(base),
  };
}

function footprintFiles(context, frozen) {
  const records = new Map();
  for (const [
    stateIndex,
    stateId,
  ] of frozen.budgets.sampleContract.packagedStates.entries()) {
    const validPairs = buildPairSchedule(
      frozen.budgets.sampleContract.validPairsPerPackagedState,
    ).map((pair, pairIndex) => {
      const launchOrdinal = stateIndex * 5 + pairIndex + 1;
      return {
        ...pair,
        participants: Object.fromEntries(
          pair.order.map((participant, orderIndex) => [
            participant,
            footprintRun(context, participant, launchOrdinal, orderIndex),
          ]),
        ),
      };
    });
    records.set(`footprint-${stateId}.json`, {
      schemaVersion: 2,
      stateId,
      validPairs,
    });
  }
  return records;
}

function epoch(context, purpose, caseId, trust) {
  const epochOrdinal = context.nextEpoch++;
  const base = 4_000_000 + epochOrdinal * 30_000;
  const sageProcess = processIdentity(
    context.nextPid++,
    trust.characterization.executableSha256,
  );
  const fixtureProcess = processIdentity(
    context.nextPid++,
    trust.characterization.fixtureExecutableSha256,
  );
  const value = {
    epochId: `epoch-${String(epochOrdinal).padStart(4, "0")}`,
    epochOrdinal,
    purpose,
    caseId,
    disposableProfileIdentitySha256: digest(`profile-${context.nextProfile++}`),
    startedAtUtc: iso(context.nextTime++),
    completedAtUtc: iso(context.nextTime++),
    startedTick: base,
    sageProcess,
    fixtureProcess,
    ...closeFields(base),
  };
  if (purpose === "cold") {
    value.readyVisibleTick = base + 415;
    value.nativeReceipt = nativeReceipt(
      fixtureProcess,
      base,
      trust,
      context.frequencyHz,
      context.walCheckpointTargetBytes,
    );
  }
  return value;
}

function latencySamples(metrics, epochs, counts, values, anchors) {
  return metrics.flatMap((metric) =>
    Array.from({ length: counts[metric] }, (_, index) => {
      const epoch = epochs[index % epochs.length];
      return {
        metric,
        sampleOrdinal: index + 1,
        epochId: epoch.epochId,
        ...(anchors?.(metric, epoch, index) ?? {}),
        durationUs: values[metric],
      };
    }),
  );
}

function coreLatencyAnchors(metric, epoch, index, epochCount) {
  if (metric === "forcedTeardown") {
    return {
      startTick: epoch.closeRequestedTick,
      endTick: epoch.zeroDescendantsTick,
    };
  }
  const ordinalWithinEpoch = Math.floor(index / epochCount);
  const startTick =
    epoch.startedTick +
    (metric === "nativeReceiptToPaint" ? 1_000 : 4_000) +
    ordinalWithinEpoch * 100;
  return { startTick, endTick: startTick + 20 };
}

function coreFile(context, frozen, trust) {
  const epochs = Array.from({ length: 5 }, () =>
    epoch(context, "core", null, trust),
  );
  const cadence = frozen.budgets.measurementContract.characterization;
  const resourceRuns = epochs.map((owner, index) => {
    const readyTick = owner.startedTick + 100;
    const samplingStartedTick = readyTick + cadence.settleAfterSemanticReadyMs;
    return {
      epochId: owner.epochId,
      readyTick,
      samplingStartedTick,
      coldProcessCount: 0,
      coldSocketCount: 0,
      coldListenerCount: 0,
      coldTimerCount: 0,
      operationalRuntimeTrees: 1,
      operationalRunningTurns: 1,
      snapshots: Array.from(
        { length: cadence.snapshotCount },
        (_, ordinal) => ({
          ordinal,
          dueTick: samplingStartedTick + ordinal * cadence.snapshotIntervalMs,
          capturedTick:
            samplingStartedTick + ordinal * cadence.snapshotIntervalMs + 1,
          sageMembershipSha256: digest(`core-${index}-sage-membership`),
          fixtureMembershipSha256: digest(`core-${index}-fixture-membership`),
          sageProcessCount: 4,
          fixtureProcessCount: 1,
          unknownProcessCount: 0,
          sageCpuTime100ns: ordinal * 2_500,
          fixtureCpuTime100ns: ordinal * 2_500,
          sagePrivateBytes: 50_000_000,
          fixturePrivateBytes: 10_000_000,
          sageWorkingSetBytes: 55_000_000,
          fixtureWorkingSetBytes: 12_000_000,
        }),
      ),
    };
  });
  const counts = frozen.budgets.sampleContract.characterizationLatencySamples;
  return {
    schemaVersion: 2,
    participant: "candidate",
    epochs,
    resourceRuns,
    latencySamples: latencySamples(
      ["nativeReceiptToPaint", "inputToPaint", "forcedTeardown"],
      epochs,
      counts,
      {
        nativeReceiptToPaint: 20_000,
        inputToPaint: 20_000,
        forcedTeardown: 500_000,
      },
      (metric, epoch, index) =>
        coreLatencyAnchors(metric, epoch, index, epochs.length),
    ),
  };
}

function coldFile(context, frozen, trust) {
  const epochs = Array.from({ length: 30 }, () =>
    epoch(context, "cold", null, trust),
  );
  const metrics = [
    "spawnToProcessReady",
    "initialize",
    "firstNormalizedOutput",
    "cancelDispatch",
    "cancelTerminal",
    "coldActivation",
    "explicitClose",
  ];
  return {
    schemaVersion: 2,
    participant: "candidate",
    epochs,
    latencySamples: latencySamples(
      metrics,
      epochs,
      frozen.budgets.sampleContract.characterizationLatencySamples,
      {
        spawnToProcessReady: 200_000,
        initialize: 200_000,
        firstNormalizedOutput: 100_000,
        cancelDispatch: 20_000,
        cancelTerminal: 500_000,
        coldActivation: 415_000,
        explicitClose: 500_000,
      },
    ),
  };
}

function saturationState(boundary, plusOne) {
  const retainedBytes =
    boundary.kind === "bytes"
      ? boundary.bytes
      : Math.min(boundary.bytes, Math.max(1, boundary.count * 8));
  return {
    acceptedCount: boundary.kind === "count" ? boundary.count : 1,
    rejectedCount: plusOne ? 1 : 0,
    retainedBytes,
    queueHighWaterCount: boundary.kind === "count" ? boundary.count : 1,
    queueHighWaterBytes: retainedBytes,
    typedError: plusOne ? "capacity-exceeded" : null,
    terminalState: plusOne ? "capacity-rejected" : "completed",
  };
}

function saturationFiles(context, frozen, trust) {
  const records = new Map();
  for (const [boundaryId, boundary] of frozen.queueBoundaries) {
    const epochs = Array.from({ length: 30 }, () =>
      epoch(context, "saturation", boundaryId, trust),
    );
    records.set(`saturation-${boundaryId}.json`, {
      schemaVersion: 1,
      participant: "candidate",
      boundaryId,
      epochs,
      cycles: epochs.map((owner, index) => ({
        cycleOrdinal: index + 1,
        epochId: owner.epochId,
        exact: saturationState(boundary, false),
        plusOne: saturationState(boundary, true),
        controlLaneDispatchUs: 20_000,
        fairnessDisposition: "bounded-fifo",
        zeroStarved: true,
        pollingDetected: false,
      })),
    });
  }
  return records;
}

function fairnessFile(context, trust) {
  const epochs = Array.from({ length: 30 }, () =>
    epoch(context, "fairness", null, trust),
  );
  return {
    schemaVersion: 1,
    participant: "candidate",
    epochs,
    observations: epochs.map((owner, index) => ({
      cycleOrdinal: index + 1,
      epochId: owner.epochId,
      controlLaneDispatchUs: 20_000,
      dataAccepted: 1,
      zeroStarved: true,
      disposition: "bounded-fifo",
      pollingDetected: false,
    })),
  };
}

function coalescingFile(context, frozen, trust) {
  const samples = frozen.budgets.sampleContract;
  const epochs = samples.coalescingPolicies.flatMap((policy) =>
    Array.from({ length: samples.coalescingEpochsPerPolicy }, () =>
      epoch(context, "coalescing", policy, trust),
    ),
  );
  const values = {
    "16ms": 14_000,
    "33ms": 12_000,
    "50ms": 13_000,
    "4kib": 15_000,
  };
  return {
    schemaVersion: 1,
    participant: "candidate",
    epochs,
    selectedPolicy: "33ms",
    policies: samples.coalescingPolicies.map((id) => {
      const owned = epochs.filter(({ caseId }) => caseId === id);
      return {
        id,
        epochIds: owned.map(({ epochId }) => epochId),
        receiptToPaintSamples: Array.from(
          {
            length:
              samples.coalescingEpochsPerPolicy *
              samples.coalescingSamplesPerEpoch,
          },
          (_, index) => ({
            sampleOrdinal: index + 1,
            epochId: owned[index % owned.length].epochId,
            durationUs: values[id],
          }),
        ),
        sqliteWritesPerTextDelta: 0,
        queueSaturationObservations: 0,
      };
    }),
  };
}

function persistenceFile(context, frozen, trust) {
  const profile = digest(`profile-${context.nextProfile++}`);
  const launches = Array.from({ length: 2 }, (_, index) => {
    const base = 30_000_000 + index * 30_000;
    return {
      launchOrdinal: index + 1,
      startedAtUtc: iso(context.nextTime++),
      completedAtUtc: iso(context.nextTime++),
      startedTick: base,
      sageProcess: processIdentity(
        context.nextPid++,
        trust.characterization.executableSha256,
      ),
      fixtureProcess: processIdentity(
        context.nextPid++,
        trust.characterization.fixtureExecutableSha256,
      ),
      ...closeFields(base),
    };
  });
  return {
    schemaVersion: 2,
    participant: "candidate",
    disposableProfileIdentitySha256: profile,
    launches,
    markers: frozen.budgets.measurementContract.persistence.markers.map(
      (id, index) => {
        const launchOrdinal = index < 2 ? 1 : 2;
        const checkpointStartedTick =
          launches[launchOrdinal - 1].startedTick + 1_000 + index * 100;
        return {
          id,
          launchOrdinal,
          databaseBytes: 4_000_000,
          walBytesBeforeCheckpoint: 17_000_000,
          walBytesAfterCheckpoint: 1_000_000,
          checkpointStartedTick,
          checkpointCompletedTick: checkpointStartedTick + 10,
          checkpointDurationUs: 10_000,
          walConverged: true,
        };
      },
    ),
    stateDirectoryBytesForFoundationFixture: 4_000_000,
    promptWritesPerTurn: 1,
    sqliteWritesPerTextDelta: 0,
    ipcNotificationsPerTextDelta: 0,
    recoveryRequired: false,
  };
}

function gesturesFile(context, frozen, trust, authenticatedPackage) {
  return {
    schemaVersion: 1,
    participant: "candidate",
    contractSha256: frozen.packagedGesturesSha256,
    runs: frozen.gestures.cases.map((item, index) => {
      const production = item.build === "production-default";
      const executable = production
        ? authenticatedPackage.participants.candidate.executableBindingSha256
        : trust.characterization.executableSha256;
      const base = 31_000_000 + index * 30_000;
      return {
        id: item.id,
        build: item.build,
        startedAtUtc: iso(context.nextTime++),
        completedAtUtc: iso(context.nextTime++),
        artifactIdentitySha256: executable,
        disposableProfileIdentitySha256: digest(
          `profile-${context.nextProfile++}`,
        ),
        sageProcess: processIdentity(context.nextPid++, executable),
        fixtureProcess: production
          ? null
          : processIdentity(
              context.nextPid++,
              trust.characterization.fixtureExecutableSha256,
            ),
        semanticAutomation: true,
        screenCoordinatesUsed: false,
        globalInputUsed: false,
        systemOrNetworkControlsUsed: false,
        debugAutomationUsed: false,
        observedOracleSha256: recordDigest(item.oracle),
        ...closeFields(base),
      };
    }),
  };
}

export function makeObservationRecords(
  frozen,
  trust,
  authenticatedPackage,
  frequencyHz,
) {
  const context = {
    authenticatedPackage,
    frequencyHz,
    nextPid: 1000,
    nextProfile: 1,
    nextEpoch: 1,
    nextTime: 1,
    walCheckpointTargetBytes:
      frozen.budgets.persistence.walCheckpointTargetBytes,
  };
  const values = new Map([
    ...footprintFiles(context, frozen),
    ["characterization-core.json", coreFile(context, frozen, trust)],
    ["characterization-cold.json", coldFile(context, frozen, trust)],
    ...saturationFiles(context, frozen, trust),
    ["characterization-fairness.json", fairnessFile(context, trust)],
    [
      "characterization-coalescing.json",
      coalescingFile(context, frozen, trust),
    ],
    [
      "characterization-persistence.json",
      persistenceFile(context, frozen, trust),
    ],
    [
      "characterization-gestures.json",
      gesturesFile(context, frozen, trust, authenticatedPackage),
    ],
  ]);
  return new Map(
    frozen.budgets.measurementContract.observationFiles.inventory.map(
      (relativePath) => {
        const value = values.get(relativePath);
        const buffer = Buffer.from(JSON.stringify(value));
        return [
          relativePath,
          { value, buffer, bytes: buffer.byteLength, sha256: sha256(buffer) },
        ];
      },
    ),
  );
}
