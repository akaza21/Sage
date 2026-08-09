import {
  exactKeys,
  finiteNumber,
  nearestRank,
  safeInteger,
} from "./ade0006a-c0b-common.mjs";
import { validateEpochBatch } from "./ade0006a-c0b-characterization-epochs.mjs";

function addGate(gates, id, actual, ceiling) {
  gates.push({
    id,
    actual,
    ceiling,
    passed: Number.isFinite(actual) && actual <= ceiling,
  });
}

function validateSaturationState(errors, state, boundary, label) {
  exactKeys(
    errors,
    state,
    [
      "acceptedCount",
      "rejectedCount",
      "retainedBytes",
      "queueHighWaterCount",
      "queueHighWaterBytes",
      "typedError",
      "terminalState",
    ],
    label,
  );
  for (const key of [
    "acceptedCount",
    "rejectedCount",
    "retainedBytes",
    "queueHighWaterCount",
    "queueHighWaterBytes",
  ]) {
    safeInteger(errors, state?.[key], `${label}.${key}`, 0, 2 ** 32);
  }
  if (
    !(
      state?.typedError === null || state?.typedError === "capacity-exceeded"
    ) ||
    !["completed", "capacity-rejected"].includes(state?.terminalState)
  ) {
    errors.push(`${label} contains an untyped disposition`);
  }
}

function saturationViolations(cycle, boundary) {
  let count = 0;
  for (const state of [cycle.exact, cycle.plusOne]) {
    if (
      state.retainedBytes < 1 ||
      state.retainedBytes > boundary.bytes ||
      state.queueHighWaterBytes < 1 ||
      state.queueHighWaterBytes > boundary.bytes ||
      state.queueHighWaterCount > boundary.count
    ) {
      count += 1;
    }
  }
  if (
    (boundary.kind === "count" &&
      (cycle.exact.acceptedCount !== boundary.count ||
        cycle.exact.queueHighWaterCount !== boundary.count)) ||
    (boundary.kind === "bytes" &&
      (cycle.exact.retainedBytes !== boundary.bytes ||
        cycle.exact.queueHighWaterBytes !== boundary.bytes)) ||
    cycle.exact.rejectedCount !== 0 ||
    cycle.exact.typedError !== null ||
    cycle.exact.terminalState !== "completed" ||
    cycle.plusOne.rejectedCount !== 1 ||
    cycle.plusOne.typedError !== "capacity-exceeded" ||
    cycle.plusOne.terminalState !== "capacity-rejected" ||
    cycle.fairnessDisposition !== "bounded-fifo" ||
    cycle.zeroStarved !== true ||
    cycle.pollingDetected !== false
  ) {
    count += 1;
  }
  return count;
}

function validateSaturationFile(
  errors,
  gates,
  file,
  boundaryId,
  boundary,
  bundle,
  results,
  identities,
  epochMap,
) {
  const value = file?.value;
  const label = `observation.saturation-${boundaryId}.json`;
  exactKeys(
    errors,
    value,
    ["schemaVersion", "participant", "boundaryId", "epochs", "cycles"],
    label,
  );
  if (
    value?.schemaVersion !== 1 ||
    value?.participant !== "candidate" ||
    value?.boundaryId !== boundaryId
  ) {
    errors.push(`${label} header is invalid`);
  }
  const count = bundle.contract.characterization.saturationCyclesPerBoundary;
  validateEpochBatch(
    errors,
    value?.epochs,
    "saturation",
    count,
    bundle,
    results,
    identities,
    epochMap,
    `${label}.epochs`,
  );
  if (!Array.isArray(value?.cycles) || value.cycles.length !== count) {
    errors.push(`${label}.cycles must contain exactly ${count} observations`);
    return;
  }
  const used = new Set();
  const dispatch = [];
  let violations = 0;
  value.cycles.forEach((cycle, index) => {
    const item = `${label}.cycles[${index}]`;
    exactKeys(
      errors,
      cycle,
      [
        "cycleOrdinal",
        "epochId",
        "exact",
        "plusOne",
        "controlLaneDispatchUs",
        "fairnessDisposition",
        "zeroStarved",
        "pollingDetected",
      ],
      item,
    );
    const epoch = epochMap.get(cycle?.epochId);
    if (
      cycle?.cycleOrdinal !== index + 1 ||
      !epoch ||
      epoch.purpose !== "saturation" ||
      epoch.caseId !== boundaryId ||
      used.has(cycle?.epochId)
    ) {
      errors.push(`${item}.epochId is foreign or reused`);
    }
    used.add(cycle?.epochId);
    validateSaturationState(errors, cycle?.exact, boundary, `${item}.exact`);
    validateSaturationState(
      errors,
      cycle?.plusOne,
      boundary,
      `${item}.plusOne`,
    );
    finiteNumber(
      errors,
      cycle?.controlLaneDispatchUs,
      `${item}.controlLaneDispatchUs`,
      0,
      60_000_000,
    );
    dispatch.push(cycle?.controlLaneDispatchUs);
    if (cycle?.exact && cycle?.plusOne) {
      violations += saturationViolations(cycle, boundary);
    }
  });
  addGate(gates, `saturation-${boundaryId}-contract`, violations, 0);
  addGate(
    gates,
    `saturation-${boundaryId}-control-lane-p95`,
    nearestRank(dispatch, 0.95) / 1000,
    bundle.frozen.budgets.latencyMs.cancelDispatchP95,
  );
}

