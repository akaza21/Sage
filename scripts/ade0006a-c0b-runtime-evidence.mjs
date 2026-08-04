import {
  exactKeys,
  recordDigest,
  safeInteger,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import { buildPairSchedule } from "./validate-ade0006a-c0b-plan.mjs";

function validateParticipant(errors, value, binding, label) {
  exactKeys(
    errors,
    value,
    [
      "launchOrdinal",
      "pidLabel",
      "creationIdentitySha256",
      "processIdentitySha256",
      "executableSha256",
      "processCount",
      "orphanProcessesAfterClose",
    ],
    label,
  );
  safeInteger(errors, value?.launchOrdinal, `${label}.launchOrdinal`, 1, 64);
  if (!/^pid-[1-9][0-9]*$/.test(value?.pidLabel ?? "")) {
    errors.push(`${label}.pidLabel is invalid`);
  }
  if (!SHA256.test(value?.creationIdentitySha256 ?? "")) {
    errors.push(`${label}.creationIdentitySha256 is invalid`);
  }
  const identity = recordDigest({
    pidLabel: value?.pidLabel,
    creationIdentitySha256: value?.creationIdentitySha256,
  });
  if (value?.processIdentitySha256 !== identity) {
    errors.push(`${label}.processIdentitySha256 is not PID-creation bound`);
  }
  if (value?.executableSha256 !== binding.executableSha256) {
    errors.push(`${label}.executableSha256 is stale`);
  }
  safeInteger(errors, value?.processCount, `${label}.processCount`, 1, 64);
  if (value?.orphanProcessesAfterClose !== 0) {
    errors.push(`${label} retained orphan processes`);
  }
}

export function validatePairs(errors, results, plan, trust) {
  const schedule = buildPairSchedule(plan.collection.validPairs);
  if (
    !Array.isArray(results.validPairs) ||
    results.validPairs.length !== schedule.length
  ) {
    errors.push(`validPairs must contain exactly ${schedule.length} pairs`);
    return new Map();
  }
  const pairs = new Map();
  const identities = new Set();
  const provenance = recordDigest(plan.provenance);
  results.validPairs.forEach((pair, index) => {
    const label = `validPairs[${index}]`;
    exactKeys(
      errors,
      pair,
      ["pairOrdinal", "order", "provenanceSha256", "participants"],
      label,
    );
    const expected = schedule[index];
    if (
      pair?.pairOrdinal !== expected.pairOrdinal ||
      JSON.stringify(pair?.order) !== JSON.stringify(expected.order)
    ) {
      errors.push(`${label} violates the fixed alternation`);
    }
    if (pair?.provenanceSha256 !== provenance) {
      errors.push(`${label} does not bind host/power/toolchain provenance`);
    }
    exactKeys(
      errors,
      pair?.participants,
      ["candidate", "control"],
      `${label}.participants`,
    );
    for (const participant of ["candidate", "control"]) {
      const process = pair?.participants?.[participant];
      validateParticipant(
        errors,
        process,
        trust[participant],
        `${label}.${participant}`,
      );
      if (identities.has(process?.processIdentitySha256)) {
        errors.push(`${label} reuses a process identity`);
      }
      identities.add(process?.processIdentitySha256);
    }
    pairs.set(pair?.pairOrdinal, pair);
  });
  validateInvalidPairs(errors, results.invalidPairs, schedule);
  validateWarmups(errors, results.warmupExclusions, plan, trust, identities);
  return pairs;
}

function validateInvalidPairs(errors, invalidPairs, schedule) {
  if (!Array.isArray(invalidPairs) || invalidPairs.length > schedule.length) {
    errors.push(
      "invalidPairs must be a bounded array with at most one replacement per pair",
    );
    return;
  }
  const attempts = new Set();
  const replacements = new Set();
  const reasons = new Set([
    "identity-mismatch",
    "launch-failure",
    "process-identity-ambiguous",
    "sample-invalid",
    "normal-data-mutation",
    "orphan-detected",
    "host-changed",
    "power-policy-changed",
  ]);
  invalidPairs.forEach((pair, index) => {
    const label = `invalidPairs[${index}]`;
    exactKeys(
      errors,
      pair,
      [
        "attemptOrdinal",
        "order",
        "reason",
        "replacementValidPairOrdinal",
        "participantsRecorded",
      ],
      label,
    );
    safeInteger(errors, pair?.attemptOrdinal, `${label}.attemptOrdinal`, 1, 64);
    const replacement = schedule[pair?.replacementValidPairOrdinal - 1];
    if (
      !replacement ||
      JSON.stringify(pair?.order) !== JSON.stringify(replacement.order)
    ) {
      errors.push(`${label} does not preserve replacement pair order`);
    }
    if (!reasons.has(pair?.reason)) errors.push(`${label}.reason is not typed`);
    if (pair?.participantsRecorded !== false) {
      errors.push(`${label} retained invalid participant samples`);
    }
    if (attempts.has(pair?.attemptOrdinal))
      errors.push("invalidPairs has a duplicate attempt ordinal");
    if (replacements.has(pair?.replacementValidPairOrdinal))
      errors.push("invalidPairs has a duplicate replacement pair");
    attempts.add(pair?.attemptOrdinal);
    replacements.add(pair?.replacementValidPairOrdinal);
  });
}

function validateWarmups(errors, warmups, plan, trust, usedIdentities) {
  const schedule = buildPairSchedule(plan.collection.excludedWarmups);
  if (!Array.isArray(warmups) || warmups.length !== schedule.length) {
    errors.push(
      `warmupExclusions must contain exactly ${schedule.length} cycles`,
    );
    return;
  }
  warmups.forEach((warmup, index) => {
    const label = `warmupExclusions[${index}]`;
    exactKeys(
      errors,
      warmup,
      ["warmupOrdinal", "order", "excluded", "participants"],
      label,
    );
    if (
      warmup?.warmupOrdinal !== index + 1 ||
      JSON.stringify(warmup?.order) !== JSON.stringify(schedule[index].order) ||
      warmup?.excluded !== true
    ) {
      errors.push(`${label} does not match the excluded warmup schedule`);
    }
    exactKeys(
      errors,
      warmup?.participants,
      ["candidate", "control"],
      `${label}.participants`,
    );
    for (const participant of ["candidate", "control"]) {
      const process = warmup?.participants?.[participant];
      validateParticipant(
        errors,
        process,
        trust[participant],
        `${label}.${participant}`,
      );
      if (usedIdentities.has(process?.processIdentitySha256)) {
        errors.push(`${label} warmup identity leaked into valid samples`);
      }
      usedIdentities.add(process?.processIdentitySha256);
    }
  });
}

function validateSaturationState(errors, state, boundary, label, plusOne) {
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
    state?.retainedBytes < 1 ||
    state?.retainedBytes > boundary.bytes ||
    state?.queueHighWaterBytes < 1 ||
    state?.queueHighWaterBytes > boundary.bytes ||
    state?.queueHighWaterCount > boundary.count
  ) {
    errors.push(`${label} violates count/byte queue accounting`);
  }
  if (
    boundary.kind === "count" &&
    (state?.acceptedCount !== boundary.count ||
      state?.queueHighWaterCount !== boundary.count)
  ) {
    errors.push(`${label} does not reach the exact count boundary`);
  }
  if (
    boundary.kind === "bytes" &&
    (state?.retainedBytes !== boundary.bytes ||
      state?.queueHighWaterBytes !== boundary.bytes)
  ) {
    errors.push(`${label} does not reach the exact byte boundary`);
  }
  if (plusOne) {
    if (
      state?.rejectedCount !== 1 ||
      state?.typedError !== "capacity-exceeded" ||
      state?.terminalState !== "capacity-rejected"
    ) {
      errors.push(`${label} does not fail closed at limit plus one`);
    }
  } else if (
    state?.rejectedCount !== 0 ||
    state?.typedError !== null ||
    state?.terminalState !== "completed"
  ) {
    errors.push(`${label} does not admit the exact boundary`);
  }
}

