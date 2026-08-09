import {
  exactKeys,
  recordDigest,
  safeInteger,
  SHA256,
  stableJson,
} from "./ade0006a-c0b-common.mjs";

function sortedUnique(values) {
  return (
    Array.isArray(values) &&
    new Set(values).size === values.length &&
    JSON.stringify(values) === JSON.stringify([...values].sort())
  );
}

function identity(record) {
  return recordDigest({
    ecosystem: record.ecosystem,
    name: record.name,
    version: record.version,
    source: record.source,
    checksum: record.checksum,
  });
}

function validateDependencyRecords(errors, records, ecosystem, maximum, label) {
  if (!Array.isArray(records) || records.length > maximum) {
    errors.push(`${label} exceeds its package bound`);
    return;
  }
  const identities = new Set(records.map(identity));
  records.forEach((record, index) => {
    const recordLabel = `${label}[${index}]`;
    exactKeys(
      errors,
      record,
      [
        "ecosystem",
        "name",
        "version",
        "source",
        "checksum",
        "features",
        "dependencies",
      ],
      recordLabel,
    );
    if (
      record?.ecosystem !== ecosystem ||
      typeof record?.name !== "string" ||
      record.name.length === 0 ||
      typeof record?.version !== "string" ||
      record.version.length === 0 ||
      (record.source !== null && typeof record.source !== "string") ||
      (record.checksum !== null && typeof record.checksum !== "string") ||
      !sortedUnique(record.features) ||
      !sortedUnique(record.dependencies) ||
      record.dependencies.some(
        (value) => !SHA256.test(value) || !identities.has(value),
      )
    ) {
      errors.push(`${recordLabel} is invalid or has an unresolved edge`);
    }
  });
  const canonical = [...records].sort((left, right) =>
    Buffer.compare(
      Buffer.from(stableJson(left)),
      Buffer.from(stableJson(right)),
    ),
  );
  if (stableJson(records) !== stableJson(canonical)) {
    errors.push(`${label} is not canonically ordered`);
  }
}

function validateFrontendMembers(errors, records, bounds, label) {
  if (!Array.isArray(records) || records.length > bounds.frontendMembers) {
    errors.push(`${label} exceeds its member bound`);
    return 0;
  }
  let bytes = 0;
  let previousPath;
  records.forEach((record, index) => {
    const recordLabel = `${label}[${index}]`;
    const regular = record?.type === "regularFile";
    exactKeys(
      errors,
      record,
      regular
        ? ["ordinal", "relativePath", "type", "bytes", "sha256"]
        : ["ordinal", "relativePath", "type"],
      recordLabel,
    );
    const relativePath = record?.relativePath;
    if (
      record?.ordinal !== index + 1 ||
      typeof relativePath !== "string" ||
      relativePath.length === 0 ||
      relativePath.includes("\\") ||
      relativePath
        .split("/")
        .some((part) => !part || part === "." || part === "..") ||
      Buffer.byteLength(relativePath) > bounds.pathUtf8Bytes ||
      (previousPath !== undefined &&
        Buffer.compare(Buffer.from(previousPath), Buffer.from(relativePath)) >=
          0) ||
      (!regular && record?.type !== "directory")
    ) {
      errors.push(`${recordLabel} is invalid or out of order`);
    }
    previousPath = relativePath;
    if (regular) {
      safeInteger(
        errors,
        record.bytes,
        `${recordLabel}.bytes`,
        0,
        bounds.frontendBytes,
      );
      if (!SHA256.test(record.sha256 ?? "")) {
        errors.push(`${recordLabel}.sha256 is invalid`);
      }
      bytes += Number.isSafeInteger(record.bytes) ? record.bytes : 0;
    }
  });
  if (bytes > bounds.frontendBytes) {
    errors.push(`${label} exceeds its byte bound`);
  }
  return bytes;
}

export function validatePackageParticipantProvenance(
  errors,
  participant,
  summary,
  source,
  frozen,
) {
  const label = `receipt.participants.${participant}.provenance`;
  const provenance = summary?.provenance;
  exactKeys(
    errors,
    provenance,
    [
      "cargoLockSha256",
      "frontendDependencyMode",
      "frontendLockSha256",
      "dependencyGraphSha256",
      "dependencyPackageCount",
      "frontendRawBytes",
      "frontendTreeSha256",
      "dependencyGraph",
      "frontendMembers",
    ],
    label,
  );
  const profile = frozen.package.participants[participant]?.provenance;
  const sourceFiles = new Map(
    (source?.files ?? []).map((file) => [file.path, file]),
  );
  const cargoLock = sourceFiles.get(profile?.cargoLock);
  const frontendLock = profile?.frontendLock
    ? sourceFiles.get(profile.frontendLock)
    : undefined;
  if (
    !profile ||
    provenance?.frontendDependencyMode !== profile.frontendDependencyMode ||
    !SHA256.test(provenance?.cargoLockSha256 ?? "") ||
    cargoLock?.sha256 !== provenance?.cargoLockSha256 ||
    (profile.frontendDependencyMode === "static-no-package-manager"
      ? provenance?.frontendLockSha256 !== null ||
        profile.frontendLock !== null ||
        provenance?.dependencyGraph?.frontend?.length !== 0
      : !SHA256.test(provenance?.frontendLockSha256 ?? "") ||
        frontendLock?.sha256 !== provenance?.frontendLockSha256)
  ) {
    errors.push(`${label} lock identity or dependency mode is invalid`);
  }
  exactKeys(
    errors,
    provenance?.dependencyGraph,
    ["cargo", "frontend"],
    `${label}.dependencyGraph`,
  );
  const maximum = frozen.package.observerBounds.dependencyPackages;
  validateDependencyRecords(
    errors,
    provenance?.dependencyGraph?.cargo,
    "cargo",
    maximum,
    `${label}.dependencyGraph.cargo`,
  );
  validateDependencyRecords(
    errors,
    provenance?.dependencyGraph?.frontend,
    "npm",
    maximum,
    `${label}.dependencyGraph.frontend`,
  );
  const count =
    (provenance?.dependencyGraph?.cargo?.length ?? 0) +
    (provenance?.dependencyGraph?.frontend?.length ?? 0);
  if (
    provenance?.dependencyPackageCount !== count ||
    count > maximum ||
    Buffer.byteLength(stableJson(provenance?.dependencyGraph)) >
      frozen.package.observerBounds.dependencyGraphBytes ||
    provenance?.dependencyGraphSha256 !==
      recordDigest(provenance?.dependencyGraph)
  ) {
    errors.push(
      `${label} graph digest, bytes, or package count does not reconcile`,
    );
  }
  const frontendBytes = validateFrontendMembers(
    errors,
    provenance?.frontendMembers,
    frozen.package.observerBounds,
    `${label}.frontendMembers`,
  );
  if (
    provenance?.frontendRawBytes !== frontendBytes ||
    provenance?.frontendTreeSha256 !== recordDigest(provenance?.frontendMembers)
  ) {
    errors.push(`${label} frontend bytes or tree digest do not reconcile`);
  }
}
