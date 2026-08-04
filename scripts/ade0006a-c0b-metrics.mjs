import { exactKeys } from "./ade0006a-c0b-common.mjs";

export function resourceMetricAnchors(frozen) {
  const ready = frozen.budgets.measurementContract.readyIdle;
  const membership = frozen.budgets.measurementContract.processMembership;
  return {
    readyIdleCpuSingleCorePercent: [
      ready.entry,
      ready.cpuFormula,
      ready.cpuAggregation,
    ].join("; "),
    fixtureTreePrivateBytes: [
      membership.fixtureTree,
      membership.privateBytes,
    ].join("; "),
    sageNativeAndRendererIncrementalPrivateBytes: [
      membership.nativeAndRenderer,
      membership.privateBytes,
      "candidate sample minus its matched control sample",
    ].join("; "),
  };
}

function nearestRank(values, percentile) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.ceil(percentile * sorted.length) - 1];
}

function validateSamples(errors, metric, label, count, anchor, pairs) {
  exactKeys(errors, metric, ["percentileMethod", "anchor", "samples"], label);
  if (metric?.percentileMethod !== "nearest-rank") {
    errors.push(`${label}.percentileMethod must be nearest-rank`);
  }
  if (typeof metric?.anchor !== "string" || metric.anchor.length === 0) {
    errors.push(`${label}.anchor is required`);
  } else if (anchor && metric.anchor !== anchor) {
    errors.push(`${label}.anchor does not match the frozen anchor`);
  }
  if (
    !Array.isArray(metric?.samples) ||
    metric.samples.length !== pairs.size * 2
  ) {
    errors.push(
      `${label}.samples must contain exactly ${pairs.size * 2} pair segments`,
    );
    return null;
  }
  const perPair = count / pairs.size;
  const segments = new Set();
  const participantCounts = new Map([
    ["candidate", 0],
    ["control", 0],
  ]);
  for (const [index, sample] of metric.samples.entries()) {
    const item = `${label}.samples[${index}]`;
    exactKeys(
      errors,
      sample,
      [
        "pairOrdinal",
        "participant",
        "launchOrdinal",
        "processIdentitySha256",
        "firstSampleOrdinal",
        "values",
      ],
      item,
    );
    const pair = pairs.get(sample?.pairOrdinal);
    if (!pair || !["candidate", "control"].includes(sample?.participant)) {
      errors.push(`${item} references a foreign pair or participant`);
      continue;
    }
    const process = pair.participants[sample.participant];
    if (
      sample.launchOrdinal !== process.launchOrdinal ||
      sample.processIdentitySha256 !== process.processIdentitySha256
    ) {
      errors.push(`${item} does not bind the measured process launch`);
    }
    const pairKey = `${sample.participant}:${sample.pairOrdinal}`;
    if (segments.has(pairKey))
      errors.push(`${label} has a duplicate pair segment`);
    segments.add(pairKey);
    const expectedFirst = (sample.pairOrdinal - 1) * perPair + 1;
    if (
      sample.firstSampleOrdinal !== expectedFirst ||
      !Array.isArray(sample.values) ||
      sample.values.length !== perPair ||
      sample.values.some((value) => !Number.isFinite(value) || value < 0)
    ) {
      errors.push(`${item} does not bind its complete sample ordinal range`);
    }
    participantCounts.set(
      sample.participant,
      participantCounts.get(sample.participant) + (sample.values?.length ?? 0),
    );
  }
  for (const participant of ["candidate", "control"]) {
    if (participantCounts.get(participant) !== count)
      errors.push(`${label} has missing ${participant} samples`);
    for (const pairOrdinal of pairs.keys()) {
      if (!segments.has(`${participant}:${pairOrdinal}`))
        errors.push(`${label} omits a valid pair segment`);
    }
  }
  return metric;
}

function participantValues(metric, participant) {
  return metric.samples
    .filter((sample) => sample?.participant === participant)
    .flatMap((sample) => (Array.isArray(sample?.values) ? sample.values : []));
}

