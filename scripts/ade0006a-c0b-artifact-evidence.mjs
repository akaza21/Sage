import {
  exactKeys,
  safeInteger,
  sameValue,
  SHA256,
} from "./ade0006a-c0b-common.mjs";

const markerIds = ["fresh", "after-100-turns", "after-close-reopen"];

export function validatePersistence(errors, results, frozen) {
  exactKeys(
    errors,
    results.persistence,
    [
      "markers",
      "stateDirectoryBytesForFoundationFixture",
      "promptWritesPerTurn",
      "sqliteWritesPerTextDelta",
      "ipcNotificationsPerTextDelta",
      "recoveryRequired",
      "normalSageDataMutations",
    ],
    "persistence",
  );
  if (
    !Array.isArray(results.persistence?.markers) ||
    results.persistence.markers.map(({ id }) => id).join("\0") !==
      markerIds.join("\0")
  ) {
    errors.push("persistence.markers are incomplete or out of order");
  }
  for (const marker of results.persistence?.markers ?? []) {
    exactKeys(
      errors,
      marker,
      ["id", "candidate", "control"],
      `persistence.${marker.id}`,
    );
    for (const participant of ["candidate", "control"]) {
      const observation = marker[participant];
      const label = `persistence.${marker.id}.${participant}`;
      exactKeys(
        errors,
        observation,
        [
          "databaseBytes",
          "walBytesBeforeCheckpoint",
          "walBytesAfterCheckpoint",
          "checkpointDurationMs",
          "walConverged",
        ],
        label,
      );
      safeInteger(
        errors,
        observation?.databaseBytes,
        `${label}.databaseBytes`,
        0,
        frozen.budgets.persistence.liveDatabaseBytes,
      );
      safeInteger(
        errors,
        observation?.walBytesBeforeCheckpoint,
        `${label}.walBytesBeforeCheckpoint`,
        0,
        frozen.budgets.persistence.walHardStopBytes,
      );
      safeInteger(
        errors,
        observation?.walBytesAfterCheckpoint,
        `${label}.walBytesAfterCheckpoint`,
        0,
        frozen.budgets.persistence.walCheckpointTargetBytes,
      );
      safeInteger(
        errors,
        observation?.checkpointDurationMs,
        `${label}.checkpointDurationMs`,
        0,
        frozen.limits.deadlinesMs.walConvergence,
      );
      if (observation?.walConverged !== true) {
        errors.push(`${label} did not converge at the checkpoint target`);
      }
    }
    if (marker.candidate && marker.control) {
      if (
        marker.candidate.databaseBytes - marker.control.databaseBytes >
        frozen.budgets.controlRegressionMax.persistence.liveDatabaseBytes
      ) {
        errors.push(
          `persistence.${marker.id} database regression is too large`,
        );
      }
      if (
        marker.candidate.walBytesAfterCheckpoint -
          marker.control.walBytesAfterCheckpoint >
        frozen.budgets.controlRegressionMax.persistence.walBytes
      ) {
        errors.push(`persistence.${marker.id} WAL regression is too large`);
      }
    }
  }
  safeInteger(
    errors,
    results.persistence?.stateDirectoryBytesForFoundationFixture,
    "persistence.stateDirectoryBytesForFoundationFixture",
    0,
    frozen.budgets.persistence.stateDirectoryBytesForFoundationFixture,
  );
  for (const key of [
    "promptWritesPerTurn",
    "sqliteWritesPerTextDelta",
    "ipcNotificationsPerTextDelta",
  ]) {
    if (results.persistence?.[key] !== frozen.budgets.persistence[key]) {
      errors.push(`persistence.${key} does not match the frozen contract`);
    }
  }
  if (
    results.persistence?.recoveryRequired !== false ||
    results.persistence?.normalSageDataMutations !== 0
  ) {
    errors.push("persistence did not remain isolated and converged");
  }
}

const packageKeys = [
  "sourceFileCount",
  "sourceBytes",
  "dependencyPackageCount",
  "dependencyGraphSha256",
  "cargoLockSha256",
  "frontendLockSha256",
  "frontendRawBytes",
  "executableSha256",
  "executableBytes",
  "productionPackageSha256",
  "productionPackageBytes",
  "installerSha256",
  "installerBytes",
  "installedCoreBytes",
];

