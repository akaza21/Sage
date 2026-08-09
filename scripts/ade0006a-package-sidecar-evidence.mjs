import { readdir } from "node:fs/promises";
import path from "node:path";

import {
  exactKeys,
  parseBoundedJson,
  recordDigest,
  safeInteger,
  SHA256,
} from "./ade0006a-c0b-common.mjs";

function validateRecord(errors, record, ordinal, label, bounds) {
  const regular = record?.type === "regularFile";
  exactKeys(
    errors,
    record,
    regular
      ? ["ordinal", "relativePath", "type", "bytes", "sha256"]
      : ["ordinal", "relativePath", "type"],
    label,
  );
  if (
    record?.ordinal !== ordinal ||
    !["directory", "regularFile"].includes(record?.type) ||
    typeof record?.relativePath !== "string" ||
    Buffer.byteLength(record.relativePath ?? "") > bounds.pathUtf8Bytes ||
    /(?:^|\/)\.\.?(?:\/|$)|\\|^[A-Za-z]:/u.test(record.relativePath ?? "")
  ) {
    errors.push(`${label} is not a canonical typed member`);
  }
  if (regular) {
    safeInteger(
      errors,
      record?.bytes,
      `${label}.bytes`,
      1,
      bounds.extractedBytesPerInstaller,
    );
    if (!SHA256.test(record?.sha256 ?? "")) {
      errors.push(`${label}.sha256 is invalid`);
    }
  }
}

export async function validateInstallerSidecars({
  errors,
  rawRoot,
  participant,
  installer,
  summary,
  frozen,
}) {
  const label = `receipt.participants.${participant}.${installer}`;
  const directory = path.join(rawRoot, "package", participant, installer);
  const entries = await readdir(directory, { withFileTypes: true });
  const expectedNames = summary.memberManifest.shards.map(
    ({ ordinal }) => `members-${String(ordinal).padStart(4, "0")}.json`,
  );
  if (
    entries.some((entry) => !entry.isFile()) ||
    entries
      .map(({ name }) => name)
      .sort()
      .join("\0") !== [...expectedNames].sort().join("\0")
  ) {
    errors.push(`${label} sidecar directory is incomplete or contains extras`);
    return;
  }
  const records = [];
  for (const [index, shard] of summary.memberManifest.shards.entries()) {
    const record = parseBoundedJson(
      path.join(directory, expectedNames[index]),
      frozen.package.observerBounds.memberManifestBytesPerInstaller,
      frozen.limits.wire,
      `${label} shard ${index + 1}`,
    );
    if (record.bytes !== shard.bytes || record.sha256 !== shard.sha256) {
      errors.push(`${label} shard ${index + 1} bytes or hash changed`);
    }
    exactKeys(
      errors,
      record.value,
      [
        "schemaVersion",
        "participant",
        "installer",
        "shardOrdinal",
        "firstRecordOrdinal",
        "records",
      ],
      `${label}.shards[${index}]`,
    );
    if (
      record.value.schemaVersion !== 1 ||
      record.value.participant !== participant ||
      record.value.installer !== installer ||
      record.value.shardOrdinal !== shard.ordinal ||
      record.value.firstRecordOrdinal !== shard.firstRecordOrdinal ||
      record.value.records?.length !== shard.recordCount
    ) {
      errors.push(`${label} shard ${index + 1} header is invalid`);
      continue;
    }
    for (const member of record.value.records) {
      records.push(member);
      validateRecord(
        errors,
        member,
        records.length,
        `${label}.records[${records.length - 1}]`,
        frozen.package.observerBounds,
      );
    }
  }
  const regularFiles = records.filter(({ type }) => type === "regularFile");
  if (
    records.length !== summary.memberManifest.recordCount ||
    regularFiles.length !== summary.memberManifest.regularFileCount ||
    regularFiles.reduce((total, record) => total + record.bytes, 0) !==
      summary.memberManifest.regularFileBytes ||
    recordDigest(records) !== summary.memberManifest.sha256
  ) {
    errors.push(`${label} sidecar records do not reconcile with the receipt`);
  }
  const installed = records.filter(
    ({ relativePath, type }) =>
      type === "regularFile" &&
      relativePath === summary.installedExecutable.logicalRelativePath,
  );
  if (
    installed.length !== 1 ||
    installed[0].bytes !== summary.installedExecutable.bytes ||
    installed[0].sha256 !== summary.installedExecutable.sha256
  ) {
    errors.push(
      `${label} installed executable is not in the authenticated sidecars`,
    );
  }
}