function validateFairness(
  errors,
  gates,
  record,
  bundle,
  results,
  identities,
  epochMap,
) {
  const value = record?.value;
  const label = "observation.characterization-fairness.json";
  exactKeys(
    errors,
    value,
    ["schemaVersion", "participant", "epochs", "observations"],
    label,
  );
  if (value?.schemaVersion !== 1 || value?.participant !== "candidate") {
    errors.push(`${label} header is invalid`);
  }
  const count = bundle.contract.characterization.fairnessCycles;
  validateEpochBatch(
    errors,
    value?.epochs,
    "fairness",
    count,
    bundle,
    results,
    identities,
    epochMap,
    `${label}.epochs`,
  );
  if (
    !Array.isArray(value?.observations) ||
    value.observations.length !== count
  ) {
    errors.push(`${label}.observations must contain exactly ${count} cycles`);
    return;
  }
  const used = new Set();
  const dispatch = [];
  let violations = 0;
  value.observations.forEach((observation, index) => {
    const item = `${label}.observations[${index}]`;
    exactKeys(
      errors,
      observation,
      [
        "cycleOrdinal",
        "epochId",
        "controlLaneDispatchUs",
        "dataAccepted",
        "zeroStarved",
        "disposition",
        "pollingDetected",
      ],
      item,
    );
    const epoch = epochMap.get(observation?.epochId);
    if (
      observation?.cycleOrdinal !== index + 1 ||
      !epoch ||
      epoch.purpose !== "fairness" ||
      epoch.caseId !== null ||
      used.has(observation?.epochId)
    ) {
      errors.push(`${item}.epochId is foreign or reused`);
    }
    used.add(observation?.epochId);
    finiteNumber(
      errors,
      observation?.controlLaneDispatchUs,
      `${item}.controlLaneDispatchUs`,
      0,
      60_000_000,
    );
    safeInteger(
      errors,
      observation?.dataAccepted,
      `${item}.dataAccepted`,
      1,
      2 ** 32,
    );
    dispatch.push(observation?.controlLaneDispatchUs);
    if (
      observation?.zeroStarved !== true ||
      observation?.disposition !== "bounded-fifo" ||
      observation?.pollingDetected !== false
    ) {
      violations += 1;
    }
  });
  addGate(gates, "fairness-contract", violations, 0);
  addGate(
    gates,
    "fairness-control-lane-p95",
    nearestRank(dispatch, 0.95) / 1000,
    bundle.frozen.budgets.latencyMs.cancelDispatchP95,
  );
}