function trustedPackage(binding) {
  return Object.fromEntries(packageKeys.map((key) => [key, binding[key]]));
}

export function validatePackage(errors, results, trust, frozen) {
  const deltaKeys = [
    "sourceFileCount",
    "sourceBytes",
    "dependencyPackageCount",
    "frontendRawBytes",
    "executableBytes",
    "productionPackageBytes",
    "installerBytes",
    "installedCoreBytes",
  ];
  exactKeys(
    errors,
    results.package,
    [
      "candidate",
      "control",
      "deltas",
      "defaultProductionFixtureArtifactsBytes",
      "defaultProductionChildProcesses",
      "runtimeDownloadBytes",
    ],
    "package",
  );
  for (const participant of ["candidate", "control"]) {
    exactKeys(
      errors,
      results.package?.[participant],
      packageKeys,
      `package.${participant}`,
    );
    sameValue(
      errors,
      results.package?.[participant],
      trustedPackage(trust[participant]),
      `package.${participant}`,
    );
    for (const key of packageKeys.filter((key) => key.endsWith("Sha256"))) {
      if (!SHA256.test(results.package?.[participant]?.[key] ?? "")) {
        errors.push(`package.${participant}.${key} is invalid`);
      }
    }
  }
  exactKeys(errors, results.package?.deltas, deltaKeys, "package.deltas");
  for (const key of deltaKeys) {
    const expected = trust.candidate[key] - trust.control[key];
    if (results.package?.deltas?.[key] !== expected) {
      errors.push(
        `package.deltas.${key} is not derived from trusted artifacts`,
      );
    }
  }
  const gatedDeltas = {
    frontendRawBytes: frozen.budgets.packageDelta.frontendRawBytesMaximum,
    executableBytes:
      frozen.budgets.packageDelta.productionExecutableBytesMaximum,
    installedCoreBytes: frozen.budgets.packageDelta.installedCoreBytesMaximum,
  };
  for (const [key, maximum] of Object.entries(gatedDeltas)) {
    if (results.package?.deltas?.[key] > maximum) {
      errors.push(`package.deltas.${key} exceeds the frozen ceiling`);
    }
  }
  for (const key of [
    "defaultProductionFixtureArtifactsBytes",
    "defaultProductionChildProcesses",
    "runtimeDownloadBytes",
  ]) {
    if (results.package?.[key] !== frozen.budgets.packageDelta[key]) {
      errors.push(`package.${key} must remain zero`);
    }
  }
}

function validTimestamp(value) {
  if (
    typeof value !== "string" ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(value)
  ) {
    return false;
  }
  const milliseconds = Date.parse(value);
  return (
    Number.isFinite(milliseconds) &&
    new Date(milliseconds).toISOString() === value
  );
}