function gate(errors, metric, label, percentile, absolute, regression) {
  const candidate = nearestRank(
    participantValues(metric, "candidate"),
    percentile,
  );
  const control = nearestRank(participantValues(metric, "control"), percentile);
  if (candidate > absolute)
    errors.push(`${label} exceeds its frozen absolute ceiling`);
  if (regression !== null && candidate - control > regression) {
    errors.push(`${label} exceeds its matched-control regression ceiling`);
  }
}

export function validateLatencyMetrics(errors, results, plan, frozen, pairs) {
  const definitions = {
    spawnToProcessReady: [
      5,
      "spawnToProcessReady",
      "spawnToProcessReadyP95",
      "spawnToProcessReadyP95",
    ],
    initialize: [5, "initialize", "initializeP95", "initializeP95"],
    firstNormalizedOutput: [
      5,
      "firstNormalizedOutput",
      "promptWriteCompleteToFirstNormalizedFixtureOutputP95",
      "promptWriteCompleteToFirstNormalizedFixtureOutputP95",
    ],
    nativeReceiptToPaint: [
      100,
      "nativeReceiptToPaint",
      "nativeReceiptToPaintP95",
      "nativeReceiptToPaintP95",
    ],
    inputToPaint: [100, "inputToPaint", "inputToPaintP95", "inputToPaintP95"],
    tabSwitch: [100, "tabSwitch", "tabSwitchP95", "tabSwitchP95"],
    cancelDispatch: [
      100,
      "cancelDispatch",
      "cancelDispatchP95",
      "cancelDispatchP95",
    ],
    cancelTerminal: [
      5,
      "cancelTerminal",
      "cancelTerminalP95",
      "cancelTerminalP95",
    ],
    coldActivation: [
      5,
      "coldActivation",
      "coldActivationP95",
      "coldActivationP95",
    ],
    explicitClose: [5, "explicitClose", "explicitCloseP95", "explicitCloseP95"],
    forcedTeardown: [5, "forcedTeardown", "forcedTeardownHardMaximum", null],
  };
  exactKeys(errors, results.latencyMs, Object.keys(definitions), "latencyMs");
  for (const [
    key,
    [count, anchor, absoluteKey, regressionKey],
  ] of Object.entries(definitions)) {
    const metric = validateSamples(
      errors,
      results.latencyMs?.[key],
      `latencyMs.${key}`,
      count,
      plan.anchors[anchor],
      pairs,
    );
    if (!metric) continue;
    gate(
      errors,
      metric,
      `latencyMs.${key}.p95`,
      0.95,
      frozen.budgets.latencyMs[absoluteKey],
      regressionKey
        ? frozen.budgets.controlRegressionMax.latencyMs[regressionKey]
        : null,
    );
    if (key === "firstNormalizedOutput") {
      gate(
        errors,
        metric,
        "latencyMs.firstNormalizedOutput.p50",
        0.5,
        frozen.budgets.latencyMs
          .promptWriteCompleteToFirstNormalizedFixtureOutputP50,
        frozen.budgets.controlRegressionMax.latencyMs
          .promptWriteCompleteToFirstNormalizedFixtureOutputP50,
      );
    }
    if (key === "coldActivation") {
      gate(
        errors,
        metric,
        "latencyMs.coldActivation.p50",
        0.5,
        frozen.budgets.latencyMs.coldActivationP50,
        frozen.budgets.controlRegressionMax.latencyMs.coldActivationP50,
      );
    }
  }
}

