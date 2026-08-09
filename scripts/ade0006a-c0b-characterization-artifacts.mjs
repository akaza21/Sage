import {
  exactKeys,
  recordDigest,
  safeInteger,
  SHA256,
  validUtcTimestamp,
} from "./ade0006a-c0b-common.mjs";
import {
  monotonicDurationUs,
  validateCloseGate,
  validateProcessIdentity,
  validateProfileIdentity,
} from "./ade0006a-c0b-observation.mjs";

function addGate(gates, id, actual, ceiling) {
  gates.push({
    id,
    actual,
    ceiling,
    passed: Number.isFinite(actual) && actual <= ceiling,
  });
}

function validateLaunch(
  errors,
  launch,
  index,
  bundle,
  results,
  identities,
  label,
) {
  exactKeys(
    errors,
    launch,
    [
      "launchOrdinal",
      "startedAtUtc",
      "completedAtUtc",
      "startedTick",
      "sageProcess",
      "fixtureProcess",
      "closeRequestedTick",
      "zeroDescendantsTick",
      "normalSageDataMutations",
      "orphanProcessesAfterClose",
    ],
    label,
  );
  if (
    launch?.launchOrdinal !== index + 1 ||
    !validUtcTimestamp(launch?.startedAtUtc) ||
    !validUtcTimestamp(launch?.completedAtUtc) ||
    Date.parse(launch?.completedAtUtc) < Date.parse(launch?.startedAtUtc) ||
    Date.parse(launch?.startedAtUtc) <
      Date.parse(results.timestamps.startedAtUtc) ||
    Date.parse(launch?.completedAtUtc) >
      Date.parse(results.timestamps.completedAtUtc)
  ) {
    errors.push(`${label} identity or timestamps are invalid`);
  }
  safeInteger(
    errors,
    launch?.startedTick,
    `${label}.startedTick`,
    0,
    Number.MAX_SAFE_INTEGER,
  );
  for (const [key, executable] of [
    ["sageProcess", bundle.trust.value.characterization.executableSha256],
    [
      "fixtureProcess",
      bundle.trust.value.characterization.fixtureExecutableSha256,
    ],
  ]) {
    const identity = validateProcessIdentity(
      errors,
      launch?.[key],
      executable,
      `${label}.${key}`,
    );
    if (identities.processes.has(identity))
      errors.push(`${label}.${key} identity is reused`);
    identities.processes.add(identity);
  }
  validateCloseGate(errors, launch, results.monotonicClock.frequencyHz, label);
}

