import { exactKeys, safeInteger, SHA256 } from "./ade0006a-c0b-common.mjs";
import { validateInstallerSidecars } from "./ade0006a-package-sidecar-evidence.mjs";

function validateManifest(errors, label, value, frozen) {
  const bounds = frozen.package.observerBounds;
  exactKeys(
    errors,
    value,
    [
      "extractedRecordCount",
      "extractedRegularFileCount",
      "extractedRegularFileBytes",
      "recordCount",
      "regularFileCount",
      "regularFileBytes",
      "pathUtf8Bytes",
      "shardCount",
      "bytes",
      "sha256",
      "shards",
    ],
    label,
  );
  for (const [field, minimum, maximum] of [
    ["extractedRecordCount", 1, bounds.membersPerInstaller],
    ["extractedRegularFileCount", 1, bounds.membersPerInstaller],
    ["recordCount", 1, bounds.membersPerInstaller],
    ["regularFileCount", 1, bounds.membersPerInstaller],
    ["extractedRegularFileBytes", 1, bounds.extractedBytesPerInstaller],
    ["regularFileBytes", 1, bounds.extractedBytesPerInstaller],
    ["pathUtf8Bytes", 1, bounds.membersPerInstaller * bounds.pathUtf8Bytes],
    ["bytes", 1, bounds.memberManifestBytesPerInstaller],
  ]) {
    safeInteger(errors, value?.[field], `${label}.${field}`, minimum, maximum);
  }
  if (!SHA256.test(value?.sha256 ?? "") || !Array.isArray(value?.shards)) {
    errors.push(`${label} digest or shards are invalid`);
    return;
  }
  safeInteger(
    errors,
    value.shardCount,
    `${label}.shardCount`,
    1,
    bounds.memberManifestShardsPerInstaller,
  );
  value.shards.forEach((shard, index) => {
    const shardLabel = `${label}.shards[${index}]`;
    exactKeys(
      errors,
      shard,
      ["ordinal", "firstRecordOrdinal", "recordCount", "bytes", "sha256"],
      shardLabel,
    );
    safeInteger(
      errors,
      shard?.recordCount,
      `${shardLabel}.recordCount`,
      1,
      bounds.memberRecordsPerShard,
    );
    safeInteger(
      errors,
      shard?.bytes,
      `${shardLabel}.bytes`,
      1,
      bounds.memberManifestBytesPerInstaller,
    );
    if (
      shard?.ordinal !== index + 1 ||
      shard?.firstRecordOrdinal !==
        1 +
          value.shards
            .slice(0, index)
            .reduce((total, entry) => total + entry.recordCount, 0) ||
      !SHA256.test(shard?.sha256 ?? "")
    ) {
      errors.push(`${shardLabel} identity is invalid`);
    }
  });
  if (
    value.shardCount !== value.shards.length ||
    value.bytes !==
      value.shards.reduce((total, shard) => total + shard.bytes, 0) ||
    value.recordCount !==
      value.shards.reduce((total, shard) => total + shard.recordCount, 0) ||
    value.regularFileCount > value.recordCount ||
    value.extractedRegularFileCount > value.extractedRecordCount ||
    value.regularFileBytes > value.extractedRegularFileBytes
  ) {
    errors.push(`${label} counts or bytes do not reconcile`);
  }
}

function validateExtractor(errors, label, kind, value, frozen) {
  exactKeys(
    errors,
    value,
    ["tool", "toolSha256", "exitCode", "stdoutBytes", "stderrBytes"],
    label,
  );
  safeInteger(
    errors,
    value?.stdoutBytes,
    `${label}.stdoutBytes`,
    0,
    frozen.package.observerBounds.toolStdoutBytes,
  );
  safeInteger(
    errors,
    value?.stderrBytes,
    `${label}.stderrBytes`,
    0,
    frozen.package.observerBounds.toolStderrBytes,
  );
  if (
    value?.tool !== (kind === "msi" ? "wix-dark" : "seven-zip") ||
    !SHA256.test(value?.toolSha256 ?? "") ||
    value?.exitCode !== 0
  ) {
    errors.push(`${label} is invalid`);
  }
}