export function validateResourceMetrics(errors, results, frozen, pairs) {
  const anchors = resourceMetricAnchors(frozen);
  const metricKeys = [
    "readyIdleCpuSingleCorePercent",
    "fixtureTreePrivateBytes",
    "sageNativeAndRendererIncrementalPrivateBytes",
  ];
  const discrete = [
    "coldProcessCount",
    "coldSocketCount",
    "coldListenerCount",
    "coldTimerCount",
    "operationalRuntimeTrees",
    "operationalRunningTurns",
    "orphanProcessesAfterClose",
  ];
  exactKeys(
    errors,
    results.resources,
    [...metricKeys, ...discrete],
    "resources",
  );
  const gates = {
    readyIdleCpuSingleCorePercent: [
      frozen.budgets.resource.readyIdleCpuSingleCorePercentP95,
      frozen.budgets.controlRegressionMax.resource
        .readyIdleCpuSingleCorePercentagePointsP95,
    ],
    fixtureTreePrivateBytes: [
      frozen.budgets.resource.fixtureTreePrivateBytesP95,
      frozen.budgets.controlRegressionMax.resource.fixtureTreePrivateBytesP95,
    ],
    sageNativeAndRendererIncrementalPrivateBytes: [
      frozen.budgets.resource.sageNativeAndRendererIncrementalPrivateBytesP95,
      frozen.budgets.controlRegressionMax.resource
        .sageNativeAndRendererIncrementalPrivateBytesP95,
    ],
  };
  for (const [key, [absolute, regression]] of Object.entries(gates)) {
    const metric = validateSamples(
      errors,
      results.resources?.[key],
      `resources.${key}`,
      200,
      anchors[key],
      pairs,
    );
    if (metric)
      gate(errors, metric, `resources.${key}.p95`, 0.95, absolute, regression);
  }
  for (const key of discrete.slice(0, 4).concat("orphanProcessesAfterClose")) {
    if (results.resources?.[key] !== 0)
      errors.push(`resources.${key} must be zero`);
  }
  for (const key of ["operationalRuntimeTrees", "operationalRunningTurns"]) {
    if (results.resources?.[key] !== frozen.budgets.resource[key])
      errors.push(`resources.${key} is wrong`);
  }
}

export function validateCoalescing(errors, results, plan, frozen, pairs) {
  exactKeys(
    errors,
    results.coalescing,
    ["selectedPolicy", "selectionRule", "policies"],
    "coalescing",
  );
  if (
    results.coalescing?.selectionRule !==
    "lowest-p95-with-zero-saturation-and-zero-durable-delta-writes"
  )
    errors.push("coalescing.selectionRule is not frozen");
  if (
    !Array.isArray(results.coalescing?.policies) ||
    results.coalescing.policies.length !==
      plan.collection.coalescingPolicies.length
  ) {
    errors.push("coalescing.policies is incomplete");
    return;
  }
  const seen = new Set();
  const candidateP95 = new Map();
  for (const policy of results.coalescing.policies) {
    exactKeys(
      errors,
      policy,
      [
        "id",
        "receiptToPaint",
        "sqliteWritesPerTextDelta",
        "queueSaturationObservations",
      ],
      `coalescing.${policy.id}`,
    );
    if (
      !plan.collection.coalescingPolicies.includes(policy.id) ||
      seen.has(policy.id)
    )
      errors.push("coalescing contains an unknown or duplicate policy");
    seen.add(policy.id);
    const metric = validateSamples(
      errors,
      policy.receiptToPaint,
      `coalescing.${policy.id}.receiptToPaint`,
      plan.collection.coalescingSamplesPerPolicy,
      plan.anchors.nativeReceiptToPaint,
      pairs,
    );
    if (metric) {
      gate(
        errors,
        metric,
        `coalescing.${policy.id}.receiptToPaint.p95`,
        0.95,
        frozen.budgets.latencyMs.nativeReceiptToPaintP95,
        frozen.budgets.controlRegressionMax.latencyMs.nativeReceiptToPaintP95,
      );
      candidateP95.set(
        policy.id,
        nearestRank(participantValues(metric, "candidate"), 0.95),
      );
    }
    if (
      policy.sqliteWritesPerTextDelta !== 0 ||
      policy.queueSaturationObservations !== 0
    )
      errors.push(`coalescing.${policy.id} retained avoidable work`);
  }
  if (!seen.has(results.coalescing?.selectedPolicy)) {
    errors.push("coalescing.selectedPolicy was not measured");
  } else if (
    candidateP95.get(results.coalescing.selectedPolicy) !==
    Math.min(...candidateP95.values())
  ) {
    errors.push("coalescing.selectedPolicy is not a lowest-P95 valid policy");
  }
}
