import {
  exactKeys,
  recordDigest,
  safeInteger,
  SHA256,
  validUtcTimestamp,
} from "./ade0006a-c0b-common.mjs";

const maximumTick = Number.MAX_SAFE_INTEGER;
const windowsToUnixEpoch100ns = 116_444_736_000_000_000n;
const maximumU64 = 18_446_744_073_709_551_615n;
const characterizationCases = new Set([
  "turn-success",
  "turn-cancelled",
  "cold-cycle",
]);

const clockFields = [
  "frequencyHz",
  "spawnStartedTick",
  "processReadyTick",
  "initializeWriteStartedTick",
  "readyCommittedTick",
  "promptWriteCompletedTick",
  "firstNormalizedItemAcceptedTick",
  "cancelReceivedTick",
  "cancelWriteCompletedTick",
  "cancelTerminalCommittedTick",
  "closeReceivedTick",
  "jobZeroTick",
  "closeCommittedTick",
  "walConvergedTick",
];

const latencyFields = [
  "spawnToProcessReady",
  "initialize",
  "firstNormalizedOutput",
  "cancelDispatch",
  "cancelTerminal",
  "nativeProcessConvergence",
];

export function validateCharacterizationNativeReceipt(
  errors,
  value,
  expected,
  label = "characterizationNativeReceipt",
) {
  const errorCount = errors.length;
  exactKeys(
    errors,
    value,
    [
      "schemaVersion",
      "case",
      "runtimeEpoch",
      "fixtureProcess",
      "monotonicClock",
      "walConvergence",
      "latencyUs",
    ],
    label,
  );
  if (
    value?.schemaVersion !== 2 ||
    !characterizationCases.has(value?.case) ||
    value?.case !== expected.case ||
    value?.runtimeEpoch !== expected.runtimeEpoch
  ) {
    errors.push(`${label} header is invalid`);
  }
  exactKeys(
    errors,
    value?.fixtureProcess,
    ["pid", "creationTimeFiletime100ns", "executableSha256", "executableBytes"],
    `${label}.fixtureProcess`,
  );
  safeInteger(
    errors,
    value?.fixtureProcess?.pid,
    `${label}.fixtureProcess.pid`,
    1,
    2 ** 32 - 1,
  );
  safeInteger(
    errors,
    value?.fixtureProcess?.executableBytes,
    `${label}.fixtureProcess.executableBytes`,
    1,
    maximumTick,
  );
  safeInteger(
    errors,
    value?.runtimeEpoch,
    `${label}.runtimeEpoch`,
    1,
    maximumTick,
  );
  if (
    value?.fixtureProcess?.executableSha256 !== expected.executableSha256 ||
    value?.fixtureProcess?.executableBytes !== expected.executableBytes
  ) {
    errors.push(`${label}.fixtureProcess does not match the admitted artifact`);
  }
  const creationTimeUtc = filetimeUtc(
    errors,
    value?.fixtureProcess?.creationTimeFiletime100ns,
    `${label}.fixtureProcess.creationTimeFiletime100ns`,
  );

  const clock = value?.monotonicClock;
  exactKeys(errors, clock, clockFields, `${label}.monotonicClock`);
  for (const key of clockFields) {
    if (clock?.[key] === null) continue;
    safeInteger(
      errors,
      clock?.[key],
      `${label}.monotonicClock.${key}`,
      key === "frequencyHz" ? 1 : 0,
      maximumTick,
    );
  }
  if (clock?.frequencyHz !== expected.frequencyHz) {
    errors.push(`${label}.monotonicClock.frequencyHz changed`);
  }
  const first = clock?.firstNormalizedItemAcceptedTick;
  const cancel = [
    clock?.cancelReceivedTick,
    clock?.cancelWriteCompletedTick,
    clock?.cancelTerminalCommittedTick,
  ];
  validateCharacterizationShape(errors, value?.case, first, cancel, label);
  if (
    clock?.processReadyTick <= clock?.spawnStartedTick ||
    clock?.initializeWriteStartedTick < clock?.processReadyTick ||
    clock?.readyCommittedTick <= clock?.initializeWriteStartedTick ||
    clock?.promptWriteCompletedTick < clock?.readyCommittedTick ||
    clock?.jobZeroTick <= clock?.closeReceivedTick ||
    clock?.closeCommittedTick < clock?.jobZeroTick ||
    clock?.walConvergedTick < clock?.closeCommittedTick
  ) {
    errors.push(`${label}.monotonicClock anchors are out of order`);
  }
  if (
    first !== null &&
    (first <= clock?.promptWriteCompletedTick ||
      first > clock?.closeReceivedTick)
  ) {
    errors.push(`${label}.monotonicClock first output is out of order`);
  }
  if (
    cancel.every((tick) => tick !== null) &&
    (cancel[0] < clock?.promptWriteCompletedTick ||
      cancel[1] <= cancel[0] ||
      cancel[2] < cancel[1] ||
      cancel[2] > clock?.closeReceivedTick ||
      (value?.case === "cold-cycle" && first > cancel[0]))
  ) {
    errors.push(`${label}.monotonicClock cancel anchors are out of order`);
  }

  const wal = value?.walConvergence;
  exactKeys(
    errors,
    wal,
    ["walBytes", "checkpointTargetBytes", "checkpointed"],
    `${label}.walConvergence`,
  );
  safeInteger(
    errors,
    wal?.walBytes,
    `${label}.walConvergence.walBytes`,
    0,
    maximumTick,
  );
  safeInteger(
    errors,
    wal?.checkpointTargetBytes,
    `${label}.walConvergence.checkpointTargetBytes`,
    1,
    maximumTick,
  );
  if (
    wal?.checkpointTargetBytes !== expected.walCheckpointTargetBytes ||
    wal?.walBytes >= wal?.checkpointTargetBytes ||
    typeof wal?.checkpointed !== "boolean"
  ) {
    errors.push(`${label}.walConvergence is invalid`);
  }

  const latency = value?.latencyUs;
  exactKeys(errors, latency, latencyFields, `${label}.latencyUs`);
  for (const key of latencyFields) {
    if (latency?.[key] === null) continue;
    safeInteger(
      errors,
      latency?.[key],
      `${label}.latencyUs.${key}`,
      1,
      maximumTick,
    );
  }
  const derived = {
    spawnToProcessReady: monotonicDurationUs(
      clock?.spawnStartedTick,
      clock?.processReadyTick,
      clock?.frequencyHz,
    ),
    initialize: monotonicDurationUs(
      clock?.initializeWriteStartedTick,
      clock?.readyCommittedTick,
      clock?.frequencyHz,
    ),
    firstNormalizedOutput:
      first === null
        ? null
        : monotonicDurationUs(
            clock?.promptWriteCompletedTick,
            first,
            clock?.frequencyHz,
          ),
    cancelDispatch:
      cancel[0] === null || cancel[1] === null
        ? null
        : monotonicDurationUs(cancel[0], cancel[1], clock?.frequencyHz),
    cancelTerminal:
      cancel[0] === null || cancel[2] === null
        ? null
        : monotonicDurationUs(cancel[0], cancel[2], clock?.frequencyHz),
    nativeProcessConvergence: monotonicDurationUs(
      clock?.closeReceivedTick,
      clock?.jobZeroTick,
      clock?.frequencyHz,
    ),
  };
  if (latencyFields.some((key) => latency?.[key] !== derived[key])) {
    errors.push(`${label}.latencyUs does not match its native anchors`);
  }
  if (!creationTimeUtc || errors.length > errorCount) return null;
  return {
    fixtureProcess: {
      pid: value.fixtureProcess.pid,
      creationTimeUtc,
      creationTimeFiletime100ns: value.fixtureProcess.creationTimeFiletime100ns,
      processIdentitySha256: recordDigest({
        pid: value.fixtureProcess.pid,
        creationTimeFiletime100ns:
          value.fixtureProcess.creationTimeFiletime100ns,
        executableIdentitySha256: expected.executableSha256,
      }),
    },
    anchors: {
      spawnStartedTick: clock.spawnStartedTick,
      readyCommittedTick: clock.readyCommittedTick,
      closeReceivedTick: clock.closeReceivedTick,
      jobZeroTick: clock.jobZeroTick,
      walConvergedTick: clock.walConvergedTick,
    },
    spawnToProcessReadyUs: derived.spawnToProcessReady,
    initializeUs: derived.initialize,
    firstNormalizedOutputUs: derived.firstNormalizedOutput,
    cancelDispatchUs: derived.cancelDispatch,
    cancelTerminalUs: derived.cancelTerminal,
    nativeProcessConvergenceUs: derived.nativeProcessConvergence,
  };
}

