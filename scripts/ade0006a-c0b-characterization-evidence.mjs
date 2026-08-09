import {
  exactKeys,
  finiteNumber,
  nearestRank,
  safeInteger,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import { validateEpochBatch } from "./ade0006a-c0b-characterization-epochs.mjs";
import {
  cpuPercent,
  monotonicDurationUs,
  validateCadence,
  validateSemanticSettle,
} from "./ade0006a-c0b-observation.mjs";

const coreMetrics = ["nativeReceiptToPaint", "inputToPaint", "forcedTeardown"];
const coldMetrics = [
  "spawnToProcessReady",
  "initialize",
  "firstNormalizedOutput",
  "cancelDispatch",
  "cancelTerminal",
  "coldActivation",
  "explicitClose",
];
const boundColdMetrics = {
  spawnToProcessReady: "spawnToProcessReadyUs",
  initialize: "initializeUs",
  firstNormalizedOutput: "firstNormalizedOutputUs",
  cancelDispatch: "cancelDispatchUs",
  cancelTerminal: "cancelTerminalUs",
  coldActivation: "coldActivationUs",
  explicitClose: "explicitCloseUs",
};

function addGate(gates, id, actual, ceiling) {
  gates.push({
    id,
    actual,
    ceiling,
    passed: Number.isFinite(actual) && actual <= ceiling,
  });
}

function validateResourceSnapshot(errors, snapshot, label) {
  exactKeys(
    errors,
    snapshot,
    [
      "ordinal",
      "dueTick",
      "capturedTick",
      "sageMembershipSha256",
      "fixtureMembershipSha256",
      "sageProcessCount",
      "fixtureProcessCount",
      "unknownProcessCount",
      "sageCpuTime100ns",
      "fixtureCpuTime100ns",
      "sagePrivateBytes",
      "fixturePrivateBytes",
      "sageWorkingSetBytes",
      "fixtureWorkingSetBytes",
    ],
    label,
  );
  for (const key of ["sageMembershipSha256", "fixtureMembershipSha256"]) {
    if (!SHA256.test(snapshot?.[key] ?? ""))
      errors.push(`${label}.${key} is invalid`);
  }
  for (const key of ["sageProcessCount", "fixtureProcessCount"]) {
    safeInteger(errors, snapshot?.[key], `${label}.${key}`, 1, 64);
  }
  if (snapshot?.unknownProcessCount !== 0) {
    errors.push(`${label}.unknownProcessCount must be zero`);
  }
  for (const key of [
    "sageCpuTime100ns",
    "fixtureCpuTime100ns",
    "sagePrivateBytes",
    "fixturePrivateBytes",
    "sageWorkingSetBytes",
    "fixtureWorkingSetBytes",
  ]) {
    safeInteger(
      errors,
      snapshot?.[key],
      `${label}.${key}`,
      0,
      Number.MAX_SAFE_INTEGER,
    );
  }
}

function validateResourceRuns(errors, gates, runs, epochs, bundle, results) {
  const count = bundle.contract.characterization.coreEpochs;
  const cadence = bundle.contract.characterization.cadence;
  if (!Array.isArray(runs) || runs.length !== count) {
    errors.push(
      `characterization core must contain exactly ${count} resource runs`,
    );
    return;
  }
  const cpuValues = [];
  const fixturePrivate = [];
  const used = new Set();
  for (const [index, run] of runs.entries()) {
    const label = `characterization.core.resourceRuns[${index}]`;
    exactKeys(
      errors,
      run,
      [
        "epochId",
        "readyTick",
        "samplingStartedTick",
        "coldProcessCount",
        "coldSocketCount",
        "coldListenerCount",
        "coldTimerCount",
        "operationalRuntimeTrees",
        "operationalRunningTurns",
        "snapshots",
      ],
      label,
    );
    const epoch = epochs.get(run?.epochId);
    if (!epoch || epoch.purpose !== "core" || used.has(run?.epochId)) {
      errors.push(`${label}.epochId is foreign or reused`);
    }
    used.add(run?.epochId);
    validateSemanticSettle(
      errors,
      run?.readyTick,
      run?.samplingStartedTick,
      cadence.settleAfterSemanticReadyMs,
      results.monotonicClock.frequencyHz,
      label,
    );
    const before = errors.length;
    validateCadence(
      errors,
      run?.snapshots,
      cadence,
      run?.samplingStartedTick,
      results.monotonicClock.frequencyHz,
      `${label}.snapshots`,
      (snapshot, snapshotLabel) =>
        validateResourceSnapshot(errors, snapshot, snapshotLabel),
    );
    const first = run?.snapshots?.[0];
    for (const snapshot of run?.snapshots ?? []) {
      if (
        snapshot?.sageMembershipSha256 !== first?.sageMembershipSha256 ||
        snapshot?.fixtureMembershipSha256 !== first?.fixtureMembershipSha256 ||
        snapshot?.sageProcessCount !== first?.sageProcessCount ||
        snapshot?.fixtureProcessCount !== first?.fixtureProcessCount
      ) {
        errors.push(`${label}.snapshots changed process membership`);
        break;
      }
      fixturePrivate.push(snapshot?.fixturePrivateBytes);
    }
    for (
      let ordinal = 1;
      ordinal < (run?.snapshots?.length ?? 0);
      ordinal += 1
    ) {
      const previous = run.snapshots[ordinal - 1];
      const current = run.snapshots[ordinal];
      if (
        current.sageCpuTime100ns < previous.sageCpuTime100ns ||
        current.fixtureCpuTime100ns < previous.fixtureCpuTime100ns
      ) {
        errors.push(`${label}.snapshots CPU time is nonmonotonic`);
        break;
      }
      cpuValues.push(
        cpuPercent(
          {
            capturedTick: previous.capturedTick,
            cpuTime100ns:
              previous.sageCpuTime100ns + previous.fixtureCpuTime100ns,
          },
          {
            capturedTick: current.capturedTick,
            cpuTime100ns:
              current.sageCpuTime100ns + current.fixtureCpuTime100ns,
          },
          results.monotonicClock.frequencyHz,
        ),
      );
    }
    for (const key of [
      "coldProcessCount",
      "coldSocketCount",
      "coldListenerCount",
      "coldTimerCount",
      "operationalRuntimeTrees",
      "operationalRunningTurns",
    ]) {
      const expected = bundle.frozen.budgets.resource[key];
      if (run?.[key] !== expected)
        errors.push(`${label}.${key} must equal ${expected}`);
    }
    if (errors.length !== before) continue;
  }
  addGate(
    gates,
    "characterization-ready-idle-cpu",
    nearestRank(cpuValues, 0.95),
    bundle.frozen.budgets.resource.readyIdleCpuSingleCorePercentP95,
  );
  addGate(
    gates,
    "characterization-fixture-private-bytes",
    nearestRank(fixturePrivate, 0.95),
    bundle.frozen.budgets.resource.fixtureTreePrivateBytesP95,
  );
}

function metricGate(gates, metric, values, budgets) {
  const definitions = {
    spawnToProcessReady: [[0.95, "spawnToProcessReadyP95"]],
    initialize: [[0.95, "initializeP95"]],
    firstNormalizedOutput: [
      [0.5, "promptWriteCompleteToFirstNormalizedFixtureOutputP50"],
      [0.95, "promptWriteCompleteToFirstNormalizedFixtureOutputP95"],
    ],
    nativeReceiptToPaint: [[0.95, "nativeReceiptToPaintP95"]],
    inputToPaint: [[0.95, "inputToPaintP95"]],
    cancelDispatch: [[0.95, "cancelDispatchP95"]],
    cancelTerminal: [[0.95, "cancelTerminalP95"]],
    coldActivation: [
      [0.5, "coldActivationP50"],
      [0.95, "coldActivationP95"],
    ],
    explicitClose: [[0.95, "explicitCloseP95"]],
    forcedTeardown: [[1, "forcedTeardownHardMaximum"]],
  };
  for (const [percentile, key] of definitions[metric]) {
    addGate(
      gates,
      `characterization-${metric}-${percentile === 1 ? "maximum" : `p${percentile * 100}`}`,
      nearestRank(values, percentile) / 1000,
      budgets[key],
    );
  }
}

function validateLatencySamples(
  errors,
  gates,
  samples,
  metrics,
  epochs,
  bundle,
  frequencyHz,
  label,
  nativeReceipts,
) {
  const counts =
    bundle.frozen.budgets.sampleContract.characterizationLatencySamples;
  const expectedTotal = metrics.reduce(
    (sum, metric) => sum + counts[metric],
    0,
  );
  if (!Array.isArray(samples) || samples.length !== expectedTotal) {
    errors.push(
      `${label} must contain exactly ${expectedTotal} latency samples`,
    );
    return;
  }
  const grouped = new Map(metrics.map((metric) => [metric, []]));
  const ordinals = new Map(metrics.map((metric) => [metric, new Set()]));
  const metricEpochs = new Map(metrics.map((metric) => [metric, new Set()]));
  const isCold = metrics === coldMetrics;
  const isCore = metrics === coreMetrics;
  const coreSpans = new Set();
  const coreOwnership = new Map(metrics.map((metric) => [metric, new Map()]));
  samples.forEach((sample, index) => {
    const item = `${label}[${index}]`;
    exactKeys(
      errors,
      sample,
      isCore
        ? [
            "metric",
            "sampleOrdinal",
            "epochId",
            "startTick",
            "endTick",
            "durationUs",
          ]
        : ["metric", "sampleOrdinal", "epochId", "durationUs"],
      item,
    );
    if (!grouped.has(sample?.metric)) {
      errors.push(`${item}.metric is not assigned to this observation file`);
      return;
    }
    const epoch = epochs.get(sample?.epochId);
    const expectedPurpose = metrics === coreMetrics ? "core" : "cold";
    safeInteger(
      errors,
      sample?.sampleOrdinal,
      `${item}.sampleOrdinal`,
      1,
      counts[sample.metric],
    );
    finiteNumber(
      errors,
      sample?.durationUs,
      `${item}.durationUs`,
      0,
      60_000_000,
    );
    if (
      !epoch ||
      epoch.purpose !== expectedPurpose ||
      ordinals.get(sample.metric).has(sample.sampleOrdinal)
    ) {
      errors.push(`${item} has foreign epoch or duplicate ordinal`);
    }
    ordinals.get(sample.metric).add(sample.sampleOrdinal);
    if (isCore) {
      safeInteger(
        errors,
        sample?.startTick,
        `${item}.startTick`,
        0,
        Number.MAX_SAFE_INTEGER,
      );
      safeInteger(
        errors,
        sample?.endTick,
        `${item}.endTick`,
        0,
        Number.MAX_SAFE_INTEGER,
      );
      const forced = sample.metric === "forcedTeardown";
      const lastTick = forced
        ? epoch?.zeroDescendantsTick
        : epoch?.closeRequestedTick;
      if (
        sample?.startTick < epoch?.startedTick ||
        sample?.endTick <= sample?.startTick ||
        sample?.endTick > lastTick ||
        sample?.durationUs !==
          monotonicDurationUs(
            sample?.startTick,
            sample?.endTick,
            frequencyHz,
          ) ||
        (forced &&
          (sample?.startTick !== epoch?.closeRequestedTick ||
            sample?.endTick !== epoch?.zeroDescendantsTick))
      ) {
        errors.push(`${item} does not match its QPC anchors`);
      }
      const span = `${sample.metric}\0${sample.startTick}\0${sample.endTick}`;
      if (coreSpans.has(span)) errors.push(`${item} reuses a QPC span`);
      coreSpans.add(span);
      const ownership = coreOwnership.get(sample.metric);
      ownership.set(sample.epochId, (ownership.get(sample.epochId) ?? 0) + 1);
    }
    if (isCold) {
      if (metricEpochs.get(sample.metric).has(sample.epochId)) {
        errors.push(`${item}.epochId is duplicated for ${sample.metric}`);
      }
      metricEpochs.get(sample.metric).add(sample.epochId);
      const nativeField = boundColdMetrics[sample.metric];
      if (
        nativeField &&
        sample.durationUs !== nativeReceipts?.get(sample.epochId)?.[nativeField]
      ) {
        errors.push(`${item}.durationUs does not match its native receipt`);
      }
    }
    grouped.get(sample.metric).push(sample.durationUs);
  });
  for (const metric of metrics) {
    if (grouped.get(metric).length !== counts[metric]) {
      errors.push(`${label} has wrong ${metric} count`);
    } else {
      metricGate(
        gates,
        metric,
        grouped.get(metric),
        bundle.frozen.budgets.latencyMs,
      );
    }
    if (
      isCold &&
      (metricEpochs.get(metric).size !== nativeReceipts?.size ||
        [...(nativeReceipts?.keys() ?? [])].some(
          (epochId) => !metricEpochs.get(metric).has(epochId),
        ))
    ) {
      errors.push(`${label} must map every ${metric} sample to one cold epoch`);
    }
    if (isCore) {
      const coreEpochIds = [...epochs.values()]
        .filter((epoch) => epoch.purpose === "core")
        .map((epoch) => epoch.epochId);
      const samplesPerEpoch = counts[metric] / coreEpochIds.length;
      if (
        !Number.isInteger(samplesPerEpoch) ||
        coreEpochIds.some(
          (epochId) =>
            coreOwnership.get(metric).get(epochId) !== samplesPerEpoch,
        )
      ) {
        errors.push(`${label} has invalid per-epoch ${metric} ownership`);
      }
    }
  }
}

export function validateCoreAndCold(
  errors,
  gates,
  bundle,
  results,
  records,
  identities,
  epochMap,
) {
  const specifications = [
    [
      "characterization-core.json",
      "core",
      bundle.contract.characterization.coreEpochs,
      coreMetrics,
    ],
    [
      "characterization-cold.json",
      "cold",
      bundle.contract.characterization.coldFixtureCycles,
      coldMetrics,
    ],
  ];
  for (const [file, purpose, count, metrics] of specifications) {
    const value = records.get(file)?.value;
    const label = `observation.${file}`;
    exactKeys(
      errors,
      value,
      purpose === "core"
        ? [
            "schemaVersion",
            "participant",
            "epochs",
            "resourceRuns",
            "latencySamples",
          ]
        : ["schemaVersion", "participant", "epochs", "latencySamples"],
      label,
    );
    if (value?.schemaVersion !== 2 || value?.participant !== "candidate") {
      errors.push(`${label} header is invalid`);
    }
    const nativeReceipts = purpose === "cold" ? new Map() : undefined;
    validateEpochBatch(
      errors,
      value?.epochs,
      purpose,
      count,
      bundle,
      results,
      identities,
      epochMap,
      `${label}.epochs`,
      nativeReceipts,
    );
    if (purpose === "core") {
      validateResourceRuns(
        errors,
        gates,
        value?.resourceRuns,
        epochMap,
        bundle,
        results,
      );
    }
    validateLatencySamples(
      errors,
      gates,
      value?.latencySamples,
      metrics,
      epochMap,
      bundle,
      results.monotonicClock.frequencyHz,
      `${label}.latencySamples`,
      nativeReceipts,
    );
  }
}