export function validateSaturation(errors, results, plan, frozen, pairs) {
  const observations = results.queueSaturation;
  if (!Array.isArray(observations)) {
    errors.push("queueSaturation must be an array");
    return;
  }
  const seen = new Set();
  for (const observation of observations) {
    const boundary = frozen.queueBoundaries.get(observation?.id);
    if (!boundary || seen.has(observation?.id)) {
      errors.push("queueSaturation contains an unknown or duplicate boundary");
      continue;
    }
    seen.add(observation.id);
    exactKeys(
      errors,
      observation,
      ["id", "kind", "cycles"],
      `queueSaturation.${observation.id}`,
    );
    if (
      observation.kind !== boundary.kind ||
      !Array.isArray(observation.cycles) ||
      observation.cycles.length !== plan.collection.saturationCyclesPerBoundary
    ) {
      errors.push(`queueSaturation.${observation.id} shape is invalid`);
      continue;
    }
    observation.cycles.forEach((cycle, index) => {
      const label = `queueSaturation.${observation.id}.cycles[${index}]`;
      exactKeys(
        errors,
        cycle,
        [
          "cycleOrdinal",
          "pairOrdinal",
          "processIdentitySha256",
          "exact",
          "plusOne",
          "controlLaneDispatchMs",
          "fairnessDisposition",
          "zeroStarved",
          "pollingDetected",
          "orphanProcessesAfterClose",
        ],
        label,
      );
      const pair = pairs.get(cycle?.pairOrdinal);
      if (
        cycle?.cycleOrdinal !== index + 1 ||
        !pair ||
        cycle?.processIdentitySha256 !==
          pair.participants.candidate.processIdentitySha256
      )
        errors.push(`${label} is not bound to a valid candidate launch`);
      validateSaturationState(
        errors,
        cycle?.exact,
        boundary,
        `${label}.exact`,
        false,
      );
      validateSaturationState(
        errors,
        cycle?.plusOne,
        boundary,
        `${label}.plusOne`,
        true,
      );
      if (
        !Number.isFinite(cycle?.controlLaneDispatchMs) ||
        cycle.controlLaneDispatchMs < 0 ||
        cycle.controlLaneDispatchMs >
          frozen.budgets.latencyMs.cancelDispatchP95 ||
        cycle?.fairnessDisposition !== "bounded-fifo" ||
        cycle?.zeroStarved !== true ||
        cycle?.pollingDetected !== false ||
        cycle?.orphanProcessesAfterClose !== 0
      )
        errors.push(`${label} fails fairness/control/orphan policy`);
    });
  }
  if (seen.size !== frozen.queueBoundaries.size) {
    errors.push("queueSaturation omits a frozen boundary");
  }
}