export function validatePersistence(
  errors,
  gates,
  bundle,
  results,
  record,
  identities,
) {
  const value = record?.value;
  const label = "observation.characterization-persistence.json";
  exactKeys(
    errors,
    value,
    [
      "schemaVersion",
      "participant",
      "disposableProfileIdentitySha256",
      "launches",
      "markers",
      "stateDirectoryBytesForFoundationFixture",
      "promptWritesPerTurn",
      "sqliteWritesPerTextDelta",
      "ipcNotificationsPerTextDelta",
      "recoveryRequired",
    ],
    label,
  );
  if (value?.schemaVersion !== 2 || value?.participant !== "candidate") {
    errors.push(`${label} header is invalid`);
  }
  validateProfileIdentity(
    errors,
    value?.disposableProfileIdentitySha256,
    `${label}.disposableProfileIdentitySha256`,
    identities.profiles,
  );
  const contract = bundle.frozen.budgets.measurementContract.persistence;
  if (
    !Array.isArray(value?.launches) ||
    value.launches.length !== contract.restartCount + 1
  ) {
    errors.push(`${label}.launches has wrong restart cardinality`);
  } else {
    value.launches.forEach((launch, index) =>
      validateLaunch(
        errors,
        launch,
        index,
        bundle,
        results,
        identities,
        `${label}.launches[${index}]`,
      ),
    );
    for (let index = 1; index < value.launches.length; index += 1) {
      const previous = value.launches[index - 1];
      const current = value.launches[index];
      if (
        current.startedTick < previous.zeroDescendantsTick ||
        Date.parse(current.startedAtUtc) < Date.parse(previous.completedAtUtc)
      ) {
        errors.push(`${label}.launches overlap or are out of order`);
      }
    }
  }
  if (
    !Array.isArray(value?.markers) ||
    value.markers.map(({ id }) => id).join("\0") !== contract.markers.join("\0")
  ) {
    errors.push(`${label}.markers is incomplete or out of order`);
  } else {
    const priorCheckpointEnd = new Map();
    value.markers.forEach((marker, index) => {
      const item = `${label}.markers[${index}]`;
      exactKeys(
        errors,
        marker,
        [
          "id",
          "launchOrdinal",
          "databaseBytes",
          "walBytesBeforeCheckpoint",
          "walBytesAfterCheckpoint",
          "checkpointStartedTick",
          "checkpointCompletedTick",
          "checkpointDurationUs",
          "walConverged",
        ],
        item,
      );
      const expectedLaunch = index < 2 ? 1 : 2;
      if (marker?.launchOrdinal !== expectedLaunch) {
        errors.push(`${item}.launchOrdinal is invalid`);
      }
      for (const key of [
        "databaseBytes",
        "walBytesBeforeCheckpoint",
        "walBytesAfterCheckpoint",
        "checkpointStartedTick",
        "checkpointCompletedTick",
        "checkpointDurationUs",
      ]) {
        safeInteger(
          errors,
          marker?.[key],
          `${item}.${key}`,
          0,
          Number.MAX_SAFE_INTEGER,
        );
      }
      const launch = value.launches?.[marker?.launchOrdinal - 1];
      if (
        !launch ||
        marker?.checkpointStartedTick < launch.startedTick ||
        marker?.checkpointCompletedTick <= marker?.checkpointStartedTick ||
        marker?.checkpointCompletedTick > launch.zeroDescendantsTick ||
        marker?.checkpointStartedTick <
          (priorCheckpointEnd.get(marker?.launchOrdinal) ?? 0) ||
        marker?.checkpointDurationUs !==
          monotonicDurationUs(
            marker?.checkpointStartedTick,
            marker?.checkpointCompletedTick,
            results.monotonicClock.frequencyHz,
          ) ||
        marker?.walBytesAfterCheckpoint > marker?.walBytesBeforeCheckpoint ||
        marker?.walBytesAfterCheckpoint >=
          bundle.frozen.budgets.persistence.walCheckpointTargetBytes ||
        marker?.walConverged !== true
      ) {
        errors.push(`${item} does not prove ordered WAL convergence`);
      }
      priorCheckpointEnd.set(
        marker?.launchOrdinal,
        marker?.checkpointCompletedTick,
      );
      addGate(
        gates,
        `persistence-${marker.id}-database-bytes`,
        marker.databaseBytes,
        bundle.frozen.budgets.persistence.liveDatabaseBytes,
      );
      addGate(
        gates,
        `persistence-${marker.id}-wal-hard-stop`,
        marker.walBytesBeforeCheckpoint,
        bundle.frozen.budgets.persistence.walHardStopBytes,
      );
      addGate(
        gates,
        `persistence-${marker.id}-wal-checkpoint`,
        marker.walBytesAfterCheckpoint,
        bundle.frozen.budgets.persistence.walCheckpointTargetBytes,
      );
      addGate(
        gates,
        `persistence-${marker.id}-checkpoint-duration`,
        marker.checkpointDurationUs / 1000,
        bundle.frozen.limits.deadlinesMs.walConvergence,
      );
      addGate(
        gates,
        `persistence-${marker.id}-converged`,
        marker.walConverged === true ? 0 : 1,
        0,
      );
    });
  }
  addGate(
    gates,
    "persistence-state-directory-bytes",
    value?.stateDirectoryBytesForFoundationFixture,
    bundle.frozen.budgets.persistence.stateDirectoryBytesForFoundationFixture,
  );
  for (const key of [
    "promptWritesPerTurn",
    "sqliteWritesPerTextDelta",
    "ipcNotificationsPerTextDelta",
  ]) {
    safeInteger(
      errors,
      value?.[key],
      `${label}.${key}`,
      0,
      Number.MAX_SAFE_INTEGER,
    );
    addGate(
      gates,
      `persistence-${key}`,
      Math.abs(value?.[key] - bundle.frozen.budgets.persistence[key]),
      0,
    );
  }
  addGate(
    gates,
    "persistence-recovery-required",
    value?.recoveryRequired === false ? 0 : 1,
    0,
  );
}