function validateCharacterizationShape(errors, caseName, first, cancel, label) {
  const hasFirst = first !== null;
  const cancelCount = cancel.filter((tick) => tick !== null).length;
  if (
    (caseName === "turn-success" && (!hasFirst || cancelCount !== 0)) ||
    (caseName === "turn-cancelled" && (hasFirst || cancelCount !== 3)) ||
    (caseName === "cold-cycle" && (!hasFirst || cancelCount !== 3))
  ) {
    errors.push(`${label} observations do not match its case`);
  }
}

function filetimeUtc(errors, value, label) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(value)) {
    errors.push(`${label} is invalid`);
    return null;
  }
  const filetime = BigInt(value);
  if (filetime < windowsToUnixEpoch100ns || filetime > maximumU64) {
    errors.push(`${label} is outside the supported FILETIME range`);
    return null;
  }
  const milliseconds = (filetime - windowsToUnixEpoch100ns) / 10_000n;
  if (milliseconds > BigInt(8_640_000_000_000_000)) {
    errors.push(`${label} exceeds the Date range`);
    return null;
  }
  return new Date(Number(milliseconds)).toISOString();
}

export function monotonicDurationUs(start, end, frequency) {
  if (
    !Number.isSafeInteger(start) ||
    !Number.isSafeInteger(end) ||
    !Number.isSafeInteger(frequency) ||
    frequency <= 0 ||
    end < start
  ) {
    return undefined;
  }
  return Number(
    (BigInt(end - start) * 1_000_000n + BigInt(frequency) - 1n) /
      BigInt(frequency),
  );
}