export function validateGestures(errors, results, trust, frozen) {
  exactKeys(errors, results.gestures, ["contractSha256", "cases"], "gestures");
  if (results.gestures?.contractSha256 !== frozen.packagedGesturesSha256) {
    errors.push("gestures.contractSha256 is stale");
  }
  if (
    !Array.isArray(results.gestures?.cases) ||
    results.gestures.cases.length !== frozen.gestures.cases.length
  ) {
    errors.push("gestures.cases must observe every frozen case");
    return;
  }
  results.gestures.cases.forEach((observation, index) => {
    const expected = frozen.gestures.cases[index];
    const label = `gestures.cases[${index}]`;
    exactKeys(
      errors,
      observation,
      [
        "id",
        "build",
        "startedAtUtc",
        "completedAtUtc",
        "artifactSha256",
        "semanticAutomation",
        "screenCoordinatesUsed",
        "globalInputUsed",
        "systemOrNetworkControlsUsed",
        "normalSageDataMutations",
        "observedOracle",
        "orphanProcessesAfterClose",
      ],
      label,
    );
    if (
      !validTimestamp(observation?.startedAtUtc) ||
      !validTimestamp(observation?.completedAtUtc) ||
      Date.parse(observation.completedAtUtc) <
        Date.parse(observation.startedAtUtc)
    ) {
      errors.push(`${label} timestamps are invalid`);
    } else if (
      validTimestamp(results.timestamps?.startedAtUtc) &&
      validTimestamp(results.timestamps?.completedAtUtc) &&
      (Date.parse(observation.startedAtUtc) <
        Date.parse(results.timestamps.startedAtUtc) ||
        Date.parse(observation.completedAtUtc) >
          Date.parse(results.timestamps.completedAtUtc))
    ) {
      errors.push(`${label} timestamps are outside the campaign window`);
    }
    if (
      observation?.id !== expected.id ||
      observation?.build !== expected.build ||
      observation?.artifactSha256 !== trust.candidate.executableSha256 ||
      observation?.semanticAutomation !== true ||
      observation?.screenCoordinatesUsed !== false ||
      observation?.globalInputUsed !== false ||
      observation?.systemOrNetworkControlsUsed !== false ||
      observation?.normalSageDataMutations !== 0 ||
      observation?.orphanProcessesAfterClose !== 0
    )
      errors.push(`${label} did not use the isolated semantic candidate`);
    sameValue(
      errors,
      observation?.observedOracle,
      expected.oracle,
      `${label}.observedOracle`,
    );
  });
}

export function validateEvidenceBounds(
  errors,
  results,
  frozen,
  serializedBytes,
) {
  const bounds = frozen.evidence.campaignBounds;
  const mapping = {
    rawFiles: "rawFiles",
    rawBytes: "rawBytes",
    campaignsForCandidateIdentity: "campaignsPerCandidateIdentity",
    rawBytesForCandidateIdentity: "rawBytesPerCandidateIdentity",
    retainedCandidateIdentities: "retainedCandidateIdentities",
    globalFiles: "globalFiles",
    globalBytes: "globalBytes",
    curatedBytes: "curatedBytes",
    compactedCuratedFiles: "compactedCuratedFiles",
    compactedCuratedBytes: "compactedCuratedBytes",
  };
  exactKeys(errors, results.evidence, Object.keys(mapping), "evidence");
  for (const [key, boundKey] of Object.entries(mapping)) {
    safeInteger(
      errors,
      results.evidence?.[key],
      `evidence.${key}`,
      0,
      bounds[boundKey],
    );
  }
  if (results.evidence?.curatedBytes !== serializedBytes) {
    errors.push("evidence.curatedBytes does not equal actual retained bytes");
  }
  exactKeys(
    errors,
    results.redaction,
    [
      "scanPerformed",
      "violations",
      "rawProtocolFramesRetained",
      "secretsRetained",
      "commandLinesRetained",
      "absolutePathsRetained",
    ],
    "redaction",
  );
  if (
    results.redaction?.scanPerformed !== true ||
    !Array.isArray(results.redaction?.violations) ||
    results.redaction.violations.length !== 0
  )
    errors.push("redaction scan is absent or failed");
  for (const key of [
    "rawProtocolFramesRetained",
    "secretsRetained",
    "commandLinesRetained",
    "absolutePathsRetained",
  ])
    if (results.redaction?.[key] !== 0)
      errors.push(`redaction.${key} must be zero`);
}

export function validTimestampRange(errors, results) {
  exactKeys(
    errors,
    results.timestamps,
    ["startedAtUtc", "completedAtUtc"],
    "timestamps",
  );
  if (
    !validTimestamp(results.timestamps?.startedAtUtc) ||
    !validTimestamp(results.timestamps?.completedAtUtc) ||
    Date.parse(results.timestamps.completedAtUtc) <
      Date.parse(results.timestamps.startedAtUtc)
  )
    errors.push("timestamps are invalid");
  if (
    !Array.isArray(results.limitations) ||
    results.limitations.length < 1 ||
    results.limitations.length > 16 ||
    results.limitations.some(
      (item) => !/^[a-z0-9][a-z0-9._-]{0,127}$/.test(item),
    )
  )
    errors.push("limitations must contain 1..16 sanitized codes");
}