function validateGestureRun(
  errors,
  gates,
  run,
  expected,
  bundle,
  results,
  identities,
  index,
) {
  const label = `observation.characterization-gestures.json.runs[${index}]`;
  exactKeys(
    errors,
    run,
    [
      "id",
      "build",
      "startedAtUtc",
      "completedAtUtc",
      "artifactIdentitySha256",
      "disposableProfileIdentitySha256",
      "sageProcess",
      "fixtureProcess",
      "semanticAutomation",
      "screenCoordinatesUsed",
      "globalInputUsed",
      "systemOrNetworkControlsUsed",
      "debugAutomationUsed",
      "observedOracleSha256",
      "closeRequestedTick",
      "zeroDescendantsTick",
      "normalSageDataMutations",
      "orphanProcessesAfterClose",
    ],
    label,
  );
  const production = expected.build === "production-default";
  const executable = production
    ? bundle.authenticatedPackage.participants.candidate.executableBindingSha256
    : bundle.trust.value.characterization.executableSha256;
  if (
    run?.id !== expected.id ||
    run?.build !== expected.build ||
    run?.artifactIdentitySha256 !== executable ||
    !validUtcTimestamp(run?.startedAtUtc) ||
    !validUtcTimestamp(run?.completedAtUtc) ||
    Date.parse(run?.completedAtUtc) < Date.parse(run?.startedAtUtc) ||
    Date.parse(run?.startedAtUtc) <
      Date.parse(results.timestamps.startedAtUtc) ||
    Date.parse(run?.completedAtUtc) >
      Date.parse(results.timestamps.completedAtUtc)
  ) {
    errors.push(`${label} identity or timestamps are invalid`);
  }
  validateProfileIdentity(
    errors,
    run?.disposableProfileIdentitySha256,
    `${label}.disposableProfileIdentitySha256`,
    identities.profiles,
  );
  const sageIdentity = validateProcessIdentity(
    errors,
    run?.sageProcess,
    executable,
    `${label}.sageProcess`,
  );
  if (identities.processes.has(sageIdentity))
    errors.push(`${label}.sageProcess identity is reused`);
  identities.processes.add(sageIdentity);
  if (production) {
    if (run?.fixtureProcess !== null)
      errors.push(`${label}.fixtureProcess must be absent`);
  } else {
    const fixtureIdentity = validateProcessIdentity(
      errors,
      run?.fixtureProcess,
      bundle.trust.value.characterization.fixtureExecutableSha256,
      `${label}.fixtureProcess`,
    );
    if (identities.processes.has(fixtureIdentity))
      errors.push(`${label}.fixtureProcess identity is reused`);
    identities.processes.add(fixtureIdentity);
  }
  if (
    run?.semanticAutomation !== true ||
    run?.screenCoordinatesUsed !== false ||
    run?.globalInputUsed !== false ||
    run?.systemOrNetworkControlsUsed !== false ||
    run?.debugAutomationUsed !== false
  ) {
    errors.push(`${label} used a forbidden observation method`);
  }
  if (!SHA256.test(run?.observedOracleSha256 ?? "")) {
    errors.push(`${label}.observedOracleSha256 is invalid`);
  }
  addGate(
    gates,
    `gesture-${expected.id}`,
    run?.observedOracleSha256 === recordDigest(expected.oracle) ? 0 : 1,
    0,
  );
  validateCloseGate(errors, run, results.monotonicClock.frequencyHz, label);
}

export function validateGestures(
  errors,
  gates,
  bundle,
  results,
  record,
  identities,
) {
  const value = record?.value;
  const label = "observation.characterization-gestures.json";
  exactKeys(
    errors,
    value,
    ["schemaVersion", "participant", "contractSha256", "runs"],
    label,
  );
  if (
    value?.schemaVersion !== 1 ||
    value?.participant !== "candidate" ||
    value?.contractSha256 !== bundle.frozen.packagedGesturesSha256 ||
    !Array.isArray(value?.runs) ||
    value.runs.length !== bundle.frozen.gestures.cases.length
  ) {
    errors.push(`${label} header or run count is invalid`);
    return;
  }
  value.runs.forEach((run, index) =>
    validateGestureRun(
      errors,
      gates,
      run,
      bundle.frozen.gestures.cases[index],
      bundle,
      results,
      identities,
      index,
    ),
  );
}