export function validateProcessIdentity(
  errors,
  value,
  executableIdentitySha256,
  label,
) {
  exactKeys(
    errors,
    value,
    [
      "pid",
      "creationTimeUtc",
      "creationTimeFiletime100ns",
      "processIdentitySha256",
    ],
    label,
  );
  safeInteger(errors, value?.pid, `${label}.pid`, 1, 2 ** 32 - 1);
  if (!validUtcTimestamp(value?.creationTimeUtc)) {
    errors.push(`${label}.creationTimeUtc is invalid`);
  }
  const derivedUtc = filetimeUtc(
    errors,
    value?.creationTimeFiletime100ns,
    `${label}.creationTimeFiletime100ns`,
  );
  if (derivedUtc !== value?.creationTimeUtc) {
    errors.push(`${label}.creationTimeUtc does not match FILETIME`);
  }
  const expected = recordDigest({
    pid: value?.pid,
    creationTimeFiletime100ns: value?.creationTimeFiletime100ns,
    executableIdentitySha256,
  });
  if (value?.processIdentitySha256 !== expected) {
    errors.push(`${label}.processIdentitySha256 is not PID-creation bound`);
  }
  return recordDigest({
    pid: value?.pid,
    creationTimeFiletime100ns: value?.creationTimeFiletime100ns,
  });
}

export function validateProfileIdentity(errors, value, label, identities) {
  if (!SHA256.test(value ?? "")) {
    errors.push(`${label} is invalid`);
    return;
  }
  if (identities.has(value)) errors.push(`${label} is reused`);
  identities.add(value);
}

export function validateCadence(
  errors,
  snapshots,
  cadence,
  samplingStartedTick,
  frequencyHz,
  label,
  validateSnapshot,
) {
  if (!Array.isArray(snapshots) || snapshots.length !== cadence.snapshotCount) {
    errors.push(
      `${label} must contain exactly ${cadence.snapshotCount} snapshots`,
    );
    return false;
  }
  safeInteger(
    errors,
    samplingStartedTick,
    `${label}.samplingStartedTick`,
    0,
    maximumTick,
  );
  const intervalTicks = Math.round(
    (cadence.snapshotIntervalMs * frequencyHz) / 1000,
  );
  let previousCaptured = -1;
  for (const [index, snapshot] of snapshots.entries()) {
    const snapshotLabel = `${label}[${index}]`;
    const expectedDue = samplingStartedTick + index * intervalTicks;
    if (
      snapshot?.ordinal !== index ||
      snapshot?.dueTick !== expectedDue ||
      !Number.isSafeInteger(snapshot?.capturedTick) ||
      snapshot.capturedTick < expectedDue ||
      snapshot.capturedTick >= expectedDue + intervalTicks ||
      snapshot.capturedTick <= previousCaptured
    ) {
      errors.push(`${snapshotLabel} violates the absolute cadence`);
    }
    previousCaptured = snapshot?.capturedTick ?? previousCaptured;
    validateSnapshot(snapshot, snapshotLabel);
  }
  const dueWindow = snapshots.at(-1)?.dueTick - snapshots[0]?.dueTick;
  if (dueWindow < (cadence.minimumSamplingWindowMs * frequencyHz) / 1000) {
    errors.push(`${label} is shorter than the minimum sampling window`);
  }
  return true;
}

export function validateSemanticSettle(
  errors,
  readyTick,
  samplingStartedTick,
  settleMs,
  frequencyHz,
  label,
) {
  safeInteger(errors, readyTick, `${label}.readyTick`, 0, maximumTick);
  if (
    !Number.isSafeInteger(samplingStartedTick) ||
    samplingStartedTick - readyTick < (settleMs * frequencyHz) / 1000
  ) {
    errors.push(`${label} did not complete its semantic settle`);
  }
}

export function validateCloseGate(errors, value, frequencyHz, label) {
  for (const key of ["closeRequestedTick", "zeroDescendantsTick"]) {
    safeInteger(errors, value?.[key], `${label}.${key}`, 0, maximumTick);
  }
  if (
    value?.zeroDescendantsTick < value?.closeRequestedTick ||
    value?.zeroDescendantsTick - value?.closeRequestedTick > 5 * frequencyHz ||
    value?.normalSageDataMutations !== 0 ||
    value?.orphanProcessesAfterClose !== 0
  ) {
    errors.push(`${label} did not close in isolation`);
  }
}

export function cpuPercent(previous, current, frequencyHz) {
  const elapsedTicks = current.capturedTick - previous.capturedTick;
  const cpu100ns = current.cpuTime100ns - previous.cpuTime100ns;
  if (elapsedTicks <= 0 || cpu100ns < 0) return Number.POSITIVE_INFINITY;
  return (cpu100ns * frequencyHz * 100) / (elapsedTicks * 10_000_000);
}