function validateCoalescing(
  errors,
  gates,
  record,
  bundle,
  results,
  identities,
  epochMap,
) {
  const value = record?.value;
  const label = "observation.characterization-coalescing.json";
  exactKeys(
    errors,
    value,
    ["schemaVersion", "participant", "epochs", "selectedPolicy", "policies"],
    label,
  );
  if (value?.schemaVersion !== 1 || value?.participant !== "candidate") {
    errors.push(`${label} header is invalid`);
  }
  const samples = bundle.frozen.budgets.sampleContract;
  const epochCount =
    samples.coalescingPolicies.length * samples.coalescingEpochsPerPolicy;
  validateEpochBatch(
    errors,
    value?.epochs,
    "coalescing",
    epochCount,
    bundle,
    results,
    identities,
    epochMap,
    `${label}.epochs`,
  );
  if (
    !Array.isArray(value?.policies) ||
    value.policies.length !== samples.coalescingPolicies.length
  ) {
    errors.push(`${label}.policies is incomplete`);
    return;
  }
  const p95 = new Map();
  let contractViolations = 0;
  value.policies.forEach((policy, policyIndex) => {
    const item = `${label}.policies[${policyIndex}]`;
    exactKeys(
      errors,
      policy,
      [
        "id",
        "epochIds",
        "receiptToPaintSamples",
        "sqliteWritesPerTextDelta",
        "queueSaturationObservations",
      ],
      item,
    );
    if (policy?.id !== samples.coalescingPolicies[policyIndex]) {
      errors.push(`${item}.id is unknown or out of order`);
    }
    if (
      !Array.isArray(policy?.epochIds) ||
      policy.epochIds.length !== samples.coalescingEpochsPerPolicy ||
      new Set(policy.epochIds).size !== policy.epochIds.length ||
      policy.epochIds.some((epochId) => {
        const epoch = epochMap.get(epochId);
        return (
          !epoch || epoch.purpose !== "coalescing" || epoch.caseId !== policy.id
        );
      })
    ) {
      errors.push(`${item}.epochIds is incomplete or foreign`);
    }
    const expectedSamples =
      samples.coalescingEpochsPerPolicy * samples.coalescingSamplesPerEpoch;
    if (
      !Array.isArray(policy?.receiptToPaintSamples) ||
      policy.receiptToPaintSamples.length !== expectedSamples
    ) {
      errors.push(
        `${item}.receiptToPaintSamples must contain exactly ${expectedSamples} samples`,
      );
      return;
    }
    const ordinals = new Set();
    const perEpoch = new Map(policy.epochIds.map((id) => [id, 0]));
    const durations = [];
    policy.receiptToPaintSamples.forEach((sample, index) => {
      const sampleLabel = `${item}.receiptToPaintSamples[${index}]`;
      exactKeys(
        errors,
        sample,
        ["sampleOrdinal", "epochId", "durationUs"],
        sampleLabel,
      );
      safeInteger(
        errors,
        sample?.sampleOrdinal,
        `${sampleLabel}.sampleOrdinal`,
        1,
        expectedSamples,
      );
      finiteNumber(
        errors,
        sample?.durationUs,
        `${sampleLabel}.durationUs`,
        0,
        60_000_000,
      );
      if (
        ordinals.has(sample?.sampleOrdinal) ||
        !perEpoch.has(sample?.epochId)
      ) {
        errors.push(`${sampleLabel} has duplicate ordinal or foreign epoch`);
      }
      ordinals.add(sample?.sampleOrdinal);
      perEpoch.set(sample?.epochId, (perEpoch.get(sample?.epochId) ?? 0) + 1);
      durations.push(sample?.durationUs);
    });
    if (
      [...perEpoch.values()].some(
        (count) => count !== samples.coalescingSamplesPerEpoch,
      )
    ) {
      errors.push(`${item} does not distribute samples evenly across epochs`);
    }
    p95.set(policy.id, nearestRank(durations, 0.95));
    addGate(
      gates,
      `coalescing-${policy.id}-paint-p95`,
      p95.get(policy.id) / 1000,
      bundle.frozen.budgets.latencyMs.nativeReceiptToPaintP95,
    );
    if (
      policy?.sqliteWritesPerTextDelta !== 0 ||
      policy?.queueSaturationObservations !== 0
    ) {
      contractViolations += 1;
    }
  });
  const selected = [...samples.coalescingPolicies].sort(
    (left, right) =>
      p95.get(left) - p95.get(right) ||
      samples.coalescingPolicies.indexOf(left) -
        samples.coalescingPolicies.indexOf(right),
  )[0];
  if (value?.selectedPolicy !== selected) contractViolations += 1;
  addGate(gates, "coalescing-selection-contract", contractViolations, 0);
}

export function validateCharacterizationLoad(
  errors,
  gates,
  bundle,
  results,
  records,
  identities,
  epochMap,
) {
  for (const [boundaryId, boundary] of bundle.frozen.queueBoundaries) {
    validateSaturationFile(
      errors,
      gates,
      records.get(`saturation-${boundaryId}.json`),
      boundaryId,
      boundary,
      bundle,
      results,
      identities,
      epochMap,
    );
  }
  validateFairness(
    errors,
    gates,
    records.get("characterization-fairness.json"),
    bundle,
    results,
    identities,
    epochMap,
  );
  validateCoalescing(
    errors,
    gates,
    records.get("characterization-coalescing.json"),
    bundle,
    results,
    identities,
    epochMap,
  );
}
