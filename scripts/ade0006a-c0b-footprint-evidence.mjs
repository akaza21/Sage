import {
  exactKeys,
  finiteNumber,
  nearestRank,
  safeInteger,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import {
  cpuPercent,
  monotonicDurationUs,
  validateCadence,
  validateCloseGate,
  validateProcessIdentity,
  validateProfileIdentity,
  validateSemanticSettle,
} from "./ade0006a-c0b-observation.mjs";
import { validatePaintReceipt } from "./ade0006a-c0b-paint-receipt.mjs";

const participants = ["candidate", "control"];
const latencyFields = {
  spawnToProcessReady: ["spawnToWindowReadyUs", "spawnToProcessReadyP95"],
  inputToPaint: ["inputToPaintUs", "inputToPaintP95"],
  tabSwitch: ["tabSwitchUs", "tabSwitchP95"],
};
const invalidReasons = new Set([
  "identity-mismatch",
  "launch-failure",
  "process-identity-ambiguous",
  "state-setup-failure",
  "semantic-verification-failure",
  "cadence-miss",
  "membership-change",
  "sample-invalid",
  "normal-data-mutation",
  "orphan-detected",
  "host-changed",
  "power-policy-changed",
]);

function validateSnapshot(errors, snapshot, label) {
  exactKeys(
    errors,
    snapshot,
    [
      "ordinal",
      "dueTick",
      "capturedTick",
      "membershipSha256",
      "coreProcessCount",
      "unknownProcessCount",
      "coreCpuTime100ns",
      "corePrivateBytes",
      "coreWorkingSetBytes",
    ],
    label,
  );
  if (!SHA256.test(snapshot?.membershipSha256 ?? "")) {
    errors.push(`${label}.membershipSha256 is invalid`);
  }
  safeInteger(
    errors,
    snapshot?.coreProcessCount,
    `${label}.coreProcessCount`,
    1,
    64,
  );
  if (snapshot?.unknownProcessCount !== 0) {
    errors.push(`${label}.unknownProcessCount must be zero`);
  }
  for (const key of [
    "coreCpuTime100ns",
    "corePrivateBytes",
    "coreWorkingSetBytes",
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

export function validatePackagedRun({
  run,
  launchOrdinal,
  executableBindingSha256,
  cadence,
  frequencyHz,
  label,
  identities,
}) {
  const errors = [];
  exactKeys(
    errors,
    run,
    [
      "launchOrdinal",
      "pid",
      "creationTimeUtc",
      "creationTimeFiletime100ns",
      "processIdentitySha256",
      "executableBindingSha256",
      "disposableProfileIdentitySha256",
      "spawnStartedTick",
      "windowReadyTick",
      "stateReadyTick",
      "samplingStartedTick",
      "stateVerified",
      "inputReceivedTick",
      "inputPaintedTick",
      "tabActivatedTick",
      "tabPaintedTick",
      "spawnToWindowReadyUs",
      "inputToPaintUs",
      "tabSwitchUs",
      "inputPaintReceipt",
      "tabPaintReceipt",
      "snapshots",
      "closeRequestedTick",
      "zeroDescendantsTick",
      "orphanCheckCompletedTick",
      "normalSageDataMutations",
      "orphanProcessesAfterClose",
    ],
    label,
  );
  if (
    run?.launchOrdinal !== launchOrdinal ||
    run?.executableBindingSha256 !== executableBindingSha256 ||
    run?.stateVerified !== true
  ) {
    errors.push(`${label} launch binding or semantic state is invalid`);
  }
  const processIdentity = validateProcessIdentity(
    errors,
    {
      pid: run?.pid,
      creationTimeUtc: run?.creationTimeUtc,
      creationTimeFiletime100ns: run?.creationTimeFiletime100ns,
      processIdentitySha256: run?.processIdentitySha256,
    },
    executableBindingSha256,
    label,
  );
  if (identities.processes.has(processIdentity)) {
    errors.push(`${label}.processIdentitySha256 is reused`);
  }
  identities.processes.add(processIdentity);
  validateProfileIdentity(
    errors,
    run?.disposableProfileIdentitySha256,
    `${label}.disposableProfileIdentitySha256`,
    identities.profiles,
  );
  for (const key of Object.values(latencyFields).map(([field]) => field)) {
    finiteNumber(errors, run?.[key], `${label}.${key}`, 0, 60_000_000);
  }
  for (const key of [
    "spawnStartedTick",
    "windowReadyTick",
    "stateReadyTick",
    "samplingStartedTick",
    "inputReceivedTick",
    "inputPaintedTick",
    "tabActivatedTick",
    "tabPaintedTick",
  ]) {
    safeInteger(
      errors,
      run?.[key],
      `${label}.${key}`,
      0,
      Number.MAX_SAFE_INTEGER,
    );
  }
  if (
    run?.windowReadyTick <= run?.spawnStartedTick ||
    run?.stateReadyTick < run?.windowReadyTick ||
    run?.inputReceivedTick < run?.windowReadyTick ||
    run?.inputPaintedTick <= run?.inputReceivedTick ||
    run?.tabActivatedTick < run?.windowReadyTick ||
    run?.tabPaintedTick <= run?.tabActivatedTick ||
    run?.inputPaintedTick > run?.closeRequestedTick ||
    run?.tabPaintedTick > run?.closeRequestedTick ||
    run?.spawnToWindowReadyUs !==
      monotonicDurationUs(
        run?.spawnStartedTick,
        run?.windowReadyTick,
        frequencyHz,
      ) ||
    run?.inputToPaintUs !==
      monotonicDurationUs(
        run?.inputReceivedTick,
        run?.inputPaintedTick,
        frequencyHz,
      ) ||
    run?.tabSwitchUs !==
      monotonicDurationUs(
        run?.tabActivatedTick,
        run?.tabPaintedTick,
        frequencyHz,
      )
  ) {
    errors.push(`${label} latency anchors are invalid`);
  }
  validatePaintReceipt(
    errors,
    run?.inputPaintReceipt,
    {
      startedTick: run?.inputReceivedTick,
      completedTick: run?.inputPaintedTick,
      durationUs: run?.inputToPaintUs,
    },
    frequencyHz,
    `${label}.inputPaintReceipt`,
  );
  validatePaintReceipt(
    errors,
    run?.tabPaintReceipt,
    {
      startedTick: run?.tabActivatedTick,
      completedTick: run?.tabPaintedTick,
      durationUs: run?.tabSwitchUs,
    },
    frequencyHz,
    `${label}.tabPaintReceipt`,
  );
  validateSemanticSettle(
    errors,
    run?.stateReadyTick,
    run?.samplingStartedTick,
    cadence.settleAfterSemanticReadyMs,
    frequencyHz,
    label,
  );
  validateCadence(
    errors,
    run?.snapshots,
    cadence,
    run?.samplingStartedTick,
    frequencyHz,
    `${label}.snapshots`,
    (snapshot, snapshotLabel) =>
      validateSnapshot(errors, snapshot, snapshotLabel),
  );
  const first = run?.snapshots?.[0];
  for (const snapshot of run?.snapshots ?? []) {
    if (
      snapshot?.membershipSha256 !== first?.membershipSha256 ||
      snapshot?.coreProcessCount !== first?.coreProcessCount
    ) {
      errors.push(`${label}.snapshots changed process membership`);
      break;
    }
  }
  for (let index = 1; index < (run?.snapshots?.length ?? 0); index += 1) {
    if (
      run.snapshots[index].coreCpuTime100ns <
      run.snapshots[index - 1].coreCpuTime100ns
    ) {
      errors.push(`${label}.snapshots CPU time is nonmonotonic`);
      break;
    }
  }
  if (run?.closeRequestedTick < run?.snapshots?.at(-1)?.capturedTick) {
    errors.push(`${label} closed before sampling completed`);
  }
  validateCloseGate(errors, run, frequencyHz, label);
  return errors;
}

function validateInvalidAttempts(errors, attempts, contract) {
  if (
    !Array.isArray(attempts) ||
    attempts.length > contract.maximumInvalidFootprintAttempts
  ) {
    errors.push("packagedFootprint.invalidAttempts exceeds its bound");
    return;
  }
  const ordinals = new Set();
  attempts.forEach((attempt, index) => {
    const label = `packagedFootprint.invalidAttempts[${index}]`;
    exactKeys(
      errors,
      attempt,
      [
        "attemptOrdinal",
        "stateId",
        "plannedPairOrdinal",
        "order",
        "failedParticipant",
        "reason",
        "replacementPairOrdinal",
        "samplesRetained",
      ],
      label,
    );
    safeInteger(
      errors,
      attempt?.attemptOrdinal,
      `${label}.attemptOrdinal`,
      1,
      contract.maximumInvalidFootprintAttempts,
    );
    const schedule = contract.pairSchedule[attempt?.plannedPairOrdinal - 1];
    if (
      ordinals.has(attempt?.attemptOrdinal) ||
      !contract.states.includes(attempt?.stateId) ||
      !schedule ||
      JSON.stringify(attempt?.order) !== JSON.stringify(schedule.order) ||
      !["candidate", "control", "pair"].includes(attempt?.failedParticipant) ||
      !invalidReasons.has(attempt?.reason) ||
      attempt?.replacementPairOrdinal !== attempt?.plannedPairOrdinal ||
      attempt?.samplesRetained !== false
    ) {
      errors.push(`${label} is not a typed discarded attempt`);
    }
    ordinals.add(attempt?.attemptOrdinal);
  });
}

function addGate(gates, id, actual, ceiling) {
  gates.push({
    id,
    actual,
    ceiling,
    passed: Number.isFinite(actual) && actual <= ceiling,
  });
}

function gateState(state, frozen, frequencyHz, gates) {
  const latency = frozen.budgets.latencyMs;
  const regression = frozen.budgets.controlRegressionMax.latencyMs;
  for (const [metric, [field, ceilingKey]] of Object.entries(latencyFields)) {
    const candidate = state.validPairs.map(
      ({ participants: value }) => value.candidate[field],
    );
    const control = state.validPairs.map(
      ({ participants: value }) => value.control[field],
    );
    const candidateP95 = nearestRank(candidate, 0.95) / 1000;
    const controlP95 = nearestRank(control, 0.95) / 1000;
    addGate(
      gates,
      `packaged-${state.stateId}-${metric}-absolute`,
      candidateP95,
      latency[ceilingKey],
    );
    addGate(
      gates,
      `packaged-${state.stateId}-${metric}-regression`,
      candidateP95 - controlP95,
      regression[ceilingKey],
    );
  }
  const close = Object.fromEntries(
    participants.map((participant) => [
      participant,
      state.validPairs.map(
        ({ participants: value }) =>
          ((value[participant].zeroDescendantsTick -
            value[participant].closeRequestedTick) *
            1000) /
          frequencyHz,
      ),
    ]),
  );
  const candidateClose = nearestRank(close.candidate, 0.95);
  const controlClose = nearestRank(close.control, 0.95);
  addGate(
    gates,
    `packaged-${state.stateId}-explicit-close-absolute`,
    candidateClose,
    latency.explicitCloseP95,
  );
  addGate(
    gates,
    `packaged-${state.stateId}-explicit-close-regression`,
    candidateClose - controlClose,
    regression.explicitCloseP95,
  );

  const cpu = { candidate: [], control: [] };
  for (const participant of participants) {
    for (const pair of state.validPairs) {
      const snapshots = pair.participants[participant].snapshots;
      for (let index = 1; index < snapshots.length; index += 1) {
        cpu[participant].push(
          cpuPercent(
            {
              ...snapshots[index - 1],
              cpuTime100ns: snapshots[index - 1].coreCpuTime100ns,
            },
            {
              ...snapshots[index],
              cpuTime100ns: snapshots[index].coreCpuTime100ns,
            },
            frequencyHz,
          ),
        );
      }
    }
  }
  const candidateCpu = nearestRank(cpu.candidate, 0.95);
  const controlCpu = nearestRank(cpu.control, 0.95);
  addGate(
    gates,
    `packaged-${state.stateId}-cpu-absolute`,
    candidateCpu,
    frozen.budgets.resource.readyIdleCpuSingleCorePercentP95,
  );
  addGate(
    gates,
    `packaged-${state.stateId}-cpu-regression`,
    candidateCpu - controlCpu,
    frozen.budgets.controlRegressionMax.resource
      .readyIdleCpuSingleCorePercentagePointsP95,
  );

  const privateValues = { candidate: [], control: [] };
  for (const participant of participants) {
    for (const pair of state.validPairs) {
      for (const ordinal of frozen.budgets.measurementContract.packagedFootprint
        .adr0005ProjectionOrdinals) {
        privateValues[participant].push(
          pair.participants[participant].snapshots[ordinal].corePrivateBytes,
        );
      }
    }
  }
  addGate(
    gates,
    `packaged-${state.stateId}-private-bytes`,
    nearestRank(privateValues.candidate, 0.95) -
      nearestRank(privateValues.control, 0.95),
    frozen.budgets.resource.sageNativeAndRendererIncrementalPrivateBytesP95,
  );
}

function validateLaunchChronology(errors, previous, current, label) {
  if (!previous) return;
  if (
    !Number.isSafeInteger(previous?.spawnStartedTick) ||
    !Number.isSafeInteger(previous?.zeroDescendantsTick) ||
    !Number.isSafeInteger(current?.spawnStartedTick) ||
    previous.spawnStartedTick >= current.spawnStartedTick ||
    previous.zeroDescendantsTick > current.spawnStartedTick
  ) {
    errors.push(`${label} launch chronology violates fixed alternation`);
  }
}

export function validatePackagedFootprint(
  errors,
  gates,
  bundle,
  results,
  records,
) {
  const { frozen, authenticatedPackage } = bundle;
  const contract = {
    ...frozen.budgets.sampleContract,
    ...bundle.contract.packagedFootprint,
  };
  const value = results?.packagedFootprint;
  exactKeys(
    errors,
    value,
    ["installerKind", "states", "invalidAttempts"],
    "packagedFootprint",
  );
  if (value?.installerKind !== authenticatedPackage.installerKind) {
    errors.push("packagedFootprint.installerKind is stale");
  }
  if (
    !Array.isArray(value?.states) ||
    value.states.length !== contract.states.length
  ) {
    errors.push("packagedFootprint.states is incomplete");
    return;
  }
  const identities = { processes: new Set(), profiles: new Set() };
  value.states.forEach((reference, stateIndex) => {
    const errorsBefore = errors.length;
    let previousRun = null;
    const label = `packagedFootprint.states[${stateIndex}]`;
    exactKeys(errors, reference, ["id", "observationFile"], label);
    const expectedFile = `footprint-${contract.states[stateIndex]}.json`;
    const state = records.get(reference?.observationFile)?.value;
    if (
      reference?.id !== contract.states[stateIndex] ||
      reference?.observationFile !== expectedFile ||
      !state
    ) {
      errors.push(`${label} does not reference its fixed observation`);
      return;
    }
    exactKeys(
      errors,
      state,
      ["schemaVersion", "stateId", "validPairs"],
      `${label}.observation`,
    );
    if (
      state?.schemaVersion !== 2 ||
      state?.stateId !== contract.states[stateIndex] ||
      !Array.isArray(state?.validPairs) ||
      state.validPairs.length !== contract.validPairsPerState
    ) {
      errors.push(`${label} is incomplete or out of order`);
      return;
    }
    state.validPairs.forEach((pair, pairIndex) => {
      const pairLabel = `${label}.validPairs[${pairIndex}]`;
      const schedule = contract.pairSchedule[pairIndex];
      exactKeys(
        errors,
        pair,
        ["pairOrdinal", "order", "participants"],
        pairLabel,
      );
      exactKeys(
        errors,
        pair?.participants,
        participants,
        `${pairLabel}.participants`,
      );
      if (
        pair?.pairOrdinal !== schedule.pairOrdinal ||
        JSON.stringify(pair?.order) !== JSON.stringify(schedule.order)
      ) {
        errors.push(`${pairLabel} violates fixed alternation`);
      }
      for (const participant of participants) {
        const expected = authenticatedPackage.participants[participant];
        errors.push(
          ...validatePackagedRun({
            run: pair?.participants?.[participant],
            launchOrdinal:
              stateIndex * contract.validPairsPerState + pairIndex + 1,
            executableBindingSha256: expected.executableBindingSha256,
            cadence: contract.cadence,
            frequencyHz: results.monotonicClock.frequencyHz,
            label: `${pairLabel}.${participant}`,
            identities,
          }),
        );
      }
      for (const participant of schedule.order) {
        const current = pair?.participants?.[participant];
        validateLaunchChronology(
          errors,
          previousRun,
          current,
          `${pairLabel}.${participant}`,
        );
        previousRun = current;
      }
    });
    if (
      state.validPairs.every((pair) =>
        participants.every(
          (participant) =>
            Array.isArray(pair?.participants?.[participant]?.snapshots) &&
            pair.participants[participant].snapshots.length ===
              contract.cadence.snapshotCount,
        ),
      ) &&
      errors.length === errorsBefore
    ) {
      gateState(state, frozen, results.monotonicClock.frequencyHz, gates);
    }
  });
  validateInvalidAttempts(errors, value?.invalidAttempts, contract);
  return identities;
}
