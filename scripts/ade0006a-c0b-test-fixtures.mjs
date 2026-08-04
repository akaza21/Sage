import { recordDigest, sha256 } from "./ade0006a-c0b-common.mjs";
import { resourceMetricAnchors } from "./ade0006a-c0b-metrics.mjs";
import { makePlan, makeTrust } from "./ade0006a-c0b-test-plan-fixtures.mjs";
import { buildPairSchedule } from "./validate-ade0006a-c0b-plan.mjs";

const hash = (digit) => digit.repeat(64);

function process(binding, launchOrdinal, pid) {
  const pidLabel = `pid-${pid}`;
  const creationIdentitySha256 = hash(String((pid % 8) + 1));
  return {
    launchOrdinal,
    pidLabel,
    creationIdentitySha256,
    processIdentitySha256: recordDigest({ pidLabel, creationIdentitySha256 }),
    executableSha256: binding.executableSha256,
    processCount: 4,
    orphanProcessesAfterClose: 0,
  };
}

function validPairs(plan, trust) {
  return buildPairSchedule(plan.collection.validPairs).map((entry) => ({
    ...entry,
    provenanceSha256: recordDigest(plan.provenance),
    participants: {
      control: process(trust.control, entry.pairOrdinal, entry.pairOrdinal * 2),
      candidate: process(
        trust.candidate,
        entry.pairOrdinal,
        entry.pairOrdinal * 2 + 1,
      ),
    },
  }));
}

function warmups(plan, trust) {
  return buildPairSchedule(plan.collection.excludedWarmups).map(
    ({ pairOrdinal, order }) => ({
      warmupOrdinal: pairOrdinal,
      order,
      excluded: true,
      participants: {
        control: process(trust.control, pairOrdinal + 5, 20 + pairOrdinal * 2),
        candidate: process(
          trust.candidate,
          pairOrdinal + 5,
          21 + pairOrdinal * 2,
        ),
      },
    }),
  );
}

function metric(count, anchor, pairs, candidateValue, controlValue) {
  const samples = [];
  const perPair = count / pairs.length;
  for (const participant of ["candidate", "control"]) {
    for (let pairOrdinal = 1; pairOrdinal <= pairs.length; pairOrdinal += 1) {
      const owner = pairs[pairOrdinal - 1].participants[participant];
      samples.push({
        pairOrdinal,
        participant,
        launchOrdinal: owner.launchOrdinal,
        processIdentitySha256: owner.processIdentitySha256,
        firstSampleOrdinal: (pairOrdinal - 1) * perPair + 1,
        values: Array.from({ length: perPair }, () =>
          participant === "candidate" ? candidateValue : controlValue,
        ),
      });
    }
  }
  return { percentileMethod: "nearest-rank", anchor, samples };
}