function validateExecutable(
  errors,
  label,
  participant,
  kind,
  summary,
  value,
  frozen,
) {
  const derivation = value?.executableDerivation;
  const installed = value?.installedExecutable;
  const mapping = value?.executableMapping;
  exactKeys(
    errors,
    derivation,
    [
      "algorithm",
      "sourceMarkerOffset",
      "sourceMarkerOccurrences",
      "targetMarkerOccurrences",
      "sourceExecutableSha256",
      "expectedInstalledExecutableSha256",
      "installedExecutableSha256",
      "installedExecutableBytes",
    ],
    `${label}.executableDerivation`,
  );
  exactKeys(
    errors,
    installed,
    ["logicalRelativePath", "extractedRelativePath", "bytes", "sha256"],
    `${label}.installedExecutable`,
  );
  exactKeys(
    errors,
    mapping,
    ["method", "metadataSha256", "mappedFileCount", "sha256"],
    `${label}.executableMapping`,
  );
  const maximum = frozen.package.observerBounds.executableArtifactBytes;
  safeInteger(
    errors,
    installed?.bytes,
    `${label}.installedExecutable.bytes`,
    1,
    maximum,
  );
  safeInteger(
    errors,
    derivation?.sourceMarkerOffset,
    `${label}.executableDerivation.sourceMarkerOffset`,
    0,
    maximum - 1,
  );
  safeInteger(
    errors,
    derivation?.sourceMarkerOccurrences,
    `${label}.executableDerivation.sourceMarkerOccurrences`,
    1,
    1,
  );
  safeInteger(
    errors,
    derivation?.targetMarkerOccurrences,
    `${label}.executableDerivation.targetMarkerOccurrences`,
    1,
    maximum,
  );
  safeInteger(
    errors,
    mapping?.mappedFileCount,
    `${label}.executableMapping.mappedFileCount`,
    1,
    frozen.package.observerBounds.membersPerInstaller,
  );
  const executable = summary?.artifactSet?.entries?.find(
    (entry) => entry.kind === "executable",
  );
  const expectedLogicalPath =
    frozen.package.participants[participant].installedExecutableLogicalPaths[
      kind
    ];
  const projection = frozen.package.inspection.installedPayloadProjection[kind];
  if (
    derivation?.algorithm !==
      frozen.package.installedExecutableIdentity.algorithm ||
    derivation?.sourceExecutableSha256 !== executable?.sha256 ||
    derivation?.expectedInstalledExecutableSha256 !== installed?.sha256 ||
    derivation?.installedExecutableSha256 !== installed?.sha256 ||
    derivation?.installedExecutableBytes !== installed?.bytes ||
    installed?.logicalRelativePath !== expectedLogicalPath ||
    typeof installed?.extractedRelativePath !== "string" ||
    !SHA256.test(installed?.sha256 ?? "") ||
    mapping?.method !== projection.method ||
    !SHA256.test(mapping?.sha256 ?? "") ||
    (kind === "msi"
      ? !SHA256.test(mapping?.metadataSha256 ?? "")
      : mapping?.metadataSha256 !== null) ||
    mapping?.mappedFileCount !== value?.memberManifest?.regularFileCount
  ) {
    errors.push(`${label} executable evidence is invalid`);
  }
}

export async function validatePackageInstallerEvidence({
  errors,
  receipt,
  participant,
  summary,
  installer,
  rawRoot,
  frozen,
}) {
  const kind = installer?.kind;
  const label = `receipt.participants.${participant}.${kind ?? "unknown"}`;
  exactKeys(
    errors,
    installer,
    [
      "kind",
      "artifactSha256",
      "artifactBytes",
      "extractor",
      "executableDerivation",
      "memberManifest",
      "executableMapping",
      "installedExecutable",
      "fixtureBasenameCount",
      "fixtureHashMatchCount",
      "bundledWebviewPayloadCount",
      "bundledWebviewPayloadBytes",
    ],
    label,
  );
  const artifact = summary?.artifactSet?.entries?.find(
    (entry) => entry.kind === kind,
  );
  if (
    !artifact ||
    installer?.artifactSha256 !== artifact.sha256 ||
    installer?.artifactBytes !== artifact.bytes
  ) {
    errors.push(`${label} artifact does not match artifactSet`);
  }
  validateExtractor(
    errors,
    `${label}.extractor`,
    kind,
    installer?.extractor,
    frozen,
  );
  validateManifest(
    errors,
    `${label}.memberManifest`,
    installer?.memberManifest,
    frozen,
  );
  validateExecutable(
    errors,
    label,
    participant,
    kind,
    summary,
    installer,
    frozen,
  );
  if (
    installer?.fixtureBasenameCount !== 0 ||
    installer?.fixtureHashMatchCount !==
      (participant === frozen.package.fixtureExclusion.participant
        ? 0
        : null) ||
    installer?.bundledWebviewPayloadCount !==
      frozen.package.webviewProvisioning.bundledPayloadCount ||
    installer?.bundledWebviewPayloadBytes !==
      frozen.package.webviewProvisioning.bundledPayloadBytes
  ) {
    errors.push(`${label} excluded payload evidence is invalid`);
  }
  const candidate = receipt?.participants?.candidate?.installers?.find(
    (value) => value.kind === kind,
  );
  if (
    participant === "control" &&
    installer?.extractor?.toolSha256 !== candidate?.extractor?.toolSha256
  ) {
    errors.push(`candidate and control ${kind} extractors differ`);
  }
  try {
    await validateInstallerSidecars({
      errors,
      rawRoot,
      participant,
      installer: kind,
      summary: installer,
      frozen,
    });
  } catch {
    errors.push(`${label} sidecars are missing or unreadable`);
  }
}
