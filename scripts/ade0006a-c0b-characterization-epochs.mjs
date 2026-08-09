import {
  exactKeys,
  safeInteger,
  validUtcTimestamp,
} from "./ade0006a-c0b-common.mjs";
import {
  monotonicDurationUs,
  validateCharacterizationNativeReceipt,
  validateCloseGate,
  validateProcessIdentity,
  validateProfileIdentity,
} from "./ade0006a-c0b-observation.mjs";

export const characterizationPurposes = [
  "core",
  "cold",
  "saturation",
  "fairness",
  "coalescing",
];
const caseId = /^[a-z0-9][a-zA-Z0-9._-]{0,127}$/;

const invalidReasons = new Set([
  "identity-mismatch",
  "launch-failure",
  "process-identity-ambiguous",
  "fixture-launch-failure",
  "semantic-verification-failure",
  "observer-unavailable",
  "cadence-miss",
  "membership-change",
  "sample-invalid",
  "normal-data-mutation",
  "orphan-detected",
  "host-changed",
  "power-policy-changed",
]);

function validateEpoch(
  errors,
  epoch,
  expectedPurpose,
  trust,
  frozen,
  results,
  identities,
  label,
) {
  const keys = [
    "epochId",
    "epochOrdinal",
    "purpose",
    "caseId",
    "disposableProfileIdentitySha256",
    "startedAtUtc",
    "completedAtUtc",
    "startedTick",
    "sageProcess",
    "fixtureProcess",
    "closeRequestedTick",
    "zeroDescendantsTick",
    "normalSageDataMutations",
    "orphanProcessesAfterClose",
  ];
  if (expectedPurpose === "cold")
    keys.push("readyVisibleTick", "nativeReceipt");
  exactKeys(errors, epoch, keys, label);
  if (
    !/^epoch-[0-9]{4}$/.test(epoch?.epochId ?? "") ||
    epoch?.purpose !== expectedPurpose ||
    !(
      epoch?.caseId === null ||
      (typeof epoch?.caseId === "string" && caseId.test(epoch.caseId))
    )
  ) {
    errors.push(`${label} identity or purpose is invalid`);
  }
  if (
    !validUtcTimestamp(epoch?.startedAtUtc) ||
    !validUtcTimestamp(epoch?.completedAtUtc) ||
    Date.parse(epoch?.completedAtUtc) < Date.parse(epoch?.startedAtUtc) ||
    Date.parse(epoch?.startedAtUtc) <
      Date.parse(results.timestamps.startedAtUtc) ||
    Date.parse(epoch?.completedAtUtc) >
      Date.parse(results.timestamps.completedAtUtc)
  ) {
    errors.push(`${label} timestamps are invalid`);
  }
  safeInteger(errors, epoch?.epochOrdinal, `${label}.epochOrdinal`, 1, 512);
  safeInteger(
    errors,
    epoch?.startedTick,
    `${label}.startedTick`,
    0,
    Number.MAX_SAFE_INTEGER,
  );
  validateProfileIdentity(
    errors,
    epoch?.disposableProfileIdentitySha256,
    `${label}.disposableProfileIdentitySha256`,
    identities.profiles,
  );
  for (const [key, executable] of [
    ["sageProcess", trust.characterization.executableSha256],
    ["fixtureProcess", trust.characterization.fixtureExecutableSha256],
  ]) {
    const identity = validateProcessIdentity(
      errors,
      epoch?.[key],
      executable,
      `${label}.${key}`,
    );
    if (identities.processes.has(identity)) {
      errors.push(`${label}.${key} identity is reused`);
    }
    identities.processes.add(identity);
  }
  validateCloseGate(errors, epoch, results.monotonicClock.frequencyHz, label);
  if (expectedPurpose !== "cold") return null;

  safeInteger(
    errors,
    epoch?.readyVisibleTick,
    `${label}.readyVisibleTick`,
    0,
    Number.MAX_SAFE_INTEGER,
  );

  const nativeReceipt = validateCharacterizationNativeReceipt(
    errors,
    epoch?.nativeReceipt,
    {
      case: "cold-cycle",
      runtimeEpoch: 1,
      executableSha256: trust.characterization.fixtureExecutableSha256,
      executableBytes: trust.characterization.fixtureExecutableBytes,
      frequencyHz: results.monotonicClock.frequencyHz,
      walCheckpointTargetBytes:
        frozen.budgets.persistence.walCheckpointTargetBytes,
    },
    `${label}.nativeReceipt`,
  );
  if (
    nativeReceipt &&
    (nativeReceipt.fixtureProcess.pid !== epoch?.fixtureProcess?.pid ||
      nativeReceipt.fixtureProcess.creationTimeUtc !==
        epoch?.fixtureProcess?.creationTimeUtc ||
      nativeReceipt.fixtureProcess.creationTimeFiletime100ns !==
        epoch?.fixtureProcess?.creationTimeFiletime100ns ||
      nativeReceipt.fixtureProcess.processIdentitySha256 !==
        epoch?.fixtureProcess?.processIdentitySha256)
  ) {
    errors.push(`${label}.nativeReceipt does not identify fixtureProcess`);
    return null;
  }
  if (
    nativeReceipt &&
    (epoch?.startedTick > nativeReceipt.anchors.spawnStartedTick ||
      nativeReceipt.anchors.readyCommittedTick > epoch?.readyVisibleTick ||
      epoch?.readyVisibleTick > epoch?.closeRequestedTick ||
      epoch?.closeRequestedTick > nativeReceipt.anchors.closeReceivedTick ||
      nativeReceipt.anchors.jobZeroTick > epoch?.zeroDescendantsTick)
  ) {
    errors.push(`${label}.nativeReceipt is outside the epoch close boundary`);
    return null;
  }
  return nativeReceipt
    ? {
        ...nativeReceipt,
        coldActivationUs: monotonicDurationUs(
          epoch.startedTick,
          epoch.readyVisibleTick,
          results.monotonicClock.frequencyHz,
        ),
        explicitCloseUs: monotonicDurationUs(
          epoch.closeRequestedTick,
          Math.max(
            epoch.zeroDescendantsTick,
            nativeReceipt.anchors.walConvergedTick,
          ),
          results.monotonicClock.frequencyHz,
        ),
      }
    : null;
}