function latency(plan, pairs) {
  const five = (key, candidate = 12, control = 10) =>
    metric(5, plan.anchors[key], pairs, candidate, control);
  const hundred = (key, candidate = 12, control = 10) =>
    metric(100, plan.anchors[key], pairs, candidate, control);
  return {
    spawnToProcessReady: five("spawnToProcessReady", 100, 90),
    initialize: five("initialize", 200, 190),
    firstNormalizedOutput: five("firstNormalizedOutput", 100, 90),
    nativeReceiptToPaint: hundred("nativeReceiptToPaint"),
    inputToPaint: hundred("inputToPaint"),
    tabSwitch: hundred("tabSwitch"),
    cancelDispatch: hundred("cancelDispatch"),
    cancelTerminal: five("cancelTerminal"),
    coldActivation: five("coldActivation", 100, 90),
    explicitClose: five("explicitClose"),
    forcedTeardown: five("forcedTeardown", 100, 90),
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

function queueSaturation(plan, frozen, pairs) {
  return [...frozen.queueBoundaries].map(([id, boundary]) => ({
    id,
    kind: boundary.kind,
    cycles: Array.from(
      { length: plan.collection.saturationCyclesPerBoundary },
      (_, index) => {
        const pair = pairs[index % pairs.length];
        return {
          cycleOrdinal: index + 1,
          pairOrdinal: pair.pairOrdinal,
          processIdentitySha256:
            pair.participants.candidate.processIdentitySha256,
          exact: saturationState(boundary, false),
          plusOne: saturationState(boundary, true),
          controlLaneDispatchMs: 10,
          fairnessDisposition: "bounded-fifo",
          zeroStarved: true,
          pollingDetected: false,
          orphanProcessesAfterClose: 0,
        };
      },
    ),
  }));
}

function trustedPackage(binding) {
  return {
    sourceFileCount: binding.sourceFileCount,
    sourceBytes: binding.sourceBytes,
    dependencyPackageCount: binding.dependencyPackageCount,
    dependencyGraphSha256: binding.dependencyGraphSha256,
    cargoLockSha256: binding.cargoLockSha256,
    frontendLockSha256: binding.frontendLockSha256,
    frontendRawBytes: binding.frontendRawBytes,
    executableSha256: binding.executableSha256,
    executableBytes: binding.executableBytes,
    productionPackageSha256: binding.productionPackageSha256,
    productionPackageBytes: binding.productionPackageBytes,
    installerSha256: binding.installerSha256,
    installerBytes: binding.installerBytes,
    installedCoreBytes: binding.installedCoreBytes,
  };
}

export function makeResults(plan, planSha256, trust, trustSha256, frozen) {
  const pairs = validPairs(plan, trust);
  const resourceAnchors = resourceMetricAnchors(frozen);
  const candidatePackage = trustedPackage(trust.candidate);
  const controlPackage = trustedPackage(trust.control);
  const result = {
    schemaVersion: 1,
    workItem: "ADE-0006A",
    phase: "C0-B",
    sanitized: true,
    result: "passed",
    timestamps: {
      startedAtUtc: "2026-08-04T00:00:00.000Z",
      completedAtUtc: "2026-08-04T01:00:00.000Z",
    },
    limitations: ["none"],
    planSha256,
    trustManifestSha256: trustSha256,
    contractManifestSha256: frozen.contractManifestSha256,
    fixtureManifestSha256: frozen.fixtureManifestSha256,
    limitsSha256: frozen.limitsSha256,
    candidateBindingSha256: recordDigest(trust.candidate),
    controlBindingSha256: recordDigest(trust.control),
    warmupExclusions: warmups(plan, trust),
    validPairs: pairs,
    invalidPairs: [],
    latencyMs: latency(plan, pairs),
    resources: {
      readyIdleCpuSingleCorePercent: metric(
        200,
        resourceAnchors.readyIdleCpuSingleCorePercent,
        pairs,
        0.1,
        0.05,
      ),
      fixtureTreePrivateBytes: metric(
        200,
        resourceAnchors.fixtureTreePrivateBytes,
        pairs,
        1_000_000,
        900_000,
      ),
      sageNativeAndRendererIncrementalPrivateBytes: metric(
        200,
        resourceAnchors.sageNativeAndRendererIncrementalPrivateBytes,
        pairs,
        2_000_000,
        1_900_000,
      ),
      coldProcessCount: 0,
      coldSocketCount: 0,
      coldListenerCount: 0,
      coldTimerCount: 0,
      operationalRuntimeTrees: 1,
      operationalRunningTurns: 1,
      orphanProcessesAfterClose: 0,
    },
    queueSaturation: queueSaturation(plan, frozen, pairs),
    fairness: {
      cycles: 30,
      observations: Array.from({ length: 30 }, (_, index) => {
        const pair = pairs[index % pairs.length];
        return {
          cycleOrdinal: index + 1,
          pairOrdinal: pair.pairOrdinal,
          processIdentitySha256:
            pair.participants.candidate.processIdentitySha256,
          controlLaneDispatchMs: 10,
          dataAccepted: 1,
          zeroStarved: true,
          disposition: "bounded-fifo",
        };
      }),
    },
    coalescing: {
      selectedPolicy: "33ms",
      selectionRule:
        "lowest-p95-with-zero-saturation-and-zero-durable-delta-writes",
      policies: plan.collection.coalescingPolicies.map((id) => ({
        id,
        receiptToPaint: metric(
          100,
          plan.anchors.nativeReceiptToPaint,
          pairs,
          { "16ms": 14, "33ms": 12, "50ms": 13, "4kib": 15 }[id],
          10,
        ),
        sqliteWritesPerTextDelta: 0,
        queueSaturationObservations: 0,
      })),
    },
    persistence: {
      markers: ["fresh", "after-100-turns", "after-close-reopen"].map((id) => ({
        id,
        candidate: {
          databaseBytes: 4_000_000,
          walBytesBeforeCheckpoint: 17_000_000,
          walBytesAfterCheckpoint: 1_000_000,
          checkpointDurationMs: 10,
          walConverged: true,
        },
        control: {
          databaseBytes: 3_000_000,
          walBytesBeforeCheckpoint: 16_000_000,
          walBytesAfterCheckpoint: 500_000,
          checkpointDurationMs: 9,
          walConverged: true,
        },
      })),
      stateDirectoryBytesForFoundationFixture: 4_000_000,
      promptWritesPerTurn: 1,
      sqliteWritesPerTextDelta: 0,
      ipcNotificationsPerTextDelta: 0,
      recoveryRequired: false,
      normalSageDataMutations: 0,
    },
    package: {
      candidate: candidatePackage,
      control: controlPackage,
      deltas: Object.fromEntries(
        [
          "sourceFileCount",
          "sourceBytes",
          "dependencyPackageCount",
          "frontendRawBytes",
          "executableBytes",
          "productionPackageBytes",
          "installerBytes",
          "installedCoreBytes",
        ].map((key) => [key, candidatePackage[key] - controlPackage[key]]),
      ),
      defaultProductionFixtureArtifactsBytes: 0,
      defaultProductionChildProcesses: 0,
      runtimeDownloadBytes: 0,
    },
    gestures: {
      contractSha256: frozen.packagedGesturesSha256,
      cases: frozen.gestures.cases.map((item) => ({
        id: item.id,
        build: item.build,
        startedAtUtc: "2026-08-04T00:00:00.000Z",
        completedAtUtc: "2026-08-04T00:01:00.000Z",
        artifactSha256: trust.candidate.executableSha256,
        semanticAutomation: true,
        screenCoordinatesUsed: false,
        globalInputUsed: false,
        systemOrNetworkControlsUsed: false,
        normalSageDataMutations: 0,
        observedOracle: structuredClone(item.oracle),
        orphanProcessesAfterClose: 0,
      })),
    },
    evidence: {
      rawFiles: 12,
      rawBytes: 2_000_000,
      campaignsForCandidateIdentity: 1,
      rawBytesForCandidateIdentity: 2_000_000,
      retainedCandidateIdentities: 2,
      globalFiles: 20,
      globalBytes: 4_000_000,
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
  let previous = -1;
  while (result.evidence.curatedBytes !== previous) {
    previous = result.evidence.curatedBytes;
    result.evidence.curatedBytes = Buffer.byteLength(JSON.stringify(result));
  }
  return result;
}

export function serializedFixture(frozen) {
  const trust = makeTrust(frozen);
  const trustText = JSON.stringify(trust);
  const trustSha256 = sha256(trustText);
  const plan = makePlan(trust, trustSha256, frozen);
  const planText = JSON.stringify(plan);
  const planSha256 = sha256(planText);
  const results = makeResults(plan, planSha256, trust, trustSha256, frozen);
  return { trust, trustText, trustSha256, plan, planText, planSha256, results };
}