export function validateFairness(errors, results, plan, frozen, pairs) {
  exactKeys(errors, results.fairness, ["cycles", "observations"], "fairness");
  if (
    results.fairness?.cycles !== plan.collection.fairnessCycles ||
    !Array.isArray(results.fairness?.observations) ||
    results.fairness.observations.length !== plan.collection.fairnessCycles
  ) {
    errors.push("fairness cycle accounting is incomplete");
    return;
  }
  results.fairness.observations.forEach((item, index) => {
    const label = `fairness.observations[${index}]`;
    exactKeys(
      errors,
      item,
      [
        "cycleOrdinal",
        "pairOrdinal",
        "processIdentitySha256",
        "controlLaneDispatchMs",
        "dataAccepted",
        "zeroStarved",
        "disposition",
      ],
      label,
    );
    const pair = pairs.get(item?.pairOrdinal);
    if (
      item?.cycleOrdinal !== index + 1 ||
      !pair ||
      item?.processIdentitySha256 !==
        pair.participants.candidate.processIdentitySha256 ||
      !Number.isFinite(item?.controlLaneDispatchMs) ||
      item.controlLaneDispatchMs > frozen.budgets.latencyMs.cancelDispatchP95 ||
      !Number.isSafeInteger(item?.dataAccepted) ||
      item.dataAccepted < 1 ||
      item?.zeroStarved !== true ||
      item?.disposition !== "bounded-fifo"
    )
      errors.push(`${label} fails bounded fairness`);
  });
}