export function validateEpochBatch(
  errors,
  epochs,
  expectedPurpose,
  expectedCount,
  bundle,
  results,
  identities,
  epochMap,
  label,
  nativeReceipts,
) {
  if (
    !characterizationPurposes.includes(expectedPurpose) ||
    !Array.isArray(epochs) ||
    epochs.length !== expectedCount
  ) {
    errors.push(
      `${label} must contain exactly ${expectedCount} ${expectedPurpose} epochs`,
    );
    return;
  }
  for (const [index, epoch] of epochs.entries()) {
    const nativeReceipt = validateEpoch(
      errors,
      epoch,
      expectedPurpose,
      bundle.trust.value,
      bundle.frozen,
      results,
      identities,
      `${label}[${index}]`,
    );
    if (epochMap.has(epoch?.epochId)) {
      errors.push(`${label}[${index}].epochId is reused`);
    }
    epochMap.set(epoch?.epochId, epoch);
    if (nativeReceipt && nativeReceipts) {
      nativeReceipts.set(epoch?.epochId, nativeReceipt);
    }
  }
}

export function validateCharacterizationInvalidAttempts(
  errors,
  attempts,
  epochs,
  frozen,
) {
  const maximum =
    frozen.budgets.sampleContract.maximumInvalidCharacterizationAttempts;
  if (!Array.isArray(attempts) || attempts.length > maximum) {
    errors.push("characterization.invalidAttempts exceeds its bound");
    return;
  }
  const ordinals = new Set();
  attempts.forEach((attempt, index) => {
    const label = `characterization.invalidAttempts[${index}]`;
    exactKeys(
      errors,
      attempt,
      [
        "attemptOrdinal",
        "purpose",
        "caseId",
        "reason",
        "replacementEpochId",
        "observationsRetained",
      ],
      label,
    );
    safeInteger(
      errors,
      attempt?.attemptOrdinal,
      `${label}.attemptOrdinal`,
      1,
      maximum,
    );
    const replacement = epochs.get(attempt?.replacementEpochId);
    if (
      ordinals.has(attempt?.attemptOrdinal) ||
      !characterizationPurposes.includes(attempt?.purpose) ||
      !(
        attempt?.caseId === null ||
        (typeof attempt?.caseId === "string" && caseId.test(attempt.caseId))
      ) ||
      !invalidReasons.has(attempt?.reason) ||
      !replacement ||
      replacement.purpose !== attempt?.purpose ||
      replacement.caseId !== attempt?.caseId ||
      attempt?.observationsRetained !== false
    ) {
      errors.push(`${label} is not a typed discarded attempt`);
    }
    ordinals.add(attempt?.attemptOrdinal);
  });
}
